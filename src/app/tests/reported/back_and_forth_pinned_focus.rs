//! #526: Option+Tab from a focused pin counts the workspace its display shows,
//! never the pin's hidden home. Reported by @coygeek.

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

#[test]
fn back_and_forth_from_a_pin_skips_its_hidden_home() {
    let displays = side_by_side();
    let config = Config::parse(
        "version = 1\n[settings]\nlayout = 'columns'\n[[workspaces]]\nworkspace = 9\nmonitor = 'third-id'\n",
    )
    .unwrap();
    let mut rt = runtime_with_config(FakeBackend::default(), config);
    let mut applications = Vec::new();
    // Pin 70 lives on workspace 7; workspaces 6 and 7 share the left display.
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
        applications
            .extend(observation_for(&format!("com.example.Test{id}"), &window, frame).applications);
    }
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

    // The pin, focused on its display while 6 shows there.
    report(&mut rt, 70);
    execute(&mut rt, &displays, WorkerAction::Retile);
    assert_eq!(visible(&rt, &displays), [1, 6, 9]);

    execute(&mut rt, &displays, WorkerAction::WorkspaceBackAndForth);
    assert_eq!(rt.backend.focused, Some(win(10)), "first press goes to 1");
    assert_eq!(visible(&rt, &displays), [1, 6, 9]);

    execute(&mut rt, &displays, WorkerAction::WorkspaceBackAndForth);
    assert_eq!(
        rt.backend.focused,
        Some(win(70)),
        "second press returns to 6 and refocuses the pin it left there (#676)"
    );
    assert_eq!(visible(&rt, &displays), [1, 6, 9], "7 stays hidden");
    assert!(!rt.parked.contains_key(&win(70)), "the pin stays on screen");
}

#[test]
fn back_and_forth_from_a_dragged_pin_counts_its_current_display() {
    let displays = side_by_side();
    let config = Config::parse(
        "version = 1
[settings]
layout = 'columns'
[[workspaces]]
workspace = 9
monitor = 'third-id'
",
    )
    .unwrap();
    let mut rt = runtime_with_config(FakeBackend::default(), config);
    let mut applications = Vec::new();
    // Pin 70 lives on workspace 7 (left display); workspaces 1 (main), 6 (left), 9 (right).
    for (id, workspace) in [(10, 1), (60, 6), (90, 9), (70, 7)] {
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
        applications
            .extend(observation_for(&format!("com.example.Test{id}"), &window, frame).applications);
    }
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

    // Drag pin 70 from its home (left display) onto main (showing 1) and focus it.
    let main_frame = rect(400, 300, 500, 340);
    rt.backend.frames.insert(win(70), main_frame);
    rt.backend.server_frames.insert(win(70), main_frame);
    report(&mut rt, 70);

    // Pin 70 is seen on main (workspace 1), so Option+Tab jumps directly to 6
    // rather than focusing a tile on 1 where the pin already sits.
    execute(&mut rt, &displays, WorkerAction::WorkspaceBackAndForth);
    assert_eq!(rt.backend.focused, Some(win(60)), "first press goes to 6");
    assert_eq!(visible(&rt, &displays), [1, 6, 9]);

    execute(&mut rt, &displays, WorkerAction::WorkspaceBackAndForth);
    assert_eq!(
        rt.backend.focused,
        Some(win(70)),
        "second press returns to 1 and the pin it left there (#676)"
    );
    assert_eq!(visible(&rt, &displays), [1, 6, 9]);
}

