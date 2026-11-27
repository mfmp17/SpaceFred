//! #747: selecting another tab of a floated AppKit tab group swaps the window the group shows.
//! The outgoing tab's float was forgotten once it left the snapshot, and the incoming tab was
//! tiled as a new window: the group turned into a full-size tile and no longer parked with
//! the workspace it floated on. Reported by @coygeek.

use super::super::reconciliation::{app_window, column, studio_display};
use super::super::*;
use crate::workspace::FloatCause;

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

const FLOAT: crate::Rect = crate::Rect {
    x: 300,
    y: 120,
    width: 620,
    height: 420,
};

/// `tab` floated by hand over `neighbor`, the one tile; `next_tab` is the group's other tab.
fn floated_group() -> (
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
    let full = column(0, 2560);
    let mut runtime = runtime(FakeBackend {
        frames: [(tab.clone(), FLOAT), (neighbor.clone(), full)].into(),
        focused: Some(tab.clone()),
        observation: Some(observe(vec![
            app(TABS, Some(&tab), &[(&tab, FLOAT)], &tab.app),
            app(OTHER, None, &[(&neighbor, full)], &neighbor.app),
        ])),
        ..FakeBackend::default()
    });
    bind(&mut runtime, WorkspaceId::ONE, &neighbor);
    runtime
        .workspaces
        .float(tab.clone(), WorkspaceId::ONE, FloatCause::Manual);
    runtime
        .cycle(studio_display(), &AtomicBool::new(false))
        .unwrap();
    assert_eq!(runtime.backend.frames[&tab], FLOAT);
    runtime.backend.calls.clear();
    (runtime, tab, next_tab, neighbor)
}

/// The window server orders `tab` out and `next_tab` is the group's selected, focused tab.
fn select_next_tab(
    runtime: &mut WorkspaceRuntime<FakeBackend>,
    tab: &crate::PhysicalWindowId,
    next_tab: &crate::PhysicalWindowId,
    neighbor: &crate::PhysicalWindowId,
) {
    runtime.backend.frames.remove(tab);
    runtime.backend.server_frames.insert(tab.clone(), FLOAT);
    runtime.backend.ordered_out.insert(tab.window_id);
    runtime.backend.frames.insert(next_tab.clone(), FLOAT);
    runtime.backend.focused = Some(next_tab.clone());
    let full = runtime.backend.frames[neighbor];
    runtime.backend.observation = Some(observe(vec![
        app(TABS, Some(next_tab), &[(next_tab, FLOAT)], &tab.app),
        app(OTHER, None, &[(neighbor, full)], &neighbor.app),
    ]));
}

fn assert_float_moved_to(
    runtime: &WorkspaceRuntime<FakeBackend>,
    tab: &crate::PhysicalWindowId,
    next_tab: &crate::PhysicalWindowId,
    neighbor: &crate::PhysicalWindowId,
) {
    assert_eq!(runtime.workspaces.workspace_of(next_tab), None);
    assert_eq!(
        runtime.workspaces.floating_workspace_of(next_tab),
        Some(WorkspaceId::ONE)
    );
    assert!(runtime.workspaces.is_manual_float(next_tab));
    assert_eq!(runtime.workspaces.workspace_of_any(tab), None);
    assert_eq!(runtime.backend.frames[next_tab], FLOAT);
    assert_eq!(
        runtime.workspaces.engine(WorkspaceId::ONE).bound_windows(),
        vec![neighbor.clone()]
    );
}

#[test]
fn the_incoming_tab_of_a_floated_group_keeps_the_float() {
    let (mut runtime, tab, next_tab, neighbor) = floated_group();
    select_next_tab(&mut runtime, &tab, &next_tab, &neighbor);

    runtime
        .cycle(studio_display(), &AtomicBool::new(false))
        .unwrap();

    assert_float_moved_to(&runtime, &tab, &next_tab, &neighbor);
    let moved =
        |call: &BackendCall| matches!(call, BackendCall::Apply(window, _) if *window == next_tab);
    assert!(
        !runtime.backend.calls.iter().any(moved),
        "the incoming tab stays where the float was"
    );
}

