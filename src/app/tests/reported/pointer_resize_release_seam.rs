//! #506: A pointer resize released against a sibling's minimum can leave the
//! window a point or two short of its tile, a seam gap no later retile closed.
//! Reported by @coygeek.

use super::super::*;

fn one_output() -> macos::DisplayOutputs {
    macos::DisplayOutputs::new(vec![outputs().main().clone()]).unwrap()
}

fn dispatch(rt: &mut WorkspaceRuntime<FakeBackend>, command: WorkerCommand) {
    let Some(WorkerResult::Status { result, .. }) = rt.execute(command, &AtomicBool::new(false))
    else {
        panic!("expected worker status");
    };
    result.unwrap();
}

fn action(rt: &mut WorkspaceRuntime<FakeBackend>, action: WorkerAction) {
    dispatch(rt, WorkerCommand::Action(action, one_output()));
}

/// Release A's right edge at `sampled` width; the app then settles at `settled`.
fn release(sampled: i32, settled: i32) -> (crate::Rect, crate::Rect) {
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
        minimum_widths: [(b.clone(), 500)].into(),
        ..FakeBackend::default()
    };
    let mut rt = runtime(backend);
    bind(&mut rt, WorkspaceId::ONE, &a);
    bind(&mut rt, WorkspaceId::ONE, &b);
    // Learn B's minimum the way a constrained write reveals it, then settle at the seam.
    action(&mut rt, WorkerAction::Resize(ResizeDirection::Grow));
    assert_eq!(rt.minimum_sizes[&b].0, 500);
    action(&mut rt, WorkerAction::Balance);
    let start = rt.backend.frames[&a];
    assert_eq!((start.x, start.width), (0, 500));
    assert_eq!(rt.backend.frames[&b].x, 500);

    rt.backend.mouse_down = true;
    let preview = crate::Rect {
        width: 550,
        ..start
    };
    rt.backend.frames.insert(a.clone(), preview);
    dispatch(
        &mut rt,
        WorkerCommand::Gesture(FinalGesture::Resize(a.clone(), preview), true, one_output()),
    );

    rt.backend.mouse_down = false;
    let sample = crate::Rect {
        width: sampled,
        ..start
    };
    dispatch(
        &mut rt,
        WorkerCommand::Gesture(FinalGesture::Resize(a.clone(), sample), false, one_output()),
    );
    // The app settles a point or two away from the last sampled frame.
    rt.backend.frames.insert(
        a.clone(),
        crate::Rect {
            width: settled,
            ..start
        },
    );
    for _ in 0..3 {
        action(&mut rt, WorkerAction::Retile);
    }
    (rt.backend.frames[&a], rt.backend.frames[&b])
}

#[test]
fn pointer_resize_release_closes_a_sub_tolerance_seam_gap() {
    for (sampled, settled) in [(500, 498), (501, 498), (501, 499)] {
        let (a, b) = release(sampled, settled);
        println!("sampled={sampled}, settled={settled}: a={a:?}, b={b:?}");
        assert_eq!(b.x, 500, "sampled={sampled}, settled={settled}");
        assert_eq!(b.width, 500, "sampled={sampled}, settled={settled}");
        assert_eq!(a.x + a.width, b.x, "sampled={sampled}, settled={settled}");
    }
}
