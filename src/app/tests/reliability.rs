use super::*;

#[test]
fn floating_disposition_wins_over_pending_replacement_reservation() {
    for resizable in [true, false] {
        let old = window(10);
        let replacement = window(11);
        let mut observed = observation_for("com.mitchellh.ghostty", &replacement, frame(50));
        let crate::AppObservation::Complete { windows, .. } = &mut observed.applications[0] else {
            unreachable!()
        };
        windows[0].resizable = resizable;
        let config = if resizable {
            Config::parse(
                "version = 1\n[[rules]]\nbundle_id = 'com.mitchellh.ghostty'\naction = 'float'",
            )
            .unwrap()
        } else {
            Config::default()
        };
        let backend = FakeBackend {
            frames: HashMap::from([(replacement.clone(), frame(50))]),
            observation: Some(observed),
            ..FakeBackend::default()
        };
        let mut runtime = runtime_with_config(backend, config);
        runtime
            .workspaces
            .engine_mut(WorkspaceId::ONE)
            .insert_slot(Slot {
                id: SlotId(1),
                binding: BindingState::Unresolved {
                    previous: old,
                    candidates: vec![replacement.clone()],
                    since: 0,
                },
            });
        runtime.cycle(outputs(), &AtomicBool::new(false)).unwrap();
        assert_eq!(runtime.workspaces.workspace_of(&replacement), None);
        assert_eq!(
            runtime.workspaces.floating_workspace_of(&replacement),
            Some(WorkspaceId::ONE)
        );
    }
}

#[test]
fn parked_window_in_a_display_gap_is_not_reparked_or_exempted() {
    let mut displays = three_outputs().displays().to_vec();
    displays[1].frame.y = 900;
    let outputs = macos::DisplayOutputs::new(displays).unwrap();
    let window = window(10);
    let original = crate::Rect {
        x: 1100,
        y: 1000,
        width: 100,
        height: 100,
    };
    // Off the secondary display's right edge, below the third display.
    let parked = crate::Rect {
        x: 1999,
        y: 1600,
        ..original
    };
    assert!(!crate::geometry::shows_on(
        parked,
        &outputs.frames().collect::<Vec<_>>()
    ));
    assert!(outputs.union().contains_center(parked));
    assert!(
        !outputs
            .frames()
            .any(|display| display.contains_center(parked))
    );
    let backend = FakeBackend {
        frames: HashMap::from([(window.clone(), parked)]),
        observation: Some(observation(&window, parked)),
        ..FakeBackend::default()
    };
    let mut runtime = WorkspaceRuntime::with_backend(
        ManagementScope::Both,
        backend,
        HashMap::from([(window.clone(), original)]),
    );
    let workspace = WorkspaceId::new(7).unwrap();
    bind(&mut runtime, workspace, &window);
    for _ in 0..10 {
        runtime
            .cycle(outputs.clone(), &AtomicBool::new(false))
            .unwrap();
    }
    assert!(runtime.parked.reparks(&window) == 0);
    assert!(!runtime.parked.is_exempt(&window));
    assert!(
        !runtime
            .backend
            .calls
            .iter()
            .any(|call| matches!(call, BackendCall::SetPosition(_, _)))
    );
    assert_eq!(runtime.parked.get(&window), Some(&original));
}

#[test]
fn ambiguous_ghostty_replacement_stays_reserved_on_its_original_display() {
    let old = window(10);
    let other = window(20);
    let replacement = window(11);
    let outputs = outputs();
    let backend = FakeBackend {
        frames: HashMap::from([(old.clone(), frame(50)), (other.clone(), frame(1050))]),
        focused: Some(old.clone()),
        frontmost: Some(old.app.clone()),
        observation: Some(observation_for_windows(
            "com.mitchellh.ghostty",
            &old,
            &[(old.clone(), frame(50)), (other.clone(), frame(1050))],
        )),
        ..FakeBackend::default()
    };
    let mut runtime = runtime(backend);
    bind(&mut runtime, WorkspaceId::ONE, &old);
    bind(&mut runtime, WorkspaceId::SIX, &other);
    let cancelled = AtomicBool::new(false);
    runtime.cycle(outputs.clone(), &cancelled).unwrap();
    let slot = runtime
        .workspaces
        .engine(WorkspaceId::ONE)
        .model()
        .workspace
        .slots[0]
        .id;
    runtime.backend.frames.remove(&old);
    let changed_frame = crate::Rect {
        width: 997,
        ..outputs.main().frame
    };
    runtime
        .backend
        .frames
        .insert(replacement.clone(), changed_frame);
    runtime.backend.focused = Some(replacement.clone());
    runtime.backend.observation = Some(observation_for_windows(
        "com.mitchellh.ghostty",
        &replacement,
        &[
            (replacement.clone(), changed_frame),
            (other.clone(), outputs.secondary().unwrap().frame),
        ],
    ));
    runtime.cycle(outputs.clone(), &cancelled).unwrap();
    assert!(matches!(
        runtime
            .workspaces
            .engine(WorkspaceId::ONE)
            .model()
            .workspace
            .slots[0]
            .binding,
        BindingState::Unresolved { .. }
    ));
    runtime.cycle(outputs.clone(), &cancelled).unwrap();
    assert_ne!(
        runtime.workspaces.workspace_of(&replacement),
        Some(WorkspaceId::SIX)
    );
    // Even drift to the other display must not steal a reserved candidate.
    runtime
        .backend
        .frames
        .insert(replacement.clone(), frame(1050));
    runtime.cycle(outputs.clone(), &cancelled).unwrap();
    assert_ne!(
        runtime.workspaces.workspace_of(&replacement),
        Some(WorkspaceId::SIX)
    );
    // Fresh exact evidence at the settling boundary still preserves identity.
    runtime
        .backend
        .frames
        .insert(replacement.clone(), outputs.main().frame);
    runtime.cycle(outputs, &cancelled).unwrap();
    assert_eq!(
        runtime.workspaces.workspace_of(&replacement),
        Some(WorkspaceId::ONE)
    );
    assert_eq!(
        runtime
            .workspaces
            .engine(WorkspaceId::ONE)
            .model()
            .workspace
            .slots[0]
            .id,
        slot
    );
}

