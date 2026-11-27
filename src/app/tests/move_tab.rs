use super::*;
use crate::app::detach_tab::TAB_DETACH_RECHECK;

const BUNDLE: &str = "com.example.Test";

fn offset_frame(x: i32, y: i32) -> crate::Rect {
    crate::Rect {
        x,
        y,
        width: 400,
        height: 300,
    }
}

/// One tiled window of `com.example.Test` on workspace 1, focused.
fn tabbed_runtime(
    detach: Option<FakeTabDetach>,
    config: Config,
) -> (WorkspaceRuntime<FakeBackend>, crate::PhysicalWindowId) {
    let source = window(1);
    let mut backend = FakeBackend {
        focused: Some(source.clone()),
        frontmost: Some(source.app.clone()),
        observation: Some(observation_for(BUNDLE, &source, frame(0))),
        tab_detach: detach,
        ..FakeBackend::default()
    };
    backend.frames.insert(source.clone(), frame(0));
    let mut rt = runtime_with_config(backend, config);
    bind(&mut rt, WorkspaceId::ONE, &source);
    rt.cycle(outputs(), &AtomicBool::new(false)).unwrap();
    rt.backend.calls.clear();
    (rt, source)
}

fn move_tab(
    rt: &mut WorkspaceRuntime<FakeBackend>,
    target: WorkspaceId,
    follow: bool,
) -> Result<CycleStatus, String> {
    match rt.execute(
        WorkerCommand::Action(WorkerAction::MoveTab { target, follow }, outputs()),
        &AtomicBool::new(false),
    ) {
        Some(WorkerResult::Status { result, .. }) => result,
        _ => panic!("an action returns a status"),
    }
}

fn applied(rt: &WorkspaceRuntime<FakeBackend>, window: &crate::PhysicalWindowId) -> bool {
    rt.backend
        .calls
        .iter()
        .any(|call| matches!(call, BackendCall::Apply(applied, _) if applied == window))
}

fn focus_calls(rt: &WorkspaceRuntime<FakeBackend>) -> Vec<crate::PhysicalWindowId> {
    rt.backend
        .calls
        .iter()
        .filter_map(|call| match call {
            BackendCall::Focus(window) => Some(window.clone()),
            _ => None,
        })
        .collect()
}

#[test]
fn detached_tab_goes_straight_to_a_hidden_workspace_and_focus_stays_on_the_source() {
    let tab = window(2);
    let (mut rt, source) = tabbed_runtime(
        Some(FakeTabDetach::NewWindow(
            tab.clone(),
            offset_frame(100, 100),
        )),
        Config::default(),
    );

    let status = move_tab(&mut rt, WorkspaceId::TWO, false).unwrap();

    assert!(
        rt.backend
            .calls
            .contains(&BackendCall::DetachTab(source.clone()))
    );
    assert_eq!(rt.workspaces.workspace_of(&tab), Some(WorkspaceId::TWO));
    assert!(
        rt.parked.contains_key(&tab),
        "the tab parks with workspace 2"
    );
    assert!(!applied(&rt, &tab), "the tab never tiles on workspace 1");
    assert_eq!(rt.workspaces.workspace_of(&source), Some(WorkspaceId::ONE));
    assert_eq!(
        rt.workspaces.windows(WorkspaceId::ONE),
        vec![source.clone()]
    );
    assert_eq!(focus_calls(&rt), vec![source.clone()]);
    assert_eq!(rt.backend.focused.as_ref(), Some(&source));
    assert_eq!(rt.focus.guarded_window(), Some(&tab));
    assert_eq!(
        rt.workspaces.active_for(&DisplaySelector::Main),
        Some(WorkspaceId::ONE)
    );
    assert!(rt.tab_detach.is_none(), "nothing left pending");
    assert!(status.hud.is_some());

    // Focus settles on the source: later cycles stay on workspace 1.
    rt.cycle(outputs(), &AtomicBool::new(false)).unwrap();
    assert_eq!(
        rt.workspaces.active_for(&DisplaySelector::Main),
        Some(WorkspaceId::ONE)
    );
    assert_eq!(rt.workspaces.workspace_of(&tab), Some(WorkspaceId::TWO));
}

