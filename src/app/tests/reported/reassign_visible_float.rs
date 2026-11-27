//! #665: a shown workspace reassigned to another display by a config reload
//! brings its float along, not only its tiles, when it next shows.
//! Reproduction contributed by @coygeek.

use super::super::*;

fn command(
    runtime: &mut WorkspaceRuntime<FakeBackend>,
    action: WorkerAction,
    displays: &macos::DisplayOutputs,
) {
    let Some(WorkerResult::Status { result, .. }) = runtime.execute(
        WorkerCommand::Action(action, displays.clone()),
        &AtomicBool::new(false),
    ) else {
        panic!("missing worker status");
    };
    assert_eq!(result.unwrap().failed, 0);
}

fn config(six: &str) -> Config {
    Config::parse(&format!(
        "version = 1
[[workspaces]]
workspace = 6
monitor = '{six}'
[[workspaces]]
workspace = 7
monitor = 'third-id'"
    ))
    .unwrap()
}

#[test]
fn shown_float_follows_a_reassigned_workspace() {
    let all = three_outputs();
    let side = all.displays()[1].frame;
    let target = all.displays()[2].frame;
    let six = WorkspaceId::SIX;
    let seven = WorkspaceId::new(7).unwrap();
    let floating = window(60);
    let tile = window(61);
    let first = frame(side.x + 100);
    let second = frame(side.x + 500);
    let backend = FakeBackend {
        frames: HashMap::from([(floating.clone(), first), (tile.clone(), second)]),
        observation: Some(observation_for_windows(
            "com.example.Test",
            &floating,
            &[(floating.clone(), first), (tile.clone(), second)],
        )),
        focused: Some(floating.clone()),
        frontmost: Some(floating.app.clone()),
        ..FakeBackend::default()
    };
    let mut runtime = runtime_with_config(backend, config("secondary-id"));
    command(&mut runtime, WorkerAction::Retile, &all);
    command(&mut runtime, WorkerAction::ToggleFloat, &all);
    runtime.backend.frames.insert(floating.clone(), first);
    assert_eq!(
        runtime.workspaces.floating_workspace_of(&floating),
        Some(six)
    );
    assert!(runtime.is_visible(six, &all));
    assert!(runtime.is_visible(seven, &all));
    assert!(!runtime.parked.contains_key(&floating));

    runtime.replace_config(config("third-id"));
    // Workspace 7 keeps the new display; 6 is hidden, and its float parks
    // with a recovery frame on the display 6 now belongs to.
    assert!(!runtime.is_visible(six, &all));
    let recovery = runtime.parked.get(&floating).copied();
    assert!(
        recovery.is_some_and(|recovery| target.contains_center(recovery)),
        "recovery frame {recovery:?}"
    );

    command(&mut runtime, WorkerAction::SwitchWorkspace(six), &all);
    let float_frame = runtime.backend.frames[&floating];
    let tile_frame = runtime.backend.frames[&tile];
    assert!(runtime.is_visible(six, &all));
    assert_eq!(
        runtime.workspaces.floating_workspace_of(&floating),
        Some(six)
    );
    assert!(!runtime.parked.contains_key(&floating));
    assert!(target.contains_center(tile_frame), "tile {tile_frame:?}");
    assert!(target.contains_center(float_frame), "float {float_frame:?}");
    assert_eq!(
        (float_frame.width, float_frame.height),
        (first.width, first.height)
    );
}
