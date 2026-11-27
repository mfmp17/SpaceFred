use super::*;

fn external_1080p() -> macos::DisplayOutputs {
    macos::DisplayOutputs::new(vec![macos::DisplayOutput {
        id: "external-1080p".into(),
        name: "External".into(),
        frame: crate::Rect {
            x: 0,
            y: 0,
            width: 1920,
            height: 1080,
        },
        is_main: true,
    }])
    .unwrap()
}

fn builtin_1512() -> macos::DisplayOutputs {
    macos::DisplayOutputs::new(vec![macos::DisplayOutput {
        id: "builtin-1512".into(),
        name: "Built-in".into(),
        frame: crate::Rect {
            x: 0,
            y: 0,
            width: 1512,
            height: 982,
        },
        is_main: true,
    }])
    .unwrap()
}

fn app_window(pid: i32, window_id: u32) -> crate::PhysicalWindowId {
    crate::PhysicalWindowId {
        app: AppInstanceId {
            pid,
            launched_at: 1,
        },
        window_id,
    }
}

fn bottom_right_sliver() -> crate::Rect {
    // Exact geometry from issue #150: 1px x 32px sliver visible on 1512x982,
    // center (2262, 1399) outside every connected display.
    crate::Rect {
        x: 1511,
        y: 950,
        width: 1502,
        height: 898,
    }
}

#[test]
fn visible_tiled_window_left_in_bottom_right_sliver_after_display_disconnect_keeps_slot_and_retiles()
 {
    let external = external_1080p();
    let built_in = builtin_1512();
    let cancelled = AtomicBool::new(false);

    let browser = app_window(10, 100);
    let parked_windows = [
        (WorkspaceId::ONE, app_window(20, 200)),
        (WorkspaceId::new(2).unwrap(), app_window(21, 201)),
        (WorkspaceId::new(3).unwrap(), app_window(22, 202)),
        (WorkspaceId::new(4).unwrap(), app_window(23, 203)),
    ];

    let config = Config::parse(
        "version = 1
[[workspaces]]
workspace = 'B'
monitor = 'main'",
    )
    .unwrap();
    let ws_b = WorkspaceId::parse("B").unwrap();

    let mut frames = HashMap::from([(browser.clone(), external.main().frame)]);
    let ext_displays: Vec<_> = external.frames().collect();
    let mut parked_map = HashMap::new();
    for (_, win) in &parked_windows {
        let original = crate::Rect {
            x: 0,
            y: 34,
            width: 1502,
            height: 898,
        };
        let parked_pos =
            crate::geometry::parked_frame(external.main().frame, &ext_displays, original);
        frames.insert(win.clone(), parked_pos);
        parked_map.insert(win.clone(), original);
    }

    let backend = FakeBackend {
        frames,
        focused: Some(browser.clone()),
        frontmost: Some(browser.app.clone()),
        observation: Some(observation_for(
            "com.example.Browser",
            &browser,
            external.main().frame,
        )),
        ..FakeBackend::default()
    };
    let mut runtime = WorkspaceRuntime::with_backend_and_config(
        ManagementScope::Both,
        backend,
        parked_map,
        config,
    );
    for (ws, win) in &parked_windows {
        bind(&mut runtime, *ws, win);
    }
    runtime.switch(ws_b, external.clone(), &cancelled).unwrap();
    runtime.cycle(external, &cancelled).unwrap();

    let slot_before = runtime
        .workspaces
        .engine(ws_b)
        .model()
        .workspace
        .slots
        .iter()
        .find(|slot| slot.bound_window() == Some(&browser))
        .expect("browser bound on workspace B")
        .id;

    // External display disconnected, lid opened to built-in 1512x982.
    // macOS leaves visible window A in a bottom-right sliver off-center.
    let sliver = bottom_right_sliver();
    assert!(!built_in.main().frame.contains_center(sliver));
    runtime.backend.frames.insert(browser.clone(), sliver);

    runtime.cycle(built_in.clone(), &cancelled).unwrap();

    assert_eq!(runtime.workspaces.workspace_of(&browser), Some(ws_b));
    let slot_after = runtime
        .workspaces
        .engine(ws_b)
        .model()
        .workspace
        .slots
        .iter()
        .find(|slot| slot.bound_window() == Some(&browser))
        .expect("browser kept its slot on workspace B")
        .id;
    assert_eq!(slot_after, slot_before);
    assert_eq!(runtime.backend.frames[&browser], built_in.main().frame);
    assert!(
        built_in
            .main()
            .frame
            .contains_center(runtime.backend.frames[&browser])
    );

    // Hidden parked windows remain owned by their hidden workspaces and off-screen.
    for (ws, win) in &parked_windows {
        assert_eq!(runtime.workspaces.workspace_of(win), Some(*ws));
        assert!(runtime.parked.contains_key(win));
        assert!(
            !built_in
                .main()
                .frame
                .contains_center(runtime.backend.frames[win])
        );
    }
}

