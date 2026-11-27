//! A seeded, virtual-time state machine over the whole worker: thousands of
//! random switches, moves, floats, pins, scratchpad toggles, drags, resizes,
//! display changes, macOS Space slides, restorations, window opens and closes
//! and outside focus changes, with the core invariants checked after every
//! step. Time only moves when the machine advances the fake clock, so a run
//! replays from its seed and takes well under a second.

use super::*;
use crate::scratchpad::ScratchpadCommand;
use crate::{Axis, ResizeDirection};

const STEPS: usize = 1_500;
const SEEDS: [u64; 2] = [0x5eed_5ace_0001, 0x0dd_ba11_c0de];

/// SplitMix64: small, seedable and the same on every machine.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }

    fn below(&mut self, bound: usize) -> usize {
        usize::try_from(self.next() % bound as u64).unwrap_or(0)
    }

    fn pick<T: Clone>(&mut self, items: &[T]) -> Option<T> {
        (!items.is_empty()).then(|| items[self.below(items.len())].clone())
    }

    fn workspace(&mut self) -> WorkspaceId {
        WorkspaceId::new(u8::try_from(1 + self.below(4)).unwrap()).unwrap()
    }

    fn direction(&mut self) -> Direction {
        [
            Direction::Left,
            Direction::Right,
            Direction::Up,
            Direction::Down,
        ][self.below(4)]
    }
}

fn single_output() -> macos::DisplayOutputs {
    macos::DisplayOutputs::new(vec![outputs().displays()[0].clone()]).unwrap()
}

fn sim_window(id: u32) -> crate::PhysicalWindowId {
    crate::PhysicalWindowId {
        app: AppInstanceId {
            pid: i32::try_from(id).unwrap(),
            launched_at: 1,
        },
        window_id: id,
    }
}

/// The desktop the machine drives: which windows are open, which sit on
/// another macOS desktop, and the displays now connected.
struct World {
    rng: Rng,
    open: Vec<crate::PhysicalWindowId>,
    away: HashSet<crate::PhysicalWindowId>,
    next_id: u32,
    outputs: macos::DisplayOutputs,
}

impl World {
    fn new(seed: u64) -> (Self, WorkspaceRuntime<FakeBackend>) {
        let mut world = Self {
            rng: Rng(seed),
            open: Vec::new(),
            away: HashSet::new(),
            next_id: 10,
            outputs: outputs(),
        };
        let config = Config::parse("version = 1\n[labs]\nscratchpad = true\n").unwrap();
        let mut rt = runtime_with_config(FakeBackend::default(), config);
        for _ in 0..5 {
            world.open_window(&mut rt);
        }
        (world, rt)
    }

    fn open_window(&mut self, rt: &mut WorkspaceRuntime<FakeBackend>) {
        let window = sim_window(self.next_id);
        self.next_id += 1;
        let display = self.rng.pick(self.outputs.displays()).unwrap().frame;
        let frame = crate::Rect {
            x: display.x + i32::try_from(self.rng.below(400)).unwrap(),
            y: display.y + 20 + i32::try_from(self.rng.below(300)).unwrap(),
            width: 300 + i32::try_from(self.rng.below(300)).unwrap(),
            height: 200 + i32::try_from(self.rng.below(200)).unwrap(),
        };
        rt.backend.frames.insert(window.clone(), frame);
        rt.backend.focused = Some(window.clone());
        rt.backend.frontmost = Some(window.app.clone());
        self.open.push(window);
    }

    fn here(&self) -> Vec<crate::PhysicalWindowId> {
        self.open
            .iter()
            .filter(|window| !self.away.contains(*window))
            .cloned()
            .collect()
    }

    /// What Accessibility lists: every app, with its windows on this desktop.
    fn observe(&self, rt: &mut WorkspaceRuntime<FakeBackend>) {
        let focused = rt.backend.focused.clone();
        let applications = (10..self.next_id)
            .map(sim_window)
            .filter(|window| !self.away.contains(window))
            .map(|window| {
                let open = self.open.contains(&window);
                crate::AppObservation::Complete {
                    app: crate::ObservedApplication {
                        id: window.app.clone(),
                        bundle_id: format!("com.example.Sim{}", window.window_id % 3),
                    },
                    focused: focused.clone().filter(|focused| open && *focused == window),
                    windows: open
                        .then(|| crate::ObservedWindow {
                            frame: rt.backend.frames[&window],
                            id: window,
                            resizable: true,
                            title: None,
                            document: None,
                        })
                        .into_iter()
                        .collect(),
                }
            })
            .collect();
        rt.backend.observation = Some(crate::NativeObservation {
            cycle: 0,
            mouse_down: false,
            applications,
        });
    }

