//! #534: a numbered switch that focuses one window of an app must not follow
//! the app's other, parked window when macOS hands focus to it instead.
//! Reported by @coygeek.

use super::super::*;

fn ws(n: u8) -> WorkspaceId {
    WorkspaceId::new(n).unwrap()
}

/// Windows of app `pid`.
fn win(pid: i32, id: u32) -> crate::PhysicalWindowId {
    crate::PhysicalWindowId {
        app: AppInstanceId {
            pid,
            launched_at: 1,
        },
        window_id: id,
    }
}

fn at(x: i32) -> crate::Rect {
    crate::Rect {
        x: x + 20,
        y: 20,
        width: 400,
        height: 300,
    }
}

fn app(pid: i32, windows: &[(u32, crate::Rect)]) -> crate::AppObservation {
    crate::AppObservation::Complete {
        app: crate::ObservedApplication {
            id: win(pid, 0).app,
            bundle_id: format!("com.example.App{pid}"),
        },
        focused: Some(win(pid, windows[0].0)),
        windows: windows
            .iter()
            .map(|(id, frame)| crate::ObservedWindow {
                id: win(pid, *id),
                frame: *frame,
                resizable: true,
                title: None,
                document: None,
            })
            .collect(),
    }
}

fn act(
    rt: &mut WorkspaceRuntime<FakeBackend>,
    displays: &macos::DisplayOutputs,
    action: WorkerAction,
) {
    let Some(WorkerResult::Status { result, .. }) = rt.execute(
        WorkerCommand::Action(action, displays.clone()),
        &AtomicBool::new(false),
    ) else {
        panic!("expected status");
    };
    assert_eq!(result.unwrap().failed, 0);
}

fn shown(rt: &WorkspaceRuntime<FakeBackend>, displays: &macos::DisplayOutputs) -> Vec<u8> {
    let mut shown = rt
        .visible_workspaces(displays)
        .iter()
        .map(|entry| entry.workspace.number().unwrap())
        .collect::<Vec<_>>();
    shown.sort_unstable();
    shown
}

fn report(rt: &mut WorkspaceRuntime<FakeBackend>, window: &crate::PhysicalWindowId) {
    rt.backend.focused = Some(window.clone());
    rt.backend.frontmost = Some(window.app.clone());
}

fn b1() -> crate::PhysicalWindowId {
    win(5, 31)
}

fn b2() -> crate::PhysicalWindowId {
    win(5, 81)
}

fn focus_calls(rt: &WorkspaceRuntime<FakeBackend>, window: &crate::PhysicalWindowId) -> usize {
    rt.backend
        .calls
        .iter()
        .filter(|call| matches!(call, BackendCall::Focus(focused) if focused == window))
        .count()
}

/// Workspaces 2, 4 and 7 shown, B1 parked on 3 and B2 on 8, then Option-3:
/// the switch's request to focus B1 is still in flight.
fn switched_to_three(displays: &macos::DisplayOutputs) -> WorkspaceRuntime<FakeBackend> {
    let config = Config::parse(
        "version = 1
[settings]
layout = 'columns'
[[workspaces]]
workspace = 2
monitor = 'main-id'
[[workspaces]]
workspace = 3
monitor = 'main-id'
[[workspaces]]
workspace = 4
monitor = 'secondary-id'
[[workspaces]]
workspace = 7
monitor = 'third-id'
[[workspaces]]
workspace = 8
monitor = 'third-id'
",
    )
    .unwrap();
    let (two, four, seven) = (win(2, 20), win(4, 40), win(7, 70));
    let (b1, b2) = (b1(), b2());
    let mut rt = runtime_with_config(FakeBackend::default(), config);
    for (window, workspace, x) in [
        (&two, 2, 0),
        (&four, 4, 1000),
        (&seven, 7, 2000),
        (&b1, 3, 0),
        (&b2, 8, 2000),
    ] {
        rt.backend.frames.insert(window.clone(), at(x));
        bind(&mut rt, ws(workspace), window);
    }
    rt.backend.observation = Some(crate::NativeObservation {
        cycle: 0,
        mouse_down: false,
        applications: vec![
            app(2, &[(20, at(0))]),
            app(4, &[(40, at(1000))]),
            app(7, &[(70, at(2000))]),
            app(5, &[(81, at(2000)), (31, at(0))]),
        ],
    });
    for (display, n) in [("main-id", 2), ("secondary-id", 4), ("third-id", 7)] {
        rt.workspaces.show_on(display, ws(n));
    }
    report(&mut rt, &seven);
    act(&mut rt, displays, WorkerAction::Retile);
    assert_eq!(shown(&rt, displays), [2, 4, 7]);

    act(&mut rt, displays, WorkerAction::SwitchWorkspace(ws(3)));
    assert_eq!(rt.backend.focused.as_ref(), Some(&b1));
    rt.backend.calls.clear();
    rt
}

