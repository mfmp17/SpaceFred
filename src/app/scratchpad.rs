//! The Scratchpad Lab's native side: hiding windows in the scratchpad, summoning them onto the
//! focused display, and keeping the pure [`crate::scratchpad`] model honest as windows close,
//! workspaces switch, or the Lab is turned off.
//!
//! Hidden scratchpad windows are parked with the same durable recovery frames as inactive
//! workspace windows and owned by no workspace. The cycle leaves them parked (instead of
//! recovering them as stray unowned windows) only while they are in the scratchpad, so pause,
//! quit, restart, a crash or switching the Lab off all bring them back on screen.

use super::parking::RESTORE_CONFIRM_TOLERANCE;
use super::*;
use crate::scratchpad::{ScratchpadCommand, summon_frame};

const LAB_OFF: &str = "Scratchpad is off (set [labs] scratchpad = true in config.toml)";

impl<B: WorkspaceBackend> WorkspaceRuntime<B> {
    pub(super) fn scratchpad_command(
        &mut self,
        command: ScratchpadCommand,
        outputs: macos::DisplayOutputs,
        cancelled: &AtomicBool,
    ) -> Result<CycleStatus, String> {
        if !self.config.labs().scratchpad {
            return Err(LAB_OFF.into());
        }
        match command {
            ScratchpadCommand::Toggle => self.toggle_scratchpad(&outputs, cancelled),
            ScratchpadCommand::Mark => self.mark_scratchpad(outputs, cancelled),
        }
    }

    /// Send the focused window to the scratchpad, or take a shown scratchpad window out of it.
    fn mark_scratchpad(
        &mut self,
        outputs: macos::DisplayOutputs,
        cancelled: &AtomicBool,
    ) -> Result<CycleStatus, String> {
        let window = self
            .current_focus()
            .ok_or("Focused window is unavailable")?;
        if let Some(entry) = self.scratchpad.entry(&window) {
            return match entry.shown_on {
                Some(workspace) => {
                    self.release_from_scratchpad(window, workspace, outputs, cancelled)
                }
                None => Err("Focused window is already hidden in the scratchpad".into()),
            };
        }
        if self.workspaces.workspace_of_any(&window).is_none() {
            // A window observed for the first time may not be bound yet.
            self.cycle(outputs.clone(), cancelled)?;
        }
        let workspace = self
            .workspaces
            .workspace_of_any(&window)
            .ok_or("Focused window is not managed by SpaceFred")?;
        if !self.is_visible(workspace, &outputs) {
            return Err("Focused window is not on an active workspace".into());
        }
        if self.parking_exempt.contains(&window) {
            return Err(
                "Focused window keeps moving itself back on screen, so it cannot be hidden".into(),
            );
        }
        // A floating window keeps its size; a tile gets the default summon size.
        let size = self
            .workspaces
            .floating_workspace_of(&window)
            .and_then(|_| self.backend.frame(&window).ok())
            .map(|frame| (frame.width, frame.height));
        let output = output_for(&self.workspaces, workspace, &outputs);
        // Parked while its workspace still owns it: the recovery frame is durable before the
        // window leaves the screen, and a crash before ownership changes restores it in place.
        match self.park_window(&window, output, &outputs) {
            macos::ApplyOutcome::Applied if self.parked.contains_key(&window) => {}
            macos::ApplyOutcome::Gone => {
                self.forget_window(workspace, &window);
                return Err("Focused window has closed".into());
            }
            _ => return Err("Focused window could not be moved off-screen".into()),
        }
        self.forget_window(workspace, &window);
        // Forgetting drops parking bookkeeping, but the park still has to settle.
        self.recently_parked.insert(window.clone(), Instant::now());
        self.scratchpad.insert(window.clone(), size);
        log_event!("scratchpad: hid {window:?} from workspace {workspace}");
        let layout = self.layout_workspace(workspace, output, cancelled);
        self.persist_workspace_state_if_changed()?;
        self.focus_after_scratchpad(workspace, std::slice::from_ref(&window));
        Ok(self.status(layout.failed, &outputs))
    }

    /// A shown scratchpad window leaves the scratchpad and joins the tiles of its display.
    fn release_from_scratchpad(
        &mut self,
        window: crate::PhysicalWindowId,
        workspace: WorkspaceId,
        outputs: macos::DisplayOutputs,
        cancelled: &AtomicBool,
    ) -> Result<CycleStatus, String> {
        self.scratchpad.remove(&window);
        self.manual_floating.remove(&window);
        self.workspaces.forget_floating(&window);
        log_event!("scratchpad: released {window:?} on workspace {workspace}");
        // Reconciliation adopts an observed window it does not own, exactly as it adopts a new one.
        let result = self.cycle(outputs, cancelled);
        if self.workspaces.workspace_of_any(&window).is_none() {
            self.workspaces.own_floating(window, workspace);
            self.persist_workspace_state_if_changed()?;
        }
        result
    }

