//! A bound window the window server still lists but has ordered out: a
//! closed window its app keeps alive, or one set aside with ⌘H or ⌘M. A
//! confirmed close or a set-aside window reflows its sibling at once; anything
//! uncertain waits out the omission settle.
use super::reconciliation::{app_window, apps_observation, column, studio_display};
use super::*;

/// Two apps share a display. The second app disappears from the observation
/// while Core Graphics still lists its former surface off screen.
fn ordered_out_pair(
    bundle: &str,
) -> (
    WorkspaceRuntime<FakeBackend>,
    crate::PhysicalWindowId,
    crate::PhysicalWindowId,
) {
    let kept = app_window(10, 1, 100);
    let missing = app_window(20, 1, 200);
    let left = column(0, 1280);
    let right = column(1280, 1280);
    let mut runtime = runtime(FakeBackend {
        frames: [(kept.clone(), left), (missing.clone(), right)].into(),
        observation: Some(apps_observation(&[
            ("com.example.Kept", &kept, left),
            (bundle, &missing, right),
        ])),
        ..FakeBackend::default()
    });
    bind(&mut runtime, WorkspaceId::ONE, &kept);
    bind(&mut runtime, WorkspaceId::ONE, &missing);
    runtime
        .cycle(studio_display(), &AtomicBool::new(false))
        .unwrap();

    runtime.backend.observation = Some(apps_observation(&[("com.example.Kept", &kept, left)]));
    runtime.backend.frames.remove(&missing);
    runtime.backend.server_frames.insert(missing.clone(), right);
    runtime.backend.ordered_out.insert(missing.window_id);
    (runtime, kept, missing)
}

#[test]
fn confirmed_last_window_close_reflows_the_sibling_in_the_same_cycle() {
    let (mut runtime, kept, missing) = ordered_out_pair("com.example.Closing");
    runtime
        .backend
        .apps_confirming_no_windows
        .insert(missing.app.clone());
    let status = runtime
        .cycle(studio_display(), &AtomicBool::new(false))
        .unwrap();

    assert_eq!(
        runtime.backend.close_probes,
        std::slice::from_ref(&missing.app)
    );
    assert!(status.close_recheck.is_none());
    assert_eq!(runtime.workspaces.workspace_of(&missing), None);
    assert_eq!(runtime.backend.frames[&kept], column(0, 2560));
}

#[test]
fn uncertain_close_requests_one_early_retry() {
    for confirmed_on_retry in [false, true] {
        let (mut runtime, kept, missing) = ordered_out_pair("com.example.Closing");
        let status = runtime
            .cycle(studio_display(), &AtomicBool::new(false))
            .unwrap();
        let deadline = status.close_recheck.expect("one prompt follow-up");
        assert!(deadline <= runtime.backend.now() + Duration::from_millis(75));
        assert_eq!(runtime.backend.frames[&kept], column(0, 1280));

        if confirmed_on_retry {
            runtime
                .backend
                .apps_confirming_no_windows
                .insert(missing.app.clone());
        }
        let wait = deadline.saturating_duration_since(runtime.backend.now());
        runtime.backend.advance(wait);
        let status = runtime
            .cycle(studio_display(), &AtomicBool::new(false))
            .unwrap();
        assert!(status.close_recheck.is_none());
        assert_eq!(
            runtime.backend.frames[&kept].width,
            if confirmed_on_retry { 2560 } else { 1280 }
        );
        assert_eq!(
            runtime.workspaces.workspace_of(&missing).is_none(),
            confirmed_on_retry
        );
        runtime
            .cycle(studio_display(), &AtomicBool::new(false))
            .unwrap();
        assert_eq!(
            runtime.backend.close_probes,
            [missing.app.clone(), missing.app]
        );
    }
}