#[test]
fn detached_tab_ignores_the_apps_route_rule() {
    let tab = window(2);
    let config = Config::parse(&format!(
        "version = 1\n[[rules]]\nbundle_id = '{BUNDLE}'\naction = 'route'\nworkspace = 3"
    ))
    .unwrap();
    let (mut rt, source) = tabbed_runtime(
        Some(FakeTabDetach::NewWindow(
            tab.clone(),
            offset_frame(100, 100),
        )),
        config,
    );

    move_tab(&mut rt, WorkspaceId::TWO, false).unwrap();

    assert_eq!(rt.workspaces.workspace_of(&tab), Some(WorkspaceId::TWO));
    assert!(rt.workspaces.windows(WorkspaceId::THREE).is_empty());
    assert_eq!(rt.workspaces.workspace_of(&source), Some(WorkspaceId::ONE));
}

#[test]
fn follow_switches_to_the_target_and_focuses_the_detached_tab() {
    let tab = window(2);
    let (mut rt, source) = tabbed_runtime(
        Some(FakeTabDetach::NewWindow(
            tab.clone(),
            offset_frame(100, 100),
        )),
        Config::default(),
    );

    move_tab(&mut rt, WorkspaceId::TWO, true).unwrap();

    assert_eq!(
        rt.workspaces.active_for(&DisplaySelector::Main),
        Some(WorkspaceId::TWO)
    );
    assert_eq!(rt.workspaces.workspace_of(&tab), Some(WorkspaceId::TWO));
    assert!(!rt.parked.contains_key(&tab));
    assert_eq!(rt.backend.focused.as_ref(), Some(&tab));
    assert!(rt.parked.contains_key(&source), "workspace 1 is hidden now");
    assert!(rt.tab_detach.is_none());
}

#[test]
fn detached_tab_tiles_on_a_target_visible_on_another_display() {
    let tab = window(2);
    let (mut rt, source) = tabbed_runtime(
        Some(FakeTabDetach::NewWindow(
            tab.clone(),
            offset_frame(100, 100),
        )),
        Config::default(),
    );

    move_tab(&mut rt, WorkspaceId::SIX, false).unwrap();

    assert_eq!(rt.workspaces.workspace_of(&tab), Some(WorkspaceId::SIX));
    assert!(!rt.parked.contains_key(&tab));
    let tab_frame = rt.backend.frames[&tab];
    assert!(
        tab_frame.x >= 1000,
        "tiled on the secondary display: {tab_frame:?}"
    );
    assert_eq!(focus_calls(&rt), vec![source.clone()]);
    assert_eq!(rt.focus.guarded_window().cloned(), None);
}

#[test]
fn appkit_tab_group_keeps_its_slot_for_the_next_tab_and_sends_the_moved_tab() {
    let next = window(2);
    let (mut rt, source) = tabbed_runtime(
        Some(FakeTabDetach::TabGroup {
            next: next.clone(),
            moved_to: offset_frame(300, 300),
        }),
        Config::default(),
    );
    let slot = rt
        .workspaces
        .engine(WorkspaceId::ONE)
        .slot_for_window(&source);

    move_tab(&mut rt, WorkspaceId::TWO, false).unwrap();

    assert_eq!(
        rt.workspaces
            .engine(WorkspaceId::ONE)
            .slot_for_window(&next),
        slot,
        "the group's next tab keeps the source slot"
    );
    assert_eq!(rt.workspaces.windows(WorkspaceId::ONE), vec![next.clone()]);
    assert_eq!(rt.workspaces.workspace_of(&source), Some(WorkspaceId::TWO));
    assert!(rt.parked.contains_key(&source));
    assert_eq!(focus_calls(&rt), vec![next.clone()]);
    assert!(rt.tab_detach.is_none());
}

