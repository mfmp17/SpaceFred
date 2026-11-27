//! #759: another tab of an AppKit document group was selected just as Move App ran. The
//! group's tile moved to the destination still bound to the tab shown before, which the
//! switch had ordered out, and the group showed the incoming tab where the tile had been.
//! The source workspace adopted that tab as a new window, so the group stayed behind while
//! the app's other window moved. Reported by @coygeek.

use super::super::reconciliation::app_window;
use super::super::*;

const DOCUMENTS: &str = "com.example.Documents";

fn observe(apps: &[(&str, &[&crate::PhysicalWindowId])]) -> crate::NativeObservation {
    crate::NativeObservation {
        cycle: 0,
        mouse_down: false,
        applications: apps
            .iter()
            .map(|(bundle_id, windows)| crate::AppObservation::Complete {
                app: crate::ObservedApplication {
                    id: windows[0].app.clone(),
                    bundle_id: (*bundle_id).into(),
                },
                focused: None,
                windows: windows
                    .iter()
                    .map(|window| crate::ObservedWindow {
                        id: (*window).clone(),
                        frame: crate::Rect::default(),
                        resizable: true,
                        title: None,
                        document: None,
                    })
                    .collect(),
            })
            .collect(),
    }
}

fn first_tab() -> crate::PhysicalWindowId {
    app_window(1, 1, 10)
}

fn next_tab() -> crate::PhysicalWindowId {
    app_window(1, 1, 13)
}

fn neighbor() -> crate::PhysicalWindowId {
    app_window(2, 1, 20)
}

fn destination() -> crate::PhysicalWindowId {
    app_window(3, 1, 30)
}

/// Workspace 1 on Main holds the group's first tab beside another app's window; workspace
/// 6 on Secondary holds a third app's window. One cycle has tiled both.
fn tiled_group() -> WorkspaceRuntime<FakeBackend> {
    let mut runtime = runtime(FakeBackend {
        frames: [
            (first_tab(), frame(0)),
            (neighbor(), frame(500)),
            (destination(), frame(1200)),
        ]
        .into(),
        focused: Some(first_tab()),
        observation: Some(observe(&[
            (DOCUMENTS, &[&first_tab()]),
            ("com.example.Neighbor", &[&neighbor()]),
            ("com.example.Destination", &[&destination()]),
        ])),
        ..FakeBackend::default()
    });
    bind(&mut runtime, WorkspaceId::ONE, &first_tab());
    bind(&mut runtime, WorkspaceId::ONE, &neighbor());
    bind(&mut runtime, WorkspaceId::SIX, &destination());
    runtime.cycle(outputs(), &AtomicBool::new(false)).unwrap();
    runtime
}

/// The group's next tab is selected: the first tab is ordered out where it was and the
/// incoming one shows at the group's frame. `rival` opens a second window of the app there.
fn select_next_tab(runtime: &mut WorkspaceRuntime<FakeBackend>, rival: bool) {
    let group = runtime.backend.frames[&first_tab()];
    runtime.backend.server_frames.insert(first_tab(), group);
    runtime.backend.ordered_out.insert(first_tab().window_id);
    runtime.backend.frames.insert(next_tab(), group);
    let mut documents = vec![next_tab()];
    if rival {
        let fourth = app_window(1, 1, 14);
        runtime.backend.frames.insert(fourth.clone(), group);
        documents.push(fourth);
    }
    let documents: Vec<&crate::PhysicalWindowId> = documents.iter().collect();
    runtime.backend.focused = Some(next_tab());
    runtime.backend.observation = Some(observe(&[
        (DOCUMENTS, documents.as_slice()),
        ("com.example.Neighbor", &[&neighbor()]),
        ("com.example.Destination", &[&destination()]),
    ]));
}

#[test]
fn a_tab_selected_during_move_app_takes_the_moved_tile_to_the_destination() {
    let cancelled = AtomicBool::new(false);
    let mut runtime = tiled_group();
    select_next_tab(&mut runtime, false);

    runtime
        .move_app(DOCUMENTS, WorkspaceId::SIX, outputs(), &cancelled)
        .unwrap();
    let slot = runtime
        .workspaces
        .engine(WorkspaceId::SIX)
        .slot_for_window(&first_tab());
    assert!(slot.is_some());
    runtime.cycle(outputs(), &cancelled).unwrap();

    // The incoming tab holds the group's tile on workspace 6, on Secondary; workspace 1
    // keeps only its neighbor.
    assert_eq!(
        runtime.workspaces.workspace_of(&next_tab()),
        Some(WorkspaceId::SIX)
    );
    assert_eq!(
        runtime
            .workspaces
            .engine(WorkspaceId::SIX)
            .slot_for_window(&next_tab()),
        slot
    );
    assert_eq!(runtime.workspaces.workspace_of_any(&first_tab()), None);
    assert_eq!(
        runtime.workspaces.workspace_of(&neighbor()),
        Some(WorkspaceId::ONE)
    );
    let secondary = outputs().displays()[1].frame;
    assert!(secondary.contains_center(runtime.backend.frames[&next_tab()]));
}

#[test]
fn two_windows_where_the_moved_tile_was_claim_nothing() {
    let cancelled = AtomicBool::new(false);
    let mut runtime = tiled_group();
    select_next_tab(&mut runtime, true);

    runtime
        .move_app(DOCUMENTS, WorkspaceId::SIX, outputs(), &cancelled)
        .unwrap();
    runtime.cycle(outputs(), &cancelled).unwrap();

    assert_ne!(
        runtime.workspaces.workspace_of(&next_tab()),
        Some(WorkspaceId::SIX)
    );
}

#[test]
fn a_tab_selected_long_after_the_move_is_a_new_window() {
    let cancelled = AtomicBool::new(false);
    let mut runtime = tiled_group();
    select_next_tab(&mut runtime, false);

    runtime
        .move_app(DOCUMENTS, WorkspaceId::SIX, outputs(), &cancelled)
        .unwrap();
    runtime
        .backend
        .advance(cycle::MOVED_TAB_SETTLE + Duration::from_millis(1));
    runtime.cycle(outputs(), &cancelled).unwrap();

    assert_ne!(
        runtime.workspaces.workspace_of(&next_tab()),
        Some(WorkspaceId::SIX)
    );
}
