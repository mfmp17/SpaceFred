//! #495: Leaving a display edge band and returning to the point its last preview
//! was shown at, while the worker is too busy to preview the excursion, docked
//! the window as that unarmed preview showed instead of into the edge column.
//! Reported by @coygeek.

use super::super::*;
use crate::app::input::GestureTracker;

fn dispatch(rt: &mut WorkspaceRuntime<FakeBackend>, command: WorkerCommand) {
    let Some(WorkerResult::Status { result, .. }) = rt.execute(command, &AtomicBool::new(false))
    else {
        panic!("expected worker status");
    };
    result.unwrap();
}

/// Drag the rightmost tile by a point inside the right edge band, preview it
/// there, optionally leave the band with no preview (busy worker), and release
/// back at the previewed point. Returns the dragged tile's final frame and its
/// siblings' final frames.
fn release_in_band(preview_first: bool, excursion: bool) -> (crate::Rect, Vec<crate::Rect>) {
    let outputs = macos::DisplayOutputs::new(vec![outputs().main().clone()]).unwrap();
    let ids = [window(10), window(20), window(30)];
    let backend = FakeBackend {
        frames: ids.iter().cloned().map(|id| (id, frame(50))).collect(),
        focused: Some(ids[2].clone()),
        ..FakeBackend::default()
    };
    let mut rt = runtime(backend);
    for id in &ids {
        bind(&mut rt, WorkspaceId::ONE, id);
    }
    rt.layout_workspace(
        WorkspaceId::ONE,
        outputs.main().frame,
        &AtomicBool::new(false),
    );
    let dragged = ids
        .iter()
        .max_by_key(|id| rt.backend.frames[*id].x)
        .unwrap()
        .clone();
    let start = rt.backend.frames[&dragged];
    let mut tracker = GestureTracker::default();
    tracker.begin_at(vec![(dragged.clone(), start)], Some((990, start.y + 5)));
    rt.backend.mouse_down = true;
    let current = crate::Rect {
        y: start.y + 200,
        ..start
    };
    rt.backend.frames.insert(dragged.clone(), current);
    let point = (990, start.y + 205);
    if preview_first {
        let preview = tracker.preview(|_| Some(current), Some(point)).unwrap();
        dispatch(
            &mut rt,
            WorkerCommand::Gesture(preview, true, outputs.clone()),
        );
    }
    if excursion {
        // Sampled on the main thread while the worker was busy: no preview.
        tracker.observe_point((600, point.1));
    }
    rt.backend.mouse_down = false;
    let release = tracker.finish(|_| Some(current), Some(point)).unwrap();
    dispatch(&mut rt, WorkerCommand::Gesture(release, false, outputs));
    let siblings = ids
        .iter()
        .filter(|id| **id != dragged)
        .map(|id| rt.backend.frames[id])
        .collect();
    (rt.backend.frames[&dragged], siblings)
}

#[test]
fn edge_return_while_busy_docks_like_an_unpreviewed_excursion() {
    let docked = release_in_band(false, true);
    let unarmed = release_in_band(true, false);
    assert_ne!(docked, unarmed, "leaving the band should arm the edge dock");
    // Docked: the full-height right half, with both siblings in the left half.
    let (column, siblings) = &docked;
    let (third, _) = &unarmed;
    assert_eq!(column.height, third.height);
    assert_eq!(column.y, third.y);
    assert_eq!(column.x + column.width, third.x + third.width);
    let left = siblings.iter().map(|frame| frame.x).min().unwrap();
    let right = siblings
        .iter()
        .map(|frame| frame.x + frame.width)
        .max()
        .unwrap();
    assert!(right <= column.x, "a sibling overlaps the docked column");
    assert!(
        (column.width - (right - left)).abs() <= 1,
        "the column is not half the tiled width: {column:?} beside {siblings:?}"
    );
    assert_eq!(
        release_in_band(true, true),
        docked,
        "the stale unarmed preview decided the drop"
    );
}
