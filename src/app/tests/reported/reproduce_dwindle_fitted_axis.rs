//! #410: If a learned minimum changes the aspect ratio of the focused tile, Dwindle can choose the direction of the unfitted tile when a new window opens.
//! Reproduction contributed by @coygeek.

use super::super::*;

const CENTER: crate::Rect = crate::Rect {
    x: 0,
    y: 30,
    width: 2560,
    height: 1410,
};
const LEFT: crate::Rect = crate::Rect {
    x: -1440,
    y: -583,
    width: 1440,
    height: 2530,
};
const RIGHT: crate::Rect = crate::Rect {
    x: 2560,
    y: -539,
    width: 1440,
    height: 2530,
};

fn displays() -> macos::DisplayOutputs {
    macos::DisplayOutputs::new(
        [("center", CENTER), ("left", LEFT), ("right", RIGHT)]
            .into_iter()
            .map(|(id, frame)| macos::DisplayOutput {
                id: id.into(),
                name: id.into(),
                frame,
                is_main: id == "center",
            })
            .collect(),
    )
    .unwrap()
}

fn command<B: WorkspaceBackend>(rt: &mut WorkspaceRuntime<B>, action: WorkerAction) -> CycleStatus {
    let Some(WorkerResult::Status { result, .. }) = rt.execute(
        WorkerCommand::Action(action, displays()),
        &AtomicBool::new(false),
    ) else {
        panic!("missing status")
    };
    result.unwrap()
}

#[test]
fn research_dwindle_splits_the_displayed_focused_tile_along_its_longer_side() {
    for minimum in [0, 1600] {
        let output = CENTER;
        let initial = crate::Rect {
            x: 100,
            y: 100,
            width: 1700,
            height: 400,
        };
        let windows = [(window(1), initial), (window(2), initial)];
        let backend = FakeBackend {
            observation: Some(observation_for_windows(
                "com.example.Test",
                &window(1),
                &windows,
            )),
            focused: Some(window(1)),
            frames: windows.iter().cloned().collect(),
            minimum_widths: [(window(1), minimum)].into_iter().collect(),
            ..FakeBackend::default()
        };
        let mut runtime = runtime(backend);
        assert_eq!(command(&mut runtime, WorkerAction::Retile).failed, 0);
        let before = runtime.backend.frames[&window(1)];
        assert_eq!(
            before.width,
            if minimum > 0 {
                minimum
            } else {
                output.width / 2
            }
        );
        runtime.backend.open_window(&window(3), before);
        assert_eq!(command(&mut runtime, WorkerAction::Retile).failed, 0);
        let first = runtime.backend.frames[&window(1)];
        let third = runtime.backend.frames[&window(3)];
        println!("minimum={minimum}, before={before:?}, anchor={first:?}, inserted={third:?}");
        assert_eq!(third.x == first.x, before.height >= before.width);
        assert_eq!(third.y == first.y, before.height < before.width);
    }
}
