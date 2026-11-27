//! How a workspace's deck mode reaches its placements, and Fullscreen Layout
//! through a hide or minimize of every tile (#668), or a tile entering macOS
//! native fullscreen, as a web video's fullscreen button does (#740).
//!
//! A tile hidden (⌘H), minimized (⌘M) or in native fullscreen releases its
//! slot so its siblings reflow, and an emptied workspace always falls back to
//! the split layout. While a workspace is in Fullscreen Layout, every tile
//! that leaves that way is remembered, one at a time or together. When the
//! last tile leaves that way, the workspace holds those windows until something next
//! arrives: if one of them comes back, the mode comes back with it. A close
//! or a move away emptying the workspace, a toggle, or another window
//! arriving first ends it. All of this lives in memory only.
use super::*;
use crate::workspace::DeckMode;

/// A Fullscreen Layout workspace's tiles that left by a hide or minimize, or
/// into native fullscreen.
#[derive(Debug, Default)]
pub(in crate::app) struct SetAsideTiles {
    windows: HashSet<crate::PhysicalWindowId>,
    /// The workspace emptied with these set aside and fell back to split.
    held: bool,
}

impl<B: WorkspaceBackend> WorkspaceRuntime<B> {
    pub(in crate::app) fn clear_fullscreen_restore_if_inactive(&mut self, workspace: WorkspaceId) {
        if !self.workspaces.deck(workspace).is_monocle() {
            self.focus.settle_fullscreen(workspace);
        }
    }

    /// Project the split tree's placements through the workspace's deck mode,
    /// recording each tile's projected frame as its target.
    pub(in crate::app) fn project_deck(
        &mut self,
        workspace: WorkspaceId,
        placements: &mut [crate::Placement],
        settings: &Settings,
    ) {
        let deck = self.workspaces.deck(workspace);
        if deck == DeckMode::Split {
            return;
        }
        deck.place(placements, settings.single());
        let engine = self.workspaces.engine_mut(workspace);
        for placement in placements.iter() {
            engine.record_placement_target(placement.slot, placement.frame);
        }
    }

    /// Before a reconciliation of `workspace`: resume Fullscreen Layout when
    /// a held window is already back, bound outside reconciliation (a route
    /// rule, a detached tab or a recall aimed at the workspace while hidden);
    /// drop a record the mode no longer backs (left by a toggle or a move, or
    /// a held workspace filled by other windows); and list the bound tiles
    /// while in Fullscreen Layout.
    pub(super) fn fullscreen_tiles_before(
        &mut self,
        workspace: WorkspaceId,
    ) -> Vec<crate::PhysicalWindowId> {
        let empty = self.slots_empty(workspace);
        let monocle = self.workspaces.deck(workspace).is_monocle();
        let bound = self.workspaces.engine(workspace).bound_windows();
        if let Some(tiles) = self.fullscreen_set_aside.get_mut(&workspace) {
            let keep = if tiles.held && !empty {
                let returned = bound.iter().any(|window| tiles.windows.contains(window));
                if returned {
                    tiles.held = false;
                    for window in &bound {
                        tiles.windows.remove(window);
                    }
                    log_event!(
                        "fullscreen layout: a set-aside window was routed back to workspace {}; resuming",
                        workspace
                    );
                    self.workspaces.set_deck(workspace, DeckMode::Monocle);
                }
                returned && !tiles.windows.is_empty()
            } else {
                tiles.held || monocle
            };
            if !keep {
                self.fullscreen_set_aside.remove(&workspace);
            }
        }
        if self.workspaces.deck(workspace).is_monocle() {
            bound
        } else {
            Vec::new()
        }
    }

    /// After a reconciliation of `workspace`, before its intents are pruned:
    /// resume Fullscreen Layout when a held window returned, and record
    /// Fullscreen Layout tiles that left by a hide or minimize. `before` is
    /// [`Self::fullscreen_tiles_before`].
    pub(super) fn settle_fullscreen_set_aside(
        &mut self,
        workspace: WorkspaceId,
        before: &[crate::PhysicalWindowId],
        inserted: &[&crate::PhysicalWindowId],
    ) {
        if let Some(tiles) = self.fullscreen_set_aside.get_mut(&workspace) {
            if tiles.held && !inserted.is_empty() {
                if !inserted
                    .iter()
                    .any(|window| tiles.windows.contains(*window))
                {
                    self.fullscreen_set_aside.remove(&workspace);
                    return;
                }
                tiles.held = false;
                log_event!(
                    "fullscreen layout: a set-aside window returned to workspace {}; resuming",
                    workspace
                );
                self.workspaces.set_deck(workspace, DeckMode::Monocle);
            }
            for window in inserted {
                tiles.windows.remove(*window);
            }
            if tiles.windows.is_empty() {
                self.fullscreen_set_aside.remove(&workspace);
            }
        }
        if before.is_empty() {
            return;
        }
        let bound: HashSet<_> = self
            .workspaces
            .engine(workspace)
            .bound_windows()
            .into_iter()
            .collect();
        let set_aside: Vec<_> = before
            .iter()
            .filter(|window| !bound.contains(*window))
            .filter(|window| {
                self.backend.confirms_set_aside(window)
                    || self.backend.confirms_native_fullscreen(window)
            })
            .cloned()
            .collect();
        self.note_set_aside_tiles(workspace, set_aside);
    }

    /// Record `set_aside`, Fullscreen Layout tiles macOS confirmed hidden,
    /// minimized or in native fullscreen and that just left `workspace`. When
    /// the workspace is now empty, hold the mode if the last tiles left that
    /// way; otherwise (a close or a move away) forget the record.
    pub(in crate::app) fn note_set_aside_tiles(
        &mut self,
        workspace: WorkspaceId,
        set_aside: Vec<crate::PhysicalWindowId>,
    ) {
        let empty = self.slots_empty(workspace);
        if set_aside.is_empty() {
            if empty {
                self.fullscreen_set_aside.remove(&workspace);
            }
            return;
        }
        let tiles = self.fullscreen_set_aside.entry(workspace).or_default();
        tiles.windows.extend(set_aside);
        if empty {
            tiles.held = true;
            log_event!(
                "fullscreen layout: workspace {} emptied by hide, minimize or native fullscreen; holding the mode for {} window(s)",
                workspace,
                tiles.windows.len()
            );
        }
    }

    fn slots_empty(&self, workspace: WorkspaceId) -> bool {
        self.workspaces
            .engine(workspace)
            .model()
            .workspace
            .slots
            .is_empty()
    }
}
