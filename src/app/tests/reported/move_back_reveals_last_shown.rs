//! #400: moving a workspace back reveals the workspace its display showed last.
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
fn move_back_reveals_the_workspace_just_left() {
    let outputs = three_outputs();
    let config = Config::parse(
        "version = 1
[[workspaces]]
workspace = 6
monitor = 'secondary-id'
[[workspaces]]
workspace = 'A'
monitor = 'secondary-id'",
    )
    .unwrap();
    let (one, six, a) = (ws("1"), ws("6"), ws("A"));
    for intervening_switches in [false, true] {
        let mut rt = runtime_with_config(FakeBackend::default(), config.clone());
        for (workspace, window, x) in [(one, window(10), 50), (six, window(60), 1050)] {
            rt.backend.frames.insert(window.clone(), frame(x));
            bind(&mut rt, workspace, &window);
        }
        run(&mut rt, &outputs, WorkerAction::SwitchWorkspace(a));
        let named = window(70);
        rt.backend.frames.insert(named.clone(), frame(1050));
        bind(&mut rt, a, &named);
        run(&mut rt, &outputs, WorkerAction::SwitchWorkspace(six));
        run(&mut rt, &outputs, WorkerAction::SwitchWorkspace(one));
        run(
            &mut rt,
            &outputs,
            WorkerAction::MoveWorkspaceToDisplay(DisplayTarget::Next),
        );
        if intervening_switches {
            run(&mut rt, &outputs, WorkerAction::SwitchWorkspace(a));
            run(&mut rt, &outputs, WorkerAction::SwitchWorkspace(one));
        }
        run(
            &mut rt,
            &outputs,
            WorkerAction::MoveWorkspaceToDisplay(DisplayTarget::Prev),
        );
        let expected = if intervening_switches { a } else { six };
        let active = rt
            .visible_workspaces(&outputs)
            .into_iter()
            .find(|display| display.display_id == "secondary-id")
            .unwrap();
        let named_parked = rt.parked.contains_key(&named);
        let six_parked = rt.parked.contains_key(&window(60));
        let six_on_screen = active
            .output
            .contains_center(rt.backend.frames[&window(60)]);
        println!(
            "intervening={intervening_switches}: reported={}, A parked={named_parked}, 6 parked={six_parked}, 6 on screen={six_on_screen}",
            active.workspace
        );
        assert_eq!(active.workspace, expected);
        assert_eq!(
            (named_parked, six_parked, six_on_screen),
            (
                !intervening_switches,
                intervening_switches,
                !intervening_switches
            )
        );
    }
}
