//! A native tab selected while Move App (or any move) takes its tile to another workspace (#759).
//!
//! A move rebinds nothing: the tile goes to its new workspace still bound to the tab the
//! group showed when the move was asked for. When another tab of that AppKit group is
//! selected just as the move runs, the moved tab is ordered out before it reaches its new
//! display, and the group shows the incoming tab where the tile was on the source workspace.
//! That workspace's reconciliation adopted it as a new window, and the destination dropped
//! the ordered-out tab's tile, so the group stayed behind. The incoming tab takes the moved
//! tile instead, so the group follows it.
//!
//! Only a tile moved within the last [`MOVED_TAB_SETTLE`] qualifies, only while its tab is
//! listed by the window server but not on screen (ordered out, not minimized or hidden), and
//! only when the incoming tab is the one window of the app nothing owns at the tile's old
//! frame.

use super::*;

/// How long after a move the group a moved tile came from may hand it its incoming tab.
pub(in crate::app) const MOVED_TAB_SETTLE: Duration = Duration::from_secs(2);

/// Where a tile moved to another workspace was on the workspace it left, and when it left.
#[derive(Clone, Debug)]
pub(in crate::app) struct DepartedTile {
    frames: Vec<crate::Rect>,
    at: Instant,
}

impl<B: WorkspaceBackend> WorkspaceRuntime<B> {
    /// Before `window`'s tile leaves `source`: remember where it was there.
    pub(in crate::app) fn note_departed_tile(
        &mut self,
        window: &crate::PhysicalWindowId,
        source: WorkspaceId,
    ) {
        let frames = self.workspaces.engine(source).known_frames(window);
        if frames.is_empty() {
            return;
        }
        let at = self.backend.now();
        self.departed_tiles
            .retain(|_, departed| at.saturating_duration_since(departed.at) < MOVED_TAB_SETTLE);
        self.departed_tiles
            .insert(window.clone(), DepartedTile { frames, at });
    }

    /// When `incoming`, a window of its app's Complete snapshot `windows` that nothing owns,
    /// is the tab its group showed in place of a tile just moved away: hands it that tile and
    /// returns the tile's workspace. `onscreen` caches the window-server read for the rest
    /// of the affinity pass.
    pub(super) fn claim_moved_tab_tile(
        &mut self,
        bundle_id: &str,
        incoming: &crate::ObservedWindow,
        windows: &[crate::ObservedWindow],
        server_frames: &HashMap<u32, crate::Rect>,
        onscreen: &mut Option<Option<HashMap<u32, crate::Rect>>>,
    ) -> Option<WorkspaceId> {
        if self.departed_tiles.is_empty() {
            return None;
        }
        let now = self.backend.now();
        self.departed_tiles
            .retain(|_, departed| now.saturating_duration_since(departed.at) < MOVED_TAB_SETTLE);
        let candidates: Vec<_> = self
            .departed_tiles
            .iter()
            .filter(|(outgoing, departed)| {
                outgoing.app == incoming.id.app
                    && !windows.iter().any(|observed| observed.id == **outgoing)
                    && server_frames.contains_key(&outgoing.window_id)
                    && !self.parked.contains_key(outgoing)
                    && departed
                        .frames
                        .iter()
                        .any(|frame| self.is_unowned_tab_at(bundle_id, incoming, *frame))
            })
            .filter_map(|(outgoing, departed)| {
                let workspace = self.workspaces.workspace_of(outgoing)?;
                Some((workspace, outgoing.clone(), departed.frames.clone()))
            })
            .collect();
        let [(workspace, outgoing, frames)] = candidates.as_slice() else {
            return None;
        };
        let (workspace, outgoing) = (*workspace, outgoing.clone());
        // Only one window of the app nothing owns may sit where the tile was: the incoming tab.
        let rivals = windows
            .iter()
            .filter(|other| {
                other.id != incoming.id
                    && frames
                        .iter()
                        .any(|frame| self.is_unowned_tab_at(bundle_id, other, *frame))
            })
            .count();
        if rivals > 0 {
            return None;
        }
        // An inactive tab is ordered out; a minimized window or one of a hidden app is set
        // aside, and keeps its tile.
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
        self.departed_tiles.remove(&outgoing);
        self.tab_handoffs.remove(&incoming.id);
        log_event!(
            "native tab {:?} takes the tile of {:?}, just moved to workspace {}",
            incoming.id,
            outgoing,
            workspace
        );
        Some(workspace)
    }
}
