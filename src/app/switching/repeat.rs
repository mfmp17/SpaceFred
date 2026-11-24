use super::*;

/// What a request for the workspace that is already up on its display does.
///
/// A repeat press of the active workspace's shortcut (`Control+2` while on
/// Space 2) walks the focus through that workspace's tiles, which is the only
/// way to reach them in Fullscreen Layout. A menu-bar or HUD click on the same
/// workspace is a re-selection, not a repeat: it leaves focus where it is.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::app) enum RepeatSwitch {
    CycleFocus,
    KeepFocus,
}

impl<B: WorkspaceBackend> WorkspaceRuntime<B> {
    /// Step focus to the next tile of an already-focused workspace, wrapping at
    /// the end of the tile order. A workspace with one tile (or none) has
    /// nowhere to go, and a failed focus falls back to the plain re-focus, so
    /// this reports whether it moved focus at all.
    pub(super) fn cycle_focus_on_repeat(
        &mut self,
        workspace: WorkspaceId,
        focused: &crate::PhysicalWindowId,
        outputs: &macos::DisplayOutputs,
    ) -> Result<bool, String> {
        let windows = self.workspaces.engine(workspace).bound_windows();
        let target = match windows.iter().position(|window| window == focused) {
            Some(_) if windows.len() < 2 => return Ok(false),
            Some(index) => windows[(index + 1) % windows.len()].clone(),
            // A float of this workspace has no place in the tile order (#178):
            // the repeat press steps onto the remembered tile, or the first one.
            None => match self
                .workspaces
                .preferred_focus(workspace)
                .filter(|window| windows.contains(window))
                .or_else(|| windows.first().cloned())
            {
                Some(target) if target != *focused => target,
                _ => return Ok(false),
            },
        };
        if let Err(error) = self.focus_tile(Some(focused.clone()), &target, outputs) {
            log_event!("repeat switch focus failed: workspace {workspace}: {error}");
            return Ok(false);
        }
        self.workspaces.remember_focus(workspace, &target);
        self.record_focused_workspace(workspace);
        self.pending_fullscreen_focus_restore.remove(&workspace);
        self.persist_workspace_state_if_changed()?;
        log_event!(
            "repeat switch cycled: workspace {workspace} window {:?}",
            target
        );
        Ok(true)
    }
}
