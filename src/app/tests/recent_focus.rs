//! `[labs] recent_focus` (#374): of several tiles bordering the focused one,
//! directional focus goes to the most recently focused.

use super::super::focus::FOCUS_SETTLE;
use super::*;

fn config(recent_focus: bool) -> Config {
    Config::parse(&format!(
        "version = 1\n[settings]\nlayout = \"columns\"\n[labs]\nrecent_focus = {recent_focus}\n"
    ))
    .unwrap()
}

/// `1 | (2 / 3)` on the main display, tile 2 focused.
fn stacked_pair(
    recent_focus: bool,
) -> (WorkspaceRuntime<FakeBackend>, [crate::PhysicalWindowId; 3]) {
    let windows = [window(10), window(20), window(30)];
    let backend = FakeBackend {
        focused: Some(windows[1].clone()),
        ..FakeBackend::default()
    };
    let mut runtime = runtime_with_config(backend, config(recent_focus));
    for window in &windows {
        bind(&mut runtime, WorkspaceId::ONE, window);
    }
    assert!(
        runtime
            .workspaces
            .engine_mut(WorkspaceId::ONE)
            .move_window_relative(&windows[2], &windows[1], Direction::Down)
    );
    let tiles = runtime.navigation_tiles(&outputs());
    let frame = |window: &crate::PhysicalWindowId| {
        tiles
            .iter()
            .find(|tile| &tile.window == window)
            .unwrap()
            .frame
    };
    let [first, second, third] = [&windows[0], &windows[1], &windows[2]].map(frame);
    assert!(first.x + first.width <= second.x, "1 lies left of 2");
    assert_eq!(second.x, third.x, "2 and 3 share a column");
    assert!(second.y + second.height <= third.y, "3 lies below 2");
    (runtime, windows)
}

fn focus(runtime: &mut WorkspaceRuntime<FakeBackend>, direction: Direction) {
    runtime
        .focus_relative(direction, outputs(), &AtomicBool::new(false))
        .unwrap();
}

/// Where `ctl debug` says the key goes from the effective focus now.
fn debug_target(
    runtime: &mut WorkspaceRuntime<FakeBackend>,
    direction: Direction,
) -> Option<crate::PhysicalWindowId> {
    let reported = runtime.backend.focused.clone();
    let origin = runtime.effective_focus(reported);
    runtime.focus_target(origin.as_ref(), direction, &outputs())
}

/// Focus 3 with a key from 2, and let macOS report it.
fn settle_on_third(runtime: &mut WorkspaceRuntime<FakeBackend>, third: &crate::PhysicalWindowId) {
    focus(runtime, Direction::Down);
    assert_eq!(runtime.backend.focused.as_ref(), Some(third));
    runtime.focus.age_request(FOCUS_SETTLE);
}

#[test]
fn left_and_back_returns_to_the_lower_tile() {
    let (mut runtime, [first, _, third]) = stacked_pair(true);
    settle_on_third(&mut runtime, &third);

    focus(&mut runtime, Direction::Left);
    assert_eq!(runtime.backend.focused.as_ref(), Some(&first));
    runtime.focus.age_request(FOCUS_SETTLE);

    assert_eq!(
        debug_target(&mut runtime, Direction::Right),
        Some(third.clone())
    );
    focus(&mut runtime, Direction::Right);
    assert_eq!(runtime.backend.focused, Some(third));
}

#[test]
fn quick_presses_return_before_the_new_focus_is_reported() {
    let (mut runtime, [first, _, third]) = stacked_pair(true);
    settle_on_third(&mut runtime, &third);

    focus(&mut runtime, Direction::Left);
    // macOS still reports the window focus left.
    runtime.backend.focused = Some(third.clone());
    assert_eq!(debug_target(&mut runtime, Direction::Left), None);
    assert_eq!(
        debug_target(&mut runtime, Direction::Right),
        Some(third.clone())
    );
    focus(&mut runtime, Direction::Right);

    let focused = runtime
        .backend
        .calls
        .iter()
        .filter_map(|call| match call {
            BackendCall::Focus(window) => Some(window.clone()),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(focused, vec![third.clone(), first, third]);
}

#[test]
fn a_window_capped_short_of_its_tile_borders_only_what_it_reaches() {
    let (mut runtime, [first, second, third]) = stacked_pair(true);
    settle_on_third(&mut runtime, &third);

    focus(&mut runtime, Direction::Left);
    assert_eq!(runtime.backend.focused.as_ref(), Some(&first));
    runtime.focus.age_request(FOCUS_SETTLE);

    // macOS kept 1 to the height of 2, leaving a gap beside 3 (#648).
    let tiles = runtime.navigation_tiles(&outputs());
    let frame = |window: &crate::PhysicalWindowId| {
        tiles
            .iter()
            .find(|tile| &tile.window == window)
            .unwrap()
            .frame
    };
    let capped = crate::Rect {
        height: frame(&second).height,
        ..frame(&first)
    };
    runtime.backend.server_frames.insert(first.clone(), capped);

    assert_eq!(
        debug_target(&mut runtime, Direction::Right),
        Some(second.clone())
    );
    focus(&mut runtime, Direction::Right);
    assert_eq!(runtime.backend.focused, Some(second));
}

#[test]
fn with_the_lab_off_left_and_back_lands_on_the_upper_tile() {
    let (mut runtime, [first, second, third]) = stacked_pair(false);
    settle_on_third(&mut runtime, &third);

    focus(&mut runtime, Direction::Left);
    assert_eq!(runtime.backend.focused.as_ref(), Some(&first));
    runtime.focus.age_request(FOCUS_SETTLE);

    assert_eq!(
        debug_target(&mut runtime, Direction::Right),
        Some(second.clone())
    );
    focus(&mut runtime, Direction::Right);
    assert_eq!(runtime.backend.focused, Some(second));
}
