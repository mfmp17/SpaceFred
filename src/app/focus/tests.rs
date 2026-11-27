use super::*;

fn win(id: u32) -> PhysicalWindowId {
    PhysicalWindowId {
        app: AppInstanceId {
            pid: i32::try_from(id).unwrap(),
            launched_at: 1,
        },
        window_id: id,
    }
}

#[test]
fn a_request_stands_in_for_lagging_and_missing_reports_until_it_settles() {
    let now = Instant::now();
    let mut focus = FocusTracker::default();
    focus.request(now, win(2), [Some(win(1))]);
    assert_eq!(focus.effective(now, Some(win(1))), Some(win(2)));
    // No report at all is no evidence against the request (#398).
    assert_eq!(focus.effective(now, None), Some(win(2)));
    // A window the request never left is a real focus change.
    assert_eq!(focus.effective(now, Some(win(3))), Some(win(3)));
    // Past the settle window the report wins.
    let later = now + FOCUS_SETTLE;
    assert_eq!(focus.effective(later, Some(win(1))), Some(win(1)));
}

#[test]
fn resolving_ends_the_request_on_its_target_or_an_unexplained_report() {
    let now = Instant::now();
    let mut focus = FocusTracker::default();
    focus.request(now, win(2), [Some(win(1))]);
    assert_eq!(focus.resolve(now, Some(win(1))), Some(win(2)));
    assert!(focus.in_flight(now).is_some());
    assert_eq!(focus.resolve(now, Some(win(2))), Some(win(2)));
    assert!(matches!(focus.phase(), FocusPhase::Synced));
}

#[test]
fn a_superseding_request_inherits_the_stale_reports_and_the_old_target() {
    let now = Instant::now();
    let mut focus = FocusTracker::default();
    focus.request(now, win(2), [Some(win(1))]);
    focus.request(now, win(3), [Some(win(2))]);
    let request = focus.in_flight(now).unwrap();
    assert_eq!(request.stale, [Some(win(1)), Some(win(2))]);
    // Re-targeting a window that was stale drops it from the list (#391).
    focus.request(now, win(1), []);
    let request = focus.in_flight(now).unwrap();
    assert_eq!(request.stale, [Some(win(2)), Some(win(3))]);
    // An expired request is not inherited.
    focus.request(now + FOCUS_SETTLE, win(4), [None]);
    assert_eq!(focus.in_flight(now + FOCUS_SETTLE).unwrap().stale, [None]);
}

#[test]
fn a_departure_guard_survives_a_request_and_its_settling() {
    let now = Instant::now();
    let mut focus = FocusTracker::default();
    focus.guard_departure(DepartureGuard {
        window: Some(win(1)),
        app: Some(win(1).app),
    });
    focus.request(now, win(2), []);
    assert_eq!(focus.guarded_window(), Some(&win(1)));
    focus.resolve(now, Some(win(2)));
    assert!(matches!(focus.phase(), FocusPhase::GuardedDeparture { .. }));
    assert!(focus.holds_back(now, Some(&win(1)), Some(&win(1).app), true));
    assert!(!focus.holds_back(now, Some(&win(3)), Some(&win(3).app), true));
    focus.clear_guard();
    assert!(matches!(focus.phase(), FocusPhase::Synced));
}

#[test]
fn a_report_the_request_explains_is_held_back_from_focus_follow() {
    let now = Instant::now();
    let mut focus = FocusTracker::default();
    focus.request(now, win(2), [Some(win(1))]);
    assert!(focus.holds_back(now, Some(&win(1)), Some(&win(1).app), true));
    assert!(!focus.holds_back(now, Some(&win(3)), Some(&win(3).app), true));
}

#[test]
fn a_failed_follow_keeps_the_guard_and_is_forgotten_on_its_own() {
    let now = Instant::now();
    let mut focus = FocusTracker::default();
    focus.guard_departure(DepartureGuard {
        window: Some(win(1)),
        app: None,
    });
    focus.follow_failed(now);
    assert_eq!(focus.failed_follow_at(), Some(now));
    assert_eq!(focus.guarded_window(), Some(&win(1)));
    focus.forget_failed_follow();
    assert_eq!(focus.failed_follow_at(), None);
    assert_eq!(focus.guarded_window(), Some(&win(1)));
}

#[test]
fn history_seeds_once_then_tracks_the_last_two_workspaces() {
    let mut focus = FocusTracker::default();
    focus.record_workspace(WorkspaceId::ONE, || Some(WorkspaceId::SIX));
    assert_eq!(focus.history().previous, Some(WorkspaceId::SIX));
    focus.record_workspace(WorkspaceId::TWO, || unreachable!());
    focus.record_workspace(WorkspaceId::TWO, || unreachable!());
    let history = focus.history();
    assert_eq!(
        (history.current, history.previous),
        (Some(WorkspaceId::TWO), Some(WorkspaceId::ONE))
    );
}
