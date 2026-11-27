//! The Workspace Steps Lab: `switch next|prev` shows the next or previous workspace of the
//! focused display, taking the same stops as a swipe; `move next|prev [--follow]` sends the
//! focused window one step along every enabled workspace of its display. Both stop at the ends.

use super::*;
use crate::WorkspaceStep;

const LAB_OFF: &str = "Workspace steps are off (set [labs] workspace_steps = true in config.toml)";

impl<B: WorkspaceBackend> WorkspaceRuntime<B> {
    /// Shows the visible workspace's neighbor among the focused display's visible and occupied
    /// workspaces, in `settings.workspace_order`. Nothing happens past either end.
    pub(in crate::app) fn step_workspace(
        &mut self,
        step: WorkspaceStep,
        outputs: macos::DisplayOutputs,
        cancelled: &AtomicBool,
    ) -> Result<CycleStatus, String> {
        if !self.config.labs().workspace_steps {
            return Err(LAB_OFF.into());
        }
        let visible = self.visible_workspaces(&outputs);
        // The display the focused window (including a dragged pin) is seen on, or with no
        // visible window focused, the workspace switched to last.
        let shown = self.focused_visible_workspace(&visible, &outputs)?.0;
        let listed = self.listed_workspaces(&shown.display_id, Some(shown.workspace), &outputs);
        match step.target(&listed, shown.workspace) {
            Some(target) => self.switch_with(
                target,
                outputs,
                cancelled,
                RepeatSwitch::KeepFocus,
                CommandOrigin::Explicit,
            ),
            None => Ok(self.status(0, &outputs)),
        }
    }

    /// Moves the focused window to the next or previous enabled workspace of the display its
    /// workspace is on, empty ones included, as `move WORKSPACE [--follow]` would. Nothing
    /// moves past either end.
    pub(in crate::app) fn move_focused_step(
        &mut self,
        step: WorkspaceStep,
        follow: bool,
        outputs: macos::DisplayOutputs,
        cancelled: &AtomicBool,
    ) -> Result<CycleStatus, String> {
        if !self.config.labs().workspace_steps {
            return Err(LAB_OFF.into());
        }
        let visible = self.visible_workspaces(&outputs);
        let (focused, window) = self.focused_visible_workspace(&visible, &outputs)?;
        let window = window.ok_or("Focused window is unavailable")?;
        let current = self
            .workspaces
            .workspace_of_any(&window)
            .unwrap_or(focused.workspace);
        let display_id = outputs
            .resolve(self.workspaces.assignment(current))
            .id
            .clone();
        let ordered = self.display_workspaces(&display_id, &outputs);
        let Some(target) = step.target(&ordered, current) else {
            return Ok(self.status(0, &outputs));
        };
        if follow {
            self.move_focused_follow(target, outputs, cancelled)
        } else {
            self.move_focused(target, outputs, cancelled)
        }
    }

    /// Every enabled, managed workspace shown on `display_id`, in `settings.workspace_order`.
    fn display_workspaces(
        &self,
        display_id: &str,
        outputs: &macos::DisplayOutputs,
    ) -> Vec<WorkspaceId> {
        self.config
            .ordered_workspaces()
            .filter(|workspace| {
                let selector = self.workspaces.assignment(*workspace);
                self.workspaces.is_enabled(*workspace)
                    && self.scope.manages_selector(selector)
                    && outputs.resolve(selector).id == display_id
            })
            .collect()
    }
}