#[test]
fn transient_recovery_failure_after_display_disconnect_preserves_slot_until_window_moves_back() {
    let external = external_1080p();
    let built_in = builtin_1512();
    let cancelled = AtomicBool::new(false);
    let browser = app_window(10, 100);

    let backend = FakeBackend {
        frames: HashMap::from([(browser.clone(), external.main().frame)]),
        observation: Some(observation_for(
            "com.example.Browser",
            &browser,
            external.main().frame,
        )),
        ..FakeBackend::default()
    };
    let mut runtime = runtime(backend);
    runtime.cycle(external, &cancelled).unwrap();
    let slot_before = runtime
        .workspaces
        .engine(WorkspaceId::ONE)
        .model()
        .workspace
        .slots[0]
        .id;

    // App is briefly unresponsive right after display disconnect.
    let sliver = bottom_right_sliver();
    runtime.backend.frames.insert(browser.clone(), sliver);
    runtime.backend.managed_outcome = Some(macos::ApplyOutcome::Unavailable);

    let status = runtime.cycle(built_in.clone(), &cancelled).unwrap();
    assert!(status.failed >= 1);
    assert_eq!(
        runtime.workspaces.workspace_of(&browser),
        Some(WorkspaceId::ONE),
        "on-screen stranded bound window must not lose its slot while recovery settles"
    );
    assert!(runtime.omitted_windows.contains_key(&browser));

    // App responds on the next cycle: window snaps back onto built-in display
    // with the exact same slot.
    runtime.backend.managed_outcome = None;
    let status = runtime.cycle(built_in.clone(), &cancelled).unwrap();
    assert_eq!(status.failed, 0);
    assert_eq!(
        runtime.workspaces.workspace_of(&browser),
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
        slot_before
    );
    assert_eq!(runtime.backend.frames[&browser], built_in.main().frame);
    assert!(runtime.omitted_windows.is_empty());
}

#[test]
fn multi_window_workspace_keeps_order_when_one_tile_is_stranded_after_disconnect() {
    let external = external_1080p();
    let built_in = builtin_1512();
    let cancelled = AtomicBool::new(false);
    let left = app_window(10, 100);
    let right = app_window(10, 101);

    let backend = FakeBackend {
        frames: HashMap::from([
            (
                left.clone(),
                crate::Rect {
                    x: 0,
                    y: 0,
                    width: 960,
                    height: 1080,
                },
            ),
            (
                right.clone(),
                crate::Rect {
                    x: 960,
                    y: 0,
                    width: 960,
                    height: 1080,
                },
            ),
        ]),
        observation: Some(observation_for_windows(
            "com.example.Browser",
            &left,
            &[
                (
                    left.clone(),
                    crate::Rect {
                        x: 0,
                        y: 0,
                        width: 960,
                        height: 1080,
                    },
                ),
                (
                    right.clone(),
                    crate::Rect {
                        x: 960,
                        y: 0,
                        width: 960,
                        height: 1080,
                    },
                ),
            ],
        )),
        ..FakeBackend::default()
    };
    let mut runtime = runtime(backend);
    runtime.cycle(external, &cancelled).unwrap();
    let slots_before: Vec<_> = runtime
        .workspaces
        .engine(WorkspaceId::ONE)
        .model()
        .workspace
        .slots
        .iter()
        .map(|slot| (slot.id, slot.bound_window().cloned()))
        .collect();

    // Disconnect external display: right tile is left in the bottom-right sliver
    // while left tile remains on screen.
    runtime.backend.frames.insert(
        left.clone(),
        crate::Rect {
            x: 0,
            y: 0,
            width: 960,
            height: 982,
        },
    );
    runtime
        .backend
        .frames
        .insert(right.clone(), bottom_right_sliver());

    runtime.cycle(built_in.clone(), &cancelled).unwrap();

    let slots_after: Vec<_> = runtime
        .workspaces
        .engine(WorkspaceId::ONE)
        .model()
        .workspace
        .slots
        .iter()
        .map(|slot| (slot.id, slot.bound_window().cloned()))
        .collect();
    assert_eq!(slots_after, slots_before);
    assert_eq!(
        runtime.backend.frames[&left],
        crate::Rect {
            x: 0,
            y: 0,
            width: 756,
            height: 982,
        }
    );
    assert_eq!(
        runtime.backend.frames[&right],
        crate::Rect {
            x: 756,
            y: 0,
            width: 756,
            height: 982,
        }
    );
}

