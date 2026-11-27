//! `[labs] window_overlay`, the worker's half: what the overlay shows over each visible window,
//! and its buttons, which act on that window whether or not it has focus. Title-free: bundle
//! IDs, frames and workspaces only.

use super::{commands::CommandOrigin, *};
use crate::navigation;

/// The most windows one display's overlay carries.
const MAX_WINDOWS: usize = 32;
/// The sides a tile can swap toward, in the order a card shows them: ← ↑ ↓ →.
const SWAP_SIDES: [Direction; 4] = [
    Direction::Left,
    Direction::Up,
    Direction::Down,
    Direction::Right,
];

/// What the window overlay asks the worker for. Every one answers with a fresh
/// [`UiWindowOverlay`].
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum WindowOverlayCommand {
    /// Only read what the overlay shows.
    Snapshot,
    /// Send the window to a workspace, focused or not.
    MoveTo {
        window: crate::PhysicalWindowId,
        workspace: WorkspaceId,
    },
    ToggleFloat(crate::PhysicalWindowId),
    TogglePin(crate::PhysicalWindowId),
    /// Fullscreen Layout for the window's workspace, with the window in front.
    ToggleFullscreen(crate::PhysicalWindowId),
    /// Swap the tile with its neighbour on that side, as `Option+Shift+H/J/K/L` does.
    Swap(crate::PhysicalWindowId, Direction),
}