    /// Hide the scratchpad window shown on the focused display's workspace, or summon the next one.
    fn toggle_scratchpad(
        &mut self,
        outputs: &macos::DisplayOutputs,
        cancelled: &AtomicBool,
    ) -> Result<CycleStatus, String> {
        if self.scratchpad.is_empty() {
            return Err("Scratchpad is empty".into());
        }
        let target = self.scratchpad_target(outputs)?;
        let shown = self.scratchpad.shown_on(target.workspace);
        if !shown.is_empty() {
            return self.hide_scratchpad(&shown, &target, outputs);
        }
        let window = self
            .scratchpad
            .next_for(target.workspace)
            .ok_or("Scratchpad is empty")?;
        self.summon_scratchpad(window, &target, outputs, cancelled)
    }

    /// The visible workspace a toggle acts on: the focused window's, else the display it is on,
    /// else the last focused workspace, else the main display's.
    fn scratchpad_target(
        &mut self,
        outputs: &macos::DisplayOutputs,
    ) -> Result<ResolvedWorkspace, String> {
        let visible: Vec<_> = self
            .visible_workspaces(outputs)
            .into_iter()
            .filter(|resolved| self.scope.manages_selector(&resolved.selector))
            .collect();
        let find = |workspace: Option<WorkspaceId>| {
            workspace.and_then(|workspace| {
                visible
                    .iter()
                    .find(|resolved| resolved.workspace == workspace)
            })
        };
        let focused = self.current_focus();
        if let Some(resolved) = find(
            focused
                .as_ref()
                .and_then(|window| self.workspaces.workspace_of_any(window)),
        ) {
            return Ok(resolved.clone());
        }
        let frame = focused
            .as_ref()
            .and_then(|window| self.backend.window_server_frame(window));
        frame
            .and_then(|frame| {
                visible
                    .iter()
                    .find(|resolved| resolved.output.contains_center(frame))
            })
            .or_else(|| find(self.last_focused_workspace))
            .or_else(|| {
                visible
                    .iter()
                    .find(|resolved| resolved.display_id == outputs.main().id)
            })
            .or_else(|| visible.first())
            .cloned()
            .ok_or_else(|| "No active workspace is available".into())
    }

    fn summon_scratchpad(
        &mut self,
        window: crate::PhysicalWindowId,
        target: &ResolvedWorkspace,
        outputs: &macos::DisplayOutputs,
        cancelled: &AtomicBool,
    ) -> Result<CycleStatus, String> {
        let size = self.scratchpad.entry(&window).and_then(|entry| entry.size);
        let frame = summon_frame(target.output, size);
        let recovery = self.parked.get(&window).copied();
        match self.write_restore_position(&window, frame) {
            macos::ApplyOutcome::Applied => {}
            macos::ApplyOutcome::Gone => {
                log_event!("scratchpad: {window:?} closed while hidden");
                self.scratchpad.remove(&window);
                if let Some(recovery) = recovery {
                    self.persist_removals_or_rollback(vec![(window, recovery)]);
                }
                return self.toggle_scratchpad(outputs, cancelled);
            }
            _ => return Err("Scratchpad window could not be shown".into()),
        }
        let placement = crate::Placement {
            slot: crate::SlotId(0),
            window: Some(window.clone()),
            frame,
        };
        let settings = self.layout_settings(target.output);
        let failed = failed_results(&self.backend.apply_managed(
            &[placement],
            &settings,
            cancelled,
        ));
        self.workspaces
            .own_floating(window.clone(), target.workspace);
        self.manual_floating.insert(window.clone());
        self.scratchpad.mark_shown(&window, target.workspace);
        // Like any restore, the recovery frame is retired only once the window server
        // confirms the window arrived at its centered summon frame; until then its owner's
        // cycles confirm or retry the centered destination rather than the former tile frame.
        if let Some(recovery) = recovery {
            let expected = self.restore_target(frame);
            if self
                .backend
                .window_server_frame(&window)
                .is_some_and(|actual| {
                    actual.position_approximately_equals(expected, RESTORE_CONFIRM_TOLERANCE)
                })
            {
                self.scratchpad.set_pending_summon(&window, None);
                self.commit_unparked(&window);
                self.persist_removals_or_rollback(vec![(window.clone(), recovery)]);
            } else {
                self.scratchpad.set_pending_summon(&window, Some(frame));
                self.recently_parked.insert(window.clone(), Instant::now());
            }
        }
        if !self.focus_scratchpad_target(&window, &[]) {
            log_event!("scratchpad: could not focus {window:?}");
        }
        self.record_focused_workspace(target.workspace);
        log_event!(
            "scratchpad: showed {window:?} on workspace {}",
            target.workspace
        );
        self.persist_workspace_state_if_changed()?;
        let mut status = self.status(failed, outputs);
        status.hud = Some(self.hud_for(target.workspace, UiHudKind::Workspace, None, outputs));
        Ok(status)
    }

