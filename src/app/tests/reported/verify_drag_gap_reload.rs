//! #390: If the worker first reads a changed outer gap on mouse-up, the final drop can select a different target from the preview.
//! Reproduction contributed by @coygeek.

use super::super::*;
use crate::app::input::GestureTracker;

fn dispatch(rt: &mut WorkspaceRuntime<FakeBackend>, command: WorkerCommand) {
    let Some(WorkerResult::Status { result, .. }) = rt.execute(command, &AtomicBool::new(false))
    else {
        panic!("expected worker status");
    };
    result.unwrap();
}

fn gap_reload(
    preview_with_new_gap: bool,
    reload_on_release: bool,
) -> (Vec<crate::Rect>, Vec<crate::Rect>) {
    let outputs = macos::DisplayOutputs::new(vec![outputs().main().clone()]).unwrap();
    let ids = [window(10), window(20), window(30)];
    let backend = FakeBackend {
        frames: ids.iter().cloned().map(|id| (id, frame(50))).collect(),
        focused: Some(ids[2].clone()),
        ..FakeBackend::default()
    };
    let mut rt = runtime(backend);
    for id in &ids {
        bind(&mut rt, WorkspaceId::ONE, id);
    }
    let config = Config::parse("version = 1\n[settings.gaps]\nleft = 80\n").unwrap();
    if preview_with_new_gap {
        rt.replace_config(config.clone());
    }
    rt.layout_workspace(
        WorkspaceId::ONE,
        outputs.main().frame,
        &AtomicBool::new(false),
    );
    assert_eq!(rt.workspaces.workspace_of(&ids[2]), Some(WorkspaceId::ONE));
    let start = rt.backend.frames[&ids[2]];
    let mut tracker = GestureTracker::default();
    tracker.begin_at(vec![(ids[2].clone(), start)], Some((800, 20)));
    rt.backend.mouse_down = true;
    let current = crate::Rect {
        x: start.x - 690,
        y: 380,
        ..start
    };
    rt.backend.frames.insert(ids[2].clone(), current);
    let point = (110, 400);
    let preview = tracker.preview(|_| Some(current), Some(point)).unwrap();
    dispatch(
        &mut rt,
        WorkerCommand::Gesture(preview.clone(), true, outputs.clone()),
    );
    let before = ids[..2].iter().map(|id| rt.backend.frames[id]).collect();
    if reload_on_release {
        rt.replace_config(config);
    }
    rt.backend.mouse_down = false;
    let release = tracker.finish(|_| Some(current), Some(point)).unwrap();
    assert_eq!(release, preview);
    dispatch(&mut rt, WorkerCommand::Gesture(release, false, outputs));
    let after = ids[..2].iter().map(|id| rt.backend.frames[id]).collect();
    (before, after)
}

#[test]
fn verify_drag_gap_reload_keeps_preview_target_at_release() {
    let old = gap_reload(false, false);
    let new = gap_reload(true, false);
    let reload = gap_reload(false, true);
    let xs = |frames: &[crate::Rect]| frames.iter().map(|p| p.x).collect::<Vec<_>>();
    println!("old padding: {:?} -> {:?}", xs(&old.0), xs(&old.1));
    println!("new padding: {:?} -> {:?}", xs(&new.0), xs(&new.1));
    println!(
        "reload on release: {:?} -> {:?}",
        xs(&reload.0),
        xs(&reload.1)
    );
    assert_eq!(old.0, old.1);
    assert_eq!(new.0, new.1);
    assert_eq!(reload.0, old.0);
    assert_ne!(old.1, new.1);
    assert!(
        reload.1[0].x > reload.1[1].x,
        "the release docked the tile that was previewing a swap"
    );
}
