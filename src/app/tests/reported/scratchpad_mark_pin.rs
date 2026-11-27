//! #659: Scratchpad Mark hides a focused pinned float like any other float — it leaves its
//! pin, parks with a durable recovery frame, and a toggle summons it at its own size — while
//! other pins still stay on screen through workspace switches. Reported by @coygeek.

use super::super::*;
use crate::scratchpad::{ScratchpadCommand, summon_frame};

fn execute(rt: &mut WorkspaceRuntime<FakeBackend>, action: WorkerAction) {
    let Some(WorkerResult::Status { result, .. }) = rt.execute(
        WorkerCommand::Action(action, outputs()),
        &AtomicBool::new(false),
    ) else {
        panic!("missing worker status");
    };
    assert_eq!(result.unwrap().failed, 0);
}

fn on_screen(rt: &WorkspaceRuntime<FakeBackend>, window: &crate::PhysicalWindowId) -> bool {
    outputs()
        .frames()
        .any(|display| display.contains_center(rt.backend.frames[window]))
}

#[test]
fn marking_a_focused_pin_hides_it_and_a_toggle_summons_it_at_its_size() {
    let marked = window(10);
    let tile = window(20);
    let other = window(30);
    let elsewhere = window(40);
    let windows = [
        (marked.clone(), frame(50)),
        (tile.clone(), frame(500)),
        (other.clone(), frame(900)),
        (elsewhere.clone(), frame(200)),
    ];
    let backend = FakeBackend {
        frames: windows.iter().cloned().collect(),
        focused: Some(marked.clone()),
        frontmost: Some(marked.app.clone()),
        observation: Some(observation_for_windows(
            "com.example.Test",
            &marked,
            &windows,
        )),
        ..FakeBackend::default()
    };
    let config = Config::parse("version = 1\n[labs]\nscratchpad = true\n").unwrap();
    let mut rt = runtime_with_config(backend, config);
    for window in [&marked, &tile, &other] {
        bind(&mut rt, WorkspaceId::ONE, window);
    }
    bind(&mut rt, WorkspaceId::TWO, &elsewhere);
    rt.cycle(outputs(), &AtomicBool::new(false)).unwrap();

    execute(&mut rt, WorkerAction::TogglePin);
    rt.backend.focused = Some(other.clone());
    execute(&mut rt, WorkerAction::TogglePin);
    assert!(rt.workspaces.is_pinned_by_hand(&marked));
    assert!(rt.workspaces.is_pinned_by_hand(&other));
    rt.backend.focused = Some(marked.clone());
    let pinned = rt.backend.frames[&marked];

    execute(&mut rt, WorkerAction::Scratchpad(ScratchpadCommand::Mark));

    let pad = rt.workspaces.scratchpad();
    assert_eq!(pad.entry(&marked).unwrap().shown_on, None);
    assert_eq!(rt.parked.get(&marked), Some(&pinned));
    assert_eq!(rt.backend.persisted.get(&marked), Some(&pinned));
    assert!(!on_screen(&rt, &marked));
    assert!(!rt.workspaces.is_pinned(&marked));
    assert_eq!(rt.workspaces.workspace_of_any(&marked), None);
    assert_eq!(rt.backend.focused, Some(tile.clone()));

    execute(&mut rt, WorkerAction::SwitchWorkspace(WorkspaceId::TWO));
    assert!(on_screen(&rt, &other), "an unmarked pin still shows");
    assert!(!on_screen(&rt, &marked));

    execute(&mut rt, WorkerAction::Scratchpad(ScratchpadCommand::Toggle));

    let size = Some((pinned.width, pinned.height));
    assert_eq!(
        rt.backend.frames[&marked],
        summon_frame(outputs().main().frame, size)
    );
    assert_eq!(rt.backend.focused, Some(marked.clone()));
    assert!(!rt.parked.contains_key(&marked));
}
