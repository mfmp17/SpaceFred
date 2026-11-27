//! #403: a float taller than the display it was carried to comes back with its workspace.
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
fn workspace_round_trip_carries_a_tall_float_back() {
    let config = Config::parse(
        "version = 1
[settings]
layout = 'columns'
[[workspaces]]
workspace = 9
monitor = 'third-id'",
    )
    .unwrap();
    for target_height in [1200, 768] {
        let mut displays = three_outputs().displays().to_vec();
        displays[1].frame.height = 2000;
        displays[2].frame.height = target_height;
        let outputs = macos::DisplayOutputs::new(displays).unwrap();
        let (six, floating) = (ws("6"), window(60));
        let mut rt = runtime_with_config(FakeBackend::default(), config.clone());
        rt.backend
            .frames
            .insert(floating.clone(), outputs.displays()[1].frame);
        bind(&mut rt, six, &floating);
        run(&mut rt, &outputs, WorkerAction::SwitchWorkspace(six));
        run(&mut rt, &outputs, WorkerAction::ToggleFloat);
        run(
            &mut rt,
            &outputs,
            WorkerAction::MoveWorkspaceToDisplay(DisplayTarget::Next),
        );
        assert_eq!(rt.backend.frames[&floating].x, 2000);
        run(
            &mut rt,
            &outputs,
            WorkerAction::MoveWorkspaceToDisplay(DisplayTarget::Prev),
        );
        assert!(rt.is_visible(six, &outputs));
        assert_eq!(rt.workspaces.floating_workspace_of(&floating), Some(six));
        let actual = rt.backend.frames[&floating];
        println!("target height={target_height}: after returning {actual:?}");
        assert_eq!(actual.x, 1000);
    }
}
