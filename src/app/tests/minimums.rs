use super::*;

fn tiled(windows: &[(crate::PhysicalWindowId, i32)]) -> WorkspaceRuntime<FakeBackend> {
    let mut backend = FakeBackend::default();
    for (window, minimum) in windows {
        backend.frames.insert(window.clone(), frame(0));
        if *minimum > 0 {
            backend.minimum_widths.insert(window.clone(), *minimum);
        }
    }
    let mut runtime = runtime(backend);
    for (window, _) in windows {
        bind(&mut runtime, WorkspaceId::ONE, window);
    }
    runtime
}

fn tile_frames(runtime: &WorkspaceRuntime<FakeBackend>) -> Vec<crate::Rect> {
    runtime
        .workspaces
        .engine(WorkspaceId::ONE)
        .placements(&Settings::for_output(outputs().main().frame))
        .into_iter()
        .map(|placement| placement.frame)
        .collect()
}

#[test]
fn a_window_that_refuses_its_tile_width_gets_a_wider_tile_from_its_neighbor() {
    let (left, middle, right) = (window(10), window(11), window(12));
    let mut runtime = tiled(&[(left.clone(), 0), (middle.clone(), 400), (right.clone(), 0)]);
    let output = outputs().main().frame;

    runtime.layout_workspace(WorkspaceId::ONE, output, &AtomicBool::new(false));

    let frames = tile_frames(&runtime);
    assert_eq!(frames[1].width, 400);
    assert!((299..=301).contains(&frames[0].width));
    assert_eq!(frames[1].x, frames[0].x + frames[0].width);
    assert_eq!(frames[2].x, frames[1].x + frames[1].width);
    assert_eq!(frames[2].x + frames[2].width, output.width);
    assert_eq!(runtime.backend.frames[&middle], frames[1]);
    assert!(!runtime.constrained[WorkspaceId::ONE.index()].contains_key(&middle));
}

#[test]
fn an_observation_cycle_fits_a_minimum_width_in_the_same_pass() {
    let (left, right) = (window(10), window(11));
    let mut backend = FakeBackend {
        observation: Some(observation_for_windows(
            "com.example.Test",
            &left,
            &[(left.clone(), frame(0)), (right.clone(), frame(500))],
        )),
        ..FakeBackend::default()
    };
    backend.frames.insert(left.clone(), frame(0));
    backend.frames.insert(right.clone(), frame(500));
    backend.minimum_widths.insert(left.clone(), 640);
    let mut runtime = runtime(backend);

    runtime.cycle(outputs(), &AtomicBool::new(false)).unwrap();

    let frames = tile_frames(&runtime);
    assert_eq!(frames[0].width, 640);
    assert_eq!(runtime.backend.frames[&right], frames[1]);
    assert_eq!(frames[1].x, 640);
}

#[test]
fn the_last_tile_grows_toward_its_leading_neighbor() {
    let (left, right) = (window(10), window(11));
    let mut runtime = tiled(&[(left.clone(), 0), (right.clone(), 700)]);
    let output = outputs().main().frame;

    runtime.layout_workspace(WorkspaceId::ONE, output, &AtomicBool::new(false));

    let frames = tile_frames(&runtime);
    assert_eq!(frames[0].width, 300);
    assert_eq!(frames[1].x, 300);
    assert_eq!(frames[1].width, 700);
    assert_eq!(runtime.backend.frames[&right], frames[1]);
}

#[test]
fn wide_neighbors_stack_temporarily_then_return_to_the_requested_row() {
    let (left, right) = (window(10), window(11));
    let mut runtime = tiled(&[(left.clone(), 600), (right.clone(), 600)]);
    let output = outputs().main().frame;
    let cancelled = AtomicBool::new(false);
    let before = runtime.workspaces.state();

    runtime.layout_workspace(WorkspaceId::ONE, output, &cancelled);
    runtime.layout_workspace(WorkspaceId::ONE, output, &cancelled);

    let a = runtime.backend.frames[&left];
    let b = runtime.backend.frames[&right];
    assert_eq!(
        (a.width, b.width, b.y),
        (output.width, output.width, a.y + a.height)
    );
    assert_eq!(runtime.workspaces.state(), before);

    let larger = crate::Rect {
        width: 1400,
        ..output
    };
    runtime.layout_workspace(WorkspaceId::ONE, larger, &cancelled);
    assert_eq!(runtime.backend.frames[&left].width, 700);
    assert_eq!(runtime.backend.frames[&right].x, larger.x + 700);
}

