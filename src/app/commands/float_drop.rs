//! Labs `float_follows_display`: a floating window dropped on another display joins the
//! workspace showing there, as a tile does. It keeps its frame and why it floats.

use super::*;
use crate::workspace::Residency;

impl<B: WorkspaceBackend> WorkspaceRuntime<B> {
    /// After a pointer drag of a float ends: when the Lab is on and the window's center
    /// landed on a display other than its workspace's, move it to the workspace showing
    /// there. Returns `None` (nothing changes) for a drop on the same display, a display
    /// showing no workspace, a pinned or summoned scratchpad window, or a parked one.
    pub(in crate::app) fn drop_floating_window(
        &mut self,
        window: crate::PhysicalWindowId,
        outputs: macos::DisplayOutputs,
        cancelled: &AtomicBool,
    ) -> Option<Result<CycleStatus, String>> {
        if !self.config.labs().float_follows_display {
            return None;
        }
        let Some(Residency::Floating {
            workspace: source,
            pin,
            ..
        }) = self.workspaces.residency(&window)
        else {
            return None;
        };
        if pin.by_hand || pin.by_rule || self.parked.contains_key(&window) {
            return None;
        }
        let visible = self.visible_workspaces(&outputs);
        let source_display = &visible
            .iter()
            .find(|shown| shown.workspace == source)?
            .display_id;
        let frame = self
            .backend
            .window_server_frame(&window)
            .or_else(|| self.backend.frame(&window).ok())?;
        // The full display bounds, so a center over the Dock or menu bar still counts (#393).
        let landed = visible
            .iter()
            .find(|shown| shown.screen.contains_center(frame))?;
        if &landed.display_id == source_display {
            return None;
        }
        let target = landed.workspace;
        log_event!(
            "drop: float {:?} joins workspace {} on its new display (was {})",
            window,
            target,
            source
        );
        Some(self.move_window(window, target, outputs, cancelled))
    }
}