#[test]
fn recheck_reflows_when_the_ordered_out_surface_disappears() {
    let (mut runtime, kept, missing) = ordered_out_pair("com.example.Closing");
    let cancelled = AtomicBool::new(false);
    let first = runtime.cycle(studio_display(), &cancelled).unwrap();
    assert!(first.close_recheck.is_some());
    assert_eq!(runtime.backend.frames[&kept], column(0, 1280));

    runtime.backend.server_frames.remove(&missing);
    let next = runtime.cycle(studio_display(), &cancelled).unwrap();
    assert!(next.close_recheck.is_none());
    assert_eq!(runtime.workspaces.workspace_of(&missing), None);
    assert_eq!(runtime.backend.frames[&kept], column(0, 2560));
    assert_eq!(runtime.backend.close_probes.len(), 1);
}

#[test]
fn stale_ax_sibling_during_space_switch_does_not_confirm_a_close() {
    let (mut runtime, kept, missing) = ordered_out_pair("com.example.Closing");
    runtime.backend.ordered_out.insert(kept.window_id);
    runtime
        .backend
        .apps_confirming_no_windows
        .insert(missing.app.clone());

    let status = runtime
        .cycle(studio_display(), &AtomicBool::new(false))
        .unwrap();

    assert!(runtime.backend.close_probes.is_empty());
    assert!(status.close_recheck.is_none());
    assert_eq!(
        runtime.workspaces.workspace_of(&missing),
        Some(WorkspaceId::ONE)
    );
}

#[test]
fn multiple_missing_slots_of_one_app_keep_the_ordinary_grace_period() {
    let (mut runtime, _, missing) = ordered_out_pair("com.example.Closing");
    let another = app_window(missing.app.pid, missing.app.launched_at, 201);
    bind(&mut runtime, WorkspaceId::ONE, &another);
    runtime
        .backend
        .apps_confirming_no_windows
        .insert(missing.app.clone());
    let status = runtime
        .cycle(studio_display(), &AtomicBool::new(false))
        .unwrap();

    assert!(runtime.backend.close_probes.is_empty());
    assert!(status.close_recheck.is_none());
    for window in [missing, another] {
        assert_eq!(
            runtime.workspaces.workspace_of(&window),
            Some(WorkspaceId::ONE)
        );
    }
}

#[test]
fn a_possible_tab_replacement_or_incomplete_snapshot_keeps_its_tile() {
    for bundle in ["com.mitchellh.ghostty", "com.example.Busy"] {
        let (mut runtime, kept, missing) = ordered_out_pair(bundle);
        if bundle == "com.example.Busy" {
            runtime
                .backend
                .observation
                .as_mut()
                .unwrap()
                .applications
                .push(crate::AppObservation::Unavailable {
                    app: missing.app.clone(),
                    error: "timeout".into(),
                });
        }
        runtime
            .backend
            .apps_confirming_no_windows
            .insert(missing.app.clone());
        let status = runtime
            .cycle(studio_display(), &AtomicBool::new(false))
            .unwrap();
        assert!(runtime.backend.close_probes.is_empty());
        assert!(status.close_recheck.is_none());
        assert_eq!(
            runtime.workspaces.workspace_of(&missing),
            Some(WorkspaceId::ONE)
        );
        assert_eq!(runtime.backend.frames[&kept], column(0, 1280));
    }
}

#[test]
fn hidden_or_minimized_window_reflows_the_sibling_in_the_same_cycle() {
    let (mut runtime, kept, missing) = ordered_out_pair("com.example.Hidden");
    runtime.backend.set_aside.insert(missing.clone());

    let status = runtime
        .cycle(studio_display(), &AtomicBool::new(false))
        .unwrap();

    assert!(status.close_recheck.is_none());
    assert!(runtime.backend.close_probes.is_empty());
    assert_eq!(runtime.workspaces.workspace_of(&missing), None);
    assert_eq!(runtime.backend.frames[&kept], column(0, 2560));
}