    fn point(&mut self) -> (i32, i32) {
        let display = self.rng.pick(self.outputs.displays()).unwrap().frame;
        (
            display.x
                + i32::try_from(self.rng.below(display.width.unsigned_abs() as usize)).unwrap(),
            display.y
                + i32::try_from(self.rng.below(display.height.unsigned_abs() as usize)).unwrap(),
        )
    }

    /// One random step; returns what it did, for the failure message.
    fn step(&mut self, rt: &mut WorkspaceRuntime<FakeBackend>) -> String {
        let cancelled = AtomicBool::new(false);
        let outputs = self.outputs.clone();
        let window = self.rng.pick(&self.here());
        let action = match self.rng.below(24) {
            0..=2 => Some(WorkerAction::Retile),
            3 | 4 => Some(WorkerAction::SwitchWorkspace(self.rng.workspace())),
            5 => Some(WorkerAction::WorkspaceBackAndForth),
            6 => Some(WorkerAction::MoveFocused(self.rng.workspace())),
            7 => Some(WorkerAction::MoveFocusedFollow(self.rng.workspace())),
            8 => Some(WorkerAction::Focus(self.rng.direction())),
            9 => Some(WorkerAction::Reorder(self.rng.direction())),
            10 => Some(WorkerAction::ToggleFloat),
            11 => Some(WorkerAction::TogglePin),
            12 => Some(WorkerAction::Scratchpad(if self.rng.below(2) == 0 {
                ScratchpadCommand::Toggle
            } else {
                ScratchpadCommand::Mark
            })),
            13 => Some(
                [
                    WorkerAction::ToggleLayout,
                    WorkerAction::ToggleFullscreenLayout,
                    WorkerAction::Balance,
                    WorkerAction::Resize(ResizeDirection::Grow),
                    WorkerAction::ResizeTo(Axis::Horizontal, 400),
                    WorkerAction::CycleFocus,
                ][self.rng.below(6)]
                .clone(),
            ),
            14 => {
                let Some(window) = window else {
                    return "drop (no window)".into();
                };
                let point = self.point();
                let preview = self.rng.below(2) == 0;
                let gesture = WorkerCommand::Gesture(
                    FinalGesture::drop(window.clone(), point),
                    preview,
                    outputs,
                );
                rt.execute(gesture, &cancelled);
                return format!("drop {window:?} at {point:?} preview={preview}");
            }
            15 => {
                let Some(window) = window else {
                    return "resize (no window)".into();
                };
                let frame = rt.backend.frames[&window];
                let resized = crate::Rect {
                    width: (frame.width + i32::try_from(self.rng.below(300)).unwrap() - 150)
                        .max(50),
                    ..frame
                };
                let preview = self.rng.below(2) == 0;
                let gesture = WorkerCommand::Gesture(
                    FinalGesture::Resize(window.clone(), resized),
                    preview,
                    outputs,
                );
                rt.execute(gesture, &cancelled);
                return format!("resize {window:?} to {resized:?} preview={preview}");
            }
            16 => {
                self.outputs = match self.rng.below(3) {
                    0 => single_output(),
                    1 => super::outputs(),
                    _ => three_outputs(),
                };
                rt.note_outputs(&self.outputs);
                return format!("displays now {}", self.outputs.displays().len());
            }
            17 => {
                // A macOS Space slide carries one window to or from the other desktop.
                let Some(moved) = self.rng.pick(&self.open) else {
                    return "space slide (no window)".into();
                };
                if !self.away.remove(&moved) {
                    self.away.insert(moved.clone());
                    rt.backend.ordered_out.insert(moved.window_id);
                } else {
                    rt.backend.ordered_out.remove(&moved.window_id);
                }
                rt.backend.space_changed_at = Some(rt.backend.now());
                return format!("space slide moves {moved:?}");
            }
            18 => {
                let restored = rt.execute(WorkerCommand::RestoreAll(Some(outputs)), &cancelled);
                return format!("restore all: {:?}", restored.map(|_| ()));
            }
            19 => {
                if self.open.len() < 9 {
                    self.open_window(rt);
                }
                return "open window".into();
            }
            20 => {
                let Some(closed) = window.filter(|_| self.open.len() > 2) else {
                    return "close (kept)".into();
                };
                self.open.retain(|window| *window != closed);
                rt.backend.frames.remove(&closed);
                rt.backend.server_frames.remove(&closed);
                return format!("close {closed:?}");
            }
            21 => {
                if let Some(window) = window {
                    rt.backend.frontmost = Some(window.app.clone());
                    rt.backend.focused = Some(window);
                }
                return "outside focus change".into();
            }
            _ => {
                let by = Duration::from_millis(self.rng.next() % 1_500);
                rt.backend.advance(by);
                return format!("advance {by:?}");
            }
        };
        let action = action.unwrap();
        let label = format!("{action:?}");
        rt.execute(WorkerCommand::Action(action, outputs), &cancelled);
        label
    }
}

