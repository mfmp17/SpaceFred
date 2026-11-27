//! #722: A switch rolled back because a window's app stopped answering. Its restore
//! write had already been sent and landed once the app answered again, so the window
//! came onto the display while its workspace stayed hidden and still counted it as
//! parked. Reported by @coygeek.

use super::super::*;

#[test]
fn a_window_whose_app_stalls_mid_switch_stays_parked_when_the_app_answers_again() {
    let outgoing = window(10);
    // A separate app, so only this window stops answering.
    let stalled = crate::PhysicalWindowId {
        app: AppInstanceId {
            pid: 2,
            launched_at: 1,
        },
        window_id: 20,
    };
    let outputs = outputs();
    let original = frame(100);
    let parked = main_parked_frame(original);
    let displays = outputs.frames().collect::<Vec<_>>();
    let mut backend = FakeBackend::default();
    backend.frames.insert(outgoing.clone(), frame(50));
    backend.frames.insert(stalled.clone(), parked);
    // Spaceman parked the window earlier, so it holds a handle to it.
    backend.handles.insert(stalled.clone());
    let mut runtime = runtime_with_parked(backend, [(stalled.clone(), original)]);
    bind(&mut runtime, WorkspaceId::ONE, &outgoing);
    let target = WorkspaceId::new(2).unwrap();
    bind(&mut runtime, target, &stalled);
    runtime.backend.unanswering.insert(stalled.app.clone());

    assert!(
        runtime
            .switch(target, outputs.clone(), &AtomicBool::new(false))
            .is_err()
    );
    assert!(
        runtime.backend.queued_writes.iter().any(|(window, frame)| {
            *window == stalled && crate::geometry::shows_on(*frame, &displays)
        }),
        "the restore reached the app before the switch gave up"
    );

    runtime.backend.answer_again(&stalled.app);

    assert_eq!(
        runtime.workspaces.active_for(&DisplaySelector::Main),
        Some(WorkspaceId::ONE)
    );
    assert_eq!(runtime.parked.get(&stalled), Some(&original));
    let landed = runtime.backend.frames[&stalled];
    assert!(
        !crate::geometry::shows_on(landed, &displays),
        "the window came on screen at {landed:?} once its app answered"
    );
    assert_eq!(runtime.backend.frames[&outgoing], frame(50));
}
