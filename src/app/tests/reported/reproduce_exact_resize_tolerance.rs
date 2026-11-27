//! #412: An exact absolute width request can update the saved tile width and report zero failed placements without changing the window frame.
//! Reproduction contributed by @coygeek.

use super::super::*;

struct ToleranceAwareBackend(FakeBackend);

impl WorkspaceBackend for ToleranceAwareBackend {
    fn now(&self) -> std::time::Instant {
        self.0.now()
    }
    fn pause(&mut self, duration: Duration) {
        self.0.pause(duration);
    }
    fn window_owners(&mut self, window_ids: &HashSet<u32>) -> HashMap<u32, AppInstanceId> {
        self.0.window_owners(window_ids)
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

fn execute(rt: &mut WorkspaceRuntime<ToleranceAwareBackend>, action: WorkerAction) {
    let Some(WorkerResult::Status { result, .. }) = rt.execute(
        WorkerCommand::Action(action, outputs()),
        &AtomicBool::new(false),
    ) else {
        panic!("missing command status")
    };
    assert_eq!(result.unwrap().failed, 0);
}

#[test]
fn research_explicit_small_resize_updates_the_window_not_just_its_target() {
    let mut outcomes = Vec::new();
    for points in [503, 501, 502] {
        let windows = [window(1), window(2)];
        let frame_for = |x| crate::Rect {
            x,
            y: 0,
            width: 500,
            height: 800,
        };
        let initial = [
            (windows[0].clone(), frame_for(0)),
            (windows[1].clone(), frame_for(500)),
        ];
        let backend = FakeBackend {
            focused: Some(windows[0].clone()),
            observation: Some(observation_for_windows(
                "com.example.Test",
                &windows[0],
                &initial,
            )),
            frames: initial.into_iter().collect(),
            ..FakeBackend::default()
        };
        let mut rt = WorkspaceRuntime::with_backend(
            ManagementScope::Both,
            ToleranceAwareBackend(backend),
            HashMap::new(),
        );
        execute(&mut rt, WorkerAction::Retile);
        execute(
            &mut rt,
            WorkerAction::ResizeTo(crate::Axis::Horizontal, points),
        );
        execute(&mut rt, WorkerAction::Retile);
        let requested = rt
            .workspaces
            .engine(WorkspaceId::ONE)
            .placements(&Settings::for_output(outputs().main().frame))[0]
            .frame;
        let actual = rt.backend.0.frames[&windows[0]];
        println!(
            "points={points}, target_width={}, actual_width={}",
            requested.width, actual.width
        );
        assert_eq!(requested.width, points);
        outcomes.push((points, actual.width));
    }
    assert_eq!(outcomes, [(503, 503), (501, 501), (502, 502)]);
}
