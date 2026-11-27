//! #528: a numbered move while a local swap was previewed, then a release at
//! the same point, moved the tile back to the workspace it had left.
//! Reported by @coygeek.

use super::super::*;
use crate::app::input::GestureTracker;

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

fn displays() -> macos::DisplayOutputs {
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

fn dispatch(rt: &mut WorkspaceRuntime<FakeBackend>, command: WorkerCommand) {
    let Some(WorkerResult::Status { result, .. }) = rt.execute(command, &AtomicBool::new(false))
    else {
        panic!("expected worker status");
    };
    result.unwrap();
}

/// Tiles A (70) and B (71) on workspace 7 (right display) and one tile on
/// workspace 4 (left display). Drag A onto B's center, preview the swap, then
/// move A to workspace 4. `release_first` releases the swap before the move.
fn run(release_first: bool) -> WorkspaceRuntime<FakeBackend> {
    let displays = displays();
    let config = Config::parse(
        "version = 1\n[settings]\nlayout = 'columns'\n\
         [[workspaces]]\nworkspace = 4\nmonitor = 'secondary'\n\
         [[workspaces]]\nworkspace = 7\nmonitor = 'third-id'\n",
    )
    .unwrap();
    let mut rt = runtime_with_config(FakeBackend::default(), config);
    let mut applications = Vec::new();
    for (id, workspace, output) in [(70, 7, 2), (71, 7, 2), (40, 4, 1)] {
        let window = win(id);
        let output = displays.displays()[output].frame;
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
    rt.workspaces.show_on("secondary-id", ws(4));
    rt.workspaces.show_on("third-id", ws(7));
    rt.backend.focused = Some(win(70));
    rt.backend.frontmost = Some(win(70).app);
    dispatch(
        &mut rt,
        WorkerCommand::Action(WorkerAction::Retile, displays.clone()),
    );
    let shown: Vec<_> = rt
        .visible_workspaces(&displays)
        .into_iter()
        .map(|resolved| (resolved.display_id, resolved.workspace))
        .collect();
    assert!(
        shown.contains(&("secondary-id".into(), ws(4)))
            && shown.contains(&("third-id".into(), ws(7))),
        "workspace 4 on the left display and 7 on the right: {shown:?}"
    );
    let a = rt.backend.frames[&win(70)];
    let b = rt.backend.frames[&win(71)];
    assert!(a.x < b.x, "A starts left of B: {a:?} {b:?}");

    let grab = (a.x + a.width / 2, a.y + 16);
    let point = (b.x + b.width / 2, b.y + b.height / 2);
    let mut tracker = GestureTracker::default();
    tracker.begin_at(vec![(win(70), a)], Some(grab));
    rt.backend.mouse_down = true;
    let carried = rect(
        a.x + point.0 - grab.0,
        a.y + point.1 - grab.1,
        a.width,
        a.height,
    );
    rt.backend.frames.insert(win(70), carried);
    let preview = tracker.preview(|_| Some(carried), Some(point)).unwrap();
    dispatch(
        &mut rt,
        WorkerCommand::Gesture(preview, true, displays.clone()),
    );
    let move_to_four = || WorkerCommand::Action(WorkerAction::MoveFocused(ws(4)), displays.clone());
    if !release_first {
        dispatch(&mut rt, move_to_four());
        assert_eq!(rt.workspaces.workspace_of(&win(70)), Some(ws(4)));
    }
    rt.backend.mouse_down = false;
    let release = tracker.finish(|_| Some(carried), Some(point)).unwrap();
    dispatch(
        &mut rt,
        WorkerCommand::Gesture(release, false, displays.clone()),
    );
    if release_first {
        let a = rt.backend.frames[&win(70)];
        let b = rt.backend.frames[&win(71)];
        assert!(b.x < a.x, "the ordinary swap still lands: {a:?} {b:?}");
        rt.backend.focused = Some(win(70));
        dispatch(&mut rt, move_to_four());
    }
    rt
}

#[test]
fn held_swap_numbered_move_keeps_the_move() {
    for release_first in [false, true] {
        let rt = run(release_first);
        let left = displays().displays()[1].frame;
        let right = displays().displays()[2].frame;
        let a = rt.backend.frames[&win(70)];
        let b = rt.backend.frames[&win(71)];
        assert_eq!(rt.workspaces.workspace_of(&win(70)), Some(ws(4)));
        assert_eq!(rt.workspaces.workspace_of(&win(71)), Some(ws(7)));
        assert!(left.contains(a.x, a.y), "A is on the left display: {a:?}");
        assert!(
            right.contains(b.x, b.y) && b.width > right.width / 2,
            "B fills workspace 7 alone: {b:?}"
        );
    }
}