#[test]
fn several_minimums_are_fitted_together_from_the_tiles_with_room_to_spare() {
    let (first, second, third) = (window(10), window(11), window(12));
    let mut runtime = tiled(&[
        (first.clone(), 450),
        (second.clone(), 400),
        (third.clone(), 0),
    ]);

    runtime.layout_workspace(
        WorkspaceId::ONE,
        outputs().main().frame,
        &AtomicBool::new(false),
    );

    let widths = tile_frames(&runtime)
        .iter()
        .map(|frame| frame.width)
        .collect::<Vec<_>>();
    assert_eq!(widths, [450, 400, 150]);
    assert!(runtime.constrained[WorkspaceId::ONE.index()].is_empty());
}

#[test]
fn a_sibling_minimum_revealed_by_refitting_is_fitted_too() {
    let (first, second, third) = (window(10), window(11), window(12));
    let mut runtime = tiled(&[
        (first.clone(), 500),
        (second.clone(), 400),
        (third.clone(), 0),
    ]);
    let output = Settings::for_output(outputs().main().frame);
    // Start from [350, 450, 200]: only the first window refuses its tile.
    let frames = tile_frames(&runtime);
    let engine = runtime.workspaces.engine_mut(WorkspaceId::ONE);
    engine.resize_window(
        &first,
        crate::Rect {
            width: 350,
            ..frames[0]
        },
        &output,
        &HashMap::new(),
    );
    engine.resize_window(
        &second,
        crate::Rect {
            x: 350,
            width: 450,
            ..frames[1]
        },
        &output,
        &HashMap::new(),
    );
    assert_eq!(
        tile_frames(&runtime)
            .iter()
            .map(|frame| frame.width)
            .collect::<Vec<_>>(),
        [350, 450, 200]
    );

    runtime.layout_workspace(WorkspaceId::ONE, output.output, &AtomicBool::new(false));

    let widths = tile_frames(&runtime)
        .iter()
        .map(|frame| frame.width)
        .collect::<Vec<_>>();
    // Recompute from the requested [350, 450, 200] split with both
    // minimums known. The two donors contribute in proportion to their slack.
    assert_eq!(widths, [500, 412, 88]);
    assert!(runtime.constrained[WorkspaceId::ONE.index()].is_empty());
}

#[test]
fn fullscreen_layout_ignores_minimum_widths() {
    let (left, right) = (window(10), window(11));
    let mut runtime = tiled(&[(left.clone(), 0), (right.clone(), 600)]);
    runtime
        .workspaces
        .set_deck(WorkspaceId::ONE, crate::workspace::DeckMode::Monocle);
    let before = tile_frames(&runtime);

    runtime.layout_workspace(
        WorkspaceId::ONE,
        outputs().main().frame,
        &AtomicBool::new(false),
    );

    assert_eq!(tile_frames(&runtime), before);
}

#[test]
fn three_chat_windows_fit_the_laptop_without_changing_the_saved_split() {
    let ids = [window(10), window(11), window(12)];
    let mut runtime = tiled(&[
        (ids[0].clone(), 660),
        (ids[1].clone(), 300),
        (ids[2].clone(), 800),
    ]);
    let output = crate::Rect {
        x: 0,
        y: 32,
        width: 1728,
        height: 1085,
    };
    let before = runtime.workspaces.state();
    runtime.layout_workspace(WorkspaceId::ONE, output, &AtomicBool::new(false));
    let [a, b, c] = ids.each_ref().map(|w| runtime.backend.frames[w]);
    assert_eq!((a.x, a.y, a.height), (0, 32, 1085));
    assert_eq!((b.x, c.x), (a.width, a.width));
    assert_eq!((b.y + b.height, c.y + c.height), (c.y, 1117));
    assert_eq!((b.x + b.width, c.x + c.width), (1728, 1728));
    assert!(a.width >= 660 && b.width >= 300 && c.width >= 800);
    assert_eq!(before, runtime.workspaces.state());
}
