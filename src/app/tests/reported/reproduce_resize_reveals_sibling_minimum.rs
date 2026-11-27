//! #492: The first absolute resize missed a feasible width when a sibling first revealed its
//! minimum during the resize; the identical repeat reached it.
//! Reported by @coygeek.

use super::super::*;

const CENTER: crate::Rect = crate::Rect {
    x: 0,
    y: 30,
    width: 2560,
    height: 1410,
};
const LEFT: crate::Rect = crate::Rect {
    x: -1440,
    y: -583,
    width: 1440,
    height: 2530,
};
const RIGHT: crate::Rect = crate::Rect {
    x: 2560,
    y: -539,
    width: 1440,
    height: 2530,
};

fn displays() -> macos::DisplayOutputs {
    macos::DisplayOutputs::new(
        [("center", CENTER), ("left", LEFT), ("right", RIGHT)]
            .into_iter()
            .map(|(id, frame)| macos::DisplayOutput {
                id: id.into(),
                name: id.into(),
                frame,
                is_main: id == "center",
            })
            .collect(),
    )
    .unwrap()
}

fn ws(n: u8) -> WorkspaceId {
    WorkspaceId::new(n).unwrap()
}

fn command<B: WorkspaceBackend>(rt: &mut WorkspaceRuntime<B>, action: WorkerAction) -> CycleStatus {
    let Some(WorkerResult::Status { result, .. }) = rt.execute(
        WorkerCommand::Action(action, displays()),
        &AtomicBool::new(false),
    ) else {
        panic!("missing status")
    };
    result.unwrap()
}

fn new_rig(
    layout: &str,
    gap: i32,
    side: u8,
    count: u32,
    minimum: impl Fn(u32) -> i32,
) -> WorkspaceRuntime<FakeBackend> {
    let output = match side {
        6 => LEFT,
        9 => RIGHT,
        _ => CENTER,
    };
    let initial = crate::Rect {
        x: output.x + 100,
        y: output.y + 100,
        width: 800,
        height: 400,
    };
    let windows: Vec<_> = (1..=count).map(|i| (window(i), initial)).collect();
    let backend = FakeBackend {
        observation: Some(observation_for_windows(
            "com.example.Test",
            &window(1),
            &windows,
        )),
        focused: Some(window(1)),
        frames: windows.iter().cloned().collect(),
        minimum_widths: windows
            .iter()
            .filter_map(|(w, _)| {
                let minimum = minimum(w.window_id);
                (minimum > 0).then(|| (w.clone(), minimum))
            })
            .collect(),
        ..FakeBackend::default()
    };
    let config = Config::parse(&format!(
        "version = 1\n[settings]\nlayout = '{layout}'\n[settings.gaps]\ninner = {gap}\n[[workspaces]]\nworkspace = 6\nmonitor = 'left'\n[[workspaces]]\nworkspace = 9\nmonitor = 'right'\n"
    )).unwrap();
    let mut runtime = runtime_with_config(backend, config);
    assert_eq!(command(&mut runtime, WorkerAction::Retile).failed, 0);
    for i in 1..=count {
        assert_eq!(runtime.workspaces.workspace_of(&window(i)), Some(ws(side)));
    }
    runtime
}

#[test]
fn an_absolute_resize_lands_when_a_sibling_first_reveals_its_minimum() {
    let mut runtime = new_rig("columns", 7, 9, 3, |id| if id == 3 { 460 } else { 0 });
    let before = [window(1), window(2), window(3)].map(|w| runtime.backend.frames[&w].width);
    // The last window has never had to refuse its width, so its minimum is still unknown.
    assert!(before[2] >= 460, "{before:?}");
    assert!(runtime.minimum_sizes.is_empty());

    let status = command(
        &mut runtime,
        WorkerAction::ResizeTo(crate::Axis::Horizontal, 700),
    );

    assert_eq!(status.failed, 0);
    let after = [window(1), window(2), window(3)].map(|w| runtime.backend.frames[&w].width);
    assert_eq!(after, [700, 266, 460]);
}

