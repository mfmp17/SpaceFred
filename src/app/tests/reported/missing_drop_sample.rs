//! #395: When the window server provides no frame for the dragged tile at mouse-up, a sibling can stay in its live-preview position and cover the dragged tile after automatic tiling restores it.
//! Reproduction contributed by @coygeek.

use super::super::*;
use crate::app::input::GestureTracker;

fn rect(x: i32, y: i32, width: i32, height: i32) -> crate::Rect {
    crate::Rect {
        x,
        y,
        width,
        height,
    }
}

fn trio() -> macos::DisplayOutputs {
    let records = [
        ("center", rect(0, 30, 2560, 1410), true),
        ("left", rect(-1440, -583, 1440, 2530), false),
        ("right", rect(2560, -539, 1440, 2530), false),
    ];
    macos::DisplayOutputs::with_bounds(
        records
            .iter()
            .map(|&(id, frame, is_main)| macos::DisplayOutput {
                id: id.into(),
                name: id.into(),
                frame,
                is_main,
            })
            .collect(),
        records
            .iter()
            .map(|&(_, frame, _)| rect(frame.x, frame.y - 30, frame.width, frame.height + 30))
            .collect(),
    )
    .unwrap()
}

fn setup(layout: &str, tiles: &[(u32, WorkspaceId, crate::Rect)]) -> WorkspaceRuntime<FakeBackend> {
    let moved = window(10);
    let windows: Vec<_> = tiles
        .iter()
        .map(|&(id, _, frame)| (window(id), frame))
        .collect();
    let backend = FakeBackend {
        frames: windows.iter().cloned().collect(),
        observation: Some(observation_for_windows(
            "com.example.Test",
            &moved,
            &windows,
        )),
        focused: Some(moved.clone()),
        frontmost: Some(moved.app),
        ..FakeBackend::default()
    };
    let config = Config::parse(&format!("version = 1\n[settings]\nlayout = '{layout}'\nmouse_follows_focus = 'display'\n[[workspaces]]\nworkspace = 9\nmonitor = 'right'\n")).unwrap();
    let mut rt = runtime_with_config(backend, config);
    for &(id, ws, _) in tiles {
        bind(&mut rt, ws, &window(id));
    }
    rt
}

fn run(rt: &mut WorkspaceRuntime<FakeBackend>, command: WorkerCommand) {
    let Some(WorkerResult::Status { result, .. }) = rt.execute(command, &AtomicBool::new(false))
    else {
        panic!("expected worker status");
    };
    assert_eq!(result.unwrap().failed, 0);
}

fn action(rt: &mut WorkspaceRuntime<FakeBackend>, action: WorkerAction) {
    run(rt, WorkerCommand::Action(action, trio()));
}

#[test]
fn missing_final_sample_leaves_preview_overlap() {
    let ws = WorkspaceId::ONE;
    let moving = window(10);
    let sibling = window(11);
    let right = WorkspaceId::new(9).unwrap();
    let mut found = vec![];
    for final_available in [true, false] {
        let mut rt = setup(
            "columns",
            &[
                (10, ws, rect(200, 100, 900, 700)),
                (11, ws, rect(1500, 100, 900, 700)),
                (90, right, rect(2560, -539, 1440, 2530)),
            ],
        );
        action(&mut rt, WorkerAction::Retile);
        let initial = rt.backend.frames[&moving];
        let original_sibling = rt.backend.frames[&sibling];
        let from = (initial.x + initial.width / 2, initial.y + 12);
        let over_sibling = (1920, 500);
        let released = (1280, 10);
        let mut tracker = GestureTracker::default();
        tracker.begin_at(vec![(moving.clone(), initial)], Some(from));
        let preview_frame = rect(
            initial.x + over_sibling.0 - from.0,
            initial.y + over_sibling.1 - from.1,
            initial.width,
            initial.height,
        );
        rt.backend.frames.insert(moving.clone(), preview_frame);
        let preview = tracker
            .preview(|_| Some(preview_frame), Some(over_sibling))
            .unwrap();
        run(&mut rt, WorkerCommand::Gesture(preview, true, trio()));
        assert_eq!(rt.backend.frames[&sibling], initial);
        let saved_layout = rt.workspaces.engine(ws).layout().clone();
        let final_frame = rect(
            initial.x + released.0 - from.0,
            initial.y,
            initial.width,
            initial.height,
        );
        rt.backend.frames.insert(moving.clone(), final_frame);
        if let Some(final_event) =
            tracker.finish(|_| final_available.then_some(final_frame), Some(released))
        {
            run(&mut rt, WorkerCommand::Gesture(final_event, false, trio()));
        }
        for _ in 0..4 {
            action(&mut rt, WorkerAction::Retile);
        }
        assert_eq!(rt.workspaces.engine(ws).layout(), &saved_layout);
        let sibling_frame = rt.backend.frames[&sibling];
        assert_eq!(rt.workspaces.workspace_of(&sibling), Some(ws));
        assert_eq!(rt.workspaces.workspace_of(&moving), Some(ws));
        assert_eq!(rt.backend.frames[&moving], initial);
        println!(
            "final_available={final_available} original={original_sibling:?} actual={sibling_frame:?}"
        );
        found.push(sibling_frame == original_sibling);
    }
    assert_eq!(found, vec![true; 2]);
}
