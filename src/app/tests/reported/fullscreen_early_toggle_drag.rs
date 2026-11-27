//! #619: Leaving Fullscreen Layout just after a held title-bar drag began, before
//! the worker saw any preview of it, turned the release on another display into
//! a cross-display move. Reported by @coygeek.

use super::super::*;
use crate::app::input::GestureTracker;

fn rect(x: i32, y: i32, width: i32, height: i32) -> crate::Rect {
    crate::Rect {
        x,
        y,
        width,
        height,
    }
}

fn trio() -> macos::DisplayOutputs {
    let records = [
        ("center", rect(0, 30, 2560, 1410), true),
        ("left", rect(-1440, -583, 1440, 2530), false),
        ("right", rect(2560, -539, 1440, 2530), false),
    ];
    macos::DisplayOutputs::with_bounds(
        records
            .iter()
            .map(|&(id, frame, is_main)| macos::DisplayOutput {
                id: id.into(),
                name: id.into(),
                frame,
                is_main,
            })
            .collect(),
        records
            .iter()
            .map(|&(_, frame, _)| rect(frame.x, frame.y - 30, frame.width, frame.height + 30))
            .collect(),
    )
    .unwrap()
}

fn run(rt: &mut WorkspaceRuntime<FakeBackend>, command: WorkerCommand) {
    let Some(WorkerResult::Status { result, .. }) = rt.execute(command, &AtomicBool::new(false))
    else {
        panic!("expected worker status");
    };
    assert_eq!(result.unwrap().failed, 0);
}

fn action(rt: &mut WorkspaceRuntime<FakeBackend>, action: WorkerAction) {
    run(rt, WorkerCommand::Action(action, trio()));
}

/// Drags A1 from Fullscreen Layout to the right display, leaving the mode
/// mid-press, with the button going down before or after the toggle ran.
/// Returns A1's workspace and the frames of A1, A2 and C1 before and after.
fn early_toggle_drag(
    pressed_before_toggle: bool,
) -> (WorkspaceId, Vec<crate::Rect>, Vec<crate::Rect>) {
    let main = WorkspaceId::ONE;
    let right = WorkspaceId::new(9).unwrap();
    let (a1, a2, c1) = (window(10), window(11), window(90));
    let tiles = [
        (a1.clone(), rect(0, 30, 1280, 1410)),
        (a2.clone(), rect(1280, 30, 1280, 1410)),
        (c1.clone(), rect(2560, -539, 1440, 2530)),
    ];
    let backend = FakeBackend {
        frames: tiles.iter().cloned().collect(),
        observation: Some(observation_for_windows("com.example.Test", &a1, &tiles)),
        focused: Some(a1.clone()),
        frontmost: Some(a1.app.clone()),
        ..FakeBackend::default()
    };
    let config = Config::parse(concat!(
        "version = 1\n[settings]\nlayout = 'columns'\n",
        "[[workspaces]]\nworkspace = 9\nmonitor = 'right'\n",
    ))
    .unwrap();
    let mut rt = runtime_with_config(backend, config);
    bind(&mut rt, main, &a1);
    bind(&mut rt, main, &a2);
    bind(&mut rt, right, &c1);
    action(&mut rt, WorkerAction::Retile);
    let ids = [&a1, &a2, &c1];
    let split: Vec<_> = ids.iter().map(|id| rt.backend.frames[*id]).collect();
    action(&mut rt, WorkerAction::ToggleFullscreenLayout);
    assert!(rt.workspaces.deck(main).is_monocle());
    let start = rt.backend.frames[&a1];

    let from = (1280, 43);
    let mut tracker = GestureTracker::default();
    let press = |rt: &mut WorkspaceRuntime<FakeBackend>, tracker: &mut GestureTracker| {
        tracker.begin_pressed(
            vec![(a1.clone(), start)],
            Some(from),
            Some(rt.backend.now()),
        );
        rt.backend.mouse_down = true;
        rt.backend.advance(Duration::from_millis(50));
    };
    if pressed_before_toggle {
        press(&mut rt, &mut tracker);
    }
    // Option+Shift+F reaches the worker before any preview of the drag.
    action(&mut rt, WorkerAction::ToggleFullscreenLayout);
    assert!(!rt.workspaces.deck(main).is_monocle());
    rt.backend.advance(Duration::from_millis(10));
    if !pressed_before_toggle {
        press(&mut rt, &mut tracker);
    }
    let moved_by = |to: (i32, i32)| {
        rect(
            start.x + to.0 - from.0,
            start.y + to.1 - from.1,
            start.width,
            start.height,
        )
    };
    for to in [(2100, 410), (2920, 460)] {
        let frame = moved_by(to);
        rt.backend.frames.insert(a1.clone(), frame);
        rt.backend.pointer = Some(to);
        if let Some(preview) = tracker.preview(|_| Some(frame), Some(to)) {
            run(&mut rt, WorkerCommand::Gesture(preview, true, trio()));
        }
    }
    let to = (2920, 460);
    rt.backend.mouse_down = false;
    let release = tracker.finish(|_| Some(moved_by(to)), Some(to)).unwrap();
    run(&mut rt, WorkerCommand::Gesture(release, false, trio()));
    for _ in 0..3 {
        action(&mut rt, WorkerAction::Retile);
    }
    let after = ids.iter().map(|id| rt.backend.frames[*id]).collect();
    (rt.workspaces.workspace_of(&a1).unwrap(), split, after)
}

#[test]
fn early_fullscreen_exit_mid_drag_restores_the_split() {
    let (workspace, split, after) = early_toggle_drag(true);
    assert_eq!(workspace, WorkspaceId::ONE);
    assert_eq!(after, split);
}

#[test]
fn drag_pressed_after_leaving_fullscreen_still_moves_across_displays() {
    let (workspace, _, _) = early_toggle_drag(false);
    assert_eq!(workspace, WorkspaceId::new(9).unwrap());
}