#[test]
fn appkit_tab_group_sends_the_moved_tab_when_an_ignore_rule_matches_the_next_tab() {
    // #670: the tab left behind matches Ignore; the moved tab still leaves.
    let source = window(1);
    let next = window(2);
    let mut observation = observation_for(BUNDLE, &source, frame(0));
    if let crate::AppObservation::Complete { windows, .. } = &mut observation.applications[0] {
        windows[0].title = Some("Work Document".into());
    }
    let mut backend = FakeBackend {
        focused: Some(source.clone()),
        frontmost: Some(source.app.clone()),
        observation: Some(observation),
        tab_detach: Some(FakeTabDetach::TabGroup {
            next: next.clone(),
            moved_to: offset_frame(300, 300),
        }),
        ..FakeBackend::default()
    };
    backend.frames.insert(source.clone(), frame(0));
    let config = Config::parse(&format!(
        "version = 1\n[[rules]]\nbundle_id = '{BUNDLE}'\ntitle = 'Work Document'\naction = 'tile'\n\
         [[rules]]\nbundle_id = '{BUNDLE}'\naction = 'ignore'"
    ))
    .unwrap();
    let mut rt = runtime_with_config(backend, config);
    bind(&mut rt, WorkspaceId::ONE, &source);
    rt.cycle(outputs(), &AtomicBool::new(false)).unwrap();
    rt.backend.calls.clear();

    move_tab(&mut rt, WorkspaceId::TWO, true).unwrap();

    assert_eq!(rt.workspaces.workspace_of(&source), Some(WorkspaceId::TWO));
    assert!(rt.workspaces.windows(WorkspaceId::ONE).is_empty());
    assert_eq!(rt.workspaces.workspace_of_any(&next), None);
    assert!(!rt.parked.contains_key(&next));
    assert_eq!(
        rt.workspaces.active_for(&DisplaySelector::Main),
        Some(WorkspaceId::TWO)
    );
    assert_eq!(rt.backend.focused.as_ref(), Some(&source));
    assert!(rt.tab_detach.is_none());
}

#[test]
fn single_tab_window_moves_whole_like_move() {
    let (mut rt, source) = tabbed_runtime(None, Config::default());

    move_tab(&mut rt, WorkspaceId::TWO, false).unwrap();

    assert!(
        rt.backend
            .calls
            .contains(&BackendCall::DetachTab(source.clone()))
    );
    assert_eq!(rt.workspaces.workspace_of(&source), Some(WorkspaceId::TWO));
    assert!(rt.parked.contains_key(&source));
    assert!(rt.workspaces.windows(WorkspaceId::ONE).is_empty());
    assert!(rt.tab_detach.is_none());
}

#[test]
fn single_tab_window_with_follow_moves_and_switches() {
    let (mut rt, source) = tabbed_runtime(None, Config::default());

    move_tab(&mut rt, WorkspaceId::TWO, true).unwrap();

    assert_eq!(rt.workspaces.workspace_of(&source), Some(WorkspaceId::TWO));
    assert_eq!(
        rt.workspaces.active_for(&DisplaySelector::Main),
        Some(WorkspaceId::TWO)
    );
    assert!(!rt.parked.contains_key(&source));
}

#[test]
fn a_late_window_is_claimed_by_a_prompt_re_observation() {
    let tab = window(2);
    let (mut rt, source) = tabbed_runtime(Some(FakeTabDetach::Silent), Config::default());

    let status = move_tab(&mut rt, WorkspaceId::TWO, false).unwrap();

    assert!(rt.tab_detach.is_some(), "waiting for the window");
    let recheck = status.close_recheck.expect("a prompt re-observation");
    assert!(recheck <= rt.backend.now() + TAB_DETACH_RECHECK);

    rt.backend.open_window(&tab, offset_frame(100, 100));
    rt.backend.focused = Some(tab.clone());
    rt.cycle(outputs(), &AtomicBool::new(false)).unwrap();

    assert_eq!(rt.workspaces.workspace_of(&tab), Some(WorkspaceId::TWO));
    assert!(rt.parked.contains_key(&tab));
    assert_eq!(rt.backend.focused.as_ref(), Some(&source));
    assert!(rt.tab_detach.is_none());
}

