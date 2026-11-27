//! #404: a reconnected display shows the workspace it showed before it was unplugged.
//! Reproduction contributed by @coygeek.

use super::super::*;

fn command(
    runtime: &mut WorkspaceRuntime<FakeBackend>,
    action: WorkerAction,
    displays: &macos::DisplayOutputs,
) -> Vec<WorkspaceId> {
    let Some(WorkerResult::Status { result, .. }) = runtime.execute(
        WorkerCommand::Action(action, displays.clone()),
        &AtomicBool::new(false),
    ) else {
        panic!("missing worker status");
    };
    let status = result.unwrap();
    assert_eq!(status.failed, 0);
    status.active
}

#[test]
fn active_workspace_returns_after_live_reconnect() {
    let all = three_outputs();
    let displays = all.displays();
    let config = Config::parse(
        "version = 1
[[workspaces]]
workspace = 4
monitor = 'secondary'
[[workspaces]]
workspace = 6
monitor = 'secondary-id'
[[workspaces]]
workspace = 9
monitor = 'third-id'",
    )
    .unwrap();
    let ws = |number| WorkspaceId::new(number).unwrap();
    let expected = vec![ws(1), ws(6), ws(9)];
    for removed in [2, 1] {
        let backend = FakeBackend {
            observation: Some(crate::NativeObservation {
                cycle: 0,
                mouse_down: false,
                applications: vec![],
            }),
            ..FakeBackend::default()
        };
        let mut runtime = runtime_with_config(backend, config.clone());
        for number in [4, 6, 9] {
            command(
                &mut runtime,
                WorkerAction::SelectWorkspace(ws(number)),
                &all,
            );
        }
        assert_eq!(command(&mut runtime, WorkerAction::Retile, &all), expected);
        let reduced =
            macos::DisplayOutputs::new(vec![displays[0].clone(), displays[3 - removed].clone()])
                .unwrap();
        command(&mut runtime, WorkerAction::Retile, &reduced);
        let after = command(&mut runtime, WorkerAction::Retile, &all);
        println!("removed index {removed}, reconnected active workspaces: {after:?}");
        let selected = command(&mut runtime, WorkerAction::SelectWorkspace(ws(6)), &all);
        assert_eq!(selected, expected);
        assert_eq!(after, expected);
    }
}
