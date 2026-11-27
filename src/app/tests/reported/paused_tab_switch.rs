//! #753: Pause restored a hidden workspace's native tab group, another tab was selected while
//! paused, and Resume adopted that tab on the visible workspace as a new window. The hidden
//! workspace kept the ordered-out tab, which could not be restored, so every switch to it
//! rolled back and its other windows stayed parked. Reported by @coygeek.

use super::super::reconciliation::app_window;
use super::super::*;

const DOCUMENTS: &str = "com.example.Documents";

fn one_output() -> macos::DisplayOutputs {
    macos::DisplayOutputs::new(vec![outputs().main().clone()]).unwrap()
}

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

fn third_tab() -> crate::PhysicalWindowId {
    app_window(1, 1, 13)
}

fn other() -> crate::PhysicalWindowId {
    app_window(2, 1, 20)
}

fn shown() -> crate::PhysicalWindowId {
    app_window(3, 1, 30)
}

/// Workspace 2 holds the first tab of a document group and another app's window; workspace
/// 3 is shown. Both workspace 2 windows are parked.
fn hidden_group() -> WorkspaceRuntime<FakeBackend> {
    let cancelled = AtomicBool::new(false);
    let mut runtime = runtime(FakeBackend {
        frames: [
            (first_tab(), frame(0)),
            (other(), frame(500)),
            (shown(), frame(200)),
        ]
        .into(),
        observation: Some(observe(&[
            (DOCUMENTS, &[&first_tab()]),
            ("com.example.Other", &[&other()]),
            ("com.example.Shown", &[&shown()]),
        ])),
        ..FakeBackend::default()
    });
    bind(&mut runtime, WorkspaceId::TWO, &first_tab());
    bind(&mut runtime, WorkspaceId::TWO, &other());
    bind(&mut runtime, WorkspaceId::THREE, &shown());
    runtime
        .switch(WorkspaceId::THREE, one_output(), &cancelled)
        .unwrap();
    runtime.cycle(one_output(), &cancelled).unwrap();
    assert!(runtime.parked.contains_key(&first_tab()));
    assert!(runtime.parked.contains_key(&other()));
    runtime
}

/// Pause restores the group, then another tab is selected: the first tab is ordered out and
/// the group shows `incoming` at its frame. Accessibility still reaches the first tab when
/// `reachable`, so the Resume cycle parks it before observing.
fn select_tab_while_paused(
    runtime: &mut WorkspaceRuntime<FakeBackend>,
    reachable: bool,
    incoming: &[&crate::PhysicalWindowId],
) {
    assert_eq!(runtime.restore_all(), 0);
    let group = runtime.backend.frames[&first_tab()];
    if !reachable {
        runtime.backend.frames.remove(&first_tab());
    }
    runtime.backend.server_frames.insert(first_tab(), group);
    runtime.backend.ordered_out.insert(first_tab().window_id);
    for window in incoming {
        runtime.backend.frames.insert((*window).clone(), group);
    }
    runtime.backend.observation = Some(observe(&[
        (DOCUMENTS, incoming),
        ("com.example.Other", &[&other()]),
        ("com.example.Shown", &[&shown()]),
    ]));
}

fn resume_keeps_the_group_on_workspace_2(reachable: bool) {
    let cancelled = AtomicBool::new(false);
    let mut runtime = hidden_group();
    let slot = runtime
        .workspaces
        .engine(WorkspaceId::TWO)
        .slot_for_window(&first_tab());
    select_tab_while_paused(&mut runtime, reachable, &[&third_tab()]);

    // Resume: the selected tab keeps the group's tile on workspace 2, and the first tab
    // keeps no recovery entry of its own.
    runtime.cycle(one_output(), &cancelled).unwrap();
    let engine = runtime.workspaces.engine(WorkspaceId::TWO);
    assert_eq!(engine.slot_for_window(&third_tab()), slot);
    assert_eq!(runtime.workspaces.workspace_of_any(&first_tab()), None);
    assert!(!runtime.parked.contains_key(&first_tab()));
    assert!(!runtime.backend.persisted.contains_key(&first_tab()));
    runtime.cycle(one_output(), &cancelled).unwrap();
    assert!(runtime.parked.contains_key(&third_tab()));
    assert!(runtime.parked.contains_key(&other()));

    // Workspace 2 opens with both of its windows, even with the first tab unreachable.
    runtime.backend.frames.remove(&first_tab());
    runtime
        .switch(WorkspaceId::TWO, one_output(), &cancelled)
        .unwrap();
    assert!(!runtime.parked.contains_key(&third_tab()));
    assert!(!runtime.parked.contains_key(&other()));
    assert_eq!(
        runtime.workspaces.workspace_of(&shown()),
        Some(WorkspaceId::THREE)
    );
}

#[test]
fn a_tab_selected_while_paused_takes_its_tile_on_the_hidden_workspace() {
    resume_keeps_the_group_on_workspace_2(false);
}

#[test]
fn a_tab_selected_while_paused_takes_its_tile_after_resume_parks_the_old_tab() {
    resume_keeps_the_group_on_workspace_2(true);
}

#[test]
fn two_new_windows_at_the_groups_corner_claim_no_tile() {
    let fourth_tab = app_window(1, 1, 14);
    let cancelled = AtomicBool::new(false);
    let mut runtime = hidden_group();
    let slot = runtime
        .workspaces
        .engine(WorkspaceId::TWO)
        .slot_for_window(&first_tab());
    select_tab_while_paused(&mut runtime, false, &[&third_tab(), &fourth_tab]);

    runtime.cycle(one_output(), &cancelled).unwrap();

    let engine = runtime.workspaces.engine(WorkspaceId::TWO);
    assert_eq!(engine.slot_for_window(&first_tab()), slot);
    for window in [third_tab(), fourth_tab] {
        assert_ne!(
            runtime.workspaces.workspace_of_any(&window),
            Some(WorkspaceId::TWO)
        );
    }
}

#[test]
fn a_window_parked_before_resume_keeps_its_tile_and_recovery_frame() {
    // Parked on workspace 2 long ago, then out of reach on another macOS Space: the window
    // server lists it ordered out and Accessibility cannot reach it. A new window of its app
    // opening at its recovery frame's corner is a new window, not its tab.
    let newer = app_window(1, 1, 14);
    let cancelled = AtomicBool::new(false);
    let mut runtime = hidden_group();
    let recovery = runtime.parked[&first_tab()];
    let parked_at = runtime.backend.frames.remove(&first_tab()).unwrap();
    runtime.backend.server_frames.insert(first_tab(), parked_at);
    runtime.backend.ordered_out.insert(first_tab().window_id);
    runtime.backend.frames.insert(newer.clone(), recovery);
    runtime.backend.observation = Some(observe(&[
        (DOCUMENTS, &[&newer]),
        ("com.example.Other", &[&other()]),
        ("com.example.Shown", &[&shown()]),
    ]));

    runtime.cycle(one_output(), &cancelled).unwrap();

    assert_eq!(
        runtime.workspaces.workspace_of(&first_tab()),
        Some(WorkspaceId::TWO)
    );
    assert_eq!(runtime.parked.get(&first_tab()), Some(&recovery));
    assert_ne!(
        runtime.workspaces.workspace_of_any(&newer),
        Some(WorkspaceId::TWO)
    );
}