#[test]
fn an_expired_detach_drops_cleanly_and_later_windows_open_normally() {
    let late = window(2);
    let (mut rt, source) = tabbed_runtime(Some(FakeTabDetach::Silent), Config::default());

    move_tab(&mut rt, WorkspaceId::TWO, false).unwrap();
    rt.tab_detach.as_mut().unwrap().expires_at = rt.backend.now();
    let status = rt.cycle(outputs(), &AtomicBool::new(false)).unwrap();

    assert!(rt.tab_detach.is_none(), "dropped at expiry");
    assert_eq!(status.close_recheck, None);
    assert_eq!(rt.workspaces.workspace_of(&source), Some(WorkspaceId::ONE));

    rt.backend.open_window(&late, offset_frame(100, 100));
    rt.cycle(outputs(), &AtomicBool::new(false)).unwrap();
    assert_eq!(rt.workspaces.workspace_of(&late), Some(WorkspaceId::ONE));
    assert!(rt.workspaces.windows(WorkspaceId::TWO).is_empty());
}

#[test]
fn a_failed_press_reports_an_error_and_arms_nothing() {
    let (mut rt, source) = tabbed_runtime(Some(FakeTabDetach::Failed), Config::default());

    let Err(error) = move_tab(&mut rt, WorkspaceId::TWO, false) else {
        panic!("a failed press is an error");
    };

    assert!(error.contains("Move Tab to New Window failed"), "{error}");
    assert!(rt.tab_detach.is_none());
    assert_eq!(rt.workspaces.workspace_of(&source), Some(WorkspaceId::ONE));
}

#[test]
fn other_apps_new_windows_are_not_claimed() {
    let tab = window(2);
    let other = crate::PhysicalWindowId {
        app: AppInstanceId {
            pid: 9,
            launched_at: 1,
        },
        window_id: 90,
    };
    let (mut rt, source) = tabbed_runtime(Some(FakeTabDetach::Silent), Config::default());
    move_tab(&mut rt, WorkspaceId::TWO, false).unwrap();
    if let Some(observation) = rt.backend.observation.as_mut() {
        observation
            .applications
            .push(crate::AppObservation::Complete {
                app: crate::ObservedApplication {
                    id: other.app.clone(),
                    bundle_id: "com.example.Other".into(),
                },
                focused: None,
                windows: Vec::new(),
            });
    }
    rt.backend.open_window(&other, offset_frame(500, 100));
    rt.cycle(outputs(), &AtomicBool::new(false)).unwrap();

    assert_eq!(rt.workspaces.workspace_of(&other), Some(WorkspaceId::ONE));
    assert!(rt.tab_detach.is_some(), "still waiting for its own window");

    // Chromium focuses the tab's new window.
    rt.backend.open_window(&tab, offset_frame(100, 100));
    rt.backend.focused = Some(tab.clone());
    rt.cycle(outputs(), &AtomicBool::new(false)).unwrap();
    assert_eq!(rt.workspaces.workspace_of(&tab), Some(WorkspaceId::TWO));
    assert_eq!(rt.workspaces.workspace_of(&source), Some(WorkspaceId::ONE));
}

#[test]
fn a_second_move_tab_lets_the_pending_detach_reach_its_own_target() {
    let first = window(2);
    let second = window(3);
    let (mut rt, source) = tabbed_runtime(Some(FakeTabDetach::Silent), Config::default());

    move_tab(&mut rt, WorkspaceId::TWO, false).unwrap();
    assert!(rt.tab_detach.is_some(), "waiting for the window");

    // The first tab's window is on screen but not yet observed when the
    // second command arrives (#570).
    rt.backend.open_window(&first, offset_frame(100, 100));
    rt.backend.focused = Some(source.clone());
    rt.backend.tab_detach = Some(FakeTabDetach::NewWindow(
        second.clone(),
        offset_frame(200, 200),
    ));
    move_tab(&mut rt, WorkspaceId::THREE, false).unwrap();

    assert_eq!(rt.workspaces.workspace_of(&first), Some(WorkspaceId::TWO));
    assert_eq!(
        rt.workspaces.workspace_of(&second),
        Some(WorkspaceId::THREE)
    );
    assert_eq!(rt.workspaces.workspace_of(&source), Some(WorkspaceId::ONE));
    assert!(rt.tab_detach.is_none());
}

