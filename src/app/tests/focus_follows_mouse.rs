//! #475: the Focus follows mouse Lab focuses the tile the pointer rests on.

use super::*;

const LAB: &str = "version = 1\n[settings]\nmouse_follows_focus = 'display'\n\
                   [labs]\nfocus_follows_mouse = true\n";

fn hover(
    rt: &mut WorkspaceRuntime<FakeBackend>,
    window: &crate::PhysicalWindowId,
    point: (i32, i32),
) {
    hover_result(rt, window, point).unwrap();
}

fn hover_result(
    rt: &mut WorkspaceRuntime<FakeBackend>,
    window: &crate::PhysicalWindowId,
    point: (i32, i32),
) -> Result<(), String> {
    let action = WorkerAction::FocusHovered {
        window_id: window.window_id,
        point,
    };
    let Some(WorkerResult::Status { result, .. }) = rt.execute(
        WorkerCommand::Action(action, outputs()),
        &AtomicBool::new(false),
    ) else {
        panic!("missing worker status");
    };
    result.map(|_| ())
}

fn two_displays(config: &str, pointer: (i32, i32)) -> WorkspaceRuntime<FakeBackend> {
    let (main_tile, side_tile) = (window(10), window(60));
    let backend = FakeBackend {
        frames: HashMap::from([
            (main_tile.clone(), frame(50)),
            (side_tile.clone(), frame(1050)),
        ]),
        focused: Some(main_tile.clone()),
        frontmost: Some(main_tile.app.clone()),
        pointer: Some(pointer),
        ..FakeBackend::default()
    };
    let mut rt = runtime_with_config(backend, Config::parse(config).unwrap());
    bind(&mut rt, WorkspaceId::ONE, &main_tile);
    bind(&mut rt, WorkspaceId::SIX, &side_tile);
    rt
}

fn warped(rt: &WorkspaceRuntime<FakeBackend>) -> bool {
    rt.backend
        .calls
        .iter()
        .any(|call| matches!(call, BackendCall::WarpPointer(_)))
}

#[test]
fn a_resting_pointer_focuses_the_tile_under_it_without_moving() {
    let point = (1200, 200);
    let mut rt = two_displays(LAB, point);
    hover(&mut rt, &window(60), point);
    assert_eq!(rt.backend.focused, Some(window(60)));
    assert!(!warped(&rt));
}

#[test]
fn hover_changes_nothing_with_the_lab_off() {
    let point = (1200, 200);
    let mut rt = two_displays("version = 1\n", point);
    hover(&mut rt, &window(60), point);
    assert_eq!(rt.backend.focused, Some(window(10)));
}

#[test]
fn hover_is_dropped_once_the_pointer_moved_on_or_spaceman_put_it_there() {
    // The pointer left before the worker got to the hover.
    let mut rt = two_displays(LAB, (300, 200));
    hover(&mut rt, &window(60), (1200, 200));
    assert_eq!(rt.backend.focused, Some(window(10)));

    // Mouse follows focus warped it there: a warp is not a hover.
    let point = (1200, 200);
    let mut rt = two_displays(LAB, point);
    rt.focus.note_warp(point);
    hover(&mut rt, &window(60), point);
    assert_eq!(rt.backend.focused, Some(window(10)));

    // A held button (a drag, a context menu) holds focus where it is.
    let mut rt = two_displays(LAB, point);
    rt.backend.other_button_down = true;
    hover(&mut rt, &window(60), point);
    assert_eq!(rt.backend.focused, Some(window(10)));
}

#[test]
fn hover_never_focuses_a_window_that_is_not_a_visible_tile() {
    let point = (1200, 200);
    let mut rt = two_displays(LAB, point);
    // An unmanaged window (or a float, or the Scratchpad) is never a hover target.
    hover(&mut rt, &window(99), point);
    assert_eq!(rt.backend.focused, Some(window(10)));
}

#[test]
fn a_window_that_refuses_focus_is_left_quietly() {
    let point = (1200, 200);
    let mut rt = two_displays(LAB, point);
    rt.backend.focus_outcome = Some(false);
    assert_eq!(hover_result(&mut rt, &window(60), point), Ok(()));
    assert!(
        rt.backend
            .calls
            .iter()
            .any(|call| matches!(call, BackendCall::Focus(window) if window.window_id == 60))
    );
    assert_eq!(rt.backend.focused, Some(window(10)));
}

#[test]
fn the_last_warp_point_hovers_again_once_the_pointer_left_and_came_back() {
    // #522: mouse follows focus warped the pointer into the side tile.
    let warp = (1200, 200);
    let mut rt = two_displays(LAB, warp);
    rt.focus.note_warp(warp);
    // Resting where the warp left it is not a hover.
    hover(&mut rt, &window(60), warp);
    assert_eq!(rt.backend.focused, Some(window(10)));
    // The user moves onto the main tile and rests there, then comes back.
    rt.backend.pointer = Some((300, 200));
    hover(&mut rt, &window(10), (300, 200));
    rt.backend.pointer = Some(warp);
    hover(&mut rt, &window(60), warp);
    assert_eq!(rt.backend.focused, Some(window(60)));
}

#[test]
fn coming_straight_back_to_the_last_warp_point_hovers() {
    // #522: the pointer left the warp point without resting on another tile
    // (a quick move, or a rest over the desktop or the menu bar) and came back.
    let warp = (1200, 200);
    let mut rt = two_displays(LAB, warp);
    rt.focus.note_warp(warp);
    // The rest the warp itself caused is not a hover.
    hover(&mut rt, &window(60), warp);
    assert_eq!(rt.backend.focused, Some(window(10)));
    // The main thread reports that point again only after the pointer moved.
    hover(&mut rt, &window(60), warp);
    assert_eq!(rt.backend.focused, Some(window(60)));
}

#[test]
fn a_queued_hover_leaves_a_float_that_now_covers_the_point_in_front() {
    // #539: the hover queued while the side tile was in front; by the time the
    // worker ran it, a workspace switch had shown a float over that point.
    let point = (1200, 200);
    let mut rt = two_displays(LAB, point);
    rt.backend.window_in_front = Some(Some(99));
    hover(&mut rt, &window(60), point);
    assert_eq!(rt.backend.focused, Some(window(10)));
    assert!(
        !rt.backend
            .calls
            .iter()
            .any(|call| matches!(call, BackendCall::Raise(_) | BackendCall::Focus(_)))
    );
    // With the tile in front again, the next hover focuses it.
    rt.backend.window_in_front = None;
    hover(&mut rt, &window(60), point);
    assert_eq!(rt.backend.focused, Some(window(60)));
}

#[test]
fn leaving_the_warp_point_before_any_hover_lets_the_first_return_hover() {
    // Pointer warps to `warp`, moves away inside the 150 ms hover delay (so no
    // hover is ever delivered for the warp), then returns to `warp` and rests.
    let warp = (1200, 200);
    let mut rt = two_displays(LAB, warp);
    rt.backend.observation = Some(observation_for_windows(
        "com.example.Test",
        &window(10),
        &[(window(10), frame(50)), (window(60), frame(1050))],
    ));
    rt.focus.note_warp(warp);

    rt.backend.pointer = Some((300, 200));
    rt.cycle(outputs(), &AtomicBool::new(false)).unwrap();

    rt.backend.pointer = Some(warp);
    hover(&mut rt, &window(60), warp);
    assert_eq!(rt.backend.focused, Some(window(60)));
}