#[test]
fn an_app_keeps_its_tiles_while_one_of_its_windows_is_still_settling() {
    let (mut runtime, kept, missing) = ordered_out_pair("com.example.Minimized");
    let settling = app_window(missing.app.pid, missing.app.launched_at, 201);
    bind(&mut runtime, WorkspaceId::ONE, &settling);
    let right = runtime.backend.server_frames[&missing];
    runtime
        .backend
        .server_frames
        .insert(settling.clone(), right);
    runtime.backend.ordered_out.insert(settling.window_id);
    runtime.backend.set_aside.insert(missing.clone());

    runtime
        .cycle(studio_display(), &AtomicBool::new(false))
        .unwrap();

    for window in [&missing, &settling] {
        assert_eq!(
            runtime.workspaces.workspace_of(window),
            Some(WorkspaceId::ONE)
        );
    }
    assert_ne!(runtime.backend.frames[&kept], column(0, 2560));
}

#[test]
fn a_parked_window_keeps_its_tile_through_the_settle() {
    let (mut runtime, kept, missing) = ordered_out_pair("com.example.Parked");
    let right = runtime.backend.server_frames[&missing];
    runtime.parked.insert(missing.clone(), right, None);
    runtime.parked.park_applied(&missing, runtime.backend.now());
    runtime.backend.set_aside.insert(missing.clone());

    runtime
        .cycle(studio_display(), &AtomicBool::new(false))
        .unwrap();

    assert_eq!(
        runtime.workspaces.workspace_of(&missing),
        Some(WorkspaceId::ONE)
    );
    assert_eq!(runtime.backend.frames[&kept], column(0, 1280));
}

#[test]
fn a_minimized_only_window_releases_its_tile_with_nothing_else_observed() {
    let only = app_window(20, 1, 200);
    let full = column(0, 2560);
    let mut runtime = runtime(FakeBackend {
        frames: [(only.clone(), full)].into(),
        observation: Some(apps_observation(&[("com.example.Only", &only, full)])),
        ..FakeBackend::default()
    });
    bind(&mut runtime, WorkspaceId::ONE, &only);
    let cancelled = AtomicBool::new(false);
    runtime.cycle(studio_display(), &cancelled).unwrap();

    runtime.backend.observation = Some(apps_observation(&[]));
    runtime.backend.frames.remove(&only);
    runtime.backend.server_frames.insert(only.clone(), full);
    runtime.backend.ordered_out.insert(only.window_id);
    // Nothing on the display is observed, as on another macOS Space: kept.
    runtime.cycle(studio_display(), &cancelled).unwrap();
    assert_eq!(
        runtime.workspaces.workspace_of(&only),
        Some(WorkspaceId::ONE)
    );

    // ⌘M confirms it left the layout (#496).
    runtime.backend.set_aside.insert(only.clone());
    runtime.cycle(studio_display(), &cancelled).unwrap();
    assert_eq!(runtime.workspaces.workspace_of(&only), None);
}

#[test]
fn returning_to_a_workspace_leaves_a_window_minimized_while_away_alone() {
    let minimized = window(10);
    let ordinary = window(11);
    let (minimized_frame, ordinary_frame) = (frame(50), frame(500));
    let mut backend = FakeBackend {
        observation: Some(observation_for_windows(
            "com.example.Test",
            &minimized,
            &[
                (minimized.clone(), minimized_frame),
                (ordinary.clone(), ordinary_frame),
            ],
        )),
        ..FakeBackend::default()
    };
    backend.frames.insert(minimized.clone(), minimized_frame);
    backend.frames.insert(ordinary.clone(), ordinary_frame);
    let mut runtime = runtime(backend);
    bind(&mut runtime, WorkspaceId::ONE, &minimized);
    bind(&mut runtime, WorkspaceId::ONE, &ordinary);
    runtime
        .workspaces
        .remember_focus(WorkspaceId::ONE, &minimized);
    let cancelled = AtomicBool::new(false);
    runtime
        .switch(WorkspaceId::new(2).unwrap(), outputs(), &cancelled)
        .unwrap();
    assert!(runtime.parked.contains_key(&minimized));
    assert!(runtime.parked.contains_key(&ordinary));

    // Minimized from the Dock while its workspace is away.
    runtime.backend.set_aside.insert(minimized.clone());
    runtime.backend.ordered_out.insert(minimized.window_id);
    runtime.backend.calls.clear();
    runtime
        .switch(WorkspaceId::ONE, outputs(), &cancelled)
        .unwrap();

    assert!(!runtime.parked.contains_key(&minimized));
    assert!(!runtime.parked.contains_key(&ordinary));
    assert_eq!(runtime.workspaces.workspace_of(&minimized), None);
    assert_eq!(
        runtime.workspaces.workspace_of(&ordinary),
        Some(WorkspaceId::ONE)
    );
    assert_eq!(
        (
            runtime.backend.frames[&minimized].x,
            runtime.backend.frames[&minimized].y
        ),
        (minimized_frame.x, minimized_frame.y)
    );
    assert!(runtime.backend.calls.iter().all(|call| !matches!(
        call,
        BackendCall::Focus(window) | BackendCall::Raise(window) | BackendCall::Apply(window, _)
            if window == &minimized
    )));
    assert!(
        runtime
            .backend
            .calls
            .contains(&BackendCall::Focus(ordinary.clone()))
    );
}

