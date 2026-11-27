//! #707: an AppKit native tab switch can order the outgoing tab out before the window server
//! shows the incoming one. A snapshot taken in between has no on-screen window of the app, so
//! the app is missing from it; an empty AXWindows list then confirmed a close, retired the tile
//! at once, and the incoming tab got a new tile beside its neighbor a cycle later, losing the
//! tile's side and width. Reported by @coygeek.

use super::super::reconciliation::{app_window, column, studio_display};
use super::super::*;

const TABS: &str = "com.example.Tabs";
const OTHER: &str = "com.example.Other";

fn app(
    bundle_id: &str,
    focused: Option<&crate::PhysicalWindowId>,
    windows: &[(&crate::PhysicalWindowId, crate::Rect)],
    app: &AppInstanceId,
) -> crate::AppObservation {
    crate::AppObservation::Complete {
        app: crate::ObservedApplication {
            id: app.clone(),
            bundle_id: bundle_id.into(),
        },
        focused: focused.cloned(),
        windows: windows
            .iter()
            .map(|(window, frame)| crate::ObservedWindow {
                id: (*window).clone(),
                frame: *frame,
                resizable: true,
                title: None,
                document: None,
            })
            .collect(),
    }
}

fn observe(applications: Vec<crate::AppObservation>) -> crate::NativeObservation {
    crate::NativeObservation {
        cycle: 0,
        mouse_down: false,
        applications,
    }
}

/// `tab` tiled beside `neighbor`, then ordered out: the snapshot omits its app, which has no
/// window on screen, and the app's AXWindows list is empty. With `tab_group`, the window
/// server still lists the group's other tab, `next_tab`, ordered out at the group's corner with
/// the narrower size it had before the last resize.
fn mid_switch(
    tab_group: bool,
) -> (
    WorkspaceRuntime<FakeBackend>,
    crate::PhysicalWindowId,
    crate::PhysicalWindowId,
    crate::PhysicalWindowId,
) {
    let (tab, next_tab, neighbor) = (
        app_window(1, 1, 10),
        app_window(1, 1, 11),
        app_window(2, 1, 20),
    );
    let (left, right) = (column(0, 1000), column(1000, 1560));
    let mut runtime = runtime(FakeBackend {
        frames: [(tab.clone(), left), (neighbor.clone(), right)].into(),
        focused: Some(tab.clone()),
        observation: Some(observe(vec![
            app(TABS, Some(&tab), &[(&tab, left)], &tab.app),
            app(OTHER, None, &[(&neighbor, right)], &neighbor.app),
        ])),
        ..FakeBackend::default()
    });
    bind(&mut runtime, WorkspaceId::ONE, &tab);
    bind(&mut runtime, WorkspaceId::ONE, &neighbor);
    runtime
        .cycle(studio_display(), &AtomicBool::new(false))
        .unwrap();
    let sized = runtime.backend.frames[&tab];

    runtime.backend.frames.remove(&tab);
    runtime.backend.server_frames.insert(tab.clone(), sized);
    runtime.backend.ordered_out.insert(tab.window_id);
    if tab_group {
        let stale = crate::Rect {
            width: sized.width - 76,
            ..sized
        };
        runtime.backend.frames.insert(next_tab.clone(), stale);
        runtime.backend.ordered_out.insert(next_tab.window_id);
    }
    runtime
        .backend
        .apps_confirming_no_windows
        .insert(tab.app.clone());
    runtime.backend.focused = None;
    runtime.backend.observation = Some(observe(vec![app(
        OTHER,
        None,
        &[(&neighbor, right)],
        &neighbor.app,
    )]));
    (runtime, tab, next_tab, neighbor)
}

#[test]
fn a_native_tab_arriving_after_its_predecessor_left_the_screen_keeps_the_tile() {
    let (mut runtime, tab, next_tab, neighbor) = mid_switch(true);
    let slot = runtime
        .workspaces
        .engine(WorkspaceId::ONE)
        .slot_for_window(&tab);
    let (tab_frame, neighbor_frame) = (
        runtime.backend.server_frames[&tab],
        runtime.backend.frames[&neighbor],
    );

    let status = runtime
        .cycle(studio_display(), &AtomicBool::new(false))
        .unwrap();
    assert_eq!(
        runtime.workspaces.workspace_of(&tab),
        Some(WorkspaceId::ONE)
    );
    assert!(
        runtime.backend.close_probes.is_empty(),
        "an empty window list must not confirm a close mid tab switch"
    );
    let recheck = status.close_recheck.expect("a prompt follow-up");
    assert!(recheck <= runtime.backend.now() + Duration::from_millis(75));
    assert_eq!(runtime.backend.frames[&neighbor], neighbor_frame);

    // The incoming tab is on screen at the tile's frame and focused.
    runtime.backend.advance(Duration::from_millis(75));
    runtime.backend.frames.insert(next_tab.clone(), tab_frame);
    runtime.backend.ordered_out.remove(&next_tab.window_id);
    runtime.backend.focused = Some(next_tab.clone());
    runtime.backend.observation = Some(observe(vec![
        app(TABS, Some(&next_tab), &[(&next_tab, tab_frame)], &tab.app),
        app(OTHER, None, &[(&neighbor, neighbor_frame)], &neighbor.app),
    ]));
    runtime
        .cycle(studio_display(), &AtomicBool::new(false))
        .unwrap();

    assert_eq!(
        runtime
            .workspaces
            .engine(WorkspaceId::ONE)
            .slot_for_window(&next_tab),
        slot
    );
    assert_eq!(runtime.backend.frames[&next_tab], tab_frame);
    assert_eq!(runtime.backend.frames[&neighbor], neighbor_frame);
}

