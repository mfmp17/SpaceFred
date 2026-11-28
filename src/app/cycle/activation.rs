//! Dock, Cmd-Tab and other activations. When an app's windows all live on one
//! hidden workspace, activating it switches there. A parked window that comes
//! back on screen is that activation, not a request to tile it into the
//! workspace currently showing. A new window of an app that was already
//! frontmost (Cmd-N) stays on the active workspace.

use std::collections::HashMap;

use super::*;

impl<B: WorkspaceBackend> WorkspaceRuntime<B> {
    /// Hidden workspace to follow when focus landed on a window this app does
    /// not own yet. Dock and Cmd-Tab can focus a window the app just created
    /// while the windows it already owns live elsewhere. None when that app
    /// was already frontmost (the departure guard still names it): Cmd-N tiles
    /// the new window here.
    pub(super) fn unowned_activation_workspace(
        &self,
        app: &AppInstanceId,
        visible: &[ResolvedWorkspace],
        alive: impl Fn(&crate::PhysicalWindowId) -> bool,
    ) -> Option<WorkspaceId> {
        if self.focus.guard().and_then(|guard| guard.app.as_ref()) == Some(app) {
            return None;
        }
        let workspace = self.workspaces.workspace_for_live_app(app, alive)?;
        visible
            .iter()
            .all(|resolved| resolved.workspace != workspace)
            .then_some(workspace)
    }

    /// The hidden workspace a frontmost app just brought a parked window back
    /// onto. None when the app already has a window on a visible workspace,
    /// when the focused window is new and not yet owned (a fresh activation
    /// follows [`Self::unowned_activation_workspace`]; Cmd-N stays here), when
    /// those windows live on more than one hidden workspace, or while a fresh
    /// park is still settling (a switch just made must not bounce on a stale
    /// frame). A window macOS put back while leaving a native fullscreen Space
    /// is not an activation (#489). A pinned window shows everywhere, so it
    /// never pulls its home workspace on screen.
    pub(super) fn workspace_summoned_on_screen(
        &mut self,
        visible: &[ResolvedWorkspace],
        frontmost: Option<&AppInstanceId>,
        focused: Option<&crate::PhysicalWindowId>,
        server_frames: &HashMap<u32, crate::Rect>,
    ) -> Option<WorkspaceId> {
        let app = frontmost?;
        if self.app_has_visible_window(app, visible) {
            return None;
        }
        // A new window of the app that was already frontmost is Cmd-N: it
        // tiles here. Its parked siblings stay on their own workspace.
        if focused.is_some_and(|focused| {
            &focused.app == app && self.workspaces.workspace_of_any(focused).is_none()
        }) {
            return None;
        }
        let slide = self.space_slide_just_settled();
        let now = self.backend.now();
        let parked: Vec<_> = self.parked.keys().cloned().collect();
        let mut home = None;
        for window in parked {
            if &window.app != app
                || self.workspaces.is_pinned(&window)
                || self.parked.awaits_display(&window)
                || self.parked.settling(&window, now)
                || (slide && focused != Some(&window))
            {
                continue;
            }
            let Some(frame) = server_frames.get(&window.window_id).copied() else {
                continue;
            };
            if !visible
                .iter()
                .any(|resolved| resolved.output.contains_center(frame))
            {
                continue;
            }
            let Some(owner) = self.workspaces.workspace_of_any(&window) else {
                continue;
            };
            if visible.iter().any(|resolved| resolved.workspace == owner) {
                continue;
            }
            if focused.is_some_and(|focused| focused == &window) {
                return Some(owner);
            }
            if home.is_some_and(|existing| existing != owner) {
                return None;
            }
            home = Some(owner);
        }
        home
    }

    /// Whether a frontmost window that reappeared on screen should join the
    /// workspace it appeared on. A window with no home (a scratchpad summon)
    /// does. A window whose home is already showing does. A window that lives
    /// on a hidden workspace joins the current one only when its app already
    /// has something on screen; otherwise the view switches to its home.
    pub(in crate::app) fn rehome_reappearance(
        &self,
        window: &crate::PhysicalWindowId,
        visible: &[ResolvedWorkspace],
    ) -> bool {
        match self.workspaces.workspace_of_any(window) {
            Some(owner) if visible.iter().all(|resolved| resolved.workspace != owner) => {
                self.app_has_visible_window(&window.app, visible)
            }
            _ => true,
        }
    }

    fn app_has_visible_window(&self, app: &AppInstanceId, visible: &[ResolvedWorkspace]) -> bool {
        visible.iter().any(|resolved| {
            self.workspaces
                .windows(resolved.workspace)
                .iter()
                .any(|window| &window.app == app)
        })
    }

    /// Switch to `workspace` because an activation put focus there. The pointer
    /// stays. A cancelled follow can run again; any other failure stays
    /// suppressed so it cannot loop, unless a macOS Space slide explains it.
    pub(super) fn follow_workspace(
        &mut self,
        workspace: WorkspaceId,
        focused: Option<&crate::PhysicalWindowId>,
        frontmost: Option<&AppInstanceId>,
        outputs: macos::DisplayOutputs,
        cancelled: &AtomicBool,
    ) -> Result<CycleStatus, String> {
        let input = self
            .backend
            .recent_input()
            .map_or_else(|| "input unknown".into(), |input| input.to_string());
        log_event!(
            "focus follow: {:?} (frontmost {:?}) -> workspace {} ({})",
            focused,
            frontmost,
            workspace,
            input
        );
        let follow_started = self.backend.now();
        // A follow answers focus the user already moved: the pointer stays.
        let followed = self.switch_with(
            workspace,
            outputs.clone(),
            cancelled,
            super::super::switching::RepeatSwitch::KeepFocus,
            super::super::commands::CommandOrigin::Passive,
        );
        // An activation (or a finished drag) cancels background cycles, often
        // the very one this follow answers (a notification click). A cancelled
        // follow lets the next cycle follow again; a switch that fails for any
        // other reason keeps focus suppressed so it can't loop, unless a macOS
        // Space slide explains the failure.
        if matches!(&followed, Err(error) if error.ends_with("cancelled")) {
            self.clear_suppressed_focus();
        } else if followed.is_err() {
            self.focus.follow_failed(follow_started);
            if self.retry_follow_after_space_change() {
                let until = self
                    .space_transition_until()
                    .unwrap_or_else(|| self.backend.now() + SPACE_TRANSITION_SETTLE);
                return Ok(self.defer_for_space_transition(until, &outputs));
            }
        }
        followed
    }
}