/// The invariants every step must keep.
fn check(rt: &WorkspaceRuntime<FakeBackend>) -> Result<(), String> {
    // One residency per window: a tile on one workspace, or one record.
    let mut tiled = HashMap::new();
    for workspace in WorkspaceId::all() {
        let engine = rt.workspaces.engine(workspace);
        for window in engine.bound_windows() {
            if let Some(other) = tiled.insert(window.clone(), workspace) {
                return Err(format!("{window:?} tiled on {other} and {workspace}"));
            }
        }
        // Layout tree integrity: its leaves are exactly the workspace's slots.
        engine
            .layout()
            .validate(&engine.model().workspace.slots)
            .map_err(|error| format!("workspace {workspace} layout: {error}"))?;
    }
    for (window, workspace) in rt.workspaces.floating_entries() {
        if let Some(tile) = tiled.get(window) {
            return Err(format!(
                "{window:?} floats on {workspace} and tiles on {tile}"
            ));
        }
    }
    let scratchpad = rt.workspaces.scratchpad();
    for window in tiled.keys() {
        if scratchpad.contains(window) {
            return Err(format!("{window:?} is tiled and in the scratchpad"));
        }
    }
    rt.workspaces
        .state()
        .validate()
        .map_err(|error| format!("persisted workspace state: {error}"))?;
    // No orphaned parks: a parked window belongs somewhere and its recovery
    // frame was persisted before it moved.
    for window in rt.parked.keys() {
        if rt.workspaces.workspace_of_any(window).is_none() && !scratchpad.contains(window) {
            return Err(format!("{window:?} is parked but owned by nothing"));
        }
        if !rt.backend.persisted.contains_key(window) {
            return Err(format!(
                "{window:?} is parked without a persisted recovery frame"
            ));
        }
    }
    // FocusTracker consistency.
    let history = rt.focus.history();
    if history.current.is_some() && history.current == history.previous {
        return Err(format!("focus history repeats {:?}", history.current));
    }
    if let super::super::focus::FocusPhase::InFlight { request, .. } = rt.focus.phase()
        && request.stale.contains(&Some(request.target.clone()))
    {
        return Err(format!(
            "{:?} is both the focus target and stale",
            request.target
        ));
    }
    Ok(())
}

/// The states a run reached, so a machine that stopped exercising one fails.
#[derive(Debug, Default)]
struct Coverage {
    tiled_on_two_workspaces: bool,
    parked: bool,
    floating: bool,
    pinned: bool,
    scratchpad: bool,
    slide_held: bool,
    three_displays: bool,
}

impl Coverage {
    fn note(&mut self, rt: &mut WorkspaceRuntime<FakeBackend>, world: &World) {
        let tiled = WorkspaceId::all()
            .filter(|workspace| !rt.workspaces.engine(*workspace).bound_windows().is_empty())
            .count();
        self.tiled_on_two_workspaces |= tiled >= 2;
        self.parked |= !rt.parked.is_empty();
        self.floating |= rt.workspaces.floating_entries().next().is_some();
        self.pinned |= world
            .open
            .iter()
            .any(|window| rt.workspaces.is_pinned(window));
        self.scratchpad |= world
            .open
            .iter()
            .any(|window| rt.workspaces.scratchpad().contains(window));
        self.slide_held |= rt.space_transition_until().is_some();
        self.three_displays |= world.outputs.displays().len() == 3;
    }

    fn complete(&self) -> bool {
        self.tiled_on_two_workspaces
            && self.parked
            && self.floating
            && self.pinned
            && self.scratchpad
            && self.slide_held
            && self.three_displays
    }
}

#[test]
fn thousands_of_random_steps_keep_every_core_invariant() {
    let started = std::time::Instant::now();
    for seed in SEEDS {
        let (mut world, mut rt) = World::new(seed);
        let mut coverage = Coverage::default();
        for step in 0..STEPS {
            world.observe(&mut rt);
            let op = world.step(&mut rt);
            if let Err(violation) = check(&rt) {
                panic!("seed {seed:#x}, step {step} ({op}): {violation}");
            }
            coverage.note(&mut rt, &world);
        }
        assert!(
            coverage.complete(),
            "seed {seed:#x} missed a state: {coverage:?}"
        );
    }
    eprintln!(
        "state machine: {} steps in {:?}",
        STEPS * SEEDS.len(),
        started.elapsed()
    );
}
