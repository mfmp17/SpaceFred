//! #509: with `[labs] route_all_windows` on, a `windows = "all"` route rule sent an app's incoming
//! native tab to the rule's workspace, so the tile it replaced vanished from the workspace the
//! user had moved it to. Reported by @coygeek.

use super::super::*;

const RULE: &str = "version = 1\n[labs]\nroute_all_windows = true\n[[rules]]\nbundle_id = 'com.example.Test'\naction = 'route'\nworkspace = 6\nwindows = 'all'";

/// A runtime whose workspace 1 tiles `tab` after one settled cycle, as if moved there by hand.
fn tiled_on_one(tab: &crate::PhysicalWindowId) -> (WorkspaceRuntime<FakeBackend>, crate::Rect) {
    let mut backend = FakeBackend {
        observation: Some(observation(tab, frame(50))),
        focused: Some(tab.clone()),
        ..FakeBackend::default()
    };
    backend.frames.insert(tab.clone(), frame(50));
    let mut runtime = runtime_with_config(backend, Config::parse(RULE).unwrap());
    bind(&mut runtime, WorkspaceId::ONE, tab);
    runtime.cycle(outputs(), &AtomicBool::new(false)).unwrap();
    let accepted = runtime.backend.frames[tab];
    runtime.backend.observation = Some(observation(tab, accepted));
    runtime.cycle(outputs(), &AtomicBool::new(false)).unwrap();
    assert_eq!(runtime.workspaces.workspace_of(tab), Some(WorkspaceId::ONE));
    (runtime, accepted)
}

#[test]
fn an_incoming_native_tab_keeps_its_slot_under_a_route_all_rule() {
    let (previous, replacement) = (window(10), window(11));
    let (mut runtime, accepted) = tiled_on_one(&previous);
    let slot = runtime
        .workspaces
        .engine(WorkspaceId::ONE)
        .slot_for_window(&previous);
    runtime.backend.frames.remove(&previous);
    runtime.backend.frames.insert(replacement.clone(), accepted);
    runtime.backend.focused = Some(replacement.clone());
    runtime.backend.observation = Some(observation(&replacement, accepted));

    runtime.cycle(outputs(), &AtomicBool::new(false)).unwrap();

    assert_eq!(
        runtime.workspaces.workspace_of(&replacement),
        Some(WorkspaceId::ONE)
    );
    assert_eq!(
        runtime
            .workspaces
            .engine(WorkspaceId::ONE)
            .slot_for_window(&replacement),
        slot
    );
    assert!(!runtime.parked.contains_key(&replacement));
    assert_eq!(runtime.backend.frames[&replacement], accepted);
}

#[test]
fn a_new_window_beside_the_tile_still_follows_the_route_all_rule() {
    let (tiled, opened) = (window(10), window(11));
    let (mut runtime, accepted) = tiled_on_one(&tiled);
    runtime.backend.frames.insert(opened.clone(), frame(100));
    runtime.backend.focused = Some(opened.clone());
    runtime.backend.observation = Some(observation_for_windows(
        "com.example.Test",
        &opened,
        &[(tiled.clone(), accepted), (opened.clone(), frame(100))],
    ));

    runtime.cycle(outputs(), &AtomicBool::new(false)).unwrap();

    assert_eq!(
        runtime.workspaces.workspace_of(&tiled),
        Some(WorkspaceId::ONE)
    );
    assert_eq!(
        runtime.workspaces.workspace_of(&opened),
        Some(WorkspaceId::SIX)
    );
}

