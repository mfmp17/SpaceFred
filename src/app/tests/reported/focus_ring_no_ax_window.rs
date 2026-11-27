//! #398: focus next keeps a focus request in flight when Accessibility reports no window.
//! Reproduction contributed by @coygeek.

use super::super::*;

fn ws(n: u8) -> WorkspaceId {
    WorkspaceId::new(n).unwrap()
}

fn win(n: u32) -> crate::PhysicalWindowId {
    let mut window = window(n);
    window.app.pid = n as i32;
    window
}

fn rect(x: i32, y: i32, width: i32, height: i32) -> crate::Rect {
    crate::Rect {
        x,
        y,
        width,
        height,
    }
}

fn trio(frames: [crate::Rect; 3]) -> macos::DisplayOutputs {
    let mut displays = three_outputs().displays().to_vec();
    for (display, frame) in displays.iter_mut().zip(frames) {
        display.frame = frame;
    }
    let bounds = frames.map(|frame| rect(frame.x, frame.y - 30, frame.width, frame.height + 30));
    macos::DisplayOutputs::with_bounds(displays, bounds.to_vec()).unwrap()
}

fn side_by_side() -> macos::DisplayOutputs {
    trio([
        rect(0, 30, 2560, 1410),
        rect(-1440, -583, 1440, 2530),
        rect(2560, -539, 1440, 2530),
    ])
}

fn execute(
    rt: &mut WorkspaceRuntime<FakeBackend>,
    displays: &macos::DisplayOutputs,
    action: WorkerAction,
) -> CycleStatus {
    let Some(WorkerResult::Status { result, .. }) = rt.execute(
        WorkerCommand::Action(action, displays.clone()),
        &AtomicBool::new(false),
    ) else {
        panic!("missing worker status");
    };
    let status = result.unwrap();
    assert_eq!(status.failed, 0);
    status
}

fn report(rt: &mut WorkspaceRuntime<FakeBackend>, id: u32) {
    rt.backend.focused = Some(win(id));
    rt.backend.frontmost = Some(win(id).app);
}

fn setup(
    displays: &macos::DisplayOutputs,
    settings: &str,
    windows: &[(u32, u8)],
    focus: u32,
) -> WorkspaceRuntime<FakeBackend> {
    let config = Config::parse(&format!(
        "version = 1\n[settings]\nlayout = 'columns'\n{settings}\n[[workspaces]]\nworkspace = 9\nmonitor = 'third-id'\n"
    ))
    .unwrap();
    let mut rt = runtime_with_config(FakeBackend::default(), config);
    let mut applications = Vec::new();
    for &(id, workspace) in windows {
        let window = win(id);
        let output = match workspace {
            6 => displays.displays()[1].frame,
            9 => displays.displays()[2].frame,
            _ => displays.main().frame,
        };
        let frame = rect(output.x + 20, output.y + 20, 400, 300);
        rt.backend.frames.insert(window.clone(), frame);
        bind(&mut rt, ws(workspace), &window);
        applications
            .extend(observation_for(&format!("com.example.Test{id}"), &window, frame).applications);
    }
    rt.backend.observation = Some(crate::NativeObservation {
        cycle: 0,
        mouse_down: false,
        applications,
    });
    report(&mut rt, focus);
    execute(&mut rt, displays, WorkerAction::Retile);
    rt
}

#[test]
fn focus_ring_no_ax_window() {
    let displays = side_by_side();
    let mut result = Vec::new();
    for report_target in [true, false] {
        let mut rt = setup(&displays, "", &[(10, 1), (11, 1), (60, 6), (90, 9)], 60);
        execute(&mut rt, &displays, WorkerAction::Focus(Direction::Right));
        assert_eq!(rt.backend.focused, Some(win(10)));
        assert_eq!(rt.backend.frontmost, Some(win(10).app));
        if !report_target {
            rt.backend.focused = None;
        }
        assert!(rt.focus.in_flight(rt.backend.now()).is_some());
        let Some(WorkerResult::Status { result: status, .. }) = rt.execute(
            WorkerCommand::Action(WorkerAction::CycleFocus, displays.clone()),
            &AtomicBool::new(false),
        ) else {
            panic!("missing worker status");
        };
        let focused = rt.backend.focused.as_ref().map(|window| window.window_id);
        println!(
            "target report={report_target} status={:?} focused={focused:?}",
            status.map(|s| s.failed)
        );
        result.push(focused);
    }
    assert_eq!(result, [Some(11), Some(11)]);
}
