//! `[labs] empty_workspace_focus`: an empty workspace chosen on another display
//! is the focused workspace for commands. Showing it focuses nothing, so the
//! window on the display the user came from stays reported as focused; with
//! the Lab on, commands treat that report as no focus until a different window
//! takes focus, and fall back to the workspace chosen last.

use super::super::*;
use super::FocusTracker;

/// The empty workspace chosen last and the window Accessibility kept reporting
/// as focused when it was chosen.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::app) struct EmptyChoice {
    pub(in crate::app) workspace: WorkspaceId,
    pub(in crate::app) left: Option<crate::PhysicalWindowId>,
}

impl FocusTracker {
    pub(in crate::app) const fn empty_choice(&self) -> Option<&EmptyChoice> {
        self.empty_choice.as_ref()
    }

    pub(in crate::app) fn set_empty_choice(&mut self, choice: Option<EmptyChoice>) {
        self.empty_choice = choice;
    }
}

impl<B: WorkspaceBackend> WorkspaceRuntime<B> {
    /// After an explicit switch to workspace (a key, `ctl switch`, the menu or
    /// the HUD), remembers it as chosen when it shows empty while a window of
    /// another visible workspace kept focus.
    pub(in crate::app) fn note_workspace_choice(
        &mut self,
        workspace: WorkspaceId,
        outputs: &macos::DisplayOutputs,
    ) {
        if !self.config.labs().empty_workspace_focus {
            self.focus.set_empty_choice(None);
            return;
        }
        let left = self.backend.focused_window();
        let kept_elsewhere = left
            .as_ref()
            .is_some_and(|window| self.focus_kept_elsewhere(window, workspace, outputs));
        let chosen = self.focus.history().current == Some(workspace)
            && self.is_visible(workspace, outputs)
            && self.workspaces.windows(workspace).is_empty()
            && kept_elsewhere;
        self.focus
            .set_empty_choice(chosen.then_some(EmptyChoice { workspace, left }));
    }

    /// Whether window, still focused, is seen on another visible workspace than
    /// workspace (whether managed, pinned, or unmanaged, #532).
    fn focus_kept_elsewhere(
        &mut self,
        window: &crate::PhysicalWindowId,
        workspace: WorkspaceId,
        outputs: &macos::DisplayOutputs,
    ) -> bool {
        let visible = self.visible_workspaces(outputs);
        self.shown_workspace_for(window, &visible, outputs)
            .is_some_and(|entry| entry.workspace != workspace)
    }

    /// Ends the choice once its workspace is no longer on a connected display.
    pub(in crate::app) fn settle_empty_choice(&mut self, outputs: &macos::DisplayOutputs) {
        if let Some(choice) = self.focus.empty_choice()
            && !self.is_visible(choice.workspace, outputs)
        {
            self.focus.set_empty_choice(None);
        }
    }

    /// Whether the empty workspace chosen last still stands for focus given
    /// what Accessibility reports now, without settling anything.
    pub(in crate::app) fn empty_choice_stands(
        &self,
        reported: Option<&crate::PhysicalWindowId>,
    ) -> bool {
        let Some(choice) = self.focus.empty_choice() else {
            return false;
        };
        let workspace = choice.workspace;
        self.config.labs().empty_workspace_focus
            && choice.left.as_ref() == reported
            && self.focus.history().current == Some(workspace)
            && self.workspaces.windows(workspace).is_empty()
            && self
                .workspaces
                .active_lanes()
                .any(|(_, active)| active == workspace)
    }

    /// [Self::empty_choice_stands], ending the choice when it no longer does:
    /// a different report, the Lab turned off, or the workspace gaining a
    /// window or leaving the screen.
    pub(in crate::app) fn holds_empty_choice(
        &mut self,
        reported: Option<&crate::PhysicalWindowId>,
    ) -> bool {
        let holds = self.empty_choice_stands(reported);
        if !holds {
            self.focus.set_empty_choice(None);
        }
        holds
    }
}
