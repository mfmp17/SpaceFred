use super::*;

enum MoveWindowOutcome {
    Moved(CycleStatus),
    Retired(CycleStatus),
}

impl<B: WorkspaceBackend> WorkspaceRuntime<B> {
    pub(in crate::app) fn move_focused(
        &mut self,
        target: WorkspaceId,
        outputs: macos::DisplayOutputs,
        cancelled: &AtomicBool,
    ) -> Result<CycleStatus, String> {
        let window = self
            .backend
            .focused_window()
            .ok_or("Focused window is unavailable")?;
        self.move_window(window, target, outputs, cancelled)
    }

    pub(in crate::app) fn move_focused_follow(
        &mut self,
        target: WorkspaceId,
        outputs: macos::DisplayOutputs,
        cancelled: &AtomicBool,
    ) -> Result<CycleStatus, String> {
        let window = self
            .backend
            .focused_window()
            .ok_or("Focused window is unavailable")?;
        let was_visible = self.is_visible(target, &outputs);
        let moved = self.move_window(window.clone(), target, outputs.clone(), cancelled)?;
        if was_visible {
            return Ok(moved);
        }
        self.workspaces.remember_focus(target, &window);
        let mut switched = self.switch(target, outputs, cancelled)?;
        switched.failed += moved.failed;
        Ok(switched)
    }

    pub(in crate::app) fn move_app(
        &mut self,
        bundle_id: &str,
        target: WorkspaceId,
        outputs: macos::DisplayOutputs,
        cancelled: &AtomicBool,
    ) -> Result<CycleStatus, String> {
        let mut windows = self.windows_for_bundle(bundle_id);
        if windows.is_empty() {
            self.with_warp_context(WarpContext::Suppressed, |rt| {
                rt.cycle(outputs.clone(), cancelled)
            })?;
            windows = self.windows_for_bundle(bundle_id);
        }
        let mut status: Option<CycleStatus> = None;
        let mut retired_failed = 0;
        for window in windows {
            match self.with_warp_context(WarpContext::Suppressed, |rt| {
                rt.move_window_outcome(window, target, outputs.clone(), cancelled)
            })? {
                MoveWindowOutcome::Moved(mut moved) => {
                    moved.failed += status.map_or(retired_failed, |earlier| earlier.failed);
                    retired_failed = 0;
                    status = Some(moved);
                }
                MoveWindowOutcome::Retired(mut retired) => {
                    if let Some(earlier) = status.take() {
                        retired.failed += earlier.failed;
                        retired.hud = earlier.hud;
                        status = Some(retired);
                    } else {
                        retired_failed += retired.failed;
                    }
                }
            }
        }
        status.ok_or_else(|| format!("No managed window found for {bundle_id}"))
    }

    /// Every window of the bundle Spaceman owns: its tiles, then its floats.
    fn windows_for_bundle(&self, bundle_id: &str) -> Vec<crate::PhysicalWindowId> {
        let mut floating: Vec<_> = self
            .workspaces
            .floating_entries()
            .map(|(window, _)| window.clone())
            .collect();
        floating.sort();
        WorkspaceId::all()
            .flat_map(|workspace| self.workspaces.engine(workspace).bound_windows())
            .chain(floating)
            .filter(|window| self.bundle_id(&window.app) == Some(bundle_id))
            .collect()
    }

    pub(in crate::app) fn move_window(
        &mut self,
        window: crate::PhysicalWindowId,
        target: WorkspaceId,
        outputs: macos::DisplayOutputs,
        cancelled: &AtomicBool,
    ) -> Result<CycleStatus, String> {
        match self.move_window_outcome(window, target, outputs, cancelled)? {
            MoveWindowOutcome::Moved(status) | MoveWindowOutcome::Retired(status) => Ok(status),
        }
    }