#[test]
fn startup_recovers_parked_window_missing_from_valid_workspace_state() {
    let window = window(10);
    let outputs = outputs();
    let normal = frame(50);
    let parked = crate::geometry::parked_frame(
        outputs.main().frame,
        &outputs.frames().collect::<Vec<_>>(),
        normal,
    );
    // Crash after durable parking, before new ownership was persisted.
    let backend = FakeBackend {
        frames: HashMap::from([(window.clone(), parked)]),
        observation: Some(observation(&window, normal)),
        ..FakeBackend::default()
    };
    let mut runtime = WorkspaceRuntime::with_backend_config_state(
        ManagementScope::Both,
        backend,
        HashMap::from([(window.clone(), normal)]),
        Config::default(),
        Some(Workspaces::default().state()),
    );
    runtime
        .cycle(outputs.clone(), &AtomicBool::new(false))
        .unwrap();
    assert!(
        outputs
            .main()
            .frame
            .contains_center(runtime.backend.frames[&window])
    );
    assert!(!runtime.parked.contains_key(&window));
    assert!(!runtime.backend.persisted.contains_key(&window));
    assert_eq!(
        runtime.workspaces.workspace_of(&window),
        Some(WorkspaceId::ONE)
    );
}

#[test]
fn orphan_recovery_failure_prevents_adoption_until_durable_confirmation() {
    let window = window(10);
    let normal = frame(50);
    let backend = FakeBackend {
        fail_persist: true,
        frames: HashMap::from([(window.clone(), frame(-5000))]),
        observation: Some(observation(&window, normal)),
        ..FakeBackend::default()
    };
    let mut runtime = WorkspaceRuntime::with_backend_config_state(
        ManagementScope::Both,
        backend,
        HashMap::from([(window.clone(), normal)]),
        Config::default(),
        Some(Workspaces::default().state()),
    );
    assert!(runtime.cycle(outputs(), &AtomicBool::new(false)).is_err());
    assert_eq!(runtime.parked.get(&window), Some(&normal));
    assert_eq!(runtime.workspaces.workspace_of(&window), None);
    runtime.backend.fail_persist = false;
    runtime.cycle(outputs(), &AtomicBool::new(false)).unwrap();
    assert!(!runtime.parked.contains_key(&window));
    assert_eq!(
        runtime.workspaces.workspace_of(&window),
        Some(WorkspaceId::ONE)
    );
}

#[test]
fn switch_keeps_outgoing_visible_when_incoming_reveal_never_arrives() {
    let outgoing = window(10);
    let incoming = window(20);
    let outputs = outputs();
    let normal = frame(100);
    let parked = crate::geometry::parked_frame(
        outputs.main().frame,
        &outputs.frames().collect::<Vec<_>>(),
        normal,
    );
    let backend = FakeBackend {
        frames: HashMap::from([(outgoing.clone(), frame(50)), (incoming.clone(), parked)]),
        server_frames: HashMap::from([(incoming.clone(), parked)]),
        frozen_server_frames: true,
        ..FakeBackend::default()
    };
    let mut runtime = WorkspaceRuntime::with_backend(
        ManagementScope::Both,
        backend,
        HashMap::from([(incoming.clone(), normal)]),
    );
    bind(&mut runtime, WorkspaceId::ONE, &outgoing);
    let target = WorkspaceId::new(2).unwrap();
    bind(&mut runtime, target, &incoming);
    assert!(
        runtime
            .switch(target, outputs.clone(), &AtomicBool::new(false))
            .is_err()
    );
    assert_eq!(
        runtime.workspaces.active_for(&DisplaySelector::Main),
        Some(WorkspaceId::ONE)
    );
    assert!(
        outputs
            .main()
            .frame
            .contains_center(runtime.backend.window_server_frame(&outgoing).unwrap())
    );
    // Rollback must supersede the outstanding asynchronous reveal write too.
    assert!(
        !outputs
            .main()
            .frame
            .contains_center(runtime.backend.frames[&incoming])
    );
    assert_eq!(runtime.parked.get(&incoming), Some(&normal));
    assert!(!runtime.parked.contains_key(&outgoing));
    runtime.backend.frozen_server_frames = false;
    runtime
        .switch(target, outputs, &AtomicBool::new(false))
        .unwrap();
    assert_eq!(
        runtime.workspaces.active_for(&DisplaySelector::Main),
        Some(target)
    );
    assert!(!runtime.parked.contains_key(&incoming));
}
