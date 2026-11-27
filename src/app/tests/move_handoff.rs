use super::*;

fn app_window(pid: i32, window_id: u32) -> crate::PhysicalWindowId {
    crate::PhysicalWindowId {
        app: AppInstanceId {
            pid,
            launched_at: 1,
        },
        window_id,
    }
}

fn observe(windows: &[(&crate::PhysicalWindowId, crate::Rect)]) -> crate::NativeObservation {
    crate::NativeObservation {
        cycle: 0,
        mouse_down: false,
        applications: windows
            .iter()
            .map(|(window, rect)| crate::AppObservation::Complete {
                app: crate::ObservedApplication {
                    id: window.app.clone(),
                    bundle_id: format!("com.example.App{}", window.app.pid),
                },
                focused: Some((*window).clone()),
                windows: vec![crate::ObservedWindow {
                    id: (*window).clone(),
                    frame: *rect,
                    resizable: true,
                    title: None,
                    document: None,
                }],
            })
            .collect(),
    }
}

#[test]
fn move_focused_to_hidden_workspace_hands_focus_to_a_remaining_tile() {
    let first = app_window(1, 10);
    let second = app_window(2, 20);
    let mut backend = FakeBackend {
        focused: Some(second.clone()),
        frontmost: Some(second.app.clone()),
        observation: Some(observe(&[(&first, frame(0)), (&second, frame(500))])),
        ..FakeBackend::default()
    };
    backend.frames.insert(first.clone(), frame(0));
    backend.frames.insert(second.clone(), frame(500));
    let mut rt = runtime(backend);
    bind(&mut rt, WorkspaceId::ONE, &first);
    bind(&mut rt, WorkspaceId::ONE, &second);
    let cancelled = AtomicBool::new(false);

    rt.move_focused(WorkspaceId::TWO, outputs(), &cancelled)
        .unwrap();

    assert!(rt.parked.contains_key(&second));
    assert_eq!(rt.workspaces.workspace_of(&second), Some(WorkspaceId::TWO));
    assert_eq!(rt.backend.focused.as_ref(), Some(&first));
    assert_eq!(rt.backend.frontmost.as_ref(), Some(&first.app));
    assert!(
        rt.backend
            .calls
            .contains(&BackendCall::Focus(first.clone()))
    );
    assert_eq!(
        rt.focus
            .in_flight(rt.backend.now())
            .map(|pending| &pending.target),
        Some(&first)
    );
    assert_eq!(rt.focus.guarded_window(), Some(&second));

    // A subsequent cycle while focus has settled on `first` stays on workspace 1.
    rt.cycle(outputs(), &cancelled).unwrap();
    assert_eq!(
        rt.workspaces.active_for(&DisplaySelector::Main),
        Some(WorkspaceId::ONE)
    );
    assert_eq!(rt.focus.guarded_window().cloned(), None);
}

#[test]
fn move_focused_to_hidden_workspace_hands_focus_to_remaining_float_when_no_tiles_left() {
    let tile = app_window(1, 10);
    let floating = app_window(2, 20);
    let mut backend = FakeBackend {
        focused: Some(tile.clone()),
        frontmost: Some(tile.app.clone()),
        ..FakeBackend::default()
    };
    backend.frames.insert(tile.clone(), frame(0));
    backend.frames.insert(floating.clone(), frame(200));
    let mut rt = runtime(backend);
    bind(&mut rt, WorkspaceId::ONE, &tile);
    rt.workspaces
        .own_floating(floating.clone(), WorkspaceId::ONE);

    rt.move_focused(WorkspaceId::TWO, outputs(), &AtomicBool::new(false))
        .unwrap();

    assert_eq!(rt.backend.focused.as_ref(), Some(&floating));
    assert!(rt.backend.calls.contains(&BackendCall::Focus(floating)));
}

