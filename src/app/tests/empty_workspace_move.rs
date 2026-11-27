//! #201: after switching to an empty workspace on a display other than main,
//! moving the focused workspace moves that empty workspace, not main's.
use super::*;

fn moved_first(occupied: u8, empty: u8, x: i32) -> Option<WorkspaceId> {
    let ws = |n| WorkspaceId::new(n).unwrap();
    let (main, other) = (window(10), window(60));
    let mut backend = FakeBackend {
        focused: Some(other.clone()),
        frontmost: Some(other.app.clone()),
        ..FakeBackend::default()
    };
    backend.frames.insert(main.clone(), frame(50));
    backend.frames.insert(other.clone(), frame(x));
    let mut rt = runtime(backend);
    bind(&mut rt, ws(1), &main);
    bind(&mut rt, ws(occupied), &other);
    let cancelled = AtomicBool::new(false);
    for n in [occupied, empty] {
        rt.switch(ws(n), three_outputs(), &cancelled).unwrap();
    }
    assert_eq!(rt.focus.history().current, Some(ws(empty)));
    rt.move_workspace_to_display(DisplayTarget::Next, three_outputs(), &cancelled)
        .unwrap();
    rt.backend.persisted_monitors.first().map(|moved| moved.0)
}

#[test]
fn moves_the_empty_workspace_on_secondary() {
    assert_eq!(moved_first(6, 7, 1050), Some(WorkspaceId::new(7).unwrap()));
}

#[test]
fn control_moves_the_empty_workspace_on_main() {
    assert_eq!(moved_first(2, 3, 60), Some(WorkspaceId::new(3).unwrap()));
}

#[test]
fn a_focused_window_on_a_visible_workspace_still_wins() {
    let ws = |n| WorkspaceId::new(n).unwrap();
    let (main, other) = (window(10), window(60));
    let mut backend = FakeBackend {
        focused: Some(main.clone()),
        frontmost: Some(main.app.clone()),
        ..FakeBackend::default()
    };
    backend.frames.insert(main.clone(), frame(50));
    backend.frames.insert(other.clone(), frame(1050));
    let mut rt = runtime(backend);
    bind(&mut rt, ws(1), &main);
    bind(&mut rt, ws(6), &other);
    let cancelled = AtomicBool::new(false);
    rt.switch(ws(6), three_outputs(), &cancelled).unwrap();
    // Simulate focus settling on ws(6) before the user clicks main on ws(1).
    rt.focus.age_request(super::super::focus::FOCUS_SETTLE);
    rt.backend.focused = Some(main.clone());
    rt.move_workspace_to_display(DisplayTarget::Next, three_outputs(), &cancelled)
        .unwrap();
    assert_eq!(
        rt.backend.persisted_monitors.first().map(|moved| moved.0),
        Some(ws(1))
    );
}

#[test]
fn moving_with_follow_switches_and_focuses_the_moved_window() {
    let moved = window(10);
    let existing = window(20);
    let target = WorkspaceId::new(2).unwrap();
    let mut backend = FakeBackend {
        focused: Some(moved.clone()),
        observation: Some(observation(&moved, frame(50))),
        ..FakeBackend::default()
    };
    backend.frames.insert(moved.clone(), frame(50));
    backend
        .frames
        .insert(existing.clone(), main_parked_frame(frame(100)));
    let mut runtime = runtime_with_parked(backend, HashMap::from([(existing.clone(), frame(100))]));
    bind(&mut runtime, WorkspaceId::ONE, &moved);
    bind(&mut runtime, target, &existing);
    runtime.workspaces.remember_focus(target, &existing);

    runtime
        .move_focused_follow(target, outputs(), &AtomicBool::new(false))
        .unwrap();

    assert_eq!(
        runtime.workspaces.active_for(&DisplaySelector::Main),
        Some(target)
    );
    assert_eq!(runtime.workspaces.workspace_of(&moved), Some(target));
    assert!(!runtime.parked.contains_key(&moved));
    assert_eq!(runtime.backend.focused.as_ref(), Some(&moved));
}
