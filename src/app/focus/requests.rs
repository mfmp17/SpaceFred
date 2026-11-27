//! The runtime's side of the focus automaton: every focus Spaceman requests
//! and every departure it guards goes through here, on the worker's clock.

use super::super::*;
use super::DepartureGuard;

impl<B: WorkspaceBackend> WorkspaceRuntime<B> {
    /// The focused window, trusting a focus command still in flight over the
    /// lagging report Accessibility gives until the target app activates.
    /// With `[labs] empty_workspace_focus`, the window left focused when an
    /// empty workspace was chosen on another display counts as no focus, so
    /// commands act on the chosen workspace.
    pub(in crate::app) fn current_focus(&mut self) -> Option<crate::PhysicalWindowId> {
        let reported = self.backend.focused_window();
        if self.holds_empty_choice(reported.as_ref()) {
            return None;
        }
        self.resolve_focus(reported)
    }

    /// Resolves reported against a focus command still in flight, settling it.
    pub(in crate::app) fn resolve_focus(
        &mut self,
        reported: Option<crate::PhysicalWindowId>,
    ) -> Option<crate::PhysicalWindowId> {
        let now = self.backend.now();
        self.focus.resolve(now, reported)
    }

    /// The effective focus without settling anything (diagnostics, and the
    /// focus a switch leaves behind).
    pub(in crate::app) fn effective_focus(
        &self,
        reported: Option<crate::PhysicalWindowId>,
    ) -> Option<crate::PhysicalWindowId> {
        self.focus.effective(self.backend.now(), reported)
    }

    /// Asks the app to focus target and, when it accepts, records the request
    /// so lagging reports of left (and of any request it supersedes) stand for
    /// target until it settles. A tile focused this way is where Dwindle
    /// splits next, even before an observation confirms it (#411).
    pub(in crate::app) fn request_focus(
        &mut self,
        target: &crate::PhysicalWindowId,
        left: impl IntoIterator<Item = Option<crate::PhysicalWindowId>>,
    ) -> bool {
        if !self.backend.focus_window(target) {
            return false;
        }
        let now = self.backend.now();
        self.focus.request(now, target.clone(), left);
        if let Some(workspace) = self.workspaces.workspace_of(target) {
            self.workspaces
                .engine_mut(workspace)
                .remember_insertion_focus(target);
        }
        true
    }

    /// While Spaceman is focusing a window on a shown workspace, its app may
    /// hand focus to another of its windows on a different workspace as it
    /// activates: a hidden one focus follow would switch to (#534), or one
    /// another display already shows, which would leave the keys there
    /// (#591). That is the app's doing, not the user's: focus the requested
    /// window again; the request still settles at its original deadline, so
    /// an app that insists is followed after that. A pinned sibling counts as
    /// the workspace its display shows, never its hidden home (#657).
    ///
    /// An app can activate late, handing focus over after the request settled
    /// (#737). Until `SIBLING_HANDOFF` has passed with no key press or
    /// click since the request, that sibling is refocused away from once too.
    pub(in crate::app) fn reclaim_from_sibling(
        &mut self,
        reported: &crate::PhysicalWindowId,
        visible: &[ResolvedWorkspace],
    ) {
        let is_shown =
            |workspace: WorkspaceId| visible.iter().any(|shown| shown.workspace == workspace);
        let now = self.backend.now();
        let (target, late) = match self.focus.sibling_target(now, reported) {
            Some(target) => (target, false),
            None => match self.focus.late_sibling_target(now, reported) {
                Some((target, issued_at)) if !self.input_since(now, issued_at) => (target, true),
                _ => return,
            },
        };
        let Some(target_workspace) = self
            .workspaces
            .workspace_of_any(&target)
            .filter(|workspace| is_shown(*workspace))
        else {
            return;
        };
        // A new, unmanaged or scratchpad window keeps focus, and so does a pin
        // whose frame is unreadable or that shares the target's display.
        let reported_workspace = if self.workspaces.is_pinned(reported) {
            self.visible_workspace_under(reported, visible)
                .map(|shown| shown.workspace)
        } else {
            self.workspaces.workspace_of_any(reported)
        };
        if reported_workspace.is_none_or(|workspace| workspace == target_workspace) {
            return;
        }
        if late {
            if !self.backend.focus_window(&target) {
                return;
            }
            self.focus.reclaim_late(now, reported);
            log_event!(
                "focus follow: {:?} took focus late from requested {:?}; focusing it again",
                reported,
                target
            );
            return;
        }
        // Refocus once per sibling; later reports of it are already explained.
        if !self.focus.explain_as_target(reported) {
            return;
        }
        log_event!(
            "focus follow: {:?} took focus from requested {:?}; focusing it again",
            reported,
            target
        );
        let _: bool = self.backend.focus_window(&target);
    }

    /// Whether the user pressed a key or clicked since `since`. With no
    /// reading it may have been, so a late report is followed as before.
    fn input_since(&mut self, now: Instant, since: Instant) -> bool {
        let Some(input) = self.backend.recent_input() else {
            return true;
        };
        let elapsed = now.saturating_duration_since(since).as_secs_f64();
        input.key_down.min(input.mouse_down) < elapsed
    }

    /// Guards the focus left behind so focus follow does not chase it.
    pub(in crate::app) fn guard_departure(
        &mut self,
        window: Option<crate::PhysicalWindowId>,
        app: Option<AppInstanceId>,
    ) {
        self.focus.guard_departure(DepartureGuard { window, app });
    }

    /// Guards whatever Accessibility reports now (a rollback leaves it behind).
    pub(in crate::app) fn guard_reported_focus(&mut self) {
        let window = self.backend.focused_window();
        let app = self.backend.frontmost_application();
        self.guard_departure(window, app);
    }

    pub(in crate::app) fn clear_suppressed_focus(&mut self) {
        self.focus.clear_guard();
    }

    /// Remember the workspace that just took focus. The first record after a
    /// launch seeds the previous workspace from the focused display's persisted
    /// lane history, so Option+Tab works straight away as it did before.
    pub(in crate::app) fn record_focused_workspace(&mut self, workspace: WorkspaceId) {
        let workspaces = &self.workspaces;
        self.focus.record_workspace(workspace, || {
            workspaces.previous_for(workspaces.assignment(workspace))
        });
    }
}