/// #743: the only other window left on the source workspace is a float the
/// user minimized. Moving the last tile to a hidden workspace must not focus
/// it (that would bring it back out of the Dock); focus is left alone.
#[test]
fn move_focused_to_hidden_workspace_skips_a_minimized_remaining_float() {
    let tile = app_window(1, 10);
    let floating = app_window(2, 20);
    let mut backend = FakeBackend {
        focused: Some(tile.clone()),
        frontmost: Some(tile.app.clone()),
        ..FakeBackend::default()
    };
    backend.frames.insert(tile.clone(), frame(0));
    backend.frames.insert(floating.clone(), frame(200));
    backend.set_aside.insert(floating.clone());
    backend.ordered_out.insert(floating.window_id);
    let mut rt = runtime(backend);
    bind(&mut rt, WorkspaceId::ONE, &tile);
    rt.workspaces
        .own_floating(floating.clone(), WorkspaceId::ONE);

    rt.move_focused(WorkspaceId::TWO, outputs(), &AtomicBool::new(false))
        .unwrap();

    assert!(rt.parked.contains_key(&tile));
    assert_eq!(
        rt.workspaces.floating_workspace_of(&floating),
        Some(WorkspaceId::ONE)
    );
    assert!(rt.backend.calls.iter().all(|call| !matches!(
        call,
        BackendCall::Focus(window) | BackendCall::Raise(window) if window == &floating
    )));
    assert_eq!(rt.focus.guarded_window(), Some(&tile));
    assert_eq!(
        rt.workspaces.active_for(&DisplaySelector::Main),
        Some(WorkspaceId::ONE)
    );
}

/// #743: a minimized float is passed over for a tile still on screen.
#[test]
fn move_focused_to_hidden_workspace_prefers_a_shown_tile_over_a_minimized_float() {
    let first = app_window(1, 10);
    let second = app_window(2, 20);
    let floating = app_window(3, 30);
    let mut backend = FakeBackend {
        focused: Some(second.clone()),
        frontmost: Some(second.app.clone()),
        ..FakeBackend::default()
    };
    backend.frames.insert(first.clone(), frame(0));
    backend.frames.insert(second.clone(), frame(500));
    backend.frames.insert(floating.clone(), frame(200));
    backend.set_aside.insert(floating.clone());
    backend.ordered_out.insert(floating.window_id);
    let mut rt = runtime(backend);
    rt.workspaces
        .own_floating(floating.clone(), WorkspaceId::ONE);
    bind(&mut rt, WorkspaceId::ONE, &first);
    bind(&mut rt, WorkspaceId::ONE, &second);
    // The float was the workspace's last focused float before it was minimized.
    rt.workspaces.remember_focus(WorkspaceId::ONE, &floating);
    assert_eq!(
        rt.workspaces.preferred_float_focus(WorkspaceId::ONE),
        Some(floating.clone())
    );

    rt.move_focused(WorkspaceId::TWO, outputs(), &AtomicBool::new(false))
        .unwrap();

    assert_eq!(rt.backend.focused.as_ref(), Some(&first));
    assert!(!rt.backend.calls.contains(&BackendCall::Focus(floating)));
}

