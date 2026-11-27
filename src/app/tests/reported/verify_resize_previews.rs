//! #387: A pointer resize changes the workspace's split as soon as its first preview runs.
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

fn resize_pause(preview: bool, release: bool) -> (i32, bool) {
    let mut rt = pair(0);
    let a = window(10);
    let b = window(20);
    let start = rt.backend.frames[&a];
    let state = rt.persisted_workspace_state.clone();
    let mut tracker = GestureTracker::default();
    tracker.begin_at(vec![(a.clone(), start)], Some((499, 350)));
    rt.backend.mouse_down = true;
    let current = crate::Rect {
        width: 620,
        ..start
    };
    rt.backend.frames.insert(a.clone(), current);
    if preview {
        let gesture = tracker
            .preview(|_| Some(current), Some((619, 350)))
            .unwrap();
        dispatch(&mut rt, WorkerCommand::Gesture(gesture, true, one_output()));
        assert_eq!(rt.backend.frames[&b].x, 620);
    }
    let was_saved_at_preview = rt.persisted_workspace_state != state;
    assert!(!was_saved_at_preview);
    if release {
        rt.backend.mouse_down = false;
        let gesture = tracker.finish(|_| Some(current), Some((619, 350))).unwrap();
        dispatch(
            &mut rt,
            WorkerCommand::Gesture(gesture, false, one_output()),
        );
    } else {
        tracker.clear();
    }
    let Some(WorkerResult::Restored { failed: 0, .. }) = rt.execute(
        WorkerCommand::RestoreAll(Some(one_output())),
        &AtomicBool::new(false),
    ) else {
        panic!("pause restoration must succeed");
    };
    rt.backend.mouse_down = false;
    action(&mut rt, WorkerAction::Retile, one_output());
    (
        rt.backend.frames[&b].x,
        rt.persisted_workspace_state != state,
    )
}

#[test]
fn verify_resize_previews_pause_discards_unreleased_resize() {
    let no_preview = resize_pause(false, false);
    let release = resize_pause(true, true);
    let paused = resize_pause(true, false);
    println!(
        "no preview and no release: {no_preview:?}; released: {release:?}; paused while held: {paused:?}"
    );
    assert_eq!(no_preview, (500, false));
    assert_eq!(release, (620, true));
    assert_eq!(paused, (500, false));
}

fn resize_three(final_width: i32, preview_width: Option<i32>) -> (Vec<i32>, Vec<i32>) {
    let a = window(10);
    let b = window(20);
    let c = window(30);
    let frames = [
        (
            a.clone(),
            crate::Rect {
                width: 333,
                height: 800,
                ..Default::default()
            },
        ),
        (
            b.clone(),
            crate::Rect {
                x: 333,
                width: 334,
                height: 800,
                ..Default::default()
            },
        ),
        (
            c.clone(),
            crate::Rect {
                x: 667,
                width: 333,
                height: 800,
                ..Default::default()
            },
        ),
    ];
    let backend = FakeBackend {
        frames: frames.iter().cloned().collect(),
        focused: Some(a.clone()),
        observation: Some(observation_for_windows("com.example.Tiles", &a, &frames)),
        minimum_widths: [(b.clone(), 300)].into(),
        ..FakeBackend::default()
    };
    let mut rt = runtime(backend);
    for (id, _) in &frames {
        bind(&mut rt, WorkspaceId::ONE, id);
    }
    action(
        &mut rt,
        WorkerAction::Resize(ResizeDirection::Grow),
        one_output(),
    );
    assert_eq!(rt.minimum_sizes[&b].0, 300);
    action(&mut rt, WorkerAction::Balance, one_output());
    assert_eq!(rt.backend.frames[&a].width, 334);
    let start = rt.backend.frames[&a];
    let mut tracker = GestureTracker::default();
    tracker.begin_at(vec![(a.clone(), start)], Some((333, 350)));
    if let Some(width) = preview_width {
        let preview = crate::Rect {
            width,
            ..rt.backend.frames[&a]
        };
        rt.backend.mouse_down = true;
        rt.backend.frames.insert(a.clone(), preview);
        let gesture = tracker
            .preview(|_| Some(preview), Some((width - 1, 350)))
            .unwrap();
        dispatch(&mut rt, WorkerCommand::Gesture(gesture, true, one_output()));
    }
    let current = crate::Rect {
        width: final_width,
        ..rt.backend.frames[&a]
    };
    rt.backend.mouse_down = false;
    rt.backend.frames.insert(a.clone(), current);
    let gesture = tracker
        .finish(|_| Some(current), Some((final_width - 1, 350)))
        .unwrap();
    dispatch(
        &mut rt,
        WorkerCommand::Gesture(gesture, false, one_output()),
    );
    action(&mut rt, WorkerAction::Retile, one_output());
    let final_frames = [&a, &b, &c].map(|id| rt.backend.frames[id].width).to_vec();
    let canvas = crate::Rect {
        width: 1400,
        ..one_output().main().frame
    };
    rt.layout_workspace(WorkspaceId::ONE, canvas, &AtomicBool::new(false));
    let wider_frames = [a, b, c]
        .iter()
        .map(|id| rt.backend.frames[id].width)
        .collect();
    (final_frames, wider_frames)
}

#[test]
fn verify_resize_previews_release_uses_starting_layout() {
    let direct = resize_three(343, None);
    let path = resize_three(343, Some(490));
    println!("direct: {direct:?}; after preview: {path:?}");
    assert_eq!(direct, path);
}