#[test]
fn stranded_floating_window_on_visible_workspace_returns_to_built_in_display() {
    let external = external_1080p();
    let built_in = builtin_1512();
    let cancelled = AtomicBool::new(false);
    let dialog = app_window(10, 100);
    let initial = crate::Rect {
        x: 1200,
        y: 300,
        width: 500,
        height: 400,
    };
    let config = Config::parse(
        "version = 1
[[rules]]
bundle_id = 'com.example.Dialog'
action = 'float'",
    )
    .unwrap();
    let backend = FakeBackend {
        frames: HashMap::from([(dialog.clone(), initial)]),
        observation: Some(observation_for("com.example.Dialog", &dialog, initial)),
        ..FakeBackend::default()
    };
    let mut runtime = runtime_with_config(backend, config);
    runtime.cycle(external, &cancelled).unwrap();
    assert_eq!(
        runtime.workspaces.floating_workspace_of(&dialog),
        Some(WorkspaceId::ONE)
    );

    // After disconnect, macOS leaves the floating window off the built-in display.
    let stranded = crate::Rect {
        x: 1511,
        y: 950,
        width: 500,
        height: 400,
    };
    runtime.backend.frames.insert(dialog.clone(), stranded);

    runtime.cycle(built_in.clone(), &cancelled).unwrap();

    assert_eq!(
        runtime.workspaces.floating_workspace_of(&dialog),
        Some(WorkspaceId::ONE)
    );
    let recovered = runtime.backend.frames[&dialog];
    assert_eq!((recovered.width, recovered.height), (500, 400));
    assert!(built_in.main().frame.contains_center(recovered));
}

#[test]
fn off_display_window_being_dragged_with_mouse_down_is_not_yanked_back() {
    let built_in = builtin_1512();
    let cancelled = AtomicBool::new(false);
    let browser = app_window(10, 100);

    let backend = FakeBackend {
        frames: HashMap::from([(browser.clone(), built_in.main().frame)]),
        observation: Some(observation_for(
            "com.example.Browser",
            &browser,
            built_in.main().frame,
        )),
        ..FakeBackend::default()
    };
    let mut runtime = runtime(backend);
    runtime.cycle(built_in.clone(), &cancelled).unwrap();

    // User drags the window until its center is off the display.
    let sliver = bottom_right_sliver();
    runtime.backend.frames.insert(browser.clone(), sliver);
    runtime.backend.mouse_down = true;
    runtime.backend.observation.as_mut().unwrap().mouse_down = true;
    runtime.backend.calls.clear();

    runtime.cycle(built_in.clone(), &cancelled).unwrap();
    assert_eq!(runtime.backend.frames[&browser], sliver);
    assert_eq!(
        runtime.workspaces.workspace_of(&browser),
        Some(WorkspaceId::ONE)
    );
    assert!(
        !runtime.omitted_windows.contains_key(&browser),
        "pointer drag off-display must not burn omission settle time"
    );

    // Once the user releases the mouse, the window snaps back cleanly.
    runtime.backend.mouse_down = false;
    runtime.backend.observation.as_mut().unwrap().mouse_down = false;
    runtime.cycle(built_in.clone(), &cancelled).unwrap();
    assert_eq!(runtime.backend.frames[&browser], built_in.main().frame);
    assert_eq!(
        runtime.workspaces.workspace_of(&browser),
        Some(WorkspaceId::ONE)
    );
}

