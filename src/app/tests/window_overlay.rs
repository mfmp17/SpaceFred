//! `[labs] window_overlay`: the card buttons act on their own window, focused or not.

use super::*;

/// Two tiles on workspace 1, the left one (`window(10)`) focused.
fn two_tiles() -> WorkspaceRuntime<FakeBackend> {
    let windows = [(window(10), frame(50)), (window(20), frame(500))];
    let backend = FakeBackend {
        frames: windows.iter().cloned().collect(),
        focused: Some(window(10)),
        frontmost: Some(window(10).app),
        observation: Some(observation_for_windows(
            "com.example.Test",
            &window(10),
            &windows,
        )),
        ..FakeBackend::default()
    };
    let mut runtime = runtime(backend);
    bind(&mut runtime, WorkspaceId::ONE, &window(10));
    bind(&mut runtime, WorkspaceId::ONE, &window(20));
    runtime.cycle(outputs(), &AtomicBool::new(false)).unwrap();
    runtime.backend.calls.clear();
    runtime
}

fn run(
    runtime: &mut WorkspaceRuntime<FakeBackend>,
    command: WindowOverlayCommand,
) -> UiWindowOverlay {
    let status = runtime
        .window_overlay_command(command, outputs(), &AtomicBool::new(false))
        .unwrap();
    status
        .window_overlay
        .expect("every command answers with the overlay")
}

fn card(overlay: &UiWindowOverlay, id: u32) -> &UiWindowOverlayWindow {
    overlay
        .displays
        .iter()
        .flat_map(|display| &display.windows)
        .find(|card| card.window == window(id))
        .expect("the window has a card")
}

#[test]
fn the_snapshot_lists_each_visible_window_title_free_with_its_swaps() {
    let mut runtime = two_tiles();
    let overlay = run(&mut runtime, WindowOverlayCommand::Snapshot);

    let main = &overlay.displays[0];
    assert_eq!(main.id, "main-id");
    assert_eq!(main.workspace, WorkspaceId::ONE);
    assert!(!main.fullscreen);
    assert_eq!(main.screen, outputs().main().frame);
    let left = card(&overlay, 10);
    let right = card(&overlay, 20);
    assert_eq!(left.bundle_id.as_deref(), Some("com.example.Test"));
    assert_eq!((left.floating, left.pinned), (false, false));
    assert_eq!(left.swaps, vec![Direction::Right]);
    assert_eq!(right.swaps, vec![Direction::Left]);
    // Every numbered workspace can be sent to; the first five sit on this display.
    let listed: Vec<_> = main
        .workspaces
        .iter()
        .map(|entry| entry.workspace)
        .collect();
    assert_eq!(listed, WorkspaceId::numbered().collect::<Vec<_>>());
    let one = main.workspaces[0];
    assert!(one.occupied && one.on_this_display);
    let six = main.workspaces[5];
    assert!(!six.occupied && !six.on_this_display);
    // Nothing a window shows but its app: the debug form carries no title field at all.
    assert!(!format!("{overlay:?}").contains("title"));
}

#[test]
fn floating_an_unfocused_window_floats_that_window_not_the_focused_one() {
    let mut runtime = two_tiles();
    let overlay = run(&mut runtime, WindowOverlayCommand::ToggleFloat(window(20)));

    assert_eq!(runtime.workspaces.workspace_of(&window(20)), None);
    assert_eq!(
        runtime.workspaces.floating_workspace_of(&window(20)),
        Some(WorkspaceId::ONE)
    );
    assert_eq!(
        runtime.workspaces.workspace_of(&window(10)),
        Some(WorkspaceId::ONE),
        "the focused window stays a tile"
    );
    assert!(card(&overlay, 20).floating);
    assert!(
        card(&overlay, 10).swaps.is_empty(),
        "a lone tile has no swaps"
    );

    // And back: it tiles again.
    let overlay = run(&mut runtime, WindowOverlayCommand::ToggleFloat(window(20)));
    assert_eq!(
        runtime.workspaces.workspace_of(&window(20)),
        Some(WorkspaceId::ONE)
    );
    assert!(!card(&overlay, 20).floating);
}

#[test]
fn pinning_an_unfocused_window_pins_that_window() {
    let mut runtime = two_tiles();
    let overlay = run(&mut runtime, WindowOverlayCommand::TogglePin(window(20)));

    assert!(runtime.workspaces.is_pinned(&window(20)));
    assert!(!runtime.workspaces.is_pinned(&window(10)));
    let pinned = card(&overlay, 20);
    assert!(pinned.pinned && pinned.floating);

    run(&mut runtime, WindowOverlayCommand::TogglePin(window(20)));
    assert!(!runtime.workspaces.is_pinned(&window(20)));
}

#[test]
fn sending_an_unfocused_window_moves_it_without_moving_focus_or_the_pointer() {
    let mut runtime = two_tiles();
    let overlay = run(
        &mut runtime,
        WindowOverlayCommand::MoveTo {
            window: window(20),
            workspace: WorkspaceId::TWO,
        },
    );

    assert_eq!(
        runtime.workspaces.workspace_of(&window(20)),
        Some(WorkspaceId::TWO)
    );
    assert_eq!(
        runtime.workspaces.workspace_of(&window(10)),
        Some(WorkspaceId::ONE)
    );
    assert!(
        overlay.displays[0]
            .windows
            .iter()
            .all(|card| card.window != window(20)),
        "a window sent to a hidden workspace leaves the overlay"
    );
    assert!(
        !runtime
            .backend
            .calls
            .iter()
            .any(|call| matches!(call, BackendCall::WarpPointer(_) | BackendCall::Focus(_))),
        "a click on the overlay neither warps the pointer nor moves focus: {:?}",
        runtime.backend.calls
    );
}

#[test]
fn swapping_an_unfocused_tile_swaps_it_with_its_neighbour() {
    let mut runtime = two_tiles();
    let before = runtime.workspaces.engine(WorkspaceId::ONE).bound_windows();
    assert_eq!(before, vec![window(10), window(20)]);

    let overlay = run(
        &mut runtime,
        WindowOverlayCommand::Swap(window(20), Direction::Left),
    );

    assert_eq!(
        runtime.workspaces.engine(WorkspaceId::ONE).bound_windows(),
        vec![window(20), window(10)]
    );
    assert_eq!(card(&overlay, 20).swaps, vec![Direction::Right]);
}

#[test]
fn fullscreen_layout_from_a_card_puts_that_window_in_front_and_shows_only_it() {
    let mut runtime = two_tiles();
    let overlay = run(
        &mut runtime,
        WindowOverlayCommand::ToggleFullscreen(window(20)),
    );

    assert!(runtime.workspaces.deck(WorkspaceId::ONE).is_monocle());
    let main = &overlay.displays[0];
    assert!(main.fullscreen);
    let shown: Vec<_> = main
        .windows
        .iter()
        .map(|card| card.window.clone())
        .collect();
    assert_eq!(
        shown,
        vec![window(20)],
        "the stack shows its front window's card"
    );
    assert!(main.windows[0].swaps.is_empty());
}
