use super::*;

#[test]
fn background_unhide_during_a_fresh_park_is_re_parked_without_waiting_for_the_grace() {
    let tiled = window(10);
    let unhidden = window(11);
    let mut backend = FakeBackend {
        observation: Some(observation_for_windows(
            "com.example.Test",
            &tiled,
            &[(tiled.clone(), frame(50)), (unhidden.clone(), frame(600))],
        )),
        frontmost: Some(AppInstanceId {
            pid: 999,
            launched_at: 999,
        }),
        ..FakeBackend::default()
    };
    backend.frames.insert(tiled.clone(), frame(50));
    // Unhiding the app in the background put the window back on screen, away
    // from its pre-park frame (#693).
    backend.frames.insert(unhidden.clone(), frame(600));
    let mut runtime = runtime(backend);
    bind(&mut runtime, WorkspaceId::ONE, &tiled);
    let hidden = WorkspaceId::new(2).unwrap();
    bind(&mut runtime, hidden, &unhidden);
    runtime.parked.insert(unhidden.clone(), frame(400), None);
    // Parked from frame(400) a second ago; the grace is still running.
    let now = runtime.backend.now();
    runtime.parked.park_applied_from(&unhidden, now, frame(400));
    runtime.backend.advance(Duration::from_secs(1));

    runtime.cycle(outputs(), &AtomicBool::new(false)).unwrap();

    assert_eq!(runtime.parked.get(&unhidden), Some(&frame(400)));
    assert_eq!(runtime.workspaces.workspace_of(&unhidden), Some(hidden));
    assert!(
        !outputs()
            .union()
            .contains_center(runtime.backend.frames[&unhidden])
    );
}

#[test]
fn stale_readback_after_a_cross_display_park_waits_out_the_grace() {
    let tiled = window(10);
    let moved = window(11);
    let mut backend = FakeBackend {
        observation: Some(observation(&tiled, frame(50))),
        frontmost: Some(AppInstanceId {
            pid: 999,
            launched_at: 999,
        }),
        // The window server still reports the pre-park frame for a moment.
        frozen_server_frames: true,
        ..FakeBackend::default()
    };
    backend.frames.insert(tiled.clone(), frame(50));
    backend.frames.insert(moved.clone(), frame(400));
    backend.server_frames.insert(moved.clone(), frame(400));
    let mut runtime = runtime(backend);
    bind(&mut runtime, WorkspaceId::ONE, &tiled);
    let hidden = WorkspaceId::new(2).unwrap();
    bind(&mut runtime, hidden, &moved);
    let main = outputs().main().frame;
    let secondary = outputs().frames().find(|output| *output != main).unwrap();

    assert_eq!(
        runtime.park_window_across(&moved, main, secondary, &outputs()),
        macos::ApplyOutcome::Applied
    );
    // The recovery frame moved onto the other display; the window did not.
    let recovery = runtime.parked.get(&moved).copied().unwrap();
    assert_ne!(recovery, frame(400));
    let writes = runtime.backend.parked_writes.len();
    runtime.backend.advance(Duration::from_secs(1));

    let server_frames = runtime.backend.window_server_frames().unwrap();
    let visible = runtime.visible_workspaces(&outputs());
    runtime.park_inactive(
        &outputs(),
        &visible,
        &server_frames,
        &AtomicBool::new(false),
        &mut 0,
    );

    assert_eq!(runtime.backend.parked_writes.len(), writes);
    assert_eq!(runtime.parked.reparks(&moved), 0);
    assert_eq!(runtime.parked.get(&moved), Some(&recovery));
    assert_eq!(runtime.workspaces.workspace_of(&moved), Some(hidden));
}