#[test]
fn ordered_out_window_off_display_is_not_moved_by_stranded_recovery() {
    let external = external_1080p();
    let built_in = builtin_1512();
    let cancelled = AtomicBool::new(false);
    let win = app_window(10, 100);

    let backend = FakeBackend {
        frames: HashMap::from([(win.clone(), external.main().frame)]),
        observation: Some(observation_for(
            "com.example.App",
            &win,
            external.main().frame,
        )),
        ..FakeBackend::default()
    };
    let mut runtime = runtime(backend);
    runtime.cycle(external, &cancelled).unwrap();

    // Window is ordered out (minimized / other Space) while its server frame
    // still sits outside the smaller built-in display.
    let off_display = bottom_right_sliver();
    runtime.backend.frames.remove(&win);
    runtime
        .backend
        .server_frames
        .insert(win.clone(), off_display);
    runtime.backend.ordered_out.insert(win.window_id);
    runtime.backend.calls.clear();

    runtime.cycle(built_in, &cancelled).unwrap();
    assert!(
        !runtime.backend.calls.iter().any(|call| matches!(
            call,
            BackendCall::Apply(applied, _) | BackendCall::SetPosition(applied, _) if applied == &win
        )),
        "ordered-out window must not be moved by stranded visible recovery"
    );
}

#[test]
fn clamshell_display_change_does_not_adopt_hidden_windows_when_repark_is_transiently_gone() {
    let before = builtin_1512();
    let after = external_1080p();
    let external = after.main().frame;

    let active = window(10);
    let hidden_win = window(11);
    let hidden_ws = WorkspaceId::new(2).unwrap();
    let original = crate::Rect {
        x: 0,
        y: 31,
        width: 1512,
        height: 951,
    };
    let mut backend = FakeBackend {
        observation: Some(observation(&active, original)),
        frontmost: Some(active.app.clone()),
        ..FakeBackend::default()
    };
    backend.frames.insert(active.clone(), original);
    // Simulate macOS pulling a hidden window onto the new external display while
    // AX transiently fails with Gone (not in backend.frames) during clamshell.
    backend.server_frames.insert(hidden_win.clone(), external);

    let mut runtime = runtime_with_parked(backend, [(hidden_win.clone(), original)]);
    bind(&mut runtime, WorkspaceId::ONE, &active);
    bind(&mut runtime, hidden_ws, &hidden_win);
    runtime.note_outputs(&before);

    // Pre-switch wobble + post-switch cycles where AX returns Gone for hidden_win.
    for _ in 0..=(REPARK_ATTEMPT_LIMIT + 3) {
        runtime.follow_displays(&after);
        let server_frames = runtime.backend.window_server_frames().unwrap();
        let visible = runtime.visible_workspaces(&after);
        runtime.park_inactive(
            &after,
            &visible,
            &server_frames,
            &AtomicBool::new(false),
            &mut 0,
        );
    }

    assert_eq!(
        runtime.workspaces.workspace_of(&hidden_win),
        Some(hidden_ws),
        "transient Gone during clamshell display change must not adopt hidden window into active workspace"
    );
    assert!(runtime.parked.contains_key(&hidden_win));
    assert!(runtime.parked.awaits_display(&hidden_win));

    // Once AX recovers, follow_displays re-parks the window off the external display.
    runtime.backend.frames.insert(hidden_win.clone(), external);
    runtime.follow_displays(&after);
    assert!(!runtime.parked.awaits_display(&hidden_win));
    assert_eq!(
        runtime.workspaces.workspace_of(&hidden_win),
        Some(hidden_ws)
    );
    let expected = crate::geometry::parked_frame(external, &[external], external);
    assert_eq!(runtime.backend.frames[&hidden_win].x, expected.x);
}

