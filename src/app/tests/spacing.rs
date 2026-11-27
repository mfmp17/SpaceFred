use super::*;

/// The main display is 1000 × 800 at the origin.
fn spaced() -> Config {
    Config::parse(
        "version = 1\n[settings]\nsingle_window_max_width = 640\n\
         [settings.gaps]\ninner = 10\nleft = 20\ntop = 30\n",
    )
    .unwrap()
}

#[test]
fn layout_applies_gaps_padding_and_the_single_window_width() {
    let (left, right) = (window(10), window(11));
    let mut backend = FakeBackend::default();
    backend.frames.insert(left.clone(), frame(0));
    backend.frames.insert(right.clone(), frame(0));
    let mut runtime = runtime_with_config(backend, spaced());
    let output = outputs().main().frame;
    let cancelled = AtomicBool::new(false);

    bind(&mut runtime, WorkspaceId::ONE, &left);
    runtime.layout_workspace(WorkspaceId::ONE, output, &cancelled);
    let lone = runtime.backend.frames[&left];
    // Usable: x 20..1000, y 30..800; 640 wide centered in its 980.
    assert_eq!(
        lone,
        crate::Rect {
            x: 20 + (980 - 640) / 2,
            y: 30,
            width: 640,
            height: 770,
        }
    );

    bind(&mut runtime, WorkspaceId::ONE, &right);
    runtime.layout_workspace(WorkspaceId::ONE, output, &cancelled);
    let (a, b) = (
        runtime.backend.frames[&left],
        runtime.backend.frames[&right],
    );
    assert_eq!((a.x, a.y, a.height), (20, 30, 770));
    assert_eq!(b.x - (a.x + a.width), 10);
    assert_eq!(b.x + b.width, 1000);
    assert!((a.width - b.width).abs() <= 1);

    runtime
        .workspaces
        .set_deck(WorkspaceId::ONE, crate::workspace::DeckMode::Monocle);
    runtime.layout_workspace(WorkspaceId::ONE, output, &cancelled);
    assert_eq!(runtime.backend.frames[&left], lone);
    assert_eq!(runtime.backend.frames[&right], lone);
}

#[test]
fn layout_pads_the_edges_outer_covers_and_skips_the_one_set_to_zero() {
    let (left, right) = (window(10), window(11));
    let mut backend = FakeBackend::default();
    backend.frames.insert(left.clone(), frame(0));
    backend.frames.insert(right.clone(), frame(0));
    let config =
        Config::parse("version = 1\n[settings.gaps]\ninner = 6\nouter = 8\ntop = 0\n").unwrap();
    let mut runtime = runtime_with_config(backend, config);
    bind(&mut runtime, WorkspaceId::ONE, &left);
    bind(&mut runtime, WorkspaceId::ONE, &right);
    runtime.layout_workspace(
        WorkspaceId::ONE,
        outputs().main().frame,
        &AtomicBool::new(false),
    );
    let (a, b) = (
        runtime.backend.frames[&left],
        runtime.backend.frames[&right],
    );
    assert_eq!((a.x, a.y, a.width, a.height), (8, 0, 489, 792));
    assert_eq!((b.x, b.y, b.width, b.height), (503, 0, 489, 792));
}

#[test]
fn a_config_without_spacing_tiles_edge_to_edge() {
    let (left, right) = (window(10), window(11));
    let mut backend = FakeBackend::default();
    backend.frames.insert(left.clone(), frame(0));
    backend.frames.insert(right.clone(), frame(0));
    let mut runtime = runtime(backend);
    bind(&mut runtime, WorkspaceId::ONE, &left);
    bind(&mut runtime, WorkspaceId::ONE, &right);
    runtime.layout_workspace(
        WorkspaceId::ONE,
        outputs().main().frame,
        &AtomicBool::new(false),
    );
    let (a, b) = (
        runtime.backend.frames[&left],
        runtime.backend.frames[&right],
    );
    assert_eq!((a.x, a.width, b.x, b.width), (0, 500, 500, 500));
}

