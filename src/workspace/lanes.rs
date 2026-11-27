//! Which workspace each display lane and each physical display shows, and
//! how assignments, switches and moves change that.

use super::{DisplaySelector, WorkspaceId, Workspaces};

impl Workspaces {
    #[must_use]
    pub fn active_for(&self, display: &DisplaySelector) -> Option<WorkspaceId> {
        self.displays.lane(display)
    }

    pub fn active_lanes(&self) -> impl Iterator<Item = (&DisplaySelector, WorkspaceId)> {
        self.displays.lanes()
    }

    /// Every physical display's record of the lane it brought forward last.
    pub(crate) fn display_fronts(&self) -> impl Iterator<Item = (&str, &DisplaySelector, u64)> {
        self.displays.fronts()
    }

    /// Records the lane a display shows when it has none yet.
    pub(crate) fn seed_display_front(&mut self, display: &str, lane: &DisplaySelector) {
        self.displays.seed_front(display, lane);
    }

    /// Shows workspace on the display: it becomes its lane's active workspace
    /// and that lane the display's front, together.
    pub(crate) fn show_on(&mut self, display: &str, workspace: WorkspaceId) {
        let lane = self.assignment(workspace).clone();
        self.displays.set_lane(lane.clone(), workspace);
        self.displays.bring_forward(display, lane);
    }

    /// The display no longer shows a workspace of its own.
    pub(crate) fn forget_display_front(&mut self, display: &str) {
        self.displays.forget_front(display);
    }

    pub fn set_assignment(&mut self, workspace: WorkspaceId, display: DisplaySelector) {
        self.set_assignment_with(workspace, display, |_, _, _| false);
    }

    /// Assigns workspace to display. When keeps_front says the workspace is
    /// shown and the old and new selectors name one physical display, it keeps
    /// showing there: its lane moves to the new selector (#423). A hidden
    /// workspace only changes its assignment (#542).
    pub(crate) fn set_assignment_with(
        &mut self,
        workspace: WorkspaceId,
        display: DisplaySelector,
        keeps_front: impl Fn(WorkspaceId, &DisplaySelector, &DisplaySelector) -> bool,
    ) {
        self.enable(workspace);
        let previous = self.assignments[workspace.index()].clone();
        if previous == display {
            return;
        }
        self.assignments[workspace.index()] = display.clone();
        let was_active = self.displays.lane(&previous) == Some(workspace);
        if was_active && keeps_front(workspace, &previous, &display) {
            // The display's history moves with the lane: what it showed before
            // workspace, not the hidden workspace the new selector covered (#544).
            let shown_before = self
                .displays
                .previous(&previous)
                .filter(|shown| *shown != workspace);
            let covered = self
                .displays
                .set_lane(display.clone(), workspace)
                .filter(|covered| *covered != workspace);
            if let Some(prior) = shown_before.or(covered) {
                self.displays.set_previous(display.clone(), prior);
            }
            self.displays.rename_front_lanes(&previous, &display);
        } else if self.displays.lane(&display).is_none() {
            self.displays.set_lane(display, workspace);
        }
        if was_active {
            if let Some(replacement) = WorkspaceId::all().find(|candidate| {
                self.is_enabled(*candidate) && self.assignment(*candidate) == &previous
            }) {
                self.displays.set_lane(previous.clone(), replacement);
            } else {
                self.displays.remove_lane(&previous);
            }
        }
        if self.displays.previous(&previous).is_some_and(|candidate| {
            candidate == workspace || self.displays.lane(&previous) == Some(candidate)
        }) {
            self.displays.remove_previous(&previous);
        }
    }

    /// Moves workspace to target, making it active there (recording any
    /// displaced active workspace on target as that display's previous
    /// workspace) and promoting source_replacement on the lane workspace left.
    pub fn move_to_display(
        &mut self,
        workspace: WorkspaceId,
        target: DisplaySelector,
        source_replacement: Option<WorkspaceId>,
    ) {
        self.enable(workspace);
        let source = self.assignments[workspace.index()].clone();
        if source == target {
            return;
        }
        self.assignments[workspace.index()] = target.clone();

        let displaced = self.displays.set_lane(target.clone(), workspace);
        match displaced {
            Some(previous) if previous != workspace => {
                self.displays.set_previous(target, previous);
            }
            _ => {
                if self.displays.previous(&target) == Some(workspace) {
                    self.displays.remove_previous(&target);
                }
            }
        }

        if self.displays.lane(&source) == Some(workspace) {
            let on_source = |candidate: WorkspaceId| {
                candidate != workspace
                    && self.is_enabled(candidate)
                    && self.assignment(candidate) == &source
            };
            let replacement = source_replacement
                .filter(|candidate| on_source(*candidate))
                .or_else(|| self.displays.previous(&source).filter(|c| on_source(*c)))
                .or_else(|| WorkspaceId::all().find(|candidate| on_source(*candidate)));
            match replacement {
                Some(replacement) => {
                    self.displays.set_lane(source.clone(), replacement);
                }
                None => self.displays.remove_lane(&source),
            }
        }
        if self.displays.previous(&source).is_some_and(|candidate| {
            candidate == workspace
                || Some(candidate) == source_replacement
                || self.displays.lane(&source) == Some(candidate)
        }) {
            self.displays.remove_previous(&source);
        }
    }

    pub fn switch(&mut self, workspace: WorkspaceId) -> WorkspaceId {
        let display = self.assignment(workspace).clone();
        let active = self.active_for(&display).unwrap_or(workspace);
        if active != workspace {
            self.displays.set_previous(display.clone(), active);
            self.displays.set_lane(display, workspace);
        }
        active
    }

    /// Switches the physical display to workspace. Its previous workspace is
    /// what the display showed (shown), even when another selector naming the
    /// same display assigns that one (#400).
    pub(crate) fn switch_on(
        &mut self,
        display: &str,
        workspace: WorkspaceId,
        shown: Option<WorkspaceId>,
    ) {
        let lane = self.assignment(workspace).clone();
        self.switch(workspace);
        if let Some(shown) = shown.filter(|shown| *shown != workspace) {
            self.displays.set_previous(lane.clone(), shown);
        }
        self.displays.bring_forward(display, lane);
    }

    /// Records shown as display's previous workspace: a move covered it
    /// there though another selector naming the same display assigns it.
    pub fn set_previous(&mut self, display: DisplaySelector, shown: WorkspaceId) {
        self.displays.set_previous(display, shown);
    }

    #[must_use]
    pub fn previous_for(&self, display: &DisplaySelector) -> Option<WorkspaceId> {
        self.displays.previous(display)
    }

    /// Brings the lane forward on the physical display.
    pub(crate) fn bring_forward(&mut self, display: &str, lane: DisplaySelector) {
        self.displays.bring_forward(display, lane);
    }
}
