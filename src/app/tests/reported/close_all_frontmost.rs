//! #516: closing an app's visible tile and its parked one together, with the app still frontmost
//! and no focused window, switched to the parked window's now-empty workspace. Reported by
//! @coygeek.

use super::super::*;

#[test]
fn frontmost_app_whose_windows_all_closed_does_not_pull_a_switch() {
    // The parked slot is not yet retired, but the window server no longer
    // lists it, so there is nothing on workspace 2 to follow.
    let closed = window(20);
    let backend = FakeBackend {
        frontmost: Some(closed.app.clone()),
        observation: Some(crate::NativeObservation {
            cycle: 0,
            mouse_down: false,
            applications: Vec::new(),
        }),
        ..FakeBackend::default()
    };
    let mut parked = HashMap::new();
    parked.insert(closed.clone(), frame(50));
    let mut runtime = WorkspaceRuntime::with_backend(ManagementScope::Both, backend, parked);
    bind(&mut runtime, WorkspaceId::new(2).unwrap(), &closed);

    let status = runtime.cycle(outputs(), &AtomicBool::new(false)).unwrap();

    assert_eq!(status.active, vec![WorkspaceId::ONE, WorkspaceId::SIX]);
    assert_eq!(
        runtime.workspaces.workspace_of_any(&closed),
        Some(WorkspaceId::new(2).unwrap())
    );
}

#[test]
fn unresolved_slot_with_no_live_candidates_does_not_pull_a_switch() {
    let gone_original = window(20);
    let gone_candidate = window(21);
    let mut workspaces = Workspaces::default();
    let ws2 = WorkspaceId::new(2).unwrap();
    workspaces.engine_mut(ws2).insert_slot(crate::Slot {
        id: crate::SlotId(1),
        binding: crate::BindingState::Unresolved {
            previous: gone_original.clone(),
            candidates: vec![gone_candidate.clone()],
            since: 1,
        },
    });

    assert_eq!(
        workspaces.workspace_for_live_app(&gone_original.app, |_| false),
        None
    );
    assert_eq!(
        workspaces.workspace_for_live_app(&gone_original.app, |w| w == &gone_candidate),
        Some(ws2)
    );
}