/// Every visible window, by display, for the overlay's cards.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct UiWindowOverlay {
    pub displays: Vec<UiWindowOverlayDisplay>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UiWindowOverlayDisplay {
    /// The display's ID, as `DisplayOutput::id` gives it.
    pub id: String,
    /// The display's full bounds in global points (top-left origin), menu bar and Dock included:
    /// what a full-display overlay covers.
    pub screen: crate::Rect,
    /// The workspace it shows.
    pub workspace: WorkspaceId,
    /// That workspace is in Fullscreen Layout.
    pub fullscreen: bool,
    /// Every workspace a window can be sent to, in listing order.
    pub workspaces: Vec<UiWindowOverlayWorkspace>,
    /// Its visible windows: tiles in layout order (only the front one in Fullscreen Layout),
    /// then floats.
    pub windows: Vec<UiWindowOverlayWindow>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct UiWindowOverlayWorkspace {
    pub workspace: WorkspaceId,
    /// It holds windows.
    pub occupied: bool,
    /// It is assigned to this display.
    pub on_this_display: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UiWindowOverlayWindow {
    pub window: crate::PhysicalWindowId,
    /// For the app's icon and name; never a title.
    pub bundle_id: Option<String>,
    /// Where it is on screen, in global points.
    pub frame: crate::Rect,
    /// The workspace that owns it (a pin's home).
    pub workspace: WorkspaceId,
    pub floating: bool,
    pub pinned: bool,
    /// The sides with a neighbouring tile to swap with, in the order a card shows them:
    /// left, up, down, right.
    pub swaps: Vec<Direction>,
}

impl<B: WorkspaceBackend> WorkspaceRuntime<B> {
    pub(super) fn window_overlay_command(
        &mut self,
        command: WindowOverlayCommand,
        outputs: macos::DisplayOutputs,
        cancelled: &AtomicBool,
    ) -> Result<CycleStatus, String> {
        let mut status = match command {
            WindowOverlayCommand::Snapshot => Ok(self.status(0, &outputs)),
            WindowOverlayCommand::MoveTo { window, workspace } => {
                self.move_window(window, workspace, outputs.clone(), cancelled)
            }
            WindowOverlayCommand::ToggleFloat(window) => {
                self.toggle_float_window(window, outputs.clone(), cancelled)
            }
            WindowOverlayCommand::TogglePin(window) => {
                self.toggle_pin_window(window, outputs.clone(), cancelled)
            }
            WindowOverlayCommand::ToggleFullscreen(window) => {
                let workspace = self.visible_tile_workspace(&window, &outputs)?;
                self.toggle_fullscreen_with(window, workspace, outputs.clone(), cancelled)
            }
            WindowOverlayCommand::Swap(window, side) => {
                let workspace = self.visible_tile_workspace(&window, &outputs)?;
                let origin = CommandOrigin::Passive;
                self.reorder_window(window, workspace, side, outputs.clone(), cancelled, origin)
            }
        }?;
        status.window_overlay = Some(self.window_overlay(&outputs));
        Ok(status)
    }

    fn visible_tile_workspace(
        &self,
        window: &crate::PhysicalWindowId,
        outputs: &macos::DisplayOutputs,
    ) -> Result<WorkspaceId, String> {
        self.workspaces
            .workspace_of(window)
            .filter(|workspace| self.is_visible(*workspace, outputs))
            .ok_or_else(|| "Window is not a tile on an active workspace".into())
    }

    /// What the overlay shows: each managed display's visible windows and the workspaces they
    /// can go to.
    pub(super) fn window_overlay(&mut self, outputs: &macos::DisplayOutputs) -> UiWindowOverlay {
        let visible = self.visible_workspaces(outputs);
        let tiles = self.navigation_tiles_for(&visible);
        let frames = self.backend.window_server_frames().unwrap_or_default();
        let displays = visible
            .iter()
            .filter(|resolved| self.scope.manages_selector(&resolved.selector))
            .take(HUD_MAX_DISPLAYS)
            .map(|resolved| self.window_overlay_display(resolved, &tiles, &frames, outputs))
            .collect();
        UiWindowOverlay { displays }
    }

    fn window_overlay_display(
        &self,
        resolved: &ResolvedWorkspace,
        tiles: &[NavigationTile],
        frames: &HashMap<u32, crate::Rect>,
        outputs: &macos::DisplayOutputs,
    ) -> UiWindowOverlayDisplay {
        let workspace = resolved.workspace;
        let fullscreen = self.workspaces.deck(workspace).is_monocle();
        let local: Vec<NavigationTile> = tiles
            .iter()
            .filter(|tile| tile.workspace == workspace)
            .cloned()
            .collect();
        // Fullscreen Layout stacks every tile in one place: only the one in front shows.
        let front = self
            .workspaces
            .preferred_focus(workspace)
            .filter(|front| local.iter().any(|tile| &tile.window == front))
            .or_else(|| local.first().map(|tile| tile.window.clone()));
        let entry =
            |window: &crate::PhysicalWindowId, frame, owner, floating| UiWindowOverlayWindow {
                window: window.clone(),
                bundle_id: self.bundle_id(&window.app).map(str::to_owned),
                frame,
                workspace: owner,
                floating,
                pinned: self.workspaces.is_pinned(window),
                swaps: Vec::new(),
            };
        let mut windows: Vec<UiWindowOverlayWindow> = local
            .iter()
            .filter(|tile| !fullscreen || front.as_ref() == Some(&tile.window))
            .map(|tile| {
                let frame = frames
                    .get(&tile.window.window_id)
                    .copied()
                    .unwrap_or(tile.frame);
                let mut card = entry(&tile.window, frame, workspace, false);
                if !fullscreen {
                    card.swaps = SWAP_SIDES
                        .into_iter()
                        .filter(|side| navigation::target(&local, &tile.window, *side).is_some())
                        .collect();
                }
                card
            })
            .collect();
        // Floats it owns and pins from anywhere, where they are on this display now: a parked
        // or hidden one is off screen.
        let mut floats: Vec<_> = self
            .workspaces
            .floating_entries()
            .filter(|(window, owner)| *owner == workspace || self.workspaces.is_pinned(window))
            .filter_map(|(window, owner)| {
                let frame = *frames.get(&window.window_id)?;
                resolved
                    .screen
                    .contains_center(frame)
                    .then(|| entry(window, frame, owner, true))
            })
            .collect();
        floats.sort_by(|a, b| a.window.cmp(&b.window));
        windows.extend(floats);
        windows.truncate(MAX_WINDOWS);
        let workspaces = self
            .config
            .ordered_workspaces()
            .filter(|listed| {
                self.workspaces.is_enabled(*listed)
                    && self
                        .scope
                        .manages_selector(self.workspaces.assignment(*listed))
            })
            .map(|listed| UiWindowOverlayWorkspace {
                workspace: listed,
                occupied: !self.workspaces.windows(listed).is_empty(),
                on_this_display: outputs.resolve(self.workspaces.assignment(listed)).id
                    == resolved.display_id,
            })
            .collect();
        UiWindowOverlayDisplay {
            id: resolved.display_id.clone(),
            screen: resolved.screen,
            workspace,
            fullscreen,
            workspaces,
            windows,
        }
    }
}