#[test]
fn a_tab_group_that_never_shows_its_next_tab_gives_up_the_tile_after_the_short_wait() {
    let (mut runtime, tab, _, neighbor) = mid_switch(true);
    runtime
        .cycle(studio_display(), &AtomicBool::new(false))
        .unwrap();
    assert_eq!(
        runtime.workspaces.workspace_of(&tab),
        Some(WorkspaceId::ONE)
    );

    runtime.backend.advance(Duration::from_millis(300));
    runtime
        .cycle(studio_display(), &AtomicBool::new(false))
        .unwrap();

    assert_eq!(runtime.workspaces.workspace_of(&tab), None);
    assert_eq!(runtime.backend.frames[&neighbor], column(0, 2560));
}

#[test]
fn a_minimized_tab_gives_up_its_tile_at_once() {
    let (mut runtime, tab, _, neighbor) = mid_switch(true);
    runtime.backend.set_aside.insert(tab.clone());
    runtime
        .cycle(studio_display(), &AtomicBool::new(false))
        .unwrap();

    assert_eq!(runtime.workspaces.workspace_of(&tab), None);
    assert_eq!(runtime.backend.frames[&neighbor], column(0, 2560));
}

/// A window its app closes by ordering it out (many SwiftUI and Electron apps) leaves no other
/// surface of the app at its corner: the empty window list confirms the close, and the
/// neighbor reflows, in the same cycle.
#[test]
fn a_window_closed_by_ordering_it_out_gives_up_its_tile_at_once() {
    let (mut runtime, tab, _, neighbor) = mid_switch(false);
    let status = runtime
        .cycle(studio_display(), &AtomicBool::new(false))
        .unwrap();

    assert_eq!(runtime.backend.close_probes, std::slice::from_ref(&tab.app));
    assert_eq!(runtime.workspaces.workspace_of(&tab), None);
    assert_eq!(runtime.backend.frames[&neighbor], column(0, 2560));
    assert!(status.close_recheck.is_none());
}

/// #719: the window server already shows the incoming tab at the tile's frame, so the snapshot
/// still lists the app, but Accessibility lists neither tab yet and the outgoing one is ordered
/// out. Retiring the tile there gave the incoming tab a new tile beside its neighbor a cycle
/// later.
fn listed_mid_switch() -> (
    WorkspaceRuntime<FakeBackend>,
    crate::PhysicalWindowId,
    crate::PhysicalWindowId,
    crate::PhysicalWindowId,
) {
    let (mut runtime, tab, next_tab, neighbor) = mid_switch(true);
    let tab_frame = runtime.backend.server_frames[&tab];
    let right = runtime.backend.frames[&neighbor];
    runtime.backend.frames.insert(next_tab.clone(), tab_frame);
    runtime.backend.ordered_out.remove(&next_tab.window_id);
    runtime.backend.observation = Some(observe(vec![
        app(TABS, None, &[], &tab.app),
        app(OTHER, None, &[(&neighbor, right)], &neighbor.app),
    ]));
    (runtime, tab, next_tab, neighbor)
}

#[test]
fn a_native_tab_not_yet_listed_while_its_app_is_keeps_the_tile() {
    let (mut runtime, tab, next_tab, neighbor) = listed_mid_switch();
    let slot = runtime
        .workspaces
        .engine(WorkspaceId::ONE)
        .slot_for_window(&tab);
    let (tab_frame, neighbor_frame) = (
        runtime.backend.server_frames[&tab],
        runtime.backend.frames[&neighbor],
    );

    let status = runtime
        .cycle(studio_display(), &AtomicBool::new(false))
        .unwrap();
    assert_eq!(
        runtime.workspaces.workspace_of(&tab),
        Some(WorkspaceId::ONE)
    );
    assert_eq!(runtime.workspaces.workspace_of(&next_tab), None);
    let recheck = status.close_recheck.expect("a prompt follow-up");
    assert!(recheck <= runtime.backend.now() + Duration::from_millis(75));
    assert_eq!(runtime.backend.frames[&neighbor], neighbor_frame);

    runtime.backend.advance(Duration::from_millis(75));
    runtime.backend.focused = Some(next_tab.clone());
    runtime.backend.observation = Some(observe(vec![
        app(TABS, Some(&next_tab), &[(&next_tab, tab_frame)], &tab.app),
        app(OTHER, None, &[(&neighbor, neighbor_frame)], &neighbor.app),
    ]));
    runtime
        .cycle(studio_display(), &AtomicBool::new(false))
        .unwrap();

    assert_eq!(
        runtime
            .workspaces
            .engine(WorkspaceId::ONE)
            .slot_for_window(&next_tab),
        slot
    );
    assert_eq!(runtime.backend.frames[&next_tab], tab_frame);
    assert_eq!(runtime.backend.frames[&neighbor], neighbor_frame);
}

#[test]
fn a_listed_app_whose_next_tab_never_shows_up_gives_up_the_tile_after_the_short_wait() {
    let (mut runtime, tab, _, neighbor) = listed_mid_switch();
    runtime
        .cycle(studio_display(), &AtomicBool::new(false))
        .unwrap();
    assert_eq!(
        runtime.workspaces.workspace_of(&tab),
        Some(WorkspaceId::ONE)
    );

    runtime.backend.advance(Duration::from_millis(300));
    runtime
        .cycle(studio_display(), &AtomicBool::new(false))
        .unwrap();

    assert_eq!(runtime.workspaces.workspace_of(&tab), None);
    assert_eq!(runtime.backend.frames[&neighbor], column(0, 2560));
}