/// #598: an ignored window from the same app, left open on the tile's display, is no new
/// window to that workspace, so the incoming tab still takes its tile's slot. Reported by
/// @coygeek.
#[test]
fn an_incoming_native_tab_keeps_its_slot_beside_an_ignored_window_of_its_app() {
    const RULES: &str = "version = 1\n[labs]\nroute_all_windows = true\n[[rules]]\nbundle_id = 'com.example.Test'\ntitle = 'Ignored'\naction = 'ignore'\n[[rules]]\nbundle_id = 'com.example.Test'\naction = 'route'\nworkspace = 6\nwindows = 'all'";
    let (previous, replacement, ignored) = (window(10), window(11), window(12));
    let mut backend = FakeBackend {
        observation: Some(observation(&previous, frame(50))),
        focused: Some(previous.clone()),
        ..FakeBackend::default()
    };
    backend.frames.insert(previous.clone(), frame(50));
    let mut runtime = runtime_with_config(backend, Config::parse(RULES).unwrap());
    bind(&mut runtime, WorkspaceId::ONE, &previous);
    runtime.cycle(outputs(), &AtomicBool::new(false)).unwrap();
    let accepted = runtime.backend.frames[&previous];
    runtime.backend.observation = Some(observation(&previous, accepted));
    runtime.cycle(outputs(), &AtomicBool::new(false)).unwrap();
    let slot = runtime
        .workspaces
        .engine(WorkspaceId::ONE)
        .slot_for_window(&previous);

    runtime.backend.frames.remove(&previous);
    runtime.backend.frames.insert(replacement.clone(), accepted);
    runtime.backend.frames.insert(ignored.clone(), frame(100));
    runtime.backend.focused = Some(replacement.clone());
    let mut tabbed = observation_for_windows(
        "com.example.Test",
        &replacement,
        &[
            (replacement.clone(), accepted),
            (ignored.clone(), frame(100)),
        ],
    );
    let crate::AppObservation::Complete { windows, .. } = &mut tabbed.applications[0] else {
        unreachable!();
    };
    windows[1].title = Some("Ignored".into());
    runtime.backend.observation = Some(tabbed);

    runtime.cycle(outputs(), &AtomicBool::new(false)).unwrap();

    assert_eq!(
        runtime.workspaces.workspace_of(&replacement),
        Some(WorkspaceId::ONE)
    );
    assert_eq!(
        runtime
            .workspaces
            .engine(WorkspaceId::ONE)
            .slot_for_window(&replacement),
        slot
    );
    assert!(!runtime.parked.contains_key(&replacement));
    assert_eq!(runtime.backend.frames[&replacement], accepted);
    assert_eq!(runtime.workspaces.workspace_of_any(&ignored), None);
    assert_eq!(runtime.backend.frames[&ignored], frame(100));
}

/// #706: a native tab switch can land in a snapshot that doesn't yet prove the replacement:
/// the outgoing tab still listed at the tile's frame, or focus not yet on the incoming tab.
/// Routing that snapshot sent the tab to the rule's workspace and retired its tile. The tab
/// is held instead, and takes its tile's slot once the evidence settles. Reported by
/// @coygeek.
#[test]
fn a_native_tab_caught_mid_handoff_keeps_its_slot_under_a_route_all_rule() {
    for outgoing_still_listed in [true, false] {
        let (previous, replacement) = (window(10), window(11));
        let (mut runtime, accepted) = tiled_on_one(&previous);
        let slot = runtime
            .workspaces
            .engine(WorkspaceId::ONE)
            .slot_for_window(&previous);

        // Mid-handoff: both tabs listed with focus still on the outgoing one, or the
        // outgoing one gone before focus reaches the incoming one.
        runtime.backend.frames.insert(replacement.clone(), accepted);
        let mut handoff = if outgoing_still_listed {
            observation_for_windows(
                "com.example.Test",
                &previous,
                &[
                    (previous.clone(), accepted),
                    (replacement.clone(), accepted),
                ],
            )
        } else {
            runtime.backend.frames.remove(&previous);
            runtime.backend.focused = None;
            observation(&replacement, accepted)
        };
        if !outgoing_still_listed {
            let crate::AppObservation::Complete { focused, .. } = &mut handoff.applications[0]
            else {
                unreachable!();
            };
            *focused = None;
        }
        runtime.backend.observation = Some(handoff);
        let status = runtime.cycle(outputs(), &AtomicBool::new(false)).unwrap();

        assert_eq!(runtime.workspaces.workspace_of_any(&replacement), None);
        assert_eq!(
            runtime
                .workspaces
                .engine(WorkspaceId::ONE)
                .slot_for_window(&previous),
            slot
        );
        assert!(status.close_recheck.is_some());

        // Settled: the outgoing tab gone and focus on the incoming one.
        runtime.backend.advance(Duration::from_millis(100));
        runtime.backend.frames.remove(&previous);
        runtime.backend.focused = Some(replacement.clone());
        runtime.backend.observation = Some(observation(&replacement, accepted));
        runtime.cycle(outputs(), &AtomicBool::new(false)).unwrap();

        assert_eq!(
            runtime.workspaces.workspace_of(&replacement),
            Some(WorkspaceId::ONE)
        );
        assert_eq!(
            runtime
                .workspaces
                .engine(WorkspaceId::ONE)
                .slot_for_window(&replacement),
            slot
        );
        assert!(!runtime.parked.contains_key(&replacement));
    }
}

