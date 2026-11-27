//! #739: a background wake with the displays asleep ran out the Accessibility
//! grace of a window the window server still listed, and Spaceman forgot a
//! live tile. Reported by @lorenzprattes.

use super::super::*;

#[test]
fn time_with_every_display_asleep_never_retires_a_listed_window() {
    let live = window(10);
    let closed = window(11);
    let mut backend = FakeBackend {
        observation: Some(observation(&live, frame(50))),
        frame_outcome: Some(macos::WindowAccessError::Gone),
        ..FakeBackend::default()
    };
    backend.server_frames.insert(live.clone(), frame(50));
    let mut runtime = runtime(backend);
    let workspace = WorkspaceId::new(2).unwrap();
    bind(&mut runtime, workspace, &live);
    let (outputs, cancelled) = (outputs(), AtomicBool::new(false));

    // Accessibility loses the window just before the lid closes.
    runtime.cycle(outputs.clone(), &cancelled).unwrap();
    assert_eq!(runtime.workspaces.workspace_of(&live), Some(workspace));
    bind(&mut runtime, workspace, &closed);

    // A background wake well past the grace, every display still asleep.
    runtime.backend.displays_asleep = true;
    runtime
        .backend
        .advance(crate::app::parking::INACCESSIBLE_GRACE * 20);
    runtime.cycle(outputs.clone(), &cancelled).unwrap();
    assert_eq!(runtime.workspaces.workspace_of(&live), Some(workspace));
    // A window the window server no longer lists still goes, asleep or not.
    assert_eq!(runtime.workspaces.workspace_of(&closed), None);

    // The display wakes: the grace starts over rather than expiring at once.
    runtime.backend.displays_asleep = false;
    runtime.backend.advance(Duration::from_secs(1));
    runtime.cycle(outputs.clone(), &cancelled).unwrap();
    assert_eq!(runtime.workspaces.workspace_of(&live), Some(workspace));

    // Accessibility reaches the same window again: its slot survived.
    runtime.backend.frame_outcome = None;
    runtime.backend.frames.insert(live.clone(), frame(50));
    runtime.cycle(outputs.clone(), &cancelled).unwrap();
    assert_eq!(runtime.workspaces.workspace_of(&live), Some(workspace));
    assert!(runtime.inaccessible_since.is_empty());
}

#[test]
fn an_awake_grace_still_retires_a_persistently_inaccessible_window() {
    let stuck = window(10);
    let mut backend = FakeBackend {
        observation: Some(observation(&stuck, frame(50))),
        frame_outcome: Some(macos::WindowAccessError::Gone),
        ..FakeBackend::default()
    };
    backend.server_frames.insert(stuck.clone(), frame(50));
    let mut runtime = runtime(backend);
    let workspace = WorkspaceId::new(2).unwrap();
    bind(&mut runtime, workspace, &stuck);
    let (outputs, cancelled) = (outputs(), AtomicBool::new(false));

    runtime.backend.displays_asleep = true;
    runtime.cycle(outputs.clone(), &cancelled).unwrap();
    runtime.backend.displays_asleep = false;
    runtime.cycle(outputs.clone(), &cancelled).unwrap();
    runtime
        .backend
        .advance(crate::app::parking::INACCESSIBLE_GRACE);
    runtime.cycle(outputs, &cancelled).unwrap();
    assert_eq!(runtime.workspaces.workspace_of(&stuck), None);
}
