//! A native tab selected while Pause showed its tile's hidden workspace (#753).
//!
//! Pause restores every window. Selecting another tab of a restored AppKit tab group orders
//! the tile's tab out and shows the incoming one at the group's origin. Hidden workspaces
//! are never reconciled, so on Resume the incoming tab was adopted on the visible workspace
//! as a new window while the hidden workspace kept the ordered-out tab, which could not be
//! restored and rolled back every switch to that workspace. The incoming tab takes the tile
//! instead, keeping its slot and workspace as a tab switch on a visible workspace does.
//!
//! Only a tile that was on screen going into this cycle can hand its tile over. A window
//! parked before this cycle that the window server lists but does not show may be on another
//! macOS Space, or closed while its app keeps it alive; its tile and its recovery record stay.

use super::*;

/// What this cycle knows going into affinity: the visible workspaces, its window-server
/// snapshot, and the recovery records it did not create.
pub(super) struct HiddenTabEvidence<'a> {
    pub(super) visible: &'a [ResolvedWorkspace],
    pub(super) server_frames: &'a HashMap<u32, crate::Rect>,
    pub(super) parked_before: &'a HashSet<crate::PhysicalWindowId>,
}

impl<B: WorkspaceBackend> WorkspaceRuntime<B> {
    /// When `incoming`, a window of its app's Complete snapshot `windows` that nothing owns,
    /// is the selected tab of a group whose ordered-out tab holds a tile on a hidden
    /// workspace: hands it that tile and returns the workspace. A tile whose window was
    /// in `evidence.parked_before` (parked going into this cycle) is never claimed.
    /// `onscreen` caches the window-server read for the rest of the affinity pass.
    pub(super) fn claim_hidden_tab_tile(
        &mut self,
        bundle_id: &str,
        incoming: &crate::ObservedWindow,
        windows: &[crate::ObservedWindow],
        evidence: &HiddenTabEvidence<'_>,
        onscreen: &mut Option<Option<HashMap<u32, crate::Rect>>>,
    ) -> Option<WorkspaceId> {
        let HiddenTabEvidence {
            visible,
            server_frames,
            parked_before,
        } = *evidence;
        if !self.is_unowned_tab_at(bundle_id, incoming, incoming.frame) {
            return None;
        }
        let tolerance = crate::reconcile::DEFAULT_FRAME_TOLERANCE;
        let at_origin =
            |frame: crate::Rect| frame.position_approximately_equals(incoming.frame, tolerance);
        // Tiles of this app on hidden workspaces that were on screen going into this cycle
        // (Pause restored them) and that the snapshot does not list. Their last on-screen
        // frame, which this cycle's parking recorded if it parked them, starts at the
        // incoming tab's origin.
        let candidates: Vec<_> = WorkspaceId::all()
            .filter(|workspace| {
                !visible
                    .iter()
                    .any(|resolved| resolved.workspace == *workspace)
            })
            .flat_map(|workspace| {
                self.workspaces
                    .windows(workspace)
                    .into_iter()
                    .map(move |window| (workspace, window))
            })
            .filter(|(workspace, window)| {
                window.app == incoming.id.app
                    && !parked_before.contains(window)
                    && self.workspaces.workspace_of(window) == Some(*workspace)
                    && !windows.iter().any(|observed| observed.id == *window)
                    && server_frames.contains_key(&window.window_id)
                    && self
                        .parked
                        .get(window)
                        .copied()
                        .or_else(|| server_frames.get(&window.window_id).copied())
                        .is_some_and(at_origin)
            })
            .collect();
        let [(workspace, outgoing)] = candidates.as_slice() else {
            return None;
        };
        let (workspace, outgoing) = (*workspace, outgoing.clone());
        // Only one unowned window of the app may sit at that origin: the incoming tab.
        let rivals = windows
            .iter()
            .filter(|other| {
                other.id != incoming.id
                    && at_origin(other.frame)
                    && self.workspaces.residency(&other.id).is_none()
                    && !self.parked.contains_key(&other.id)
            })
            .count();
        if rivals > 0 {
            return None;
        }
        // A parked window is still on screen to the window server, just off every display;
        // an inactive tab is ordered out. Minimized or hidden windows are set aside instead.
        let frames = onscreen.get_or_insert_with(|| self.backend.onscreen_window_frames());
        if frames
            .as_ref()
            .is_none_or(|frames| frames.contains_key(&outgoing.window_id))
            || self.backend.confirms_set_aside(&outgoing)
        {
            return None;
        }
        if !self.hand_slot_to(&outgoing, &incoming.id) {
            return None;
        }
        log_event!(
            "native tab {:?} takes the tile of {:?} on hidden workspace {}",
            incoming.id,
            outgoing,
            workspace
        );
        // Any recovery record the outgoing tab has was written by this cycle's parking from
        // the frame Pause had just restored, which its group now shows through the incoming
        // tab; selecting the outgoing tab again shows it at the group's frame. Left in place,
        // it would be an unowned recovery entry Accessibility cannot restore. The incoming
        // tab parks with its own frame like any hidden tile.
        let record = self.commit_unparked(&outgoing);
        self.persist_removals_or_rollback(record.map(|record| (outgoing, record)));
        self.tab_handoffs.remove(&incoming.id);
        Some(workspace)
    }
}
