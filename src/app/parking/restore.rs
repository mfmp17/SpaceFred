use super::super::*;
use super::{INACCESSIBLE_GRACE, ParkRecord, RESTORE_CONFIRM_TOLERANCE};

impl<B: WorkspaceBackend> WorkspaceRuntime<B> {
    /// Confirms that an Accessibility `Gone` result means `window` actually
    /// closed, and if so retires its parked recovery entry and workspace binding.
    pub(in crate::app) fn retire_confirmed_gone_window(
        &mut self,
        workspace: WorkspaceId,
        window: &crate::PhysicalWindowId,
    ) -> bool {
        if !self.confirm_gone(window) {
            return false;
        }
        log_event!(
            "forget {:?} (workspace {}): window closed",
            window,
            workspace
        );
        let record = self.commit_unparked(window);
        self.persist_removals_or_rollback(record.map(|record| (window.clone(), record)));
        self.forget_window(workspace, window);
        true
    }

    /// Whether an Accessibility "gone" for `window` is real: the window
    /// server no longer lists it, its process exited, or Accessibility has
    /// failed to find it for [`INACCESSIBLE_GRACE`] while a display is awake
    /// (time with every display asleep never counts, #739). A parked window never
    /// times out: Accessibility cannot reach a window on another macOS
    /// desktop, and dropping its recovery frame would strand it off screen
    /// (#328), so only the window server or its process may retire it.
    pub(super) fn confirm_gone(&mut self, window: &crate::PhysicalWindowId) -> bool {
        if self.backend.window_server_frame(window).is_none()
            || !self.backend.is_process_running(&window.app)
        {
            self.inaccessible_since.remove(window);
            return true;
        }
        if self.parked.contains_key(window) {
            if !self.inaccessible_since.contains_key(window) {
                log_event!(
                    "keeping recovery for {:?}: Accessibility cannot reach it but the window server lists it (another macOS desktop?); it restores once that desktop is shown",
                    window
                );
                let now = self.backend.now();
                self.inaccessible_since.insert(window.clone(), now);
            }
            return false;
        }
        let now = self.backend.now();
        let since = *self
            .inaccessible_since
            .entry(window.clone())
            .or_insert_with(|| {
                log_event!(
                    "keeping {:?}: Accessibility cannot find it but the window server still lists it",
                    window
                );
                now
            });
        if self.backend.displays_asleep() {
            // Asleep, Accessibility misses windows that are still there; a
            // background wake must not run out the grace (#739). It restarts
            // once a display is awake to recheck.
            self.inaccessible_since.insert(window.clone(), now);
            return false;
        }
        if now.saturating_duration_since(since) < INACCESSIBLE_GRACE {
            return false;
        }
        log_event!(
            "forgetting {:?}: inaccessible for {}s although the window server lists it",
            window,
            INACCESSIBLE_GRACE.as_secs()
        );
        self.inaccessible_since.remove(window);
        true
    }

    pub(in crate::app) fn restore_all(&mut self) -> usize {
        let parked: Vec<_> = self.parked.keys().cloned().collect();
        self.restore_windows(parked)
    }

    pub(in crate::app) fn restore_windows(
        &mut self,
        windows: Vec<crate::PhysicalWindowId>,
    ) -> usize {
        let mut restored = Vec::new();
        for window in &windows {
            let Some(record) = self.parked.record(window).cloned() else {
                continue;
            };
            self.restore_window_uncommitted(window);
            if !self.parked.contains_key(window) {
                restored.push((window.clone(), record));
            }
        }
        self.persist_removals_or_rollback(restored);
        // Persistence failure rolls recovery entries back into this map. The
        // caller may only exit when both native restoration and durable state
        // removal have completed.
        windows
            .iter()
            .filter(|window| self.parked.contains_key(window))
            .count()
    }

    pub(in crate::app) fn mark_parked(
        &mut self,
        window: crate::PhysicalWindowId,
        frame: crate::Rect,
    ) -> bool {
        if self.parked.contains_key(&window) {
            return true;
        }
        self.record_recovery(&window, frame)
    }

    /// Durably record `frame` as the window's recovery frame, replacing any
    /// earlier one; memory rolls back when persistence fails.
    pub(in crate::app) fn record_recovery(
        &mut self,
        window: &crate::PhysicalWindowId,
        frame: crate::Rect,
    ) -> bool {
        if self.parked.get(window) == Some(&frame) {
            return true;
        }
        let display = self.recovery_display(frame);
        let previous = self.parked.insert(window.clone(), frame, display);
        if let Err(error) = self.persist_parked() {
            match previous {
                Some(previous) => self.parked.put(window.clone(), previous),
                None => drop(self.parked.remove(window)),
            }
            log_event!("failed to persist parked window: {error}");
            return false;
        }
        true
    }

