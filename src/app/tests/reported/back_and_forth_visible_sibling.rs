//! #591: Option+Tab from a pin focuses the previous workspace's window even
//! when its app hands focus to its other window, which another display
//! already shows. Reported by @coygeek.

use super::super::*;

fn ws(n: u8) -> WorkspaceId {
    WorkspaceId::new(n).unwrap()
}

/// B1 (10) and B2 (61) are windows of one app; every other window has its own.
fn win(n: u32) -> crate::PhysicalWindowId {
    let mut window = window(n);
    window.app.pid = if n == 61 { 10 } else { n as i32 };
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

fn side_by_side() -> macos::DisplayOutputs {
    let frames = [
        rect(0, 30, 2560, 1410),
        rect(-1440, -583, 1440, 2530),
        rect(2560, -539, 1440, 2530),
    ];
    let mut displays = three_outputs().displays().to_vec();
    for (display, frame) in displays.iter_mut().zip(frames) {
        display.frame = frame;
    }
    let bounds = frames.map(|frame| rect(frame.x, frame.y - 30, frame.width, frame.height + 30));
    macos::DisplayOutputs::with_bounds(displays, bounds.to_vec()).unwrap()
}

fn execute(
    rt: &mut WorkspaceRuntime<FakeBackend>,
    displays: &macos::DisplayOutputs,
    action: WorkerAction,
) {
    let Some(WorkerResult::Status { result, .. }) = rt.execute(
        WorkerCommand::Action(action, displays.clone()),
        &AtomicBool::new(false),
    ) else {
        panic!("missing worker status");
    };
    assert_eq!(result.unwrap().failed, 0);
}

fn report(rt: &mut WorkspaceRuntime<FakeBackend>, id: u32) {
    rt.backend.focused = Some(win(id));
    rt.backend.frontmost = Some(win(id).app);
}

fn visible(rt: &WorkspaceRuntime<FakeBackend>, displays: &macos::DisplayOutputs) -> Vec<u8> {
    let mut shown: Vec<u8> = rt
        .visible_workspaces(displays)
        .iter()
        .filter_map(|resolved| resolved.workspace.number())
        .collect();
    shown.sort_unstable();
    shown
}

fn focus_calls(rt: &WorkspaceRuntime<FakeBackend>, id: u32) -> usize {
    rt.backend
        .calls
        .iter()
        .filter(|call| matches!(call, BackendCall::Focus(window) if *window == win(id)))
        .count()
}

#[test]
fn back_and_forth_from_a_pin_reclaims_focus_from_a_visible_sibling() {
    let displays = side_by_side();
    let config = Config::parse(
        "version = 1\n[settings]\nlayout = 'columns'\n[[workspaces]]\nworkspace = 9\nmonitor = 'third-id'\n",
    )
    .unwrap();
    let mut rt = runtime_with_config(FakeBackend::default(), config);
    let mut applications = Vec::new();
    let mut app_b = Vec::new();
    // B1 on 1 and B2 on 6; pin 70 lives on workspace 7, which shares the
    // left display with 6.
    for (id, workspace) in [(10, 1), (60, 6), (61, 6), (90, 9), (70, 7)] {
        let window = win(id);
        let display = match workspace {
            6 | 7 => 1,
            9 => 2,
            _ => 0,
        };
        let output = displays.displays()[display].frame;
        let frame = rect(output.x + 200, output.y + 800, 500, 340);
        rt.backend.frames.insert(window.clone(), frame);
        if id == 70 {
            rt.workspaces
                .float(window.clone(), ws(7), crate::workspace::FloatCause::Manual);
            assert!(rt.workspaces.pin(window.clone()));
        } else {
            bind(&mut rt, ws(workspace), &window);
        }
        if matches!(id, 10 | 61) {
            app_b.push((window, frame));
        } else {
            applications.extend(
                observation_for(&format!("com.example.Test{id}"), &window, frame).applications,
            );
        }
    }
    applications
        .extend(observation_for_windows("com.example.TestB", &win(10), &app_b).applications);
    rt.backend.observation = Some(crate::NativeObservation {
        cycle: 0,
        mouse_down: false,
        applications,
    });
    report(&mut rt, 10);
    execute(&mut rt, &displays, WorkerAction::Retile);
    for workspace in [7, 1, 6] {
        execute(
            &mut rt,
            &displays,
            WorkerAction::SwitchWorkspace(ws(workspace)),
        );
    }
    assert_eq!(visible(&rt, &displays), [1, 6, 9]);
    report(&mut rt, 70);
    execute(&mut rt, &displays, WorkerAction::Retile);

    execute(&mut rt, &displays, WorkerAction::WorkspaceBackAndForth);
    assert_eq!(rt.backend.focused, Some(win(10)));
    rt.backend.calls.clear();
    // The app activates with B2, its last key window, on the left display.
    report(&mut rt, 61);
    rt.backend.advance(Duration::from_millis(50));
    execute(&mut rt, &displays, WorkerAction::Retile);
    assert_eq!(rt.backend.focused, Some(win(10)), "first press lands on B1");
    assert_eq!(focus_calls(&rt, 10), 1);
    assert_eq!(visible(&rt, &displays), [1, 6, 9]);

    rt.backend.advance(super::super::super::focus::FOCUS_SETTLE);
    execute(&mut rt, &displays, WorkerAction::Retile);
    execute(&mut rt, &displays, WorkerAction::WorkspaceBackAndForth);
    assert_eq!(
        rt.backend.focused,
        Some(win(70)),
        "second press returns to 6 and the pin it left there (#676)"
    );
    assert_eq!(visible(&rt, &displays), [1, 6, 9], "7 stays hidden");
}

/// Clicking the visible sibling once the request has settled still moves focus there.
#[test]
fn settled_click_on_a_visible_sibling_keeps_focus() {
    let displays = side_by_side();
    let mut rt = runtime(FakeBackend::default());
    for (id, workspace) in [(10, 1), (61, 6)] {
        bind(&mut rt, ws(workspace), &win(id));
    }
    rt.workspaces.switch(ws(6));
    rt.workspaces.switch(ws(1));
    assert!(rt.request_focus(&win(10), [None]));
    rt.backend.advance(super::super::super::focus::FOCUS_SETTLE);
    rt.backend.calls.clear();
    report(&mut rt, 61);
    let shown = rt.visible_workspaces(&displays);
    rt.reclaim_from_sibling(&win(61), &shown);
    assert_eq!(focus_calls(&rt, 10), 0);
    assert_eq!(rt.backend.focused, Some(win(61)));
}

/// #657: the sibling the app hands focus to is itself pinned on another
/// display. It counts as the workspace that display shows, so the requested
/// window is focused again; a pin on the target's own display keeps focus.
#[test]
fn a_pinned_sibling_on_another_display_is_reclaimed_once() {
    let displays = side_by_side();
    let config =
        Config::parse("version = 1\n[[workspaces]]\nworkspace = 9\nmonitor = 'third-id'\n")
            .unwrap();
    for (pin_display, reclaimed) in [(2, 1), (0, 0)] {
        let mut rt = runtime_with_config(FakeBackend::default(), config.clone());
        bind(&mut rt, ws(1), &win(10));
        rt.workspaces
            .float(win(61), ws(9), crate::workspace::FloatCause::Manual);
        assert!(rt.workspaces.pin(win(61)));
        let output = displays.displays()[pin_display].frame;
        rt.backend
            .frames
            .insert(win(61), rect(output.x + 260, output.y + 100, 560, 400));
        rt.workspaces.switch(ws(9));
        rt.workspaces.switch(ws(1));
        let shown = rt.visible_workspaces(&displays);
        assert!(shown.iter().any(|resolved| resolved.workspace == ws(9)));

        assert!(rt.request_focus(&win(10), [None]));
        rt.backend.calls.clear();
        report(&mut rt, 61);
        rt.backend.advance(Duration::from_millis(50));
        rt.reclaim_from_sibling(&win(61), &shown);
        rt.reclaim_from_sibling(&win(61), &shown);
        assert_eq!(
            focus_calls(&rt, 10),
            reclaimed,
            "pin on display {pin_display}"
        );
    }
}
