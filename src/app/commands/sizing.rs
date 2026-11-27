//! Keyboard edits of the focused tile's split: `resize shrink|grow`, `resize width|height`, and
//! `balance`.

use super::*;

const KEYBOARD_RESIZE_INCREMENT: i32 = 50;

impl<B: WorkspaceBackend> WorkspaceRuntime<B> {
    pub(in crate::app) fn resize_focused(
        &mut self,
        direction: ResizeDirection,
        outputs: macos::DisplayOutputs,
        cancelled: &AtomicBool,
    ) -> Result<CycleStatus, String> {
        let mut start = None;
        let mut written = None;
        let mut rounds = super::super::minimums::learning_rounds(0);
        let mut round = 0;
        while round < rounds {
            let known = self.minimum_sizes.clone();
            let Some((workspace, status)) = self.try_edit_focused_split_from(
                "resizing",
                &outputs,
                cancelled,
                Some(&mut start),
                |engine, window, settings, minimums| {
                    engine.resize_focused_tile(
                        window,
                        direction,
                        KEYBOARD_RESIZE_INCREMENT,
                        settings,
                        minimums,
                    )
                },
            )?
            else {
                break;
            };
            let tiles = self.workspaces.engine(workspace).bound_windows().len();
            rounds = rounds.max(super::super::minimums::learning_rounds(tiles));
            written = Some(status);
            // A grow or shrink is relative, so when its write reveals a minimum nobody knew,
            // the refit around it moves seams the edit never touched. Make the same edit again
            // from the same starting layout, now clamped by what was learned (#607).
            if self.minimum_sizes == known || cancelled.load(Ordering::Acquire) {
                break;
            }
            round += 1;
        }
        Ok(written.unwrap_or_else(|| self.status(0, &outputs)))
    }

    pub(in crate::app) fn resize_focused_to(
        &mut self,
        axis: crate::Axis,
        points: i32,
        outputs: macos::DisplayOutputs,
        cancelled: &AtomicBool,
    ) -> Result<CycleStatus, String> {
        let mut written = None;
        let mut rounds = super::super::minimums::learning_rounds(0);
        let mut round = 0;
        while round < rounds {
            let known = self.minimum_sizes.clone();
            let Some((workspace, status)) = self.try_edit_focused_split(
                "resizing",
                &outputs,
                cancelled,
                |engine, window, settings, minimums| {
                    engine.resize_focused_tile_to(window, axis, points, settings, minimums)
                },
            )?
            else {
                break;
            };
            let tiles = self.workspaces.engine(workspace).bound_windows().len();
            rounds = rounds.max(super::super::minimums::learning_rounds(tiles));
            written = Some(status);
            // Writing the new split can reveal a sibling's minimum, and the refit around it
            // takes space from the resized tile too. Ask again with what was learned so the
            // request still lands when it fits (#492).
            if self.minimum_sizes == known || cancelled.load(Ordering::Acquire) {
                break;
            }
            round += 1;
        }
        Ok(written.unwrap_or_else(|| self.status(0, &outputs)))
    }

    pub(in crate::app) fn balance_focused(
        &mut self,
        outputs: macos::DisplayOutputs,
        cancelled: &AtomicBool,
    ) -> Result<CycleStatus, String> {
        self.edit_focused_split(
            "balancing",
            outputs,
            cancelled,
            |engine, window, settings, minimums| {
                engine.balance_focused_parent(window, settings, minimums)
            },
        )
    }

    /// Apply a keyboard edit of the focused tile's split layout, then relayout and persist when
    /// it changed anything. Fullscreen Layout has no split to edit.
    fn edit_focused_split(
        &mut self,
        verb: &str,
        outputs: macos::DisplayOutputs,
        cancelled: &AtomicBool,
        edit: impl FnOnce(
            &mut crate::Engine,
            &crate::PhysicalWindowId,
            &Settings,
            &crate::runtime::WindowMinimums,
        ) -> bool,
    ) -> Result<CycleStatus, String> {
        let status = self
            .try_edit_focused_split(verb, &outputs, cancelled, edit)?
            .map(|(_, status)| status);
        Ok(status.unwrap_or_else(|| self.status(0, &outputs)))
    }

    /// `edit_focused_split`, but `None` when the edit changed nothing and no layout was written.
    fn try_edit_focused_split(
        &mut self,
        verb: &str,
        outputs: &macos::DisplayOutputs,
        cancelled: &AtomicBool,
        edit: impl FnOnce(
            &mut crate::Engine,
            &crate::PhysicalWindowId,
            &Settings,
            &crate::runtime::WindowMinimums,
        ) -> bool,
    ) -> Result<Option<(WorkspaceId, CycleStatus)>, String> {
        self.try_edit_focused_split_from(verb, outputs, cancelled, None, edit)
    }

    /// `try_edit_focused_split`, optionally from a fixed start: the first call records the
    /// workspace's layout in `start`, and later calls put it back before editing, so an edit
    /// made again after learning a minimum starts where the first one did. A later call on
    /// another workspace edits nothing.
    fn try_edit_focused_split_from(
        &mut self,
        verb: &str,
        outputs: &macos::DisplayOutputs,
        cancelled: &AtomicBool,
        start: Option<&mut Option<(WorkspaceId, crate::runtime::LayoutSnapshot)>>,
        edit: impl FnOnce(
            &mut crate::Engine,
            &crate::PhysicalWindowId,
            &Settings,
            &crate::runtime::WindowMinimums,
        ) -> bool,
    ) -> Result<Option<(WorkspaceId, CycleStatus)>, String> {
        let (window, workspace) = self.focused_active_workspace(outputs, cancelled, false)?;
        self.workspaces.deck(workspace).require_split(verb)?;
        let settings = self.layout_settings(output_for(&self.workspaces, workspace, outputs));
        let engine = self.workspaces.engine_mut(workspace);
        let mut restored = false;
        if let Some(start) = start {
            match start {
                Some((first, snapshot)) if *first == workspace => {
                    engine.restore_layout_snapshot(snapshot.clone());
                    restored = true;
                }
                Some(_) => return Ok(None),
                None => *start = Some((workspace, engine.layout_snapshot())),
            }
        }
        // After a restore the windows still show the earlier attempt, so write the layout even
        // when the edit itself now changes nothing.
        if !edit(engine, &window, &settings, &self.minimum_sizes) && !restored {
            return Ok(None);
        }
        // An explicit size is written exactly, even a point or two away (#412).
        self.exact_spacing_workspaces.insert(workspace);
        self.relayout_persist(workspace, outputs, cancelled)
            .map(|status| Some((workspace, status)))
    }
}
