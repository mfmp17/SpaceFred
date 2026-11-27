//! #401: a restart keeps a display's previous workspace saved under another selector.
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
fn restored_history_returns_to_the_displaced_workspace() {
    let outputs = three_outputs();
    for preferred in [false, true] {
        let selector = if preferred {
            "secondary-id"
        } else {
            "secondary"
        };
        let config = Config::parse(&format!(
            "version = 1
[[workspaces]]
workspace = 6
monitor = '{selector}'"
        ))
        .unwrap();
        let mut rt = runtime_with_config(FakeBackend::default(), config);
        for (workspace, win, x) in [(ws("1"), window(10), 50), (ws("6"), window(60), 1050)] {
            rt.backend.frames.insert(win.clone(), frame(x));
            bind(&mut rt, workspace, &win);
        }
        run(&mut rt, &outputs, WorkerAction::SwitchWorkspace(ws("6")));
        run(&mut rt, &outputs, WorkerAction::SwitchWorkspace(ws("1")));
        run(
            &mut rt,
            &outputs,
            WorkerAction::MoveWorkspaceToDisplay(DisplayTarget::Next),
        );
        let mut visible = rt
            .visible_workspaces(&outputs)
            .into_iter()
            .map(|entry| (entry.display_id, entry.workspace))
            .collect::<Vec<_>>();
        visible.sort();
        let encoded = serde_json::to_vec(rt.backend.workspace_state.as_ref().unwrap()).unwrap();
        let state: crate::workspace::WorkspaceState = serde_json::from_slice(&encoded).unwrap();
        state.validate().unwrap();
        let parked = rt.backend.persisted.clone();
        let mut restored = WorkspaceRuntime::with_backend_config_state(
            ManagementScope::Both,
            rt.backend,
            parked,
            rt.config,
            Some(state),
        );
        let mut resumed = restored
            .visible_workspaces(&outputs)
            .into_iter()
            .map(|entry| (entry.display_id, entry.workspace))
            .collect::<Vec<_>>();
        resumed.sort();
        assert_eq!(resumed, visible);
        assert_eq!(restored.backend.focused.as_ref(), Some(&window(10)));
        let result = action(&mut restored, &outputs, WorkerAction::WorkspaceBackAndForth);
        println!(
            "explicit display={preferred}: {:?}",
            result.as_ref().map(|_| ())
        );
        assert!(result.is_ok());
        assert!(restored.is_visible(ws("6"), &outputs));
    }
}