/// Mirrors `macos::apply::apply_element`'s tolerance short-circuit so tests
/// prove both `cycle`'s placement filter and the native writer apply small
/// spacing changes (e.g. 0 -> 4 pt inner gap, a 2 pt delta per tile).
pub(super) struct ToleranceAwareBackend(pub(super) FakeBackend);

impl WorkspaceBackend for ToleranceAwareBackend {
    fn now(&self) -> Instant {
        self.0.now()
    }
    fn pause(&mut self, duration: Duration) {
        self.0.pause(duration);
    }
    fn snapshot(
        &mut self,
        cycle: u64,
        outputs: &macos::DisplayOutputs,
    ) -> Result<crate::NativeObservation, String> {
        self.0.snapshot(cycle, outputs)
    }
    fn is_process_running(&mut self, app: &AppInstanceId) -> bool {
        self.0.is_process_running(app)
    }
    fn confirms_no_windows(&mut self, app: &AppInstanceId) -> bool {
        self.0.confirms_no_windows(app)
    }
    fn confirms_set_aside(&mut self, window: &crate::PhysicalWindowId) -> bool {
        self.0.confirms_set_aside(window)
    }
    fn frame(
        &mut self,
        window: &crate::PhysicalWindowId,
    ) -> Result<crate::Rect, macos::WindowAccessError> {
        self.0.frame(window)
    }
    fn window_server_frame(&mut self, window: &crate::PhysicalWindowId) -> Option<crate::Rect> {
        self.0.window_server_frame(window)
    }
    fn window_server_frames(&mut self) -> Option<HashMap<u32, crate::Rect>> {
        self.0.window_server_frames()
    }
    fn onscreen_window_frames(&mut self) -> Option<HashMap<u32, crate::Rect>> {
        self.0.onscreen_window_frames()
    }
    fn onscreen_window_order(&mut self) -> Option<Vec<u32>> {
        self.0.onscreen_window_order()
    }
    fn window_owners(&mut self, window_ids: &HashSet<u32>) -> HashMap<u32, AppInstanceId> {
        self.0.window_owners(window_ids)
    }
    fn set_position(
        &mut self,
        window: &crate::PhysicalWindowId,
        target: crate::Rect,
    ) -> macos::ApplyOutcome {
        self.0.set_position(window, target)
    }
    fn focused_window(&mut self) -> Option<crate::PhysicalWindowId> {
        self.0.focused_window()
    }
    fn frontmost_application(&mut self) -> Option<AppInstanceId> {
        self.0.frontmost_application()
    }
    fn raise_window(&mut self, window: &crate::PhysicalWindowId) -> bool {
        self.0.raise_window(window)
    }
    fn focus_window(&mut self, window: &crate::PhysicalWindowId) -> bool {
        self.0.focus_window(window)
    }
    fn detach_tab(&mut self, window: &crate::PhysicalWindowId) -> macos::TabDetachResult {
        self.0.detach_tab(window)
    }
    fn apply_managed(
        &mut self,
        placements: &[crate::Placement],
        settings: &Settings,
        cancelled: &AtomicBool,
    ) -> Vec<macos::ApplyResult> {
        placements
            .iter()
            .take_while(|_| !cancelled.load(Ordering::Acquire))
            .filter_map(|placement| {
                let window = placement.window.clone()?;
                let before = self.0.frames.get(&window).copied();
                if before.is_some_and(|frame| {
                    frame.approximately_equals(placement.frame, settings.frame_tolerance)
                }) {
                    return Some(macos::ApplyResult {
                        window,
                        target: placement.frame,
                        actual: before,
                        outcome: macos::ApplyOutcome::Applied,
                    });
                }
                self.0
                    .apply_managed(std::slice::from_ref(placement), settings, cancelled)
                    .into_iter()
                    .next()
            })
            .collect()
    }
    fn apply_managed_during_drag(
        &mut self,
        placements: &[crate::Placement],
        settings: &Settings,
        dragged: &crate::PhysicalWindowId,
        cancelled: &AtomicBool,
    ) -> Vec<macos::ApplyResult> {
        self.0
            .apply_managed_during_drag(placements, settings, dragged, cancelled)
    }
    fn persist_parked(
        &mut self,
        parked: &HashMap<crate::PhysicalWindowId, crate::Rect>,
    ) -> Result<(), String> {
        self.0.persist_parked(parked)
    }
    fn persist_workspace_state(
        &mut self,
        state: &crate::workspace::WorkspaceState,
    ) -> Result<(), String> {
        self.0.persist_workspace_state(state)
    }
    fn persist_window_memory(
        &mut self,
        memory: &crate::window_memory::WindowMemory,
    ) -> Result<(), String> {
        self.0.persist_window_memory(memory)
    }
    fn persist_workspace_monitor(
        &mut self,
        workspace: WorkspaceId,
        selector: &DisplaySelector,
    ) -> Result<(), String> {
        self.0.persist_workspace_monitor(workspace, selector)
    }
    fn journal(&mut self, line: &crate::journal::JournalLine) {
        self.0.journal(line);
    }
}

