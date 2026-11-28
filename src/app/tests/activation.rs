use super::*;

fn other_app_window(pid: i32, window_id: u32) -> crate::PhysicalWindowId {
    crate::PhysicalWindowId {
        app: AppInstanceId {
            pid,
            launched_at: 1,
        },
        window_id,
    }
}

fn tiles(
    runtime: &WorkspaceRuntime<FakeBackend>,
    workspace: WorkspaceId,
) -> Vec<crate::PhysicalWindowId> {
    runtime.workspaces.engine(workspace).bound_windows()
}

/// Dock, Cmd-Tab, or a click that brings an app's window back on screen switches
/// to that window's workspace. The workspace on screen keeps its tiles.
#[test]
fn activating_an_app_on_another_workspace_switches_instead_of_splitting() {
    // 2 and 5 share main; 10 is hidden on the other display. `suppressed` is the
    // focus left behind by a move, which used to let heal tile the window here.
    for (number, suppressed) in [(2, false), (5, true), (10, false)] {
        let here = window(10);
        let there = other_app_window(2, 20);
        let target = WorkspaceId::new(number).unwrap();
        let mut backend = FakeBackend {
            focused: Some(there.clone()),
            frontmost: Some(there.app.clone()),
            observation: Some(observation(&here, frame(40))),
            ..FakeBackend::default()
        };
        backend.frames.insert(here.clone(), frame(40));
        backend.frames.insert(there.clone(), frame(480));
        let mut runtime = runtime_with_parked(backend, [(there.clone(), frame(480))]);
        bind(&mut runtime, WorkspaceId::ONE, &here);
        bind(&mut runtime, target, &there);
        if suppressed {
            runtime.guard_departure(Some(there.clone()), Some(there.app.clone()));
        }

        runtime.cycle(outputs(), &AtomicBool::new(false)).unwrap();

        let on_main = number < 6;
        assert_eq!(
            runtime.workspaces.active_for(&DisplaySelector::Main),
            Some(if on_main { target } else { WorkspaceId::ONE }),
            "workspace {number}"
        );
        if !on_main {
            assert_eq!(
                runtime.workspaces.active_for(&DisplaySelector::Secondary),
                Some(target)
            );
        }
        assert_eq!(runtime.workspaces.workspace_of(&there), Some(target));
        assert_eq!(tiles(&runtime, WorkspaceId::ONE), vec![here]);
    }
}

/// The Dock focused a window this app just created. Its existing windows are on
/// workspace 5, so the view goes there instead of tiling the new one here.
#[test]
fn an_unowned_focus_follows_the_apps_hidden_workspace() {
    let here = window(10);
    let there = other_app_window(2, 20);
    let fresh = other_app_window(2, 21);
    let target = WorkspaceId::new(5).unwrap();
    let mut backend = FakeBackend {
        focused: Some(fresh),
        frontmost: Some(there.app.clone()),
        observation: Some(observation(&here, frame(40))),
        ..FakeBackend::default()
    };
    backend.frames.insert(here.clone(), frame(40));
    backend
        .frames
        .insert(there.clone(), main_parked_frame(frame(200)));
    let mut runtime = runtime_with_parked(backend, [(there.clone(), frame(200))]);
    bind(&mut runtime, WorkspaceId::ONE, &here);
    bind(&mut runtime, target, &there);

    runtime.cycle(outputs(), &AtomicBool::new(false)).unwrap();

    assert_eq!(
        runtime.workspaces.active_for(&DisplaySelector::Main),
        Some(target)
    );
    assert_eq!(runtime.workspaces.workspace_of(&there), Some(target));
    assert_eq!(tiles(&runtime, WorkspaceId::ONE), vec![here]);
}

/// Cmd-N while the app is already frontmost tiles the new window here, and does
/// not pull that app's parked window off its workspace.
#[test]
fn a_new_window_of_the_frontmost_app_stays_on_the_current_workspace() {
    let here = window(10);
    let hidden = other_app_window(2, 20);
    let opened = other_app_window(2, 30);
    let target = WorkspaceId::new(5).unwrap();
    let mut observed = observation(&here, frame(40));
    observed.applications.extend(
        observation_for_windows(
            "com.example.Other",
            &opened,
            &[(opened.clone(), frame(500)), (hidden.clone(), frame(200))],
        )
        .applications,
    );
    let mut backend = FakeBackend {
        focused: Some(opened.clone()),
        frontmost: Some(hidden.app.clone()),
        observation: Some(observed),
        ..FakeBackend::default()
    };
    backend.frames.insert(here.clone(), frame(40));
    backend.frames.insert(opened.clone(), frame(500));
    backend.frames.insert(hidden.clone(), frame(200));
    let mut runtime = runtime_with_parked(backend, [(hidden.clone(), frame(200))]);
    bind(&mut runtime, WorkspaceId::ONE, &here);
    bind(&mut runtime, target, &hidden);
    runtime.guard_departure(Some(hidden.clone()), Some(hidden.app.clone()));

    runtime.cycle(outputs(), &AtomicBool::new(false)).unwrap();

    assert_eq!(
        runtime.workspaces.active_for(&DisplaySelector::Main),
        Some(WorkspaceId::ONE)
    );
    assert_eq!(runtime.workspaces.workspace_of(&hidden), Some(target));
    assert_eq!(
        runtime.workspaces.workspace_of(&opened),
        Some(WorkspaceId::ONE)
    );
    let owned = tiles(&runtime, WorkspaceId::ONE);
    assert_eq!(owned.len(), 2);
    assert!(owned.contains(&here) && owned.contains(&opened));
}

/// A park that just landed still reports the old on-screen frame. That stale
/// frame must not switch back, and must not tile the window into the workspace
/// on screen.
#[test]
fn a_fresh_park_stays_put_until_it_settles() {
    let here = window(10);
    let there = other_app_window(2, 20);
    let target = WorkspaceId::new(5).unwrap();
    let mut backend = FakeBackend {
        focused: Some(there.clone()),
        frontmost: Some(there.app.clone()),
        observation: Some(observation(&here, frame(40))),
        ..FakeBackend::default()
    };
    backend.frames.insert(here.clone(), frame(40));
    backend.frames.insert(there.clone(), frame(480));
    let mut runtime = runtime_with_parked(backend, [(there.clone(), frame(480))]);
    bind(&mut runtime, WorkspaceId::ONE, &here);
    bind(&mut runtime, target, &there);
    runtime.parked.park_applied(&there, runtime.backend.now());
    runtime.guard_departure(Some(there.clone()), Some(there.app.clone()));

    runtime.cycle(outputs(), &AtomicBool::new(false)).unwrap();

    assert_eq!(
        runtime.workspaces.active_for(&DisplaySelector::Main),
        Some(WorkspaceId::ONE)
    );
    assert_eq!(runtime.workspaces.workspace_of(&there), Some(target));
    assert_eq!(tiles(&runtime, WorkspaceId::ONE), vec![here]);
}