/// #406: a parked window whose display was unplugged restores onto Main on
/// pause or quit, even when another display now covers its old position.
#[test]
fn restore_all_sends_a_window_from_an_unplugged_display_to_main() {
    let all = three_outputs();
    let displays = all.displays();
    let original = frame(displays[1].frame.x + 100);
    let window = window(60);
    for remaining_covers_it in [false, true] {
        let backend = FakeBackend {
            frames: HashMap::from([(window.clone(), original)]),
            observation: Some(observation(&window, original)),
            focused: Some(window.clone()),
            frontmost: Some(window.app.clone()),
            ..FakeBackend::default()
        };
        let mut runtime = runtime(backend);
        for action in [
            WorkerAction::Retile,
            WorkerAction::ToggleFloat,
            WorkerAction::SelectWorkspace(WorkspaceId::new(7).unwrap()),
        ] {
            runtime.backend.frames.insert(window.clone(), original);
            let Some(WorkerResult::Status { result, .. }) = runtime.execute(
                WorkerCommand::Action(action, all.clone()),
                &AtomicBool::new(false),
            ) else {
                panic!("missing worker status");
            };
            assert_eq!(result.unwrap().failed, 0);
        }
        assert_eq!(runtime.parked.get(&window), Some(&original));

        let mut remaining = displays[2].clone();
        if remaining_covers_it {
            remaining.frame = displays[1].frame;
        }
        let connected = macos::DisplayOutputs::new(vec![displays[0].clone(), remaining]).unwrap();
        let Some(WorkerResult::Restored { failed, .. }) = runtime.execute(
            WorkerCommand::RestoreAll(Some(connected)),
            &AtomicBool::new(false),
        ) else {
            panic!("missing restoration result");
        };
        assert_eq!(failed, 0);
        assert!(!runtime.backend.persisted.contains_key(&window));
        let restored = runtime.backend.frames[&window];
        assert!(
            all.main().frame.contains_center(restored),
            "covered: {remaining_covers_it}, restored at {restored:?}"
        );
    }
}