/// #676: the return press refocuses a pin dragged onto another display, not
/// that display's tile, while the pin's home stays hidden.
#[test]
fn back_and_forth_returns_to_a_dragged_pin_on_another_display() {
    let displays = side_by_side();
    let config = Config::parse(
        "version = 1\n[settings]\nlayout = 'columns'\n[[workspaces]]\nworkspace = 9\nmonitor = 'third-id'\n",
    )
    .unwrap();
    let mut rt = runtime_with_config(FakeBackend::default(), config);
    let mut applications = Vec::new();
    // Pin 70 lives on workspace 7; workspaces 6 and 7 share the left display.
    for (id, workspace) in [(10, 1), (60, 6), (90, 9), (70, 7)] {
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
        applications
            .extend(observation_for(&format!("com.example.Test{id}"), &window, frame).applications);
    }
    rt.backend.observation = Some(crate::NativeObservation {
        cycle: 0,
        mouse_down: false,
        applications,
    });
    report(&mut rt, 10);
    execute(&mut rt, &displays, WorkerAction::Retile);
    for workspace in [7, 6, 1] {
        execute(
            &mut rt,
            &displays,
            WorkerAction::SwitchWorkspace(ws(workspace)),
        );
    }
    assert_eq!(visible(&rt, &displays), [1, 6, 9]);

    // Drag pin 70 onto the right display (showing 9) and focus it there.
    let right_frame = rect(2840, 144, 560, 380);
    rt.backend.frames.insert(win(70), right_frame);
    rt.backend.server_frames.insert(win(70), right_frame);
    report(&mut rt, 70);

    execute(&mut rt, &displays, WorkerAction::WorkspaceBackAndForth);
    assert_eq!(rt.backend.focused, Some(win(10)), "first press goes to 1");
    execute(&mut rt, &displays, WorkerAction::WorkspaceBackAndForth);
    assert_eq!(
        rt.backend.focused,
        Some(win(70)),
        "second press refocuses the pin, not 9's tile"
    );
    assert_eq!(visible(&rt, &displays), [1, 6, 9], "7 stays hidden");
    assert_eq!(rt.workspaces.workspace_of_any(&win(70)), Some(ws(7)));
    assert!(!rt.parked.contains_key(&win(70)), "the pin stays on screen");
}

#[test]
fn empty_workspace_focus_engages_behind_a_focused_pin() {
    let displays = side_by_side();
    let config = Config::parse(
        "version = 1
[settings]
layout = 'columns'
[labs]
empty_workspace_focus = true
[[workspaces]]
workspace = 4
monitor = 'secondary-id'
[[workspaces]]
workspace = 9
monitor = 'third-id'
",
    )
    .unwrap();
    let mut rt = runtime_with_config(FakeBackend::default(), config);
    let main_frame = rect(200, 200, 500, 340);
    let left_frame = rect(-1200, 200, 500, 340);
    let pin_frame = rect(400, 400, 400, 300);
    for (id, frame) in [(10, main_frame), (60, left_frame), (50, pin_frame)] {
        rt.backend.frames.insert(win(id), frame);
    }
    bind(&mut rt, ws(1), &win(10));
    bind(&mut rt, ws(6), &win(60));
    // Pin 50's home is hidden workspace 5, but it shows on main (workspace 1).
    rt.workspaces
        .float(win(50), ws(5), crate::workspace::FloatCause::Manual);
    assert!(rt.workspaces.pin(win(50)));
    let mut applications = Vec::new();
    for (id, frame) in [(10, main_frame), (60, left_frame), (50, pin_frame)] {
        applications.extend(
            observation_for(&format!("com.example.Test{id}"), &win(id), frame).applications,
        );
    }
    rt.backend.observation = Some(crate::NativeObservation {
        cycle: 0,
        mouse_down: false,
        applications,
    });
    report(&mut rt, 50);
    execute(&mut rt, &displays, WorkerAction::Retile);

    // Switch to empty workspace 4 on secondary while pin 50 keeps focus on main.
    execute(&mut rt, &displays, WorkerAction::SwitchWorkspace(ws(4)));
    assert_eq!(
        rt.focus.empty_choice().map(|choice| choice.workspace),
        Some(ws(4)),
        "empty workspace 4 should be chosen when a focused pin sits on main"
    );
    // Commands must see no focused window instead of acting on the pin.
    let Some(WorkerResult::Status { result, .. }) = rt.execute(
        WorkerCommand::Action(WorkerAction::MoveFocused(ws(2)), displays.clone()),
        &AtomicBool::new(false),
    ) else {
        panic!("expected worker status");
    };
    assert!(
        result.is_err(),
        "move should refuse on the chosen empty workspace"
    );
    assert_eq!(rt.workspaces.workspace_of_any(&win(50)), Some(ws(5)));
}
