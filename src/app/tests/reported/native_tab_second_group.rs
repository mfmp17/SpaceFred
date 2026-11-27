//! #749: one app with two native tab groups, tiled first and second in Fullscreen Layout with
//! another app's window third. Selecting another tab in the second group ordered its tab out
//! while focus passed through the first group, which sits at the same deck frame. The short
//! wait for the incoming tab only covered an app's one tile, so the second group's slot was
//! retired and the incoming tab became a new tile at the end. Reported by @coygeek.

use super::super::reconciliation::{app_window, column, studio_display};
use super::super::*;
use crate::workspace::DeckMode;

const TABS: &str = "com.example.Tabs";
const OTHER: &str = "com.example.Other";

fn app(
    bundle_id: &str,
    focused: Option<&crate::PhysicalWindowId>,
    windows: &[&crate::PhysicalWindowId],
    frame: crate::Rect,
) -> crate::AppObservation {
    let id = windows
        .first()
        .or(focused.as_ref())
        .map(|window| window.app.clone())
        .expect("an app with a window");
    crate::AppObservation::Complete {
        app: crate::ObservedApplication {
            id,
            bundle_id: bundle_id.into(),
        },
        focused: focused.cloned(),
        windows: windows
            .iter()
            .map(|window| crate::ObservedWindow {
                id: (*window).clone(),
                frame,
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

#[test]
fn a_tab_switch_in_an_apps_second_tab_group_keeps_its_slot_in_fullscreen_layout() {
    let (first, second, next, other) = (
        app_window(1, 1, 10),
        app_window(1, 1, 13),
        app_window(1, 1, 14),
        app_window(2, 1, 20),
    );
    let deck = column(0, 2560);
    let mut runtime = runtime(FakeBackend {
        frames: [
            (first.clone(), deck),
            (second.clone(), deck),
            (other.clone(), deck),
        ]
        .into(),
        focused: Some(second.clone()),
        observation: Some(observe(vec![
            app(TABS, Some(&second), &[&first, &second], deck),
            app(OTHER, None, &[&other], deck),
        ])),
        ..FakeBackend::default()
    });
    bind(&mut runtime, WorkspaceId::ONE, &first);
    bind(&mut runtime, WorkspaceId::ONE, &second);
    bind(&mut runtime, WorkspaceId::ONE, &other);
    runtime
        .workspaces
        .set_deck(WorkspaceId::ONE, DeckMode::Monocle);
    runtime
        .cycle(studio_display(), &AtomicBool::new(false))
        .unwrap();
    let engine = runtime.workspaces.engine(WorkspaceId::ONE);
    let slots = [&first, &second, &other].map(|window| engine.slot_for_window(window));
    let frame = runtime.backend.frames[&second];

    // Previous Tab: the second group's tab is ordered out, the incoming one is listed at the
    // same frame, and focus is on the first group for a moment.
    runtime.backend.frames.remove(&second);
    runtime.backend.server_frames.insert(second.clone(), frame);
    runtime.backend.ordered_out.insert(second.window_id);
    runtime.backend.frames.insert(next.clone(), frame);
    runtime.backend.focused = Some(first.clone());
    runtime.backend.observation = Some(observe(vec![
        app(TABS, Some(&first), &[&first, &next], frame),
        app(OTHER, None, &[&other], frame),
    ]));
    runtime
        .cycle(studio_display(), &AtomicBool::new(false))
        .unwrap();
    assert_eq!(
        runtime.workspaces.workspace_of(&second),
        Some(WorkspaceId::ONE),
        "the second group keeps its tile while its next tab arrives"
    );

    runtime.backend.advance(Duration::from_millis(75));
    runtime.backend.focused = Some(next.clone());
    runtime.backend.observation = Some(observe(vec![
        app(TABS, Some(&next), &[&first, &next], frame),
        app(OTHER, None, &[&other], frame),
    ]));
    runtime
        .cycle(studio_display(), &AtomicBool::new(false))
        .unwrap();

    let engine = runtime.workspaces.engine(WorkspaceId::ONE);
    assert_eq!(
        [&first, &next, &other].map(|window| engine.slot_for_window(window)),
        slots
    );
    assert!(runtime.workspaces.deck(WorkspaceId::ONE).is_monocle());
}
