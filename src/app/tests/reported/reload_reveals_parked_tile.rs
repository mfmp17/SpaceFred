//! #701: a config reload that reveals a hidden workspace on another display
//! lays its parked tile out there, even when the restore that preceded the
//! layout is still unconfirmed; a later restore retry must not write the old
//! display's recovery frame back over the tile.
//! Reported by @coygeek.

use super::super::*;

fn retile(runtime: &mut WorkspaceRuntime<FakeBackend>, displays: &macos::DisplayOutputs) {
    let Some(WorkerResult::Status { result, .. }) = runtime.execute(
        WorkerCommand::Action(WorkerAction::Retile, displays.clone()),
        &AtomicBool::new(false),
    ) else {
        panic!("missing worker status");
    };
    result.unwrap();
}

/// Workspaces 6-10 on main; workspace 4 on `four` when given (main by default).
fn config(four: Option<&str>) -> Config {
    let mut source = String::from("version = 1\n");
    for n in 6..=10 {
        source.push_str(&format!(
            "[[workspaces]]\nworkspace = {n}\nmonitor = 'main'\n"
        ));
    }
    if let Some(selector) = four {
        source.push_str(&format!(
            "[[workspaces]]\nworkspace = 4\nmonitor = '{selector}'\n"
        ));
    }
    Config::parse(&source).unwrap()
}

#[test]
fn reload_revealed_tile_stays_on_its_new_display_after_a_pending_restore() {
    let displays = outputs();
    let secondary = displays.displays()[1].frame;
    let anchor = window(10);
    let tile = window(20);
    let backend = FakeBackend {
        frames: HashMap::from([(anchor.clone(), frame(0)), (tile.clone(), frame(500))]),
        observation: Some(observation_for_windows(
            "com.example.Test",
            &anchor,
            &[(anchor.clone(), frame(0)), (tile.clone(), frame(500))],
        )),
        focused: Some(anchor.clone()),
        frontmost: Some(anchor.app.clone()),
        ..FakeBackend::default()
    };
    let mut runtime = runtime_with_config(backend, config(None));
    bind(&mut runtime, WorkspaceId::ONE, &anchor);
    bind(&mut runtime, WorkspaceId::new(4).unwrap(), &tile);
    retile(&mut runtime, &displays);
    let recovery = *runtime.parked.get(&tile).expect("hidden tile parks");
    assert!(displays.displays()[0].frame.contains_center(recovery));

    // The app moves the window only later: its restore is not yet confirmed.
    let parked_at = runtime.backend.frames[&tile];
    runtime
        .backend
        .server_frames
        .insert(tile.clone(), parked_at);
    runtime.backend.frozen_server_frames = true;
    // Let the preceding park write age out of the restore retry throttle.
    runtime.backend.advance(RESTORE_RETRY_INTERVAL * 2);
    runtime.replace_config(config(Some("secondary")));
    retile(&mut runtime, &displays);
    assert!(secondary.contains_center(runtime.backend.frames[&tile]));

    // The window arrives; a later cycle past the restore retry interval.
    runtime.backend.frozen_server_frames = false;
    let now_at = runtime.backend.frames[&tile];
    runtime.backend.server_frames.insert(tile.clone(), now_at);
    runtime.backend.calls.clear();
    runtime.backend.advance(RESTORE_RETRY_INTERVAL * 2);
    retile(&mut runtime, &displays);
    retile(&mut runtime, &displays);
    assert!(
        runtime.backend.calls.iter().all(|call| !matches!(
            call, BackendCall::SetPosition(window, _) if window == &tile
        )),
        "a laid-out tile must not retry its old-display recovery write"
    );

    let final_frame = runtime.backend.frames[&tile];
    assert!(
        secondary.contains_center(final_frame),
        "tile {final_frame:?} left its display"
    );
    assert!(!runtime.parked.contains_key(&tile));
    assert!(!runtime.backend.persisted.contains_key(&tile));
}
