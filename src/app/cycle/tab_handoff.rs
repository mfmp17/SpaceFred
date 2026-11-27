//! A native-tab switch caught mid-handoff under a route-every-window rule (#706).
//!
//! Routing runs before any display is reconciled, so a rule that routes every new window
//! decides on one snapshot. An incoming native tab can show up at its tile's frame while the
//! outgoing tab is still listed, or before focus reaches it. That snapshot can't prove a
//! replacement yet, and routing it would send the tab to the rule's workspace and retire
//! its tile. Such a window is held instead, its app left unreconciled, for a bounded settle;
//! a window still unexplained after that is routed as new.

use super::*;

/// How long an incoming tab at its tile's frame may wait for the evidence of a replacement.
pub(in crate::app) const TAB_HANDOFF_SETTLE: Duration = Duration::from_secs(1);

impl<B: WorkspaceBackend> WorkspaceRuntime<B> {
    /// When `window`, unbound and at the frame of one of its app's tiles on the visible
    /// workspace it opened over, is held mid-handoff: the deadline its hold ends.
    pub(super) fn hold_tab_handoff(
        &mut self,
        window: &crate::ObservedWindow,
        windows: &[crate::ObservedWindow],
        visible: &[ResolvedWorkspace],
    ) -> Option<Instant> {
        let resolved = visible
            .iter()
            .find(|resolved| resolved.screen.contains_center(window.frame))?;
        let pending = self
            .workspaces
            .engine(resolved.workspace)
            .native_tab_handoff_pending(&self.layout_settings(resolved.output), window, windows);
        if !pending {
            return None;
        }
        let now = self.backend.now();
        let since = *self
            .tab_handoffs
            .entry(window.id.clone())
            .or_insert_with(|| {
                log_event!(
                    "hold {:?}: a native tab mid-handoff at workspace {}'s tile",
                    window.id,
                    resolved.workspace
                );
                now
            });
        let deadline = since + TAB_HANDOFF_SETTLE;
        (now < deadline).then_some(deadline)
    }
}

impl<B: WorkspaceBackend> WorkspaceRuntime<B> {
    /// Before `app`'s window affinities freeze: the app's own "Move Tab to New Window" (#756).
    ///
    /// An AppKit tab group moves the detached tab's own window out, still keyed, and shows its
    /// next tab's window in place. That window is held as a handoff at the tile's frame, but the
    /// outgoing window never leaves the list. Once the settle runs out with the tile's window
    /// still listed there and focused, the held window is the group: it takes over the tile, and
    /// the window that left it is the new one, returned with its route's workspace. A genuinely
    /// new window there takes focus, so it still routes as new.
    pub(super) fn claim_app_detached_tab(
        &mut self,
        app: &crate::ObservedApplication,
        focused: Option<&crate::PhysicalWindowId>,
        windows: &[crate::ObservedWindow],
        visible: &[ResolvedWorkspace],
    ) -> Option<(crate::PhysicalWindowId, WorkspaceId)> {
        let focused = focused?;
        let source = windows.iter().find(|window| &window.id == focused)?;
        let owner = self.workspaces.workspace_of(&source.id)?;
        let resolved = visible.iter().find(|resolved| {
            resolved.workspace == owner && resolved.screen.contains_center(source.frame)
        })?;
        if !self.config.routes_every_window(&app.bundle_id, source) {
            return None;
        }
        let target = self
            .config
            .patch(&app.bundle_id, source)
            .home
            .filter(|target| *target != owner)?;
        let settings = self.layout_settings(resolved.output);
        let now = self.backend.now();
        let engine = self.workspaces.engine(owner);
        let remaining = windows.iter().find(|window| {
            window.id != source.id
                && self
                    .tab_handoffs
                    .get(&window.id)
                    .is_some_and(|since| now >= *since + TAB_HANDOFF_SETTLE)
                && self.workspaces.workspace_of_any(&window.id).is_none()
                && window
                    .frame
                    .approximately_equals(source.frame, settings.frame_tolerance)
                && engine.native_tab_handoff_pending(&settings, window, windows)
        })?;
        let remaining = remaining.id.clone();
        if !self.hand_slot_to(&source.id, &remaining) {
            return None;
        }
        self.tab_handoffs.remove(&remaining);
        log_event!(
            "app detached {:?} from its tab group: {remaining:?} keeps workspace {owner}'s tile; \
             routing it to workspace {target}",
            source.id
        );
        Some((source.id.clone(), target))
    }
}
