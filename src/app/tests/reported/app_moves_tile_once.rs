//! #723: an app that moves and resizes its tile once, shortly after Spaceman
//! wrote it, was frozen as a constrained tile and left overlapping its neighbor.
//! Reported by @coygeek.

use super::super::*;

#[test]
fn a_one_shot_app_move_after_a_write_is_written_back() {
    let (left, right) = (window(10), window(11));
    let initial = [(left.clone(), frame(50)), (right.clone(), frame(450))];
    let mut runtime = runtime(FakeBackend {
        frames: initial.iter().cloned().collect(),
        observation: Some(observation_for_windows(
            "com.example.AppKit",
            &left,
            &initial,
        )),
        ..FakeBackend::default()
    });
    let (outputs, cancelled, ws) = (outputs(), AtomicBool::new(false), WorkspaceId::ONE.index());

    runtime.cycle(outputs.clone(), &cancelled).unwrap();
    let target = runtime.backend.frames[&left];
    let neighbor = runtime.backend.frames[&right];
    assert!(runtime.applied[ws].contains_key(&left));
    let writes = runtime.backend.observed_apply_count;

    // Within the bounce window the app places the window itself, once: moved
    // and larger, over its neighbor.
    let displaced = crate::Rect {
        x: target.x + 48,
        y: target.y + 60,
        width: target.width + 40,
        height: target.height - 120,
    };
    runtime.backend.frames.insert(left.clone(), displaced);
    runtime.cycle(outputs.clone(), &cancelled).unwrap();

    assert_eq!(runtime.backend.observed_apply_count, writes + 1);
    assert_eq!(runtime.backend.frames[&left], target);
    assert_eq!(runtime.backend.frames[&right], neighbor);
    assert!(!runtime.constrained[ws].contains_key(&left));

    runtime.cycle(outputs, &cancelled).unwrap();
    assert_eq!(runtime.backend.frames[&left], target);
    assert!(!runtime.constrained[ws].contains_key(&left));
}

#[test]
fn an_app_that_keeps_moving_its_tile_settles_at_the_attempt_limit() {
    let (left, right) = (window(10), window(11));
    let initial = [(left.clone(), frame(50)), (right.clone(), frame(450))];
    let mut runtime = runtime(FakeBackend {
        frames: initial.iter().cloned().collect(),
        observation: Some(observation_for_windows(
            "com.example.AppKit",
            &left,
            &initial,
        )),
        ..FakeBackend::default()
    });
    let (outputs, cancelled, ws) = (outputs(), AtomicBool::new(false), WorkspaceId::ONE.index());

    runtime.cycle(outputs.clone(), &cancelled).unwrap();
    let target = runtime.backend.frames[&left];
    let first = runtime.backend.observed_apply_count;
    let displaced = crate::Rect {
        x: target.x + 48,
        y: target.y + 60,
        width: target.width + 40,
        ..target
    };
    for _ in 0..8 {
        runtime.backend.frames.insert(left.clone(), displaced);
        runtime.cycle(outputs.clone(), &cancelled).unwrap();
    }
    let limit = usize::from(super::super::super::cycle::APPLY_ATTEMPT_LIMIT);
    assert!(runtime.backend.observed_apply_count - first < limit);
    assert!(runtime.constrained[ws].contains_key(&left));
}