fn config_with_inner_gap(inner: i32) -> Config {
    Config::parse(&format!("version = 1\n[settings.gaps]\ninner = {inner}\n")).unwrap()
}

fn observe_windows(windows: &[&crate::PhysicalWindowId]) -> crate::NativeObservation {
    crate::NativeObservation {
        cycle: 1,
        mouse_down: false,
        applications: vec![crate::AppObservation::Complete {
            app: crate::ObservedApplication {
                id: windows[0].app.clone(),
                bundle_id: "com.example.Test".into(),
            },
            focused: Some(windows[0].clone()),
            windows: windows
                .iter()
                .map(|id| crate::ObservedWindow {
                    id: (*id).clone(),
                    frame: frame(0),
                    resizable: true,
                    title: None,
                    document: None,
                })
                .collect(),
        }],
    }
}

fn retile(runtime: &mut WorkspaceRuntime<ToleranceAwareBackend>) {
    let result = runtime.execute(
        WorkerCommand::Action(WorkerAction::Retile, outputs()),
        &AtomicBool::new(false),
    );
    let Some(WorkerResult::Status {
        result: Ok(status), ..
    }) = result
    else {
        panic!("retile failed");
    };
    assert_eq!(status.failed, 0);
}

#[test]
fn small_inner_gap_change_from_zero_to_four_separates_adjacent_tiles() {
    let (left, right) = (window(10), window(11));
    let mut fake = FakeBackend {
        observation: Some(observe_windows(&[&left, &right])),
        ..FakeBackend::default()
    };
    fake.frames.insert(left.clone(), frame(0));
    fake.frames.insert(right.clone(), frame(100));
    let mut runtime = WorkspaceRuntime::with_backend_and_config(
        ManagementScope::Both,
        ToleranceAwareBackend(fake),
        HashMap::new(),
        Config::default(),
    );

    // Baseline: zero inner gap tiles edge-to-edge at 500 + 500.
    retile(&mut runtime);
    let (a, b) = (
        runtime.backend.0.frames[&left],
        runtime.backend.0.frames[&right],
    );
    assert_eq!((a.x, a.width, b.x, b.width), (0, 500, 500, 500));
    assert_eq!(b.x - (a.x + a.width), 0);

    // Changing inner gap from 0 to 4 moves each tile by at most 2 pt (within default tolerance).
    runtime.replace_config(config_with_inner_gap(4));
    retile(&mut runtime);
    let (a, b) = (
        runtime.backend.0.frames[&left],
        runtime.backend.0.frames[&right],
    );
    assert_eq!((a.x, a.width, b.x, b.width), (0, 498, 502, 498));
    assert_eq!(b.x - (a.x + a.width), 4);

    // Control: 6-point gap separates by 6 points.
    runtime.replace_config(config_with_inner_gap(6));
    retile(&mut runtime);
    let (a, b) = (
        runtime.backend.0.frames[&left],
        runtime.backend.0.frames[&right],
    );
    assert_eq!((a.x, a.width, b.x, b.width), (0, 497, 503, 497));
    assert_eq!(b.x - (a.x + a.width), 6);

    // Returning to 0 restores edge-to-edge touching tiles.
    runtime.replace_config(config_with_inner_gap(0));
    retile(&mut runtime);
    let (a, b) = (
        runtime.backend.0.frames[&left],
        runtime.backend.0.frames[&right],
    );
    assert_eq!((a.x, a.width, b.x, b.width), (0, 500, 500, 500));
    assert_eq!(b.x - (a.x + a.width), 0);

    // Even a 1- or 2-point gap applies accurately from zero.
    for inner in [1, 2, 3, 4] {
        runtime.replace_config(config_with_inner_gap(0));
        retile(&mut runtime);
        runtime.replace_config(config_with_inner_gap(inner));
        retile(&mut runtime);
        let (a, b) = (
            runtime.backend.0.frames[&left],
            runtime.backend.0.frames[&right],
        );
        assert_eq!(b.x - (a.x + a.width), inner, "inner={inner}");
    }
}