    /// Confirms an arrived scratchpad summon or promotes its pending target over
    /// the stale pre-summon tile recovery frame before moving the float to another
    /// workspace so its floating position is preserved (#304).
    pub(super) fn settle_pending_summon_for_move(&mut self, window: &crate::PhysicalWindowId) {
        let Some(summon) = self.scratchpad.pending_summon(window) else {
            return;
        };
        let expected = self.restore_target(summon);
        if self.parked.contains_key(window) {
            let _: macos::ApplyOutcome = self.finish_scratchpad_summon(window, summon, true);
            if self.parked.contains_key(window) {
                let _: bool = self.record_recovery(window, expected);
            }
        }
    }

    /// Confirms or retries a summoned scratchpad window's centered destination without
    /// replacing it with the old parked recovery frame.
    pub(super) fn finish_scratchpad_summon(
        &mut self,
        window: &crate::PhysicalWindowId,
        target: crate::Rect,
        retry_throttled: bool,
    ) -> macos::ApplyOutcome {
        let Some(&recovery) = self.parked.get(window) else {
            self.scratchpad.set_pending_summon(window, None);
            return macos::ApplyOutcome::Applied;
        };
        let target = self.restore_target(target);
        if self
            .backend
            .window_server_frame(window)
            .is_some_and(|actual| {
                actual.position_approximately_equals(target, RESTORE_CONFIRM_TOLERANCE)
            })
        {
            self.scratchpad.set_pending_summon(window, None);
            self.commit_unparked(window);
            self.persist_removals_or_rollback(vec![(window.clone(), recovery)]);
            return macos::ApplyOutcome::Applied;
        }
        if retry_throttled {
            return macos::ApplyOutcome::Applied;
        }
        let outcome = self.write_restore_position(window, target);
        match outcome {
            macos::ApplyOutcome::Applied => {
                if self
                    .backend
                    .window_server_frame(window)
                    .is_some_and(|actual| {
                        actual.position_approximately_equals(target, RESTORE_CONFIRM_TOLERANCE)
                    })
                {
                    self.scratchpad.set_pending_summon(window, None);
                    self.commit_unparked(window);
                    self.persist_removals_or_rollback(vec![(window.clone(), recovery)]);
                } else {
                    self.recently_parked.insert(window.clone(), Instant::now());
                }
            }
            macos::ApplyOutcome::Gone => {
                self.scratchpad.set_pending_summon(window, None);
                self.commit_unparked(window);
                self.persist_removals_or_rollback(vec![(window.clone(), recovery)]);
            }
            _ => {}
        }
        outcome
    }

    fn hide_scratchpad(
        &mut self,
        windows: &[crate::PhysicalWindowId],
        target: &ResolvedWorkspace,
        outputs: &macos::DisplayOutputs,
    ) -> Result<CycleStatus, String> {
        let mut failed = 0;
        for window in windows {
            let size = self
                .backend
                .frame(window)
                .ok()
                .map(|frame| (frame.width, frame.height));
            match self.park_scratchpad_window(window, target.output, outputs) {
                macos::ApplyOutcome::Applied => self.scratchpad.mark_hidden(window, size),
                macos::ApplyOutcome::Gone => {}
                _ => failed += 1,
            }
        }
        self.persist_workspace_state_if_changed()?;
        self.focus_after_scratchpad(target.workspace, windows);
        Ok(self.status(failed, outputs))
    }

    /// Park a shown scratchpad window and release its floating ownership. A closed window
    /// leaves the scratchpad (`Gone`); any other failure leaves it shown.
    fn park_scratchpad_window(
        &mut self,
        window: &crate::PhysicalWindowId,
        output: crate::Rect,
        outputs: &macos::DisplayOutputs,
    ) -> macos::ApplyOutcome {
        let displays: Vec<_> = outputs.frames().collect();
        let already_hidden = self.parked.contains_key(window)
            && self.scratchpad.pending_summon(window).is_none()
            && self
                .backend
                .window_server_frame(window)
                .is_some_and(|frame| !crate::geometry::shows_on(frame, &displays));
        if !already_hidden {
            match self.park_window(window, output, outputs) {
                macos::ApplyOutcome::Applied if self.parked.contains_key(window) => {}
                macos::ApplyOutcome::Gone => {
                    self.scratchpad.remove(window);
                    self.workspaces.forget_floating(window);
                    self.manual_floating.remove(window);
                    return macos::ApplyOutcome::Gone;
                }
                macos::ApplyOutcome::Applied => return macos::ApplyOutcome::Unavailable,
                outcome => return outcome,
            }
        }
        self.workspaces.forget_floating(window);
        self.manual_floating.remove(window);
        log_event!("scratchpad: hid {window:?}");
        macos::ApplyOutcome::Applied
    }