/// #607: A relative grow that first revealed a sibling's minimum moved the untargeted seam; the
/// same grow with the minimum already known kept it. Reported by @coygeek.
#[test]
fn a_relative_grow_keeps_the_other_seam_when_a_sibling_first_reveals_its_minimum() {
    let grow = |learn_first: bool| {
        let mut runtime = new_rig("columns", 0, 9, 3, |id| if id == 3 { 460 } else { 0 });
        let frames = |rt: &WorkspaceRuntime<FakeBackend>| {
            [window(1), window(2), window(3)].map(|w| rt.backend.frames[&w])
        };
        assert_eq!(frames(&runtime).map(|f| f.width), [480, 480, 480]);
        if learn_first {
            runtime.backend.focused = Some(window(3));
            command(
                &mut runtime,
                WorkerAction::ResizeTo(crate::Axis::Horizontal, 370),
            );
            command(&mut runtime, WorkerAction::Balance);
            assert_eq!(frames(&runtime).map(|f| f.width), [480, 480, 480]);
            assert!(!runtime.minimum_sizes.is_empty());
        } else {
            assert!(runtime.minimum_sizes.is_empty());
        }
        runtime.backend.focused = Some(window(2));
        let status = command(&mut runtime, WorkerAction::Resize(ResizeDirection::Grow));
        assert_eq!(status.failed, 0);
        frames(&runtime)
    };

    let known = grow(true);
    let first = grow(false);
    assert_eq!(known.map(|f| f.width), [480, 500, 460]);
    assert_eq!(first, known);
    assert_eq!(first[1].x, 3040);
    assert_eq!(first[2].x + first[2].width, RIGHT.x + RIGHT.width);
}

struct RecordingToleranceBackend {
    inner: FakeBackend,
    tolerances: Vec<i32>,
}

impl WorkspaceBackend for RecordingToleranceBackend {
    fn now(&self) -> std::time::Instant {
        self.inner.now()
    }
    fn pause(&mut self, duration: Duration) {
        self.inner.pause(duration);
    }
    fn window_owners(&mut self, window_ids: &HashSet<u32>) -> HashMap<u32, AppInstanceId> {
        self.inner.window_owners(window_ids)
    }
    fn snapshot(
        &mut self,
        cycle: u64,
        outputs: &macos::DisplayOutputs,
    ) -> Result<crate::NativeObservation, String> {
        self.inner.snapshot(cycle, outputs)
    }
    fn is_process_running(&mut self, app: &AppInstanceId) -> bool {
        self.inner.is_process_running(app)
    }
    fn confirms_no_windows(&mut self, app: &AppInstanceId) -> bool {
        self.inner.confirms_no_windows(app)
    }
    fn confirms_set_aside(&mut self, window: &crate::PhysicalWindowId) -> bool {
        self.inner.confirms_set_aside(window)
    }
    fn frame(
        &mut self,
        window: &crate::PhysicalWindowId,
    ) -> Result<crate::Rect, macos::WindowAccessError> {
        self.inner.frame(window)
    }
    fn window_server_frame(&mut self, window: &crate::PhysicalWindowId) -> Option<crate::Rect> {
        self.inner.window_server_frame(window)
    }
    fn window_server_frames(&mut self) -> Option<HashMap<u32, crate::Rect>> {
        self.inner.window_server_frames()
    }
    fn onscreen_window_frames(&mut self) -> Option<HashMap<u32, crate::Rect>> {
        self.inner.onscreen_window_frames()
    }
    fn onscreen_window_order(&mut self) -> Option<Vec<u32>> {
        self.inner.onscreen_window_order()
    }
    fn set_position(
        &mut self,
        window: &crate::PhysicalWindowId,
        target: crate::Rect,
    ) -> macos::ApplyOutcome {
        self.inner.set_position(window, target)
    }
    fn focused_window(&mut self) -> Option<crate::PhysicalWindowId> {
        self.inner.focused_window()
    }
    fn frontmost_application(&mut self) -> Option<AppInstanceId> {
        self.inner.frontmost_application()
    }
    fn raise_window(&mut self, window: &crate::PhysicalWindowId) -> bool {
        self.inner.raise_window(window)
    }
    fn focus_window(&mut self, window: &crate::PhysicalWindowId) -> bool {
        self.inner.focus_window(window)
    }
    fn detach_tab(&mut self, window: &crate::PhysicalWindowId) -> macos::TabDetachResult {
        self.inner.detach_tab(window)
    }
    fn apply_managed(
        &mut self,
        placements: &[crate::Placement],
        settings: &Settings,
        cancelled: &AtomicBool,
    ) -> Vec<macos::ApplyResult> {
        self.tolerances.push(settings.frame_tolerance);
        self.inner.apply_managed(placements, settings, cancelled)
    }
    fn apply_managed_during_drag(
        &mut self,
        placements: &[crate::Placement],
        settings: &Settings,
        dragged: &crate::PhysicalWindowId,
        cancelled: &AtomicBool,
    ) -> Vec<macos::ApplyResult> {
        self.inner
            .apply_managed_during_drag(placements, settings, dragged, cancelled)
    }
    fn apply_observed(
        &mut self,
        placements: &[crate::Placement],
        settings: &Settings,
        cancelled: &AtomicBool,
    ) -> Vec<macos::ApplyResult> {
        self.inner.apply_observed(placements, settings, cancelled)
    }
    fn persist_parked(
        &mut self,
        parked: &HashMap<crate::PhysicalWindowId, crate::Rect>,
    ) -> Result<(), String> {
        self.inner.persist_parked(parked)
    }
    fn persist_workspace_state(
        &mut self,
        state: &crate::workspace::WorkspaceState,
    ) -> Result<(), String> {
        self.inner.persist_workspace_state(state)
    }
    fn persist_window_memory(
        &mut self,
        memory: &crate::window_memory::WindowMemory,
    ) -> Result<(), String> {
        self.inner.persist_window_memory(memory)
    }
    fn persist_workspace_monitor(
        &mut self,
        workspace: WorkspaceId,
        selector: &DisplaySelector,
    ) -> Result<(), String> {
        self.inner.persist_workspace_monitor(workspace, selector)
    }
    fn journal(&mut self, line: &crate::journal::JournalLine) {
        self.inner.journal(line);
    }
    fn pointer_location(&mut self) -> Option<(i32, i32)> {
        self.inner.pointer_location()
    }
    fn hover_window_at(&mut self, point: (i32, i32)) -> Option<u32> {
        self.inner.hover_window_at(point)
    }
    fn mouse_down(&mut self) -> bool {
        self.inner.mouse_down()
    }
    fn any_mouse_button_down(&mut self) -> bool {
        self.inner.any_mouse_button_down()
    }
    fn warp_pointer(&mut self, point: (i32, i32)) -> bool {
        self.inner.warp_pointer(point)
    }
    fn recent_input(&mut self) -> Option<crate::macos::RecentInput> {
        self.inner.recent_input()
    }
}