#[test]
fn an_incoming_tab_listed_beside_its_outgoing_float_waits_for_the_handoff() {
    let (mut runtime, tab, next_tab, neighbor) = floated_group();
    let full = runtime.backend.frames[&neighbor];
    // Mid-switch: both tabs are listed at the group's frame.
    runtime.backend.frames.insert(next_tab.clone(), FLOAT);
    runtime.backend.focused = Some(next_tab.clone());
    runtime.backend.observation = Some(observe(vec![
        app(
            TABS,
            Some(&next_tab),
            &[(&tab, FLOAT), (&next_tab, FLOAT)],
            &tab.app,
        ),
        app(OTHER, None, &[(&neighbor, full)], &neighbor.app),
    ]));
    runtime
        .cycle(studio_display(), &AtomicBool::new(false))
        .unwrap();
    assert_eq!(runtime.workspaces.workspace_of_any(&next_tab), None);
    assert_eq!(runtime.backend.frames[&next_tab], FLOAT);

    runtime.backend.advance(Duration::from_millis(75));
    select_next_tab(&mut runtime, &tab, &next_tab, &neighbor);
    runtime
        .cycle(studio_display(), &AtomicBool::new(false))
        .unwrap();

    assert_float_moved_to(&runtime, &tab, &next_tab, &neighbor);
}

#[test]
fn a_new_window_at_a_float_that_stays_listed_is_adopted_after_the_wait() {
    let (mut runtime, tab, next_tab, neighbor) = floated_group();
    let full = runtime.backend.frames[&neighbor];
    runtime.backend.frames.insert(next_tab.clone(), FLOAT);
    runtime.backend.observation = Some(observe(vec![
        app(
            TABS,
            Some(&tab),
            &[(&tab, FLOAT), (&next_tab, FLOAT)],
            &tab.app,
        ),
        app(OTHER, None, &[(&neighbor, full)], &neighbor.app),
    ]));
    runtime
        .cycle(studio_display(), &AtomicBool::new(false))
        .unwrap();
    assert_eq!(runtime.workspaces.workspace_of_any(&next_tab), None);

    runtime.backend.advance(cycle::TAB_HANDOFF_SETTLE);
    runtime
        .cycle(studio_display(), &AtomicBool::new(false))
        .unwrap();

    assert_eq!(
        runtime.workspaces.workspace_of(&next_tab),
        Some(WorkspaceId::ONE)
    );
    assert_eq!(
        runtime.workspaces.floating_workspace_of(&tab),
        Some(WorkspaceId::ONE)
    );
}

/// #754: a native tab group shown from the Scratchpad selects another tab. The incoming tab
/// takes the Scratchpad entry, so Toggle hides the group instead of finding the Scratchpad
/// empty, and it summons again at the size it was shown at. Reported by @coygeek.
#[test]
fn the_incoming_tab_of_a_shown_scratchpad_group_stays_in_the_scratchpad() {
    let (mut runtime, tab, next_tab, neighbor) = floated_group();
    runtime.replace_config(Config::parse("version = 1\n[labs]\nscratchpad = true\n").unwrap());
    let size = Some((FLOAT.width, FLOAT.height));
    assert!(runtime.workspaces.stash(tab.clone(), size));
    runtime.workspaces.show_scratchpad(&tab, WorkspaceId::ONE);
    select_next_tab(&mut runtime, &tab, &next_tab, &neighbor);

    runtime
        .cycle(studio_display(), &AtomicBool::new(false))
        .unwrap();

    let pad = runtime.workspaces.scratchpad();
    let entry = pad
        .entry(&next_tab)
        .expect("the incoming tab is in the scratchpad");
    assert_eq!(entry.shown_on, Some(WorkspaceId::ONE));
    assert_eq!(entry.size, size);
    assert!(pad.entry(&tab).is_none());
    assert_eq!(runtime.workspaces.workspace_of(&next_tab), None);
    assert_eq!(runtime.backend.frames[&next_tab], FLOAT);
    assert_eq!(
        runtime.workspaces.engine(WorkspaceId::ONE).bound_windows(),
        vec![neighbor.clone()]
    );
}

/// #769: leaving a desktop orders a hand float out while the window server still lists it at
/// its frame, and a window of its app on the arriving desktop can sit at that same frame. That
/// window is not a tab replacing the float: it must not take the float (or wait for it).
/// Reported by @coygeek.
#[test]
fn a_window_on_the_arriving_desktop_at_a_floats_frame_does_not_take_the_float() {
    let (mut runtime, tab, arriving, neighbor) = floated_group();
    runtime.backend.space_changed_at = Some(runtime.backend.now());
    runtime
        .backend
        .advance(SPACE_TRANSITION_SETTLE + Duration::from_millis(100));
    select_next_tab(&mut runtime, &tab, &arriving, &neighbor);

    runtime
        .cycle(studio_display(), &AtomicBool::new(false))
        .unwrap();

    assert_eq!(runtime.workspaces.floating_workspace_of(&arriving), None);
    assert_eq!(
        runtime.workspaces.workspace_of(&arriving),
        Some(WorkspaceId::ONE)
    );
}