#[test]
fn a_second_move_tab_waits_out_a_pending_detach_that_never_opens() {
    let second = window(3);
    let (mut rt, source) = tabbed_runtime(Some(FakeTabDetach::Silent), Config::default());

    move_tab(&mut rt, WorkspaceId::TWO, false).unwrap();
    rt.backend.tab_detach = Some(FakeTabDetach::NewWindow(
        second.clone(),
        offset_frame(200, 200),
    ));
    move_tab(&mut rt, WorkspaceId::THREE, false).unwrap();

    assert!(rt.workspaces.windows(WorkspaceId::TWO).is_empty());
    assert_eq!(
        rt.workspaces.workspace_of(&second),
        Some(WorkspaceId::THREE)
    );
    assert_eq!(rt.workspaces.workspace_of(&source), Some(WorkspaceId::ONE));
    assert!(rt.tab_detach.is_none());
}

#[test]
fn rapid_double_move_tab_follow_detaches_two_tabs_from_the_source() {
    let first = window(2);
    let second = window(3);
    let (mut rt, source) = tabbed_runtime(Some(FakeTabDetach::Silent), Config::default());

    move_tab(&mut rt, WorkspaceId::TWO, true).unwrap();
    assert!(rt.tab_detach.is_some(), "waiting for first detached tab");

    // The first tab's window appeared on screen, not yet observed; the user
    // still has the source window focused and immediately presses move-tab 3 --follow.
    rt.backend.open_window(&first, offset_frame(100, 100));
    rt.backend.focused = Some(source.clone());
    rt.backend.tab_detach = Some(FakeTabDetach::NewWindow(
        second.clone(),
        offset_frame(200, 200),
    ));
    move_tab(&mut rt, WorkspaceId::THREE, true).unwrap();

    assert_eq!(rt.workspaces.workspace_of(&first), Some(WorkspaceId::TWO));
    assert_eq!(
        rt.workspaces.workspace_of(&second),
        Some(WorkspaceId::THREE)
    );
    assert_eq!(rt.workspaces.workspace_of(&source), Some(WorkspaceId::ONE));
    let detached_from: Vec<_> = rt
        .backend
        .calls
        .iter()
        .filter_map(|call| match call {
            BackendCall::DetachTab(window) => Some(window.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(detached_from, vec![source.clone(), source]);
    assert!(rt.tab_detach.is_none());
}

#[test]
fn rapid_double_move_tab_follow_appkit_tab_group_follows_remaining() {
    let remaining = window(2);
    let third = window(3);
    let (mut rt, source) = tabbed_runtime(Some(FakeTabDetach::Silent), Config::default());

    // Send first tab to visible workspace 6 on secondary with --follow so
    // workspace 1 stays on screen while focus jumps to the detached tab.
    move_tab(&mut rt, WorkspaceId::SIX, true).unwrap();
    assert!(rt.tab_detach.is_some());

    // AppKit tab group: source moves out to a new position, and `remaining`
    // takes over source's frame on workspace 1.
    let source_frame = rt.backend.frames[&source];
    rt.backend
        .frames
        .insert(source.clone(), offset_frame(120, 120));
    rt.backend.open_window(&remaining, source_frame);
    rt.backend.focused = Some(source.clone());
    rt.backend.tab_detach = Some(FakeTabDetach::TabGroup {
        next: third.clone(),
        moved_to: offset_frame(240, 240),
    });
    move_tab(&mut rt, WorkspaceId::THREE, true).unwrap();

    assert_eq!(rt.workspaces.workspace_of(&source), Some(WorkspaceId::SIX));
    assert_eq!(
        rt.workspaces.workspace_of(&remaining),
        Some(WorkspaceId::THREE)
    );
    assert_eq!(rt.workspaces.workspace_of(&third), Some(WorkspaceId::ONE));
    let detached_from: Vec<_> = rt
        .backend
        .calls
        .iter()
        .filter_map(|call| match call {
            BackendCall::DetachTab(window) => Some(window.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(detached_from, vec![source, remaining]);
}

#[test]
fn queued_move_tab_follow_detaches_the_next_tab_after_the_first_settled() {
    // #597: both commands were queued before the first claim, but the first
    // detach settled in an ordinary cycle before the second command ran.
    let remaining = window(2);
    let third = window(3);
    let (mut rt, source) = tabbed_runtime(Some(FakeTabDetach::Silent), Config::default());

    move_tab(&mut rt, WorkspaceId::SIX, true).unwrap();
    let source_frame = rt.backend.frames[&source];
    rt.backend
        .frames
        .insert(source.clone(), offset_frame(120, 120));
    rt.backend.open_window(&remaining, source_frame);
    rt.cycle(outputs(), &AtomicBool::new(false)).unwrap();
    assert!(rt.tab_detach.is_none(), "the first detach settled");
    assert_eq!(rt.workspaces.workspace_of(&source), Some(WorkspaceId::SIX));
    assert_eq!(rt.backend.focused, Some(source.clone()), "followed");

    rt.backend.tab_detach = Some(FakeTabDetach::TabGroup {
        next: third.clone(),
        moved_to: offset_frame(240, 240),
    });
    move_tab(&mut rt, WorkspaceId::THREE, true).unwrap();

    assert_eq!(rt.workspaces.workspace_of(&source), Some(WorkspaceId::SIX));
    assert_eq!(
        rt.workspaces.workspace_of(&remaining),
        Some(WorkspaceId::THREE)
    );
    assert_eq!(rt.workspaces.workspace_of(&third), Some(WorkspaceId::ONE));
    assert!(rt.tab_detach.is_none());
}

#[test]
fn a_second_move_tab_detaches_from_the_group_its_follow_switched_away_from() {
    // #597: `move-tab 2 --follow` hides workspace 1 on the same display, so
    // the group's next tab is parked when the second command presses on it.
    for follow in [false, true] {
        let remaining = window(2);
        let third = window(3);
        let (mut rt, source) = tabbed_runtime(Some(FakeTabDetach::Silent), Config::default());

        move_tab(&mut rt, WorkspaceId::TWO, true).unwrap();
        let source_frame = rt.backend.frames[&source];
        rt.backend
            .frames
            .insert(source.clone(), offset_frame(120, 120));
        rt.backend.open_window(&remaining, source_frame);
        rt.cycle(outputs(), &AtomicBool::new(false)).unwrap();
        assert_eq!(
            rt.workspaces.active_for(&DisplaySelector::Main),
            Some(WorkspaceId::TWO)
        );
        assert_eq!(
            rt.workspaces.workspace_of(&remaining),
            Some(WorkspaceId::ONE)
        );
        assert!(rt.parked.contains_key(&remaining));
        assert_eq!(rt.backend.focused.as_ref(), Some(&source));

        rt.backend.tab_detach = Some(FakeTabDetach::TabGroup {
            next: third.clone(),
            moved_to: offset_frame(240, 240),
        });
        move_tab(&mut rt, WorkspaceId::THREE, follow).unwrap();

        assert_eq!(rt.workspaces.workspace_of(&source), Some(WorkspaceId::TWO));
        assert_eq!(
            rt.workspaces.workspace_of(&remaining),
            Some(WorkspaceId::THREE),
            "follow={follow}"
        );
        assert_eq!(rt.workspaces.workspace_of(&third), Some(WorkspaceId::ONE));
        let (shown, focused) = if follow {
            (WorkspaceId::THREE, &remaining)
        } else {
            (WorkspaceId::TWO, &source)
        };
        assert_eq!(
            rt.workspaces.active_for(&DisplaySelector::Main),
            Some(shown),
            "follow={follow}"
        );
        assert_eq!(
            rt.backend.focused.as_ref(),
            Some(focused),
            "follow={follow}"
        );
        assert!(rt.tab_detach.is_none());

        // Focus follow does not chase the parked group back to workspace 1.
        rt.cycle(outputs(), &AtomicBool::new(false)).unwrap();
        assert_eq!(
            rt.workspaces.active_for(&DisplaySelector::Main),
            Some(shown),
            "follow={follow}"
        );
    }
}

#[test]
fn a_second_move_tab_without_follow_leaves_focus_on_the_followed_tab() {
    let remaining = window(2);
    let third = window(3);
    let (mut rt, source) = tabbed_runtime(Some(FakeTabDetach::Silent), Config::default());

    move_tab(&mut rt, WorkspaceId::SIX, true).unwrap();
    let source_frame = rt.backend.frames[&source];
    rt.backend
        .frames
        .insert(source.clone(), offset_frame(120, 120));
    rt.backend.open_window(&remaining, source_frame);
    rt.cycle(outputs(), &AtomicBool::new(false)).unwrap();
    assert_eq!(rt.backend.focused.as_ref(), Some(&source));

    rt.backend.tab_detach = Some(FakeTabDetach::TabGroup {
        next: third.clone(),
        moved_to: offset_frame(240, 240),
    });
    move_tab(&mut rt, WorkspaceId::THREE, false).unwrap();

    assert_eq!(rt.workspaces.workspace_of(&source), Some(WorkspaceId::SIX));
    assert_eq!(
        rt.workspaces.workspace_of(&remaining),
        Some(WorkspaceId::THREE)
    );
    assert_eq!(rt.workspaces.workspace_of(&third), Some(WorkspaceId::ONE));
    assert_eq!(rt.backend.focused.as_ref(), Some(&source));
    assert_eq!(
        rt.workspaces.active_for(&DisplaySelector::Main),
        Some(WorkspaceId::ONE)
    );
}

/// `source` was just focused by a Spaceman command from `left` on workspace
/// 6, but Accessibility still reports `left`.
fn lagging_focus_runtime(
    detach: Option<FakeTabDetach>,
) -> (
    WorkspaceRuntime<FakeBackend>,
    crate::PhysicalWindowId,
    crate::PhysicalWindowId,
) {
    let left = window(5);
    let (mut rt, source) = tabbed_runtime(detach, Config::default());
    rt.backend.open_window(&left, offset_frame(1100, 100));
    bind(&mut rt, WorkspaceId::SIX, &left);
    rt.backend.focused = Some(left.clone());
    rt.cycle(outputs(), &AtomicBool::new(false)).unwrap();
    assert!(rt.request_focus(&source, [Some(left.clone())]));
    rt.backend.focused = Some(left.clone());
    rt.backend.calls.clear();
    (rt, source, left)
}

#[test]
fn move_tab_with_a_lagging_focus_report_keeps_focus_on_the_remaining_tabs() {
    let tab = window(2);
    let (mut rt, source, left) = lagging_focus_runtime(Some(FakeTabDetach::NewWindow(
        tab.clone(),
        offset_frame(100, 100),
    )));

    move_tab(&mut rt, WorkspaceId::TWO, false).unwrap();

    assert_eq!(rt.workspaces.workspace_of(&tab), Some(WorkspaceId::TWO));
    assert_eq!(rt.workspaces.workspace_of(&source), Some(WorkspaceId::ONE));
    assert_eq!(rt.backend.focused.as_ref(), Some(&source));
    assert!(!focus_calls(&rt).contains(&left), "focus never goes back");
}

#[test]
fn single_tab_move_tab_with_a_lagging_focus_report_moves_like_move() {
    let (mut rt, source, left) = lagging_focus_runtime(None);

    move_tab(&mut rt, WorkspaceId::TWO, false).unwrap();

    assert_eq!(rt.workspaces.workspace_of(&source), Some(WorkspaceId::TWO));
    assert!(rt.parked.contains_key(&source));
    assert!(!focus_calls(&rt).contains(&left), "focus never goes back");
    // `move` of the focused window guards it as it leaves its emptied workspace.
    assert_eq!(rt.focus.guarded_window(), Some(&source));
}

#[test]
fn move_tab_does_not_claim_unowned_window_when_cg_snapshot_fails() {
    let stray = window(2);
    let (mut rt, source) = tabbed_runtime(Some(FakeTabDetach::Silent), Config::default());
    rt.backend.server_snapshot_unavailable = true;

    move_tab(&mut rt, WorkspaceId::TWO, false).unwrap();
    rt.backend.server_snapshot_unavailable = false;
    rt.backend.open_window(&stray, offset_frame(100, 100));
    rt.cycle(outputs(), &AtomicBool::new(false)).unwrap();

    // Without pre-press CG evidence, `stray` must not be routed to workspace 2.
    assert_eq!(rt.workspaces.workspace_of(&stray), Some(WorkspaceId::ONE));
    assert_eq!(rt.workspaces.workspace_of(&source), Some(WorkspaceId::ONE));
    assert!(rt.workspaces.windows(WorkspaceId::TWO).is_empty());
}
