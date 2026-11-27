//! #393: With a side Dock, a floating window can be entirely inside the destination display yet move back to its original display on release.
//! Reproduction contributed by @coygeek.

use super::super::*;
use crate::app::input::GestureTracker;

fn dock_display() -> macos::DisplayOutputs {
    let mut displays = three_outputs().displays().to_vec();
    let full: Vec<_> = displays.iter().map(|display| display.frame).collect();
    displays[2].frame.width -= 100;
    macos::DisplayOutputs::with_bounds(displays, full).unwrap()
}

#[test]
fn a_visible_float_keeps_its_release_frame() {
    let moving = window(10);
    let owner = WorkspaceId::ONE;
    let start = crate::Rect {
        x: 120,
        y: 150,
        width: 120,
        height: 200,
    };
    let mut result = Vec::new();
    let outputs = dock_display();
    for x in [2740, 2880] {
        let backend = FakeBackend {
            frames: [(moving.clone(), start)].into(),
            observation: Some(observation(&moving, start)),
            focused: Some(moving.clone()),
            frontmost: Some(moving.app.clone()),
            ..FakeBackend::default()
        };
        let config = Config::parse(
            "version = 1\n[[rules]]\nbundle_id = 'com.example.Test'\naction = 'float'",
        )
        .unwrap();
        let mut rt = runtime_with_config(backend, config);
        let cancelled = AtomicBool::new(false);
        rt.cycle(outputs.clone(), &cancelled).unwrap();
        assert_eq!(rt.workspaces.floating_workspace_of(&moving), Some(owner));
        let released = crate::Rect { x, y: 300, ..start };
        let point = (x + 60, released.y + 12);
        assert!(outputs.screens()[2].contains_center(released));
        assert_eq!(
            outputs.display_at(point).unwrap().id,
            outputs.displays()[2].id
        );
        let mut tracker = GestureTracker::default();
        tracker.begin_at(
            vec![(moving.clone(), start)],
            Some((start.x + 60, start.y + 12)),
        );
        rt.backend.frames.insert(moving.clone(), released);
        let gesture = tracker.finish(|_| Some(released), Some(point)).unwrap();
        let Some(WorkerResult::Status { result: status, .. }) = rt.execute(
            WorkerCommand::Gesture(gesture, false, outputs.clone()),
            &cancelled,
        ) else {
            panic!("expected worker status");
        };
        assert_eq!(status.unwrap().failed, 0);
        assert_eq!(rt.workspaces.floating_workspace_of(&moving), Some(owner));
        let actual = rt.backend.frames[&moving];
        println!("release={released:?} actual={actual:?}");
        result.push(actual == released);
    }
    assert_eq!(result, [true, true]);
}