#[test]
fn parked_sibling_takes_focus() {
    let displays = three_outputs();
    let mut rt = switched_to_three(&displays);
    let (b1, b2) = (b1(), b2());
    // The app activates with its last key window, parked on workspace 8.
    report(&mut rt, &b2);
    rt.backend.advance(Duration::from_millis(125));
    act(&mut rt, &displays, WorkerAction::Retile);
    assert_eq!(shown(&rt, &displays), [3, 4, 7]);
    assert_eq!(rt.backend.focused.as_ref(), Some(&b1));
    // The app insists: B1 is asked for once, and the display still stays.
    report(&mut rt, &b2);
    rt.backend.advance(Duration::from_millis(25));
    act(&mut rt, &displays, WorkerAction::Retile);
    assert_eq!(shown(&rt, &displays), [3, 4, 7]);
    assert_eq!(focus_calls(&rt, &b1), 1);

    // Once that settles, choosing the parked window still reveals its workspace.
    report(&mut rt, &b1);
    rt.backend.advance(super::super::super::focus::FOCUS_SETTLE);
    act(&mut rt, &displays, WorkerAction::Retile);
    report(&mut rt, &b2);
    act(&mut rt, &displays, WorkerAction::Retile);
    assert_eq!(shown(&rt, &displays), [3, 4, 8]);
    assert_eq!(rt.backend.focused.as_ref(), Some(&b2));
}

/// A window of the same app that no workspace owns yet (a new window, a
/// palette) keeps focus while the switch's request is in flight.
#[test]
fn unowned_sibling_keeps_focus() {
    let displays = three_outputs();
    let mut rt = switched_to_three(&displays);
    let fresh = win(5, 99);
    assert_eq!(rt.workspaces.workspace_of_any(&fresh), None);
    report(&mut rt, &fresh);
    rt.backend.advance(Duration::from_millis(50));
    act(&mut rt, &displays, WorkerAction::Retile);
    assert_eq!(rt.backend.focused.as_ref(), Some(&fresh));
    assert_eq!(focus_calls(&rt, &b1()), 0);
    assert_eq!(shown(&rt, &displays), [3, 4, 7]);
}

/// #737: the app activates late, handing focus to its parked window after the
/// switch's request settled on B1. With no input since, B1 is focused again
/// once and the display stays; an app that insists is followed after that.
/// Reported by @coygeek.
#[test]
fn late_parked_sibling_takes_focus() {
    let displays = three_outputs();
    let mut rt = switched_to_three(&displays);
    let (b1, b2) = (b1(), b2());
    // No key press or click for a minute.
    rt.backend.recent_input = Some(macos::RecentInput {
        key_down: 60.0,
        mouse_down: 60.0,
        modifiers_changed: 60.0,
        command_held: false,
    });
    rt.backend.advance(super::super::super::focus::FOCUS_SETTLE);
    act(&mut rt, &displays, WorkerAction::Retile);
    rt.backend.advance(Duration::from_millis(300));
    report(&mut rt, &b2);
    act(&mut rt, &displays, WorkerAction::Retile);
    assert_eq!(shown(&rt, &displays), [3, 4, 7]);
    assert_eq!(rt.backend.focused.as_ref(), Some(&b1));
    assert_eq!(focus_calls(&rt, &b1), 1);

    report(&mut rt, &b2);
    rt.backend.advance(super::super::super::focus::FOCUS_SETTLE);
    act(&mut rt, &displays, WorkerAction::Retile);
    assert_eq!(shown(&rt, &displays), [3, 4, 8]);
    assert_eq!(rt.backend.focused.as_ref(), Some(&b2));
}

/// A key press after the switch (⌘` to the parked window) is the user's
/// choice: the late report is followed.
#[test]
fn late_sibling_after_input_is_followed() {
    let displays = three_outputs();
    let mut rt = switched_to_three(&displays);
    let b2 = b2();
    rt.backend.advance(super::super::super::focus::FOCUS_SETTLE);
    act(&mut rt, &displays, WorkerAction::Retile);
    rt.backend.advance(Duration::from_millis(300));
    rt.backend.recent_input = Some(macos::RecentInput {
        key_down: 0.1,
        mouse_down: 60.0,
        modifiers_changed: 0.2,
        command_held: true,
    });
    report(&mut rt, &b2);
    act(&mut rt, &displays, WorkerAction::Retile);
    assert_eq!(shown(&rt, &displays), [3, 4, 8]);
    assert_eq!(rt.backend.focused.as_ref(), Some(&b2));
    assert_eq!(focus_calls(&rt, &b1()), 0);
}
