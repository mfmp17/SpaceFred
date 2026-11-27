//! #402: A named workspace in Columns layout can keep its chosen vertical axis while empty, then lose that choice across a restart.
//! Reproduction contributed by @coygeek.

use super::super::*;

fn ws(value: &str) -> WorkspaceId {
    WorkspaceId::parse(value).unwrap()
}

fn action(
    rt: &mut WorkspaceRuntime<FakeBackend>,
    outputs: &macos::DisplayOutputs,
    action: WorkerAction,
) -> Result<CycleStatus, String> {
    let Some(WorkerResult::Status { result, .. }) = rt.execute(
        WorkerCommand::Action(action, outputs.clone()),
        &AtomicBool::new(false),
    ) else {
        panic!("expected worker status");
    };
    result
}

fn run(
    rt: &mut WorkspaceRuntime<FakeBackend>,
    outputs: &macos::DisplayOutputs,
    action_id: WorkerAction,
) {
    assert_eq!(action(rt, outputs, action_id).unwrap().failed, 0);
}

#[test]
fn emptied_named_workspace_restores_the_selected_columns_axis() {
    let outputs = three_outputs();
    let config = Config::parse(
        "version = 1
[settings]
layout = 'columns'
[[workspaces]]
workspace = 'A'
monitor = 'secondary'
[[workspaces]]
workspace = 9
monitor = 'third-id'",
    )
    .unwrap();
    let (named, nine) = (ws("A"), ws("9"));
    for restarted in [false, true] {
        let mut rt = runtime_with_config(FakeBackend::default(), config.clone());
        run(&mut rt, &outputs, WorkerAction::SwitchWorkspace(named));
        let (first, second) = (window(10), window(20));
        for (workspace, win, x) in [(named, first.clone(), 1050), (nine, second.clone(), 2050)] {
            rt.backend.frames.insert(win.clone(), frame(x));
            bind(&mut rt, workspace, &win);
        }
        run(&mut rt, &outputs, WorkerAction::SwitchWorkspace(named));
        run(&mut rt, &outputs, WorkerAction::ToggleLayout);
        run(&mut rt, &outputs, WorkerAction::MoveFocused(nine));
        assert!(rt.workspaces.windows(named).is_empty());
        assert_eq!(
            rt.workspaces.engine(named).root_axis(),
            crate::Axis::Vertical
        );
        if restarted {
            let saved = serde_json::to_vec(rt.backend.workspace_state.as_ref().unwrap()).unwrap();
            let parked = rt.backend.persisted.clone();
            rt = WorkspaceRuntime::with_backend_config_state(
                ManagementScope::Both,
                rt.backend,
                parked,
                rt.config,
                Some(serde_json::from_slice(&saved).unwrap()),
            );
        }
        run(&mut rt, &outputs, WorkerAction::MoveFocused(named));
        run(&mut rt, &outputs, WorkerAction::SwitchWorkspace(nine));
        run(&mut rt, &outputs, WorkerAction::MoveFocused(named));
        let (first, second) = (rt.backend.frames[&first], rt.backend.frames[&second]);
        println!("restarted={restarted}: first={first:?}, second={second:?}");
        assert_eq!(first.x, second.x);
        assert_ne!(first.y, second.y);
    }
}