/// Two Fullscreen Layout tiles on workspace 1, then both hidden (⌘H) or
/// minimized (⌘M): every slot is released and the workspace is empty.
fn fullscreen_pair_set_aside() -> (
    WorkspaceRuntime<FakeBackend>,
    crate::PhysicalWindowId,
    crate::PhysicalWindowId,
) {
    let (mut runtime, first, second) = fullscreen_pair(Config::default());
    let cancelled = AtomicBool::new(false);
    runtime.backend.focused = None;
    runtime.backend.observation = Some(apps_observation(&[]));
    for window in [&first, &second] {
        set_aside(&mut runtime, window);
    }
    runtime.cycle(studio_display(), &cancelled).unwrap();
    for window in [&first, &second] {
        assert_eq!(runtime.workspaces.workspace_of(window), None);
    }
    (runtime, first, second)
}

/// Mark `window` hidden or minimized: off the Accessibility list, ordered out
/// in the window server, and confirmed set aside.
fn set_aside(runtime: &mut WorkspaceRuntime<FakeBackend>, window: &crate::PhysicalWindowId) {
    let frame = runtime.backend.frames.remove(window).unwrap();
    runtime.backend.server_frames.insert(window.clone(), frame);
    runtime.backend.ordered_out.insert(window.window_id);
    runtime.backend.set_aside.insert(window.clone());
}

/// Two tiles (`com.example.First`, `com.example.Second`) on workspace 1 in
/// Fullscreen Layout.
fn fullscreen_pair(
    config: Config,
) -> (
    WorkspaceRuntime<FakeBackend>,
    crate::PhysicalWindowId,
    crate::PhysicalWindowId,
) {
    let first = app_window(10, 1, 100);
    let second = app_window(20, 1, 200);
    let (left, right) = (column(0, 1280), column(1280, 1280));
    let mut runtime = runtime_with_config(
        FakeBackend {
            focused: Some(second.clone()),
            frames: [(first.clone(), left), (second.clone(), right)].into(),
            observation: Some(apps_observation(&[
                ("com.example.First", &first, left),
                ("com.example.Second", &second, right),
            ])),
            ..FakeBackend::default()
        },
        config,
    );
    bind(&mut runtime, WorkspaceId::ONE, &first);
    bind(&mut runtime, WorkspaceId::ONE, &second);
    let cancelled = AtomicBool::new(false);
    runtime.cycle(studio_display(), &cancelled).unwrap();
    runtime
        .toggle_fullscreen_layout(studio_display(), &cancelled)
        .unwrap();
    assert!(runtime.workspaces.deck(WorkspaceId::ONE).is_monocle());
    (runtime, first, second)
}

fn bring_back(
    runtime: &mut WorkspaceRuntime<FakeBackend>,
    windows: &[(&str, &crate::PhysicalWindowId)],
) {
    let (left, right) = (column(0, 1280), column(1280, 1280));
    let mut observed = Vec::new();
    for (index, (bundle, window)) in windows.iter().enumerate() {
        let returning = runtime.backend.set_aside.remove(*window);
        runtime.backend.server_frames.remove(*window);
        runtime.backend.ordered_out.remove(&window.window_id);
        let fallback = if index == 0 { left } else { right };
        let frame = if returning {
            fallback
        } else {
            runtime
                .backend
                .frames
                .get(*window)
                .copied()
                .unwrap_or(fallback)
        };
        runtime.backend.frames.insert((*window).clone(), frame);
        observed.push((*bundle, *window, frame));
    }
    runtime.backend.observation = Some(apps_observation(&observed));
    runtime
        .cycle(studio_display(), &AtomicBool::new(false))
        .unwrap();
}

