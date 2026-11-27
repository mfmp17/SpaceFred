//! #389: Pausing automatic tiling during a live swap preview discards the gesture and leaves the saved split unchanged.
//! Reproduction contributed by @coygeek.

use super::super::*;
use crate::app::input::GestureTracker;

fn one_output() -> macos::DisplayOutputs {
    macos::DisplayOutputs::new(vec![outputs().main().clone()]).unwrap()
}

fn pair(minimum: i32) -> WorkspaceRuntime<FakeBackend> {
    let a = window(10);
    let b = window(20);
    let frames = [
        (
            a.clone(),
            crate::Rect {
                width: 500,
                height: 800,
                ..Default::default()
            },
        ),
        (
            b.clone(),
            crate::Rect {
                x: 500,
                width: 500,
                height: 800,
                ..Default::default()
            },
        ),
    ];
    let backend = FakeBackend {
        frames: frames.iter().cloned().collect(),
        focused: Some(a.clone()),
        observation: Some(observation_for_windows("com.example.Tiles", &a, &frames)),
        minimum_widths: if minimum > 0 {
            [(a.clone(), minimum), (b.clone(), minimum)].into()
        } else {
            HashMap::new()
        },
        ..FakeBackend::default()
    };
    let mut rt = runtime(backend);
    bind(&mut rt, WorkspaceId::ONE, &a);
    bind(&mut rt, WorkspaceId::ONE, &b);
    for _ in 0..2 {
        rt.layout_workspace(
            WorkspaceId::ONE,
            one_output().main().frame,
            &AtomicBool::new(false),
        );
    }
    action(&mut rt, WorkerAction::Retile, one_output());
    rt
}

fn action(
    rt: &mut WorkspaceRuntime<FakeBackend>,
    action: WorkerAction,
    outputs: macos::DisplayOutputs,
) {
    dispatch(rt, WorkerCommand::Action(action, outputs));
}

fn dispatch(rt: &mut WorkspaceRuntime<FakeBackend>, command: WorkerCommand) {
    let Some(WorkerResult::Status { result, .. }) = rt.execute(command, &AtomicBool::new(false))
    else {
        panic!("expected worker status");
    };
    result.unwrap();
}

fn drop_pause(preview: bool) -> (crate::Rect, crate::Rect, bool) {
    let mut rt = pair(0);
    let (a, b) = (window(10), window(20));
    let start = rt.backend.frames[&a];
    let sibling_start = rt.backend.frames[&b];
    let persisted = rt.persisted_workspace_state.clone();
    let mut tracker = GestureTracker::default();
    tracker.begin_at(vec![(a.clone(), start)], Some((250, 20)));
    rt.backend.mouse_down = true;
    let moved = crate::Rect {
        x: 500,
        y: 380,
        ..start
    };
    rt.backend.frames.insert(a.clone(), moved);
    if preview {
        let gesture = tracker.preview(|_| Some(moved), Some((750, 400))).unwrap();
        dispatch(&mut rt, WorkerCommand::Gesture(gesture, true, one_output()));
        assert_eq!(rt.backend.frames[&b], start);
    }
    tracker.clear();
    let Some(WorkerResult::Restored { failed: 0, .. }) = rt.execute(
        WorkerCommand::RestoreAll(Some(one_output())),
        &AtomicBool::new(false),
    ) else {
        panic!("pause restoration must succeed");
    };
    rt.backend.mouse_down = false;
    assert!(tracker.finish(|_| Some(moved), Some((750, 400))).is_none());
    (
        sibling_start,
        rt.backend.frames[&b],
        rt.persisted_workspace_state == persisted,
    )
}

#[test]
fn verify_paused_drop_restores_sibling() {
    let no_preview = drop_pause(false);
    let preview = drop_pause(true);
    println!(
        "(start, paused, saved state unchanged): no preview {no_preview:?}; preview {preview:?}"
    );
    assert_eq!(no_preview.0, no_preview.1);
    assert!(preview.2);
    assert_eq!(preview.0, preview.1);
}