#[test]
fn move_focused_follow_skips_source_handoff_and_focuses_moved_window_on_target() {
    let first = app_window(1, 10);
    let second = app_window(2, 20);
    let mut backend = FakeBackend {
        focused: Some(second.clone()),
        frontmost: Some(second.app.clone()),
        ..FakeBackend::default()
    };
    backend.frames.insert(first.clone(), frame(0));
    backend.frames.insert(second.clone(), frame(500));
    let mut rt = runtime(backend);
    bind(&mut rt, WorkspaceId::ONE, &first);
    bind(&mut rt, WorkspaceId::ONE, &second);

    rt.move_focused_follow(WorkspaceId::TWO, outputs(), &AtomicBool::new(false))
        .unwrap();

    assert_eq!(
        rt.workspaces.active_for(&DisplaySelector::Main),
        Some(WorkspaceId::TWO)
    );
    assert_eq!(rt.backend.focused.as_ref(), Some(&second));
    let focused_calls: Vec<_> = rt
        .backend
        .calls
        .iter()
        .filter_map(|call| match call {
            BackendCall::Focus(window) => Some(window.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(focused_calls, vec![second]);
}

#[test]
fn move_app_to_hidden_workspace_hands_focus_to_remaining_window_after_moving_all() {
    let app_a1 = app_window(1, 10);
    let app_a2 = app_window(1, 11);
    let app_b = app_window(2, 20);
    let mut backend = FakeBackend {
        focused: Some(app_a1.clone()),
        frontmost: Some(app_a1.app.clone()),
        ..FakeBackend::default()
    };
    backend.frames.insert(app_a1.clone(), frame(0));
    backend.frames.insert(app_a2.clone(), frame(300));
    backend.frames.insert(app_b.clone(), frame(600));
    let mut rt = runtime(backend);
    bind(&mut rt, WorkspaceId::ONE, &app_a1);
    bind(&mut rt, WorkspaceId::ONE, &app_a2);
    bind(&mut rt, WorkspaceId::ONE, &app_b);
    rt.memory
        .bundles
        .insert(app_a1.app.clone(), "com.example.A".into());
    rt.memory
        .bundles
        .insert(app_b.app.clone(), "com.example.B".into());

    rt.move_app(
        "com.example.A",
        WorkspaceId::TWO,
        outputs(),
        &AtomicBool::new(false),
    )
    .unwrap();

    assert_eq!(rt.workspaces.workspace_of(&app_a1), Some(WorkspaceId::TWO));
    assert_eq!(rt.workspaces.workspace_of(&app_a2), Some(WorkspaceId::TWO));
    assert_eq!(rt.backend.focused.as_ref(), Some(&app_b));
    let focused_calls: Vec<_> = rt
        .backend
        .calls
        .iter()
        .filter_map(|call| match call {
            BackendCall::Focus(window) => Some(window.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(focused_calls, vec![app_b]);
}

#[test]
fn move_app_hands_focus_away_from_a_parked_window_when_a_later_window_refuses() {
    let app_a1 = app_window(1, 10);
    let app_a2 = app_window(1, 11);
    let app_b = app_window(2, 20);
    let mut backend = FakeBackend {
        focused: Some(app_a1.clone()),
        frontmost: Some(app_a1.app.clone()),
        ..FakeBackend::default()
    };
    backend.frames.insert(app_a1.clone(), frame(0));
    backend.frames.insert(app_a2.clone(), frame(300));
    backend.frames.insert(app_b.clone(), frame(600));
    backend.refuses_parking.insert(app_a2.clone());
    let mut rt = runtime(backend);
    bind(&mut rt, WorkspaceId::ONE, &app_a1);
    bind(&mut rt, WorkspaceId::ONE, &app_a2);
    bind(&mut rt, WorkspaceId::ONE, &app_b);
    rt.memory
        .bundles
        .insert(app_a1.app.clone(), "com.example.A".into());
    rt.memory
        .bundles
        .insert(app_b.app.clone(), "com.example.B".into());

    let result = rt.move_app(
        "com.example.A",
        WorkspaceId::TWO,
        outputs(),
        &AtomicBool::new(false),
    );

    let Err(error) = result else {
        panic!("a refused park must fail the move");
    };
    assert_eq!(error, "Window could not be parked in the target workspace");
    assert!(rt.parked.contains_key(&app_a1));
    assert_eq!(rt.workspaces.workspace_of(&app_a1), Some(WorkspaceId::TWO));
    assert_eq!(rt.workspaces.workspace_of(&app_a2), Some(WorkspaceId::ONE));
    assert_ne!(rt.backend.focused.as_ref(), Some(&app_a1));
    assert!(matches!(
        rt.backend.focused.as_ref(),
        Some(window) if *window == app_a2 || *window == app_b
    ));
}

#[test]
fn move_focused_in_fullscreen_layout_updates_remembered_focus_to_remaining_tile() {
    let first = app_window(1, 10);
    let second = app_window(2, 20);
    let mut backend = FakeBackend {
        focused: Some(second.clone()),
        frontmost: Some(second.app.clone()),
        ..FakeBackend::default()
    };
    backend.frames.insert(first.clone(), frame(0));
    backend.frames.insert(second.clone(), frame(0));
    let mut rt = runtime(backend);
    bind(&mut rt, WorkspaceId::ONE, &first);
    bind(&mut rt, WorkspaceId::ONE, &second);
    rt.workspaces
        .set_deck(WorkspaceId::ONE, crate::workspace::DeckMode::Monocle);
    rt.workspaces.remember_focus(WorkspaceId::ONE, &second);

    rt.move_focused(WorkspaceId::TWO, outputs(), &AtomicBool::new(false))
        .unwrap();

    assert_eq!(rt.backend.focused.as_ref(), Some(&first));
    assert_eq!(rt.workspaces.preferred_focus(WorkspaceId::ONE), Some(first));
}
