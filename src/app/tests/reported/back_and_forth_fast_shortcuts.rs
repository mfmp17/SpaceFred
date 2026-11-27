//! #399: Option+Tab after two fast workspace shortcuts on different displays.
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
fn back_and_forth_returns_after_two_fast_workspace_shortcuts() {
    let outputs = three_outputs();
    let config = Config::parse(
        "version = 1
[[workspaces]]
workspace = 7
monitor = 'third-id'
[[workspaces]]
workspace = 9
monitor = 'third-id'",
    )
    .unwrap();
    for delayed in [false, true] {
        let mut rt = runtime_with_config(FakeBackend::default(), config.clone());
        let mut windows = Vec::new();
        for (workspace, id, x) in [(ws("1"), 10, 50), (ws("6"), 60, 1050), (ws("9"), 90, 2050)] {
            let mut window = window(id);
            window.app.pid = id as i32;
            rt.backend.frames.insert(window.clone(), frame(x));
            bind(&mut rt, workspace, &window);
            windows.push(window);
        }
        let [main, left, right] = windows.as_slice() else {
            unreachable!()
        };
        run(&mut rt, &outputs, WorkerAction::SwitchWorkspace(ws("9")));
        run(&mut rt, &outputs, WorkerAction::SwitchWorkspace(ws("7")));
        run(&mut rt, &outputs, WorkerAction::SwitchWorkspace(ws("1")));
        run(&mut rt, &outputs, WorkerAction::SwitchWorkspace(ws("6")));
        if delayed {
            rt.backend.focused = Some(main.clone());
            rt.backend.frontmost = Some(main.app.clone());
        }
        run(&mut rt, &outputs, WorkerAction::SwitchWorkspace(ws("9")));
        if delayed {
            rt.backend.focused = Some(left.clone());
            rt.backend.frontmost = Some(left.app.clone());
        }
        rt.backend.calls.clear();
        run(&mut rt, &outputs, WorkerAction::WorkspaceBackAndForth);
        let focus = rt.backend.focused.as_ref().unwrap().window_id;
        println!(
            "delayed={delayed}: first back-and-forth focused {focus}, expected {}",
            left.window_id
        );
        assert_eq!(focus, left.window_id);
        run(&mut rt, &outputs, WorkerAction::WorkspaceBackAndForth);
        assert_eq!(rt.backend.focused.as_ref(), Some(right));
    }
}