#[test]
fn startup_with_small_inner_gap_separates_already_touching_windows() {
    let (left, right) = (window(10), window(11));
    let mut fake = FakeBackend {
        observation: Some(observe_windows(&[&left, &right])),
        ..FakeBackend::default()
    };
    fake.frames.insert(
        left.clone(),
        crate::Rect {
            x: 0,
            y: 0,
            width: 500,
            height: 800,
        },
    );
    fake.frames.insert(
        right.clone(),
        crate::Rect {
            x: 500,
            y: 0,
            width: 500,
            height: 800,
        },
    );
    let mut runtime = WorkspaceRuntime::with_backend_and_config(
        ManagementScope::Both,
        ToleranceAwareBackend(fake),
        HashMap::new(),
        config_with_inner_gap(4),
    );
    for (index, win) in [&left, &right].into_iter().enumerate() {
        runtime
            .workspaces
            .engine_mut(WorkspaceId::ONE)
            .insert_slot(Slot {
                id: SlotId((index + 1) as u64),
                binding: BindingState::Bound(win.clone()),
            });
    }

    retile(&mut runtime);
    let (a, b) = (
        runtime.backend.0.frames[&left],
        runtime.backend.0.frames[&right],
    );
    assert_eq!((a.x, a.width, b.x, b.width), (0, 498, 502, 498));
    assert_eq!(b.x - (a.x + a.width), 4);
}

/// #614: layout toggles while the button is held write nothing, so a small gap
/// change made during the drag must still reach the seam after release.
/// Reported by @coygeek.
#[test]
fn layout_toggles_during_a_held_drag_keep_a_small_gap_change_owed() {
    for toggles in [0, 2] {
        let (left, right) = (window(10), window(11));
        let mut fake = FakeBackend {
            observation: Some(observe_windows(&[&left, &right])),
            focused: Some(left.clone()),
            ..FakeBackend::default()
        };
        fake.frames.insert(left.clone(), frame(0));
        fake.frames.insert(right.clone(), frame(100));
        let mut runtime = WorkspaceRuntime::with_backend_and_config(
            ManagementScope::Both,
            ToleranceAwareBackend(fake),
            HashMap::new(),
            Config::default(),
        );
        retile(&mut runtime);
        let (a, b) = (
            runtime.backend.0.frames[&left],
            runtime.backend.0.frames[&right],
        );
        assert_eq!((a.x, a.width, b.x, b.width), (0, 500, 500, 500));

        // Held: the native writer defers every write.
        runtime.backend.0.mouse_down = true;
        runtime.backend.0.managed_outcome = Some(macos::ApplyOutcome::Deferred);
        runtime.replace_config(config_with_inner_gap(2));
        for _ in 0..toggles {
            let result = runtime.execute(
                WorkerCommand::Action(WorkerAction::ToggleLayout, outputs()),
                &AtomicBool::new(false),
            );
            assert!(matches!(
                result,
                Some(WorkerResult::Status { result: Ok(_), .. })
            ));
        }
        assert_eq!(runtime.backend.0.frames[&left], a);

        runtime.backend.0.mouse_down = false;
        runtime.backend.0.managed_outcome = None;
        retile(&mut runtime);
        let (a, b) = (
            runtime.backend.0.frames[&left],
            runtime.backend.0.frames[&right],
        );
        assert_eq!(
            (a.x, a.width, b.x, b.width),
            (0, 499, 501, 499),
            "toggles={toggles}"
        );
    }
}