#[test]
fn hiding_every_fullscreen_tile_keeps_fullscreen_layout_for_their_return() {
    let (mut runtime, first, second) = fullscreen_pair_set_aside();

    bring_back(
        &mut runtime,
        &[
            ("com.example.First", &first),
            ("com.example.Second", &second),
        ],
    );

    assert!(runtime.workspaces.deck(WorkspaceId::ONE).is_monocle());
    for window in [&first, &second] {
        assert_eq!(
            runtime.workspaces.workspace_of(window),
            Some(WorkspaceId::ONE)
        );
        assert_eq!(runtime.backend.frames[window], column(0, 2560));
    }
}

#[test]
fn a_different_window_arriving_first_ends_the_set_aside_fullscreen_hold() {
    let (mut runtime, first, _second) = fullscreen_pair_set_aside();
    let other = app_window(30, 1, 300);

    bring_back(&mut runtime, &[("com.example.Other", &other)]);
    assert!(!runtime.workspaces.deck(WorkspaceId::ONE).is_monocle());

    bring_back(
        &mut runtime,
        &[("com.example.Other", &other), ("com.example.First", &first)],
    );
    assert!(!runtime.workspaces.deck(WorkspaceId::ONE).is_monocle());
}

#[test]
fn fullscreen_tiles_minimized_one_at_a_time_resume_fullscreen_layout() {
    let (mut runtime, first, second) = fullscreen_pair(Config::default());
    let cancelled = AtomicBool::new(false);
    let full = column(0, 2560);

    // ⌘M on `first`: `second` keeps the workspace in Fullscreen Layout.
    set_aside(&mut runtime, &first);
    runtime.backend.observation = Some(apps_observation(&[("com.example.Second", &second, full)]));
    runtime.cycle(studio_display(), &cancelled).unwrap();
    assert_eq!(runtime.workspaces.workspace_of(&first), None);
    assert!(runtime.workspaces.deck(WorkspaceId::ONE).is_monocle());

    // ⌘M on `second` empties the workspace.
    runtime.backend.focused = None;
    set_aside(&mut runtime, &second);
    runtime.backend.observation = Some(apps_observation(&[]));
    runtime.cycle(studio_display(), &cancelled).unwrap();
    assert_eq!(runtime.workspaces.workspace_of(&second), None);

    bring_back(&mut runtime, &[("com.example.First", &first)]);
    assert!(runtime.workspaces.deck(WorkspaceId::ONE).is_monocle());
    assert_eq!(runtime.backend.frames[&first], full);

    bring_back(
        &mut runtime,
        &[
            ("com.example.First", &first),
            ("com.example.Second", &second),
        ],
    );
    assert!(runtime.workspaces.deck(WorkspaceId::ONE).is_monocle());
    for window in [&first, &second] {
        assert_eq!(
            runtime.workspaces.workspace_of(window),
            Some(WorkspaceId::ONE)
        );
        assert_eq!(runtime.backend.frames[window], full);
    }
}

#[test]
fn fullscreen_tiles_minimized_while_away_resume_fullscreen_layout() {
    let (mut runtime, first, second) = fullscreen_pair(Config::default());
    let cancelled = AtomicBool::new(false);
    let two = WorkspaceId::new(2).unwrap();
    runtime.switch(two, studio_display(), &cancelled).unwrap();
    assert!(runtime.parked.contains_key(&first));
    assert!(runtime.parked.contains_key(&second));

    // Both minimized from the Dock while workspace 1 is away.
    for window in [&first, &second] {
        runtime.backend.set_aside.insert(window.clone());
        runtime.backend.ordered_out.insert(window.window_id);
    }
    runtime
        .switch(WorkspaceId::ONE, studio_display(), &cancelled)
        .unwrap();
    for window in [&first, &second] {
        assert_eq!(runtime.workspaces.workspace_of(window), None);
    }

    bring_back(&mut runtime, &[("com.example.First", &first)]);

    assert!(runtime.workspaces.deck(WorkspaceId::ONE).is_monocle());
    assert_eq!(runtime.backend.frames[&first], column(0, 2560));
}