#[test]
fn exact_spacing_stays_zero_through_minimum_refit_passes() {
    let initial = crate::Rect {
        x: RIGHT.x + 100,
        y: RIGHT.y + 100,
        width: 800,
        height: 400,
    };
    let windows: Vec<_> = (1..=3).map(|i| (window(i), initial)).collect();
    let backend = RecordingToleranceBackend {
        inner: FakeBackend {
            observation: Some(observation_for_windows(
                "com.example.Test",
                &window(1),
                &windows,
            )),
            focused: Some(window(1)),
            frames: windows.iter().cloned().collect(),
            minimum_widths: [(window(3), 460)].into_iter().collect(),
            ..FakeBackend::default()
        },
        tolerances: Vec::new(),
    };
    let config = Config::parse(
        "version = 1
[settings]
layout = 'columns'
[settings.gaps]
inner = 7
[[workspaces]]
workspace = 9
monitor = 'right'
",
    )
    .unwrap();
    let mut rt = WorkspaceRuntime::with_backend_and_config(
        ManagementScope::Both,
        backend,
        HashMap::new(),
        config,
    );
    assert_eq!(command(&mut rt, WorkerAction::Retile).failed, 0);
    rt.backend.tolerances.clear();

    let status = command(
        &mut rt,
        WorkerAction::ResizeTo(crate::Axis::Horizontal, 700),
    );
    assert_eq!(status.failed, 0);
    // Both the initial apply and every minimum-refit pass keep frame_tolerance == 0.
    assert!(
        rt.backend.tolerances.len() >= 2,
        "{:?}",
        rt.backend.tolerances
    );
    assert!(
        rt.backend.tolerances.iter().all(|&tol| tol == 0),
        "refit pass dropped exact spacing: {:?}",
        rt.backend.tolerances
    );
    assert!(!rt.exact_spacing_workspaces.contains(&ws(9)));
}
