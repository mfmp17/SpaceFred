//! A native-tab switch inside a floated tab group (#747).
//!
//! Selecting another tab of an AppKit tab group swaps which window the group shows: the
//! outgoing tab is ordered out and the incoming one appears at the same frame. A tile keeps
//! its slot through that swap (reconciliation binds the incoming tab to it), but a float is
//! a record keyed by window, so the outgoing tab's float was forgotten once it left the
//! snapshot and the incoming tab was tiled as a new window. The incoming tab takes the float
//! instead: its workspace, why it floats, and its pin. While both tabs are still listed at
//! the same frame, the incoming one is held for a bounded settle rather than adopted.
//! A Scratchpad window that is shown floats too, and its tab switch hands over its
//! Scratchpad entry the same way, so Toggle still hides the group (#754).
//!
//! Right after a macOS Space slide, though, a float the snapshot omits is on the desktop the
//! slide left, still listed by the window server at its frame, and a window of its app at
//! that frame is on the arriving desktop, not a tab that replaced it: neither hands over nor
//! holds (#769).

use super::tab_handoff::TAB_HANDOFF_SETTLE;
use super::*;

impl<B: WorkspaceBackend> WorkspaceRuntime<B> {
    /// Hands `outgoing`, an owned float its app's Complete snapshot omitted, to the one
    /// window of `windows` that nothing owns yet and that sits where the window server
    /// still lists `outgoing`: the tab that replaced it. Returns whether it did.
    pub(super) fn hand_float_to_incoming_tab(
        &mut self,
        bundle_id: &str,
        outgoing: &crate::PhysicalWindowId,
        windows: &[crate::ObservedWindow],
        server_frames: &HashMap<u32, crate::Rect>,
    ) -> bool {
        let Some(&frame) = server_frames.get(&outgoing.window_id) else {
            return false;
        };
        if self.space_slide_just_settled() {
            return false;
        }
        let mut incoming = windows
            .iter()
            .filter(|window| self.is_unowned_tab_at(bundle_id, window, frame));
        let (Some(incoming), None) = (incoming.next(), incoming.next()) else {
            return false;
        };
        let incoming = incoming.id.clone();
        if !self.workspaces.hand_float_to(outgoing, &incoming) {
            return false;
        }
        log_event!("native tab {incoming:?} takes the float of {outgoing:?}");
        self.tab_handoffs.remove(&incoming);
        true
    }

    /// When `window`, owned by nothing yet, opened at the frame of one of its app's floats
    /// that is still listed: the deadline its hold ends, so the outgoing tab can leave the
    /// snapshot and hand over its float before `window` is adopted as a new window.
    pub(super) fn hold_float_tab_handoff(
        &mut self,
        bundle_id: &str,
        window: &crate::ObservedWindow,
        windows: &[crate::ObservedWindow],
    ) -> Option<Instant> {
        if !self.is_unowned_tab_at(bundle_id, window, window.frame)
            || self.space_slide_just_settled()
        {
            return None;
        }
        let tolerance = crate::reconcile::DEFAULT_FRAME_TOLERANCE;
        let float = windows.iter().find(|other| {
            other.id != window.id
                && self.workspaces.floating_workspace_of(&other.id).is_some()
                && !self.parked.contains_key(&other.id)
                && other.frame.approximately_equals(window.frame, tolerance)
        })?;
        let now = self.backend.now();
        let since = *self
            .tab_handoffs
            .entry(window.id.clone())
            .or_insert_with(|| {
                log_event!(
                    "hold {:?}: a native tab mid-handoff at the float {:?}",
                    window.id,
                    float.id
                );
                now
            });
        let deadline = since + TAB_HANDOFF_SETTLE;
        (now < deadline).then_some(deadline)
    }

    /// Whether `window` is owned by nothing, not parked, not ignored, and at `frame`.
    pub(super) fn is_unowned_tab_at(
        &self,
        bundle_id: &str,
        window: &crate::ObservedWindow,
        frame: crate::Rect,
    ) -> bool {
        window
            .frame
            .approximately_equals(frame, crate::reconcile::DEFAULT_FRAME_TOLERANCE)
            && self.workspaces.residency(&window.id).is_none()
            && !self.parked.contains_key(&window.id)
            && !self.config.patch(bundle_id, window).is_ignored()
    }
}