    fn move_window_outcome(
        &mut self,
        window: crate::PhysicalWindowId,
        target: WorkspaceId,
        outputs: macos::DisplayOutputs,
        cancelled: &AtomicBool,
    ) -> Result<MoveWindowOutcome, String> {
        self.ensure_managed_workspace(target)?;
        let source = if let Some(source) = self.workspaces.workspace_of_any(&window) {
            source
        } else {
            self.with_warp_context(WarpContext::Suppressed, |rt| {
                rt.cycle(outputs.clone(), cancelled)
            })?;
            self.workspaces
                .workspace_of_any(&window)
                .ok_or("Window is not managed by SpaceFred")?
        };
        if source == target {
            return Ok(MoveWindowOutcome::Moved(self.status(0, &outputs)));
        }
        let is_floating = self.workspaces.floating_workspace_of(&window).is_some();
        if is_floating {
            self.settle_pending_summon_for_move(&window);
        }
        let source_output = output_for(&self.workspaces, source, &outputs);
        let target_output = output_for(&self.workspaces, target, &outputs);
        let window_output = if is_floating {
            self.window_display_output(&window, &outputs)
                .unwrap_or(source_output)
        } else {
            source_output
        };
        let target_is_active = self.is_visible(target, &outputs);
        if !target_is_active {
            let parked = if window_output == target_output {
                self.park_window(&window, target_output, &outputs)
            } else {
                self.park_window_across(&window, window_output, target_output, &outputs)
            };
            if parked == macos::ApplyOutcome::Gone
                && self.retire_confirmed_gone_window(source, &window)
            {
                let mut failed = 0;
                if !is_floating && self.is_visible(source, &outputs) {
                    failed += self
                        .layout_workspace(source, source_output, cancelled)
                        .failed;
                }
                self.persist_workspace_state_if_changed()?;
                return Ok(MoveWindowOutcome::Retired(self.status(failed, &outputs)));
            }
            if !matches!(parked, macos::ApplyOutcome::Applied) {
                return Err("Window could not be parked in the target workspace".into());
            }
        }
        if is_floating {
            self.workspaces.own_floating(window.clone(), target);
        } else {
            self.workspaces
                .move_window(&window, target)
                .ok_or("Window could not be moved")?;
            self.exit_fullscreen_if_split(target);
        }
        self.clear_fullscreen_restore_if_inactive(source);
        self.clear_relocation_evidence(source, target, &window);
        self.note_hand_move(&window, source, target);
        if !target_is_active {
            self.suppressed_app = Some(window.app.clone());
            self.suppressed_focus = Some(window.clone());
        }
        let mut failed = 0;
        if !is_floating && self.is_visible(source, &outputs) {
            let layout = self.layout_workspace(source, source_output, cancelled);
            failed += layout.failed;
        }
        if target_is_active {
            let was_parked = self.parked.contains_key(&window);
            if was_parked {
                if is_floating && let Some(&recovery) = self.parked.get(&window) {
                    let displays: Vec<_> = outputs.frames().collect();
                    let recovery_output =
                        Self::display_for_frame(recovery, &displays).unwrap_or(source_output);
                    if recovery_output != target_output {
                        let relocated = super::super::move_workspace::translate_frame_to_output(
                            recovery,
                            recovery_output,
                            target_output,
                        );
                        let _: bool = self.record_recovery(&window, relocated);
                    }
                }
                if !matches!(
                    self.restore_window(&window),
                    macos::ApplyOutcome::Applied | macos::ApplyOutcome::Gone
                ) {
                    failed += 1;
                }
            }
            if !is_floating {
                let layout = self.layout_workspace(target, target_output, cancelled);
                failed += layout.failed;
            } else if !was_parked
                && window_output != target_output
                && !self.carry_floating_window(&window, window_output, target_output)
            {
                failed += 1;
            }
            if self.backend.focus_window(&window) {
                // Focus followed the window to the workspace on screen
                // elsewhere; Option+Tab goes back to where it came from.
                self.record_focused_workspace(target);
                let frame = self.focus_window_frame(&window, target, target_output);
                let target_display_id = outputs
                    .resolve(self.workspaces.assignment(target))
                    .id
                    .as_str();
                self.follow_pointer_to(target_display_id, target_output, frame, &outputs);
            } else {
                log_event!("move focus follow failed for {:?}", window);
            }
        }
        self.persist_workspace_state_if_changed()?;
        let mut status = self.status(failed, &outputs);
        status.hud = Some(self.hud_for(target, UiHudKind::Moved, None, &outputs));
        Ok(MoveWindowOutcome::Moved(status))
    }

    /// A floating window has no tile to carry it, so moving it to a workspace
    /// on screen on another display moves it there directly, keeping its size
    /// and its place relative to the display. A window already there stays.
    fn carry_floating_window(
        &mut self,
        window: &crate::PhysicalWindowId,
        from: crate::Rect,
        to: crate::Rect,
    ) -> bool {
        let frame = match self.backend.frame(window) {
            Ok(frame) => self.backend.window_server_frame(window).unwrap_or(frame),
            Err(macos::WindowAccessError::Gone) => return true,
            Err(macos::WindowAccessError::Unavailable) => return false,
        };
        if to.contains_center(frame) {
            return true;
        }
        let target = super::super::move_workspace::translate_frame_to_output(frame, from, to);
        let outcome = self.backend.set_position(window, target);
        log_event!(
            "move floating: {:?} to {:?} ({:?})",
            window,
            target,
            outcome
        );
        matches!(
            outcome,
            macos::ApplyOutcome::Applied | macos::ApplyOutcome::Gone
        )
    }
}