/// 2->1 display unplug where live windows are left with 0 px on the remaining
/// built-in display: visible windows (tiled and floating) recover onto the
/// built-in display, a newly-hidden workspace's window is clamped and parked
/// so Pause/Quit restores it on screen, and a set-aside (minimized) window is
/// left alone.
#[test]
fn two_to_one_unplug_recovers_zero_px_visible_and_parks_newly_hidden_workspace() {
    let built_in_output = macos::DisplayOutput {
        id: "builtin-1512".into(),
        name: "Built-in".into(),
        frame: crate::Rect {
            x: 0,
            y: 0,
            width: 1512,
            height: 982,
        },
        is_main: true,
    };
    let external_output = macos::DisplayOutput {
        id: "external-1080p".into(),
        name: "External".into(),
        frame: crate::Rect {
            x: 1512,
            y: 0,
            width: 1920,
            height: 1080,
        },
        is_main: false,
    };
    let two_displays =
        macos::DisplayOutputs::new(vec![built_in_output.clone(), external_output.clone()]).unwrap();
    let one_display = macos::DisplayOutputs::new(vec![built_in_output.clone()]).unwrap();
    let cancelled = AtomicBool::new(false);

    let visible_tile = app_window(10, 100);
    let visible_float = app_window(11, 101);
    let hidden_ext_win = app_window(12, 102);
    let minimized_tile = app_window(13, 103);

    let on_builtin = crate::Rect {
        x: 50,
        y: 50,
        width: 700,
        height: 600,
    };
    let on_external = crate::Rect {
        x: 1700,
        y: 100,
        width: 800,
        height: 600,
    };

    let config = Config::parse(
        "version = 1
[[workspaces]]
workspace = 1
monitor = 'main'
[[workspaces]]
workspace = 6
monitor = 'secondary'",
    )
    .unwrap();

    let complete_app = |win: &crate::PhysicalWindowId,
                        bundle: &str,
                        rect: crate::Rect,
                        resizable: bool,
                        focused: bool| crate::AppObservation::Complete {
        app: crate::ObservedApplication {
            id: win.app.clone(),
            bundle_id: bundle.into(),
        },
        focused: focused.then(|| win.clone()),
        windows: vec![crate::ObservedWindow {
            id: win.clone(),
            frame: rect,
            resizable,
            title: None,
            document: None,
        }],
    };
    let obs = crate::NativeObservation {
        cycle: 0,
        mouse_down: false,
        applications: vec![
            complete_app(&visible_tile, "com.example.Tile", on_builtin, true, true),
            complete_app(
                &visible_float,
                "com.example.Float",
                on_builtin,
                false,
                false,
            ),
            complete_app(
                &hidden_ext_win,
                "com.example.External",
                on_external,
                true,
                false,
            ),
            complete_app(
                &minimized_tile,
                "com.example.Minimized",
                on_builtin,
                true,
                false,
            ),
        ],
    };

    let backend = FakeBackend {
        frames: HashMap::from([
            (visible_tile.clone(), on_builtin),
            (visible_float.clone(), on_builtin),
            (hidden_ext_win.clone(), on_external),
            (minimized_tile.clone(), on_builtin),
        ]),
        focused: Some(visible_tile.clone()),
        frontmost: Some(visible_tile.app.clone()),
        observation: Some(obs),
        ..FakeBackend::default()
    };
    let mut runtime = runtime_with_config(backend, config);
    runtime.cycle(two_displays.clone(), &cancelled).unwrap();
    // Make Workspace 1 the most recently shown workspace so unplugging External
    // keeps Workspace 1 visible on Built-in and hides Workspace 6 immediately.
    runtime.workspaces.show_on("builtin-1512", WorkspaceId::ONE);
    let ws6 = WorkspaceId::new(6).unwrap();
    assert_eq!(
        runtime.workspaces.workspace_of(&visible_tile),
        Some(WorkspaceId::ONE)
    );
    assert_eq!(
        runtime.workspaces.floating_workspace_of(&visible_float),
        Some(WorkspaceId::ONE)
    );
    assert_eq!(runtime.workspaces.workspace_of(&hidden_ext_win), Some(ws6));
    assert_eq!(
        runtime.workspaces.workspace_of(&minimized_tile),
        Some(WorkspaceId::ONE)
    );

    // Unplug External: live windows sit completely off the remaining display
    // (0 px overlap -> omitted from CG OptionOnScreenOnly, simulated via
    // ordered_out) while AX still reaches them. `minimized_tile` is minimized
    // (`set_aside`) and must not be moved.
    let zero_px_off = crate::Rect {
        x: 2000,
        y: 120,
        width: 800,
        height: 600,
    };
    assert!(!crate::geometry::shows_on(
        zero_px_off,
        &[built_in_output.frame]
    ));
    for win in [
        &visible_tile,
        &visible_float,
        &hidden_ext_win,
        &minimized_tile,
    ] {
        runtime.backend.frames.insert(win.clone(), zero_px_off);
        runtime.backend.ordered_out.insert(win.window_id);
    }
    runtime.backend.set_aside.insert(minimized_tile.clone());
    runtime.backend.calls.clear();

    runtime.cycle(one_display.clone(), &cancelled).unwrap();

    // Visible tiled and floating windows were rescued onto Built-in.
    assert_eq!(
        runtime.workspaces.workspace_of(&visible_tile),
        Some(WorkspaceId::ONE)
    );
    assert!(
        built_in_output
            .frame
            .contains_center(runtime.backend.frames[&visible_tile])
    );
    assert_eq!(
        runtime.workspaces.floating_workspace_of(&visible_float),
        Some(WorkspaceId::ONE)
    );
    assert!(
        built_in_output
            .frame
            .contains_center(runtime.backend.frames[&visible_float])
    );

    // Minimized (set-aside) window was NOT moved by stranded recovery.
    assert!(!runtime.backend.calls.iter().any(|call| matches!(
        call,
        BackendCall::Apply(w, _) | BackendCall::SetPosition(w, _) if w == &minimized_tile
    )));

    // Newly-hidden workspace 6 window was clamped onto Built-in for recovery
    // and explicitly parked instead of being stranded unparked.
    assert_eq!(runtime.workspaces.workspace_of(&hidden_ext_win), Some(ws6));
    let recorded = runtime
        .parked
        .get(&hidden_ext_win)
        .copied()
        .expect("newly hidden off-display window must have a clamped recovery frame");
    assert!(
        built_in_output.frame.contains_center(recorded),
        "recovery frame {recorded:?} must sit on Built-in"
    );
    assert_eq!(
        runtime.backend.persisted.get(&hidden_ext_win),
        Some(&recorded)
    );

    // Pause / Quit restores the parked window squarely onto Built-in.
    assert_eq!(runtime.restore_all(), 0);
    assert!(
        built_in_output
            .frame
            .contains_center(runtime.backend.frames[&hidden_ext_win]),
        "restored frame {:?} must sit on Built-in",
        runtime.backend.frames[&hidden_ext_win]
    );
    assert!(runtime.backend.persisted.is_empty());
}

