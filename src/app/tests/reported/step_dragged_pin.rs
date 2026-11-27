//! #561: `switch next` from a pin dragged to another display steps the display the
//! pin is on (#606: even with its center in that display's menu bar strip), never the one
//! showing its home workspace; `move-workspace` keeps moving the pin's home. Reported by
//! @coygeek. #652: the same holds for an ordinary float dragged away while Dragged Floats
//! Follow Their Display is off, so it keeps its workspace.

use super::super::*;
use crate::WorkspaceStep;

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

/// Main landscape, a left portrait (`secondary-id`) and a right portrait (`third-id`).
fn portraits() -> macos::DisplayOutputs {
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

fn visible(rt: &WorkspaceRuntime<FakeBackend>, displays: &macos::DisplayOutputs) -> Vec<u8> {
    let mut shown: Vec<u8> = rt
        .visible_workspaces(displays)
        .iter()
        .filter_map(|resolved| resolved.workspace.number())
        .collect();
    shown.sort_unstable();
    shown
}

const CONFIG: &str = "version = 1
[settings]
layout = 'columns'
workspace_order = [6, 4, 5, 8, 7, 9]
[labs]
workspace_steps = true
[[workspaces]]
workspace = 4
monitor = 'secondary-id'
[[workspaces]]
workspace = 5
monitor = 'secondary-id'
[[workspaces]]
workspace = 6
monitor = 'secondary-id'
[[workspaces]]
workspace = 7
monitor = 'third-id'
[[workspaces]]
workspace = 8
monitor = 'third-id'
[[workspaces]]
workspace = 9
monitor = 'third-id'
";

const PIN: u32 = 40;

fn observe(
    rt: &mut WorkspaceRuntime<FakeBackend>,
    placed: &[(crate::PhysicalWindowId, crate::Rect)],
) {
    let applications = placed
        .iter()
        .flat_map(|(window, frame)| {
            let bundle = format!("com.example.Test{}", window.window_id);
            observation_for(&bundle, window, *frame).applications
        })
        .collect();
    rt.backend.observation = Some(crate::NativeObservation {
        cycle: 0,
        mouse_down: false,
        applications,
    });
}

/// Tiles on 1, 5, 8 and 9, and a pin from 4 dragged to `pin_frame`; 1, 4 and 7 show.
fn dragged_pin(pin_frame: crate::Rect) -> (WorkspaceRuntime<FakeBackend>, macos::DisplayOutputs) {
    dragged_float(pin_frame, true)
}

/// Tiles on 1, 5, 8 and 9, and a float (pinned or not) from 4 dragged to `frame`.
fn dragged_float(
    pin_frame: crate::Rect,
    pinned: bool,
) -> (WorkspaceRuntime<FakeBackend>, macos::DisplayOutputs) {
    let displays = portraits();
    let mut rt = runtime_with_config(FakeBackend::default(), Config::parse(CONFIG).unwrap());
    let mut placed = Vec::new();
    for (id, workspace) in [(10, 1), (50, 5), (80, 8), (90, 9), (PIN, 4)] {
        let window = win(id);
        let display = match workspace {
            4..=6 => 1,
            7..=9 => 2,
            _ => 0,
        };
        let output = displays.displays()[display].frame;
        let frame = rect(output.x + 200, output.y + 800, 500, 340);
        rt.backend.frames.insert(window.clone(), frame);
        if id == PIN {
            rt.workspaces
                .float(window.clone(), ws(4), crate::workspace::FloatCause::Manual);
            assert!(!pinned || rt.workspaces.pin(window.clone()));
        } else {
            bind(&mut rt, ws(workspace), &window);
        }
        placed.push((window, frame));
    }
    observe(&mut rt, &placed);
    rt.backend.focused = Some(win(10));
    rt.backend.frontmost = Some(win(10).app);
    execute(&mut rt, &displays, WorkerAction::Retile);
    for workspace in [7, 4] {
        execute(
            &mut rt,
            &displays,
            WorkerAction::SwitchWorkspace(ws(workspace)),
        );
    }
    assert_eq!(visible(&rt, &displays), [1, 4, 7]);

    // The pin, dragged and still key.
    rt.backend.frames.insert(win(PIN), pin_frame);
    rt.backend.server_frames.insert(win(PIN), pin_frame);
    if let Some(entry) = placed.iter_mut().find(|(window, _)| window == &win(PIN)) {
        entry.1 = pin_frame;
    }
    observe(&mut rt, &placed);
    rt.backend.focused = Some(win(PIN));
    rt.backend.frontmost = Some(win(PIN).app);
    (rt, displays)
}

#[test]
fn switch_next_from_a_pin_dragged_right_steps_the_right_display() {
    let (mut rt, displays) = dragged_pin(rect(2820, 100, 600, 400));
    execute(
        &mut rt,
        &displays,
        WorkerAction::StepWorkspace(WorkspaceStep::Next),
    );
    assert_eq!(visible(&rt, &displays), [1, 4, 9], "right 7 steps to 9");
    assert_eq!(rt.workspaces.workspace_of_any(&win(PIN)), Some(ws(4)));
}

#[test]
fn switch_next_from_a_pin_on_its_home_display_steps_that_display() {
    let (mut rt, displays) = dragged_pin(rect(-1150, 100, 600, 400));
    execute(
        &mut rt,
        &displays,
        WorkerAction::StepWorkspace(WorkspaceStep::Next),
    );
    assert_eq!(visible(&rt, &displays), [1, 5, 7], "left 4 steps to 5");
}

#[test]
fn move_workspace_from_a_dragged_pin_still_moves_its_home() {
    let (mut rt, displays) = dragged_pin(rect(2820, 100, 600, 400));
    let main = displays.selector_for(&displays.displays()[0]);
    execute(
        &mut rt,
        &displays,
        WorkerAction::MoveWorkspaceToDisplay(DisplayTarget::Selector(main)),
    );
    let moved = rt.backend.persisted_monitors.first().map(|moved| moved.0);
    assert_eq!(moved, Some(ws(4)), "only switch next|prev follows the pin");
}

/// #606: a pin dragged so its center sits in the right portrait's menu bar strip, above its
/// usable top, is still on the right display. Reported by @coygeek.
#[test]
fn switch_next_from_a_pin_centered_in_a_menu_strip_steps_that_display() {
    // Centers 11 points above and 29 points below the right portrait's usable top (y=-539).
    for y in [-660, -620] {
        let (mut rt, displays) = dragged_pin(rect(2930, y, 420, 220));
        execute(
            &mut rt,
            &displays,
            WorkerAction::StepWorkspace(WorkspaceStep::Next),
        );
        assert_eq!(
            visible(&rt, &displays),
            [1, 4, 9],
            "y={y}: right 7 steps to 9"
        );
        assert_eq!(rt.workspaces.workspace_of_any(&win(PIN)), Some(ws(4)));
    }
}

/// #652: an ordinary float dragged right keeps workspace 4; `switch next` steps the right
/// display it is on, and `move-workspace` still moves 4.
#[test]
fn switch_next_from_an_unpinned_float_dragged_right_steps_the_right_display() {
    let (mut rt, displays) = dragged_float(rect(2815, 103, 500, 340), false);
    assert!(!rt.workspaces.is_pinned(&win(PIN)));
    execute(
        &mut rt,
        &displays,
        WorkerAction::StepWorkspace(WorkspaceStep::Next),
    );
    assert_eq!(visible(&rt, &displays), [1, 4, 9], "right 7 steps to 9");
    assert_eq!(rt.workspaces.workspace_of_any(&win(PIN)), Some(ws(4)));

    let (mut rt, displays) = dragged_float(rect(2815, 103, 500, 340), false);
    let main = displays.selector_for(&displays.displays()[0]);
    execute(
        &mut rt,
        &displays,
        WorkerAction::MoveWorkspaceToDisplay(DisplayTarget::Selector(main)),
    );
    let moved = rt.backend.persisted_monitors.first().map(|moved| moved.0);
    assert_eq!(moved, Some(ws(4)));
}