    pub(in crate::app) fn restore_window(
        &mut self,
        window: &crate::PhysicalWindowId,
    ) -> macos::ApplyOutcome {
        let record = self.parked.record(window).cloned();
        let outcome = self.restore_window_uncommitted(window);
        if !self.parked.contains_key(window) {
            self.persist_removals_or_rollback(record.map(|record| (window.clone(), record)));
        }
        outcome
    }

    pub(super) fn restore_window_uncommitted(
        &mut self,
        window: &crate::PhysicalWindowId,
    ) -> macos::ApplyOutcome {
        let Some(target) = self.recovery_target(window) else {
            return macos::ApplyOutcome::Applied;
        };
        let outcome = self.write_recovery_frame(window, target);
        if let macos::ApplyOutcome::Applied = outcome {
            // Accessibility read-back is optimistic for apps that apply
            // moves asynchronously. Keep the recovery frame until the
            // window server confirms arrival; cycles retry until it does.
            // A window the window server cannot see is not confirmed.
            let confirmed = self
                .backend
                .window_server_frame(window)
                .is_some_and(|actual| {
                    actual.position_approximately_equals(target, RESTORE_CONFIRM_TOLERANCE)
                });
            if confirmed {
                self.commit_unparked(window);
            } else {
                log_event!(
                    "restore pending: {:?} not yet at {:?} per the window server",
                    window,
                    target
                );
            }
        }
        outcome
    }

    /// Apply a restore write to `target` (already resolved onto the connected
    /// displays) and update transient parking state. Confirmation and durable
    /// removal remain the caller's responsibility so bulk paths can share one
    /// window-server snapshot and one persistence transaction.
    pub(in crate::app) fn write_restore_position(
        &mut self,
        window: &crate::PhysicalWindowId,
        target: crate::Rect,
    ) -> macos::ApplyOutcome {
        let outcome = self.backend.set_position(window, target);
        self.settle_restore_write(window, target, outcome)
    }

    /// [`Self::write_restore_position`] for a recovery frame: the window also
    /// gets back the size it had when parked, should its app have changed it
    /// meanwhile (#554).
    pub(in crate::app) fn write_recovery_frame(
        &mut self,
        window: &crate::PhysicalWindowId,
        target: crate::Rect,
    ) -> macos::ApplyOutcome {
        let outcome = self.backend.set_restore_frame(window, target);
        self.settle_restore_write(window, target, outcome)
    }

    fn settle_restore_write(
        &mut self,
        window: &crate::PhysicalWindowId,
        target: crate::Rect,
        mut outcome: macos::ApplyOutcome,
    ) -> macos::ApplyOutcome {
        log_event!("restore: {:?} to {:?} ({:?})", window, target, outcome);
        // Accessibility reports a window it cannot resolve as gone; only the
        // window server or the process exiting may retire its recovery frame.
        if outcome == macos::ApplyOutcome::Gone && !self.confirm_gone(window) {
            outcome = macos::ApplyOutcome::Unavailable;
        }
        match outcome {
            macos::ApplyOutcome::Applied => {
                // Also throttles the next retry and heal consideration.
                self.parked.restore_written(window, self.backend.now());
            }
            macos::ApplyOutcome::Gone => drop(self.commit_unparked(window)),
            _ => {}
        }
        outcome
    }

    /// A layout placed these windows on screen. Any still recorded as parked (a
    /// config reload made their workspace visible) are no longer hidden; retire
    /// their recovery frames, or a later park or restore would reuse them.
    pub(in crate::app) fn retire_laid_out(&mut self, results: &[macos::ApplyResult]) {
        let mut retired = Vec::new();
        for result in results {
            if matches!(
                result.outcome,
                macos::ApplyOutcome::Applied | macos::ApplyOutcome::Constrained
            ) && let Some(record) = self.commit_unparked(&result.window)
            {
                retired.push((result.window.clone(), record));
            }
        }
        self.persist_removals_or_rollback(retired);
    }

    /// The window is confirmed no longer parked; retire all parking state and
    /// return its record, which stays in memory until its removal persists.
    pub(in crate::app) fn commit_unparked(
        &mut self,
        window: &crate::PhysicalWindowId,
    ) -> Option<ParkRecord> {
        self.inaccessible_since.remove(window);
        self.parked.remove(window)
    }

    /// Durably write the recovery frames, in the `parked-windows.json` format.
    pub(in crate::app) fn persist_parked(&mut self) -> Result<(), String> {
        self.backend.persist_parked(&self.parked.recovery_frames())
    }

    /// Persist the removal of `restored`, or put their records back as plain
    /// recovery data when that fails.
    pub(in crate::app) fn persist_removals_or_rollback(
        &mut self,
        restored: impl IntoIterator<Item = (crate::PhysicalWindowId, ParkRecord)>,
    ) {
        let restored: Vec<_> = restored.into_iter().collect();
        if restored.is_empty() {
            return;
        }
        if let Err(error) = self.persist_parked() {
            for (window, record) in restored {
                self.parked.put(window, record.resting());
            }
            log_event!("failed to persist restored windows: {error}");
        }
    }
}
