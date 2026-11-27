//! #388: When an automatic fit stack has already been resized, dragging a tile while the pointer stays over its own starting tile makes its sibling jump back to the stack's original split.
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

#[test]
fn verify_stack_preview_preserves_resize() {
    let mut rt = pair(600);
    let a = window(10);
    let b = window(20);
    assert_eq!(rt.backend.frames[&a].x, rt.backend.frames[&b].x);
    let initial = rt.backend.frames[&b];
    action(
        &mut rt,
        WorkerAction::Resize(ResizeDirection::Grow),
        one_output(),
    );
    let resized = rt.backend.frames[&b];
    assert_ne!(initial, resized);
    let start = rt.backend.frames[&a];
    let mut tracker = GestureTracker::default();
    tracker.begin_at(vec![(a.clone(), start)], Some((250, 20)));
    rt.backend.mouse_down = true;
    let moved = crate::Rect {
        x: start.x + 50,
        ..start
    };
    rt.backend.frames.insert(a, moved);
    let preview_gesture = tracker.preview(|_| Some(moved), Some((300, 20))).unwrap();
    dispatch(
        &mut rt,
        WorkerCommand::Gesture(preview_gesture, true, one_output()),
    );
    let preview = rt.backend.frames[&b];
    rt.backend.mouse_down = false;
    let drop = tracker.finish(|_| Some(moved), Some((300, 20))).unwrap();
    dispatch(&mut rt, WorkerCommand::Gesture(drop, false, one_output()));
    let release = rt.backend.frames[&b];
    println!(
        "initial: {initial:?}; resized: {resized:?}; preview: {preview:?}; release: {release:?}"
    );
    assert_eq!(release, resized);
    assert_eq!(preview, resized);
}