#[test]
fn a_set_aside_tile_routed_back_while_hidden_resumes_fullscreen_layout() {
    let config = Config::parse(
        "version = 1\n[[rules]]\nbundle_id = 'com.example.First'\naction = 'route'\nworkspace = 1",
    )
    .unwrap();
    let (mut runtime, first, second) = fullscreen_pair(config);
    let cancelled = AtomicBool::new(false);
    runtime.backend.focused = None;
    runtime.backend.observation = Some(apps_observation(&[]));
    for window in [&first, &second] {
        set_aside(&mut runtime, window);
    }
    runtime.cycle(studio_display(), &cancelled).unwrap();
    let two = WorkspaceId::new(2).unwrap();
    runtime.switch(two, studio_display(), &cancelled).unwrap();

    // Restored on workspace 2's display; its route rule sends it back to
    // workspace 1, hidden, outside reconciliation.
    bring_back(&mut runtime, &[("com.example.First", &first)]);
    assert_eq!(
        runtime.workspaces.workspace_of(&first),
        Some(WorkspaceId::ONE)
    );
    assert!(runtime.parked.contains_key(&first));

    runtime
        .switch(WorkspaceId::ONE, studio_display(), &cancelled)
        .unwrap();
    runtime.cycle(studio_display(), &cancelled).unwrap();

    assert!(runtime.workspaces.deck(WorkspaceId::ONE).is_monocle());
    assert_eq!(runtime.backend.frames[&first], column(0, 2560));
}

/// #740: a web video's fullscreen button puts the browser window, the
/// workspace's one Fullscreen Layout tile, into macOS native fullscreen.
/// Observation skips it while the window server shows it on its own Space,
/// so its slot is released once the omission settles; leaving the video's
/// fullscreen brings Fullscreen Layout back with it.
#[test]
fn leaving_native_fullscreen_resumes_fullscreen_layout() {
    let (mut runtime, first, second) = fullscreen_pair(Config::default());
    let cancelled = AtomicBool::new(false);
    let full = column(0, 2560);

    // `first` is minimized: `second` is the workspace's one tile.
    set_aside(&mut runtime, &first);
    runtime.backend.observation = Some(apps_observation(&[("com.example.Second", &second, full)]));
    runtime.cycle(studio_display(), &cancelled).unwrap();
    assert_eq!(runtime.workspaces.workspace_of(&first), None);
    assert!(runtime.workspaces.deck(WorkspaceId::ONE).is_monocle());

    // `second` enters native fullscreen and stays there past the settle.
    runtime.backend.frames.remove(&second);
    runtime.backend.server_frames.insert(second.clone(), full);
    runtime.backend.native_fullscreen.insert(second.clone());
    runtime.backend.focused = None;
    // The app answers, but its fullscreen window is omitted by observation.
    // Omitting the entire app would preserve its lone tile on another Space.
    runtime.backend.observation = Some(apps_observation(&[("com.example.Second", &second, full)]));
    runtime.cycle(studio_display(), &cancelled).unwrap();
    runtime.backend.advance(OMISSION_SETTLE * 2);
    runtime.cycle(studio_display(), &cancelled).unwrap();
    assert_eq!(runtime.workspaces.workspace_of(&second), None);

    runtime.backend.native_fullscreen.remove(&second);
    bring_back(&mut runtime, &[("com.example.Second", &second)]);

    assert_eq!(
        runtime.workspaces.workspace_of(&second),
        Some(WorkspaceId::ONE)
    );
    assert!(runtime.workspaces.deck(WorkspaceId::ONE).is_monocle());
    assert_eq!(runtime.backend.frames[&second], full);
}