    /// Hand focus back to the workspace's tiles once a scratchpad window leaves the screen.
    fn focus_after_scratchpad(
        &mut self,
        workspace: WorkspaceId,
        hidden: &[crate::PhysicalWindowId],
    ) {
        let target = self.workspaces.preferred_focus(workspace).or_else(|| {
            self.workspaces
                .engine(workspace)
                .bound_windows()
                .into_iter()
                .next()
        });
        if let Some(target) = target {
            if !self.focus_scratchpad_target(&target, hidden) {
                log_event!("scratchpad: could not focus {target:?} on workspace {workspace}");
            }
        } else {
            self.pending_focus = None;
        }
    }

    /// Requests focus on `target` and records it in `pending_focus` so immediate
    /// follow-up commands address `target` while Accessibility still reports the
    /// previous or just-hidden window.
    fn focus_scratchpad_target(
        &mut self,
        target: &crate::PhysicalWindowId,
        extra_stale: &[crate::PhysicalWindowId],
    ) -> bool {
        let reported = self.backend.focused_window();
        let mut stale = self.take_pending_focus_stale();
        stale.retain(|window| window.as_ref() != Some(target));
        if reported.as_ref() != Some(target) && !stale.contains(&reported) {
            stale.push(reported);
        }
        for hidden in extra_stale {
            let entry = Some(hidden.clone());
            if hidden != target && !stale.contains(&entry) {
                stale.push(entry);
            }
        }
        if !self.backend.focus_window(target) {
            return false;
        }
        self.pending_focus = Some(super::commands::PendingFocus {
            target: target.clone(),
            stale,
            at: Instant::now(),
        });
        true
    }

    /// After every worker command: hide windows whose workspace was switched away (i3 keeps them
    /// hidden until summoned again), prune windows that closed or left by other means, and bring
    /// everything back when the Lab is turned off.
    pub(super) fn sync_scratchpad(&mut self, outputs: &macos::DisplayOutputs) {
        if self.scratchpad.is_empty() {
            return;
        }
        if !self.config.labs().scratchpad {
            let hidden = self.scratchpad.hidden();
            self.scratchpad.clear();
            log_event!("scratchpad: Lab off; restoring {} window(s)", hidden.len());
            self.restore_windows(hidden);
            return;
        }
        let visible: HashSet<_> = self
            .visible_workspaces(outputs)
            .into_iter()
            .map(|resolved| resolved.workspace)
            .collect();
        let entries: Vec<_> = self.scratchpad.entries().cloned().collect();
        let mut closed = Vec::new();
        let mut hid = false;
        for entry in entries {
            let window = entry.window;
            match entry.shown_on {
                // Closed, tiled again, or moved to another workspace: it left the scratchpad.
                Some(workspace)
                    if self.workspaces.floating_workspace_of(&window) != Some(workspace) =>
                {
                    self.scratchpad.remove(&window);
                }
                Some(workspace) if visible.contains(&workspace) => {}
                Some(workspace) => {
                    let output = output_for(&self.workspaces, workspace, outputs);
                    // Prefer the window's live (or pending-summon) size over `self.parked`:
                    // when a workspace switch happens before a delayed summon is confirmed,
                    // `self.parked` still holds the pre-summon tile recovery frame (#258).
                    let size = self
                        .backend
                        .frame(&window)
                        .ok()
                        .or(entry.pending_summon)
                        .or_else(|| self.parked.get(&window).copied())
                        .map(|frame| (frame.width, frame.height));
                    if self.park_scratchpad_window(&window, output, outputs)
                        == macos::ApplyOutcome::Applied
                    {
                        self.scratchpad.mark_hidden(&window, size);
                        hid = true;
                    }
                }
                // Back on screen or owned by other means (healed, recovered): an ordinary window.
                None if !self.parked.contains_key(&window)
                    || self.workspaces.workspace_of_any(&window).is_some() =>
                {
                    self.scratchpad.remove(&window);
                }
                None => {
                    if self.backend.is_process_running(&window.app)
                        && self.backend.window_server_frame(&window).is_some()
                    {
                        continue;
                    }
                    log_event!("scratchpad: {window:?} closed while hidden");
                    if let Some(frame) = self.parked.get(&window).copied() {
                        self.commit_unparked(&window);
                        closed.push((window.clone(), frame));
                    }
                    self.scratchpad.remove(&window);
                }
            }
        }
        self.persist_removals_or_rollback(closed);
        if hid && let Err(error) = self.persist_workspace_state_if_changed() {
            log_event!("scratchpad: could not persist workspace state: {error}");
        }
    }
}
