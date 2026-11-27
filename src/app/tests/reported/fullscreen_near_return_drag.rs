//! #613: A title drag in Fullscreen Layout released a point or two from where
//! it started left the capped tile off-center, inside the write tolerance.
//! Reported by @coygeek.

use super::super::spacing::ToleranceAwareBackend;
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

fn run(rt: &mut WorkspaceRuntime<ToleranceAwareBackend>, command: WorkerCommand) {
    let Some(WorkerResult::Status { result, .. }) = rt.execute(command, &AtomicBool::new(false))
    else {
        panic!("expected worker status");
    };
    assert_eq!(result.unwrap().failed, 0);
}

fn action(rt: &mut WorkspaceRuntime<ToleranceAwareBackend>, action: WorkerAction) {
    run(rt, WorkerCommand::Action(action, trio()));
}

/// Drag A away and back to `held` points off its start, then release it there.
/// Returns A's start frame and A's and B's frames once settled.
fn release_offset_by(held: i32) -> (crate::Rect, crate::Rect, crate::Rect) {
    let workspace = WorkspaceId::new(8).unwrap();
    let a = window(10);
    let b = window(20);
    let tiles = [
        (a.clone(), rect(2600, -400, 900, 900)),
        (b.clone(), rect(2700, -300, 900, 900)),
    ];
    let backend = FakeBackend {
        frames: tiles.iter().cloned().collect(),
        observation: Some(observation_for_windows("com.example.Test", &a, &tiles)),
        focused: Some(a.clone()),
        frontmost: Some(a.app.clone()),
        ..FakeBackend::default()
    };
    let config = Config::parse(concat!(
        "version = 1\n[settings]\nsingle_window_max_width = 1100\n",
        "single_window_max_height = 1700\n[labs]\nsingle_window_height = true\n",
        "[[workspaces]]\nworkspace = 8\nmonitor = 'right'\n",
    ))
    .unwrap();
    let mut rt = WorkspaceRuntime::with_backend_and_config(
        ManagementScope::Both,
        ToleranceAwareBackend(backend),
        HashMap::new(),
        config,
    );
    for tile in [&a, &b] {
        rt.workspaces
            .engine_mut(workspace)
            .insert_slot(crate::Slot {
                id: crate::SlotId(u64::from(tile.window_id)),
                binding: crate::BindingState::Bound(tile.clone()),
            });
    }
    action(&mut rt, WorkerAction::Retile);
    action(&mut rt, WorkerAction::ToggleFullscreenLayout);
    assert!(rt.workspaces.deck(workspace).is_monocle());
    let start = rt.backend.0.frames[&a];

    let from = (start.x + 530, start.y + 13);
    let mut tracker = GestureTracker::default();
    tracker.begin_at(vec![(a.clone(), start)], Some(from));
    // Visibly moved while held, then brought back near the start.
    let far = rect(start.x + 100, start.y + 76, start.width, start.height);
    rt.backend.0.frames.insert(a.clone(), far);
    let preview = tracker
        .preview(|_| Some(far), Some((from.0 + 100, from.1 + 76)))
        .unwrap();
    run(&mut rt, WorkerCommand::Gesture(preview, true, trio()));
    let held_frame = rect(start.x + held, start.y + held, start.width, start.height);
    let to = (from.0 + held, from.1 + held);
    rt.backend.0.frames.insert(a.clone(), held_frame);
    let release = tracker.finish(|_| Some(held_frame), Some(to)).unwrap();
    run(&mut rt, WorkerCommand::Gesture(release, false, trio()));
    for _ in 0..3 {
        action(&mut rt, WorkerAction::Retile);
    }
    assert!(rt.workspaces.deck(workspace).is_monocle());
    (start, rt.backend.0.frames[&a], rt.backend.0.frames[&b])
}

#[test]
fn fullscreen_near_return_drag_restores_the_exact_cap() {
    for held in [2, 9] {
        let (start, a, b) = release_offset_by(held);
        println!("held={held}: start={start:?} a={a:?} b={b:?}");
        assert_eq!(start, rect(2730, -124, 1100, 1700), "held={held}");
        assert_eq!(a, start, "held={held}");
        assert_eq!(b, start, "held={held}");
    }
}