/// After an updater restart (`original_display: None` on disk) or display
/// rearrangement leaving a 2-px overlap, restoring a parked window must clamp
/// it squarely onto the remaining display instead of confirming arrival at the
/// 2-px off-screen sliver.
#[test]
fn restore_all_clamps_two_px_sliver_recovery_frame_onto_remaining_display() {
    let built_in = builtin_1512();
    let window = app_window(10, 100);
    let two_px_sliver = crate::Rect {
        x: 1510,
        y: 100,
        width: 800,
        height: 600,
    };
    assert!(crate::geometry::shows_on(
        two_px_sliver,
        &[built_in.main().frame]
    ));
    assert!(!built_in.main().frame.contains_center(two_px_sliver));

    let parked_pos = crate::geometry::parked_frame(
        built_in.main().frame,
        &[built_in.main().frame],
        two_px_sliver,
    );
    let backend = FakeBackend {
        frames: HashMap::from([(window.clone(), parked_pos)]),
        ..FakeBackend::default()
    };
    // Loaded from disk: original_display is None.
    let mut runtime = runtime_with_parked(backend, [(window.clone(), two_px_sliver)]);
    runtime.note_outputs(&built_in);

    assert_eq!(runtime.restore_all(), 0);
    let restored = runtime.backend.frames[&window];
    assert!(
        built_in.main().frame.contains_center(restored),
        "2-px sliver should clamp onto built-in display, got {restored:?}"
    );
    assert!(runtime.backend.persisted.is_empty());
}

#[test]
fn a_window_stranded_on_an_unplugged_display_still_parks_when_an_ignore_rule_matches_it() {
    // Only a window really on screen is released by a newly matching `ignore` rule (#663):
    // one the unplug left off every remaining display keeps a clamped recovery frame, so
    // Pause or its workspace brings it back where the user can see it.
    let built_in = builtin_1512().displays()[0].clone();
    let external = macos::DisplayOutput {
        id: "external-1080p".into(),
        name: "External".into(),
        frame: crate::Rect {
            x: 1512,
            y: 0,
            width: 1920,
            height: 1080,
        },
        is_main: false,
    };
    let two_displays = macos::DisplayOutputs::new(vec![built_in.clone(), external]).unwrap();
    let one_display = builtin_1512();
    let stranded = app_window(12, 102);
    let on_external = crate::Rect {
        x: 1700,
        y: 100,
        width: 800,
        height: 600,
    };
    let backend = FakeBackend {
        frames: HashMap::from([(stranded.clone(), on_external)]),
        rule_probes: HashMap::from([(102, ("com.example.Test", Some("Keep Visible"), true))]),
        ..FakeBackend::default()
    };
    let config =
        Config::parse("version = 1\n[[rules]]\nbundle_id = 'com.example.Test'\naction = 'ignore'")
            .unwrap();
    let mut runtime = runtime_with_config(backend, config);
    let hidden = WorkspaceId::new(2).unwrap();
    bind(&mut runtime, hidden, &stranded);
    runtime.note_outputs(&two_displays);
    runtime.note_outputs(&one_display);

    let mut failed = 0;
    let visible = runtime.visible_workspaces(&one_display);
    let server_frames = runtime.backend.window_server_frames().unwrap();
    runtime.park_inactive(
        &one_display,
        &visible,
        &server_frames,
        &AtomicBool::new(false),
        &mut failed,
    );

    assert_eq!(runtime.workspaces.workspace_of_any(&stranded), Some(hidden));
    let recorded = runtime
        .parked
        .get(&stranded)
        .copied()
        .expect("a stranded window keeps a recovery frame");
    assert!(built_in.frame.contains_center(recorded));
}