/// #706: a window left at a tile's frame with its tile's window still there is only held
/// for a bounded settle; past it, the rule routes it as the new window it is.
#[test]
fn a_new_window_at_a_tiles_frame_follows_the_route_all_rule_after_the_settle() {
    let (tiled, opened) = (window(10), window(11));
    let (mut runtime, accepted) = tiled_on_one(&tiled);
    runtime.backend.frames.insert(opened.clone(), accepted);
    runtime.backend.focused = Some(opened.clone());
    runtime.backend.observation = Some(observation_for_windows(
        "com.example.Test",
        &opened,
        &[(tiled.clone(), accepted), (opened.clone(), accepted)],
    ));
    runtime.cycle(outputs(), &AtomicBool::new(false)).unwrap();
    assert_eq!(runtime.workspaces.workspace_of_any(&opened), None);

    runtime.backend.advance(cycle::TAB_HANDOFF_SETTLE);
    runtime.cycle(outputs(), &AtomicBool::new(false)).unwrap();

    assert_eq!(
        runtime.workspaces.workspace_of(&tiled),
        Some(WorkspaceId::ONE)
    );
    assert_eq!(
        runtime.workspaces.workspace_of(&opened),
        Some(WorkspaceId::SIX)
    );
}

/// #718: a native tab switch can pass through a snapshot with no focused window, the outgoing
/// tab still listed at its tile. That snapshot erased which tab was focused, so the incoming
/// tab could no longer prove it replaced the outgoing one, and after the settle the rule
/// routed it to its workspace. Reported by @coygeek.
#[test]
fn a_native_tab_after_a_focusless_snapshot_keeps_its_slot_under_a_route_all_rule() {
    let (previous, replacement) = (window(10), window(11));
    let (mut runtime, accepted) = tiled_on_one(&previous);
    let slot = runtime
        .workspaces
        .engine(WorkspaceId::ONE)
        .slot_for_window(&previous);

    // Mid-switch: the outgoing tab still listed, but the app reports no focused window.
    runtime.backend.focused = None;
    let mut focusless = observation(&previous, accepted);
    let crate::AppObservation::Complete { focused, .. } = &mut focusless.applications[0] else {
        unreachable!();
    };
    *focused = None;
    runtime.backend.observation = Some(focusless);
    runtime.cycle(outputs(), &AtomicBool::new(false)).unwrap();

    // Settled: the incoming tab focused at the tile's frame, the outgoing one gone.
    runtime.backend.advance(Duration::from_millis(100));
    runtime.backend.frames.remove(&previous);
    runtime.backend.frames.insert(replacement.clone(), accepted);
    runtime.backend.focused = Some(replacement.clone());
    runtime.backend.observation = Some(observation(&replacement, accepted));
    runtime.cycle(outputs(), &AtomicBool::new(false)).unwrap();
    runtime.backend.advance(cycle::TAB_HANDOFF_SETTLE);
    runtime.cycle(outputs(), &AtomicBool::new(false)).unwrap();

    assert_eq!(
        runtime.workspaces.workspace_of(&replacement),
        Some(WorkspaceId::ONE)
    );
    assert_eq!(
        runtime
            .workspaces
            .engine(WorkspaceId::ONE)
            .slot_for_window(&replacement),
        slot
    );
    assert!(!runtime.parked.contains_key(&replacement));
}

/// #756: the app's own "Move Tab to New Window" moves the keyed tab's window out of its group
/// and shows the group's next tab in place, while the moved window stays listed and keyed.
/// Past the handoff settle the rule routed the group's tab as new and left the moved tab on
/// the tile. The group keeps the tile now, and the moved tab takes the route. Reported by
/// @coygeek.
#[test]
fn an_app_detached_tab_takes_the_route_all_rule_and_its_group_keeps_the_tile() {
    let (detached, group) = (window(10), window(11));
    let (mut runtime, accepted) = tiled_on_one(&detached);
    let slot = runtime
        .workspaces
        .engine(WorkspaceId::ONE)
        .slot_for_window(&detached);
    runtime.backend.frames.insert(group.clone(), accepted);
    runtime.backend.focused = Some(detached.clone());
    runtime.backend.observation = Some(observation_for_windows(
        "com.example.Test",
        &detached,
        &[(detached.clone(), accepted), (group.clone(), accepted)],
    ));
    runtime.cycle(outputs(), &AtomicBool::new(false)).unwrap();
    assert_eq!(runtime.workspaces.workspace_of_any(&group), None);

    runtime.backend.advance(cycle::TAB_HANDOFF_SETTLE);
    runtime.cycle(outputs(), &AtomicBool::new(false)).unwrap();

    assert_eq!(
        runtime.workspaces.workspace_of(&group),
        Some(WorkspaceId::ONE)
    );
    assert_eq!(
        runtime
            .workspaces
            .engine(WorkspaceId::ONE)
            .slot_for_window(&group),
        slot
    );
    assert!(!runtime.parked.contains_key(&group));
    assert_eq!(
        runtime.workspaces.workspace_of(&detached),
        Some(WorkspaceId::SIX)
    );
}
