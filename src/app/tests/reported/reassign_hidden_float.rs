//! #543: a hidden workspace reassigned to another display by a config reload
//! reveals its parked float on that display, not the one it left.
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
workspace = 9
monitor = 'third-id'
[[workspaces]]
workspace = 5
monitor = 'secondary-id'"
    ))
    .unwrap()
}

#[test]
fn parked_float_follows_a_reassigned_hidden_workspace() {
    let all = three_outputs();
    let side = all.displays()[1].frame;
    let six = WorkspaceId::SIX;
    let five = WorkspaceId::new(5).unwrap();
    let floating = window(60);
    let tile = window(61);
    let first = frame(side.x + 100);
    let second = frame(side.x + 500);

    for (selector, destination) in [("third-id", 2), ("main", 0)] {
        let target = all.displays()[destination].frame;
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
        // Workspace 5 joins the side display only after 6 owns its windows.
        let initial = Config::parse(
            "version = 1
[[workspaces]]
workspace = 6
monitor = 'secondary-id'
[[workspaces]]
workspace = 9
monitor = 'third-id'",
        )
        .unwrap();
        let mut runtime = runtime_with_config(backend, initial);
        command(&mut runtime, WorkerAction::Retile, &all);
        command(&mut runtime, WorkerAction::ToggleFloat, &all);
        runtime.backend.frames.insert(floating.clone(), first);
        assert_eq!(
            runtime.workspaces.floating_workspace_of(&floating),
            Some(six)
        );
        assert_eq!(runtime.workspaces.workspace_of(&tile), Some(six));

        runtime.replace_config(config("secondary-id"));
        command(&mut runtime, WorkerAction::SwitchWorkspace(five), &all);
        assert!(!runtime.is_visible(six, &all));
        assert!(runtime.parked.contains_key(&floating));

        runtime.replace_config(config(selector));
        // Reassignment alone leaves the float parked.
        assert!(runtime.parked.contains_key(&floating));

        command(&mut runtime, WorkerAction::SwitchWorkspace(six), &all);
        let float_frame = runtime.backend.frames[&floating];
        let tile_frame = runtime.backend.frames[&tile];
        assert!(runtime.is_visible(six, &all));
        assert!(runtime.is_visible(five, &all));
        assert_eq!(
            runtime.workspaces.floating_workspace_of(&floating),
            Some(six)
        );
        assert!(!runtime.parked.contains_key(&floating));
        assert!(
            target.contains_center(tile_frame),
            "{selector}: tile {tile_frame:?}"
        );
        assert!(
            target.contains_center(float_frame),
            "{selector}: float {float_frame:?}"
        );
        assert_eq!(
            (float_frame.width, float_frame.height),
            (first.width, first.height)
        );
    }
}
