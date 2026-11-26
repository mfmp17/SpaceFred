use super::*;

/// How long a fresh park must settle before an on-screen observation of that
/// window is trusted as a genuine reappearance rather than in-flight movement.
pub(super) const PARK_SETTLE_GRACE: Duration = Duration::from_secs(2);
/// Accessibility lookups fail transiently, notably right after Spaceman or an
/// application launches. A window the window server still lists keeps its
/// slot for this long before an Accessibility "gone" is believed.
pub(super) const INACCESSIBLE_GRACE: Duration = Duration::from_secs(30);
/// Two confirmed self-un-parks within this window exempt the window from parking:
/// its application insists on managing it, and fighting back loses.
const PARKING_EXEMPT_WINDOW: Duration = Duration::from_secs(300);
/// A restore is confirmed when the window server reports the window near its
/// target. Loose on purpose: apps may clamp the exact position, while a
/// still-parked window sits a full display dimension away.
pub(super) const RESTORE_CONFIRM_TOLERANCE: i32 = 64;
/// After this many undone re-parks in a row the window's reappearance is
/// honored (re-homed) instead of fought; repeated honors exempt it from parking.
pub(super) const REPARK_ATTEMPT_LIMIT: u8 = 3;

/// Shared per-cycle context for parking decisions.
struct ParkContext<'a> {
    displays: &'a [crate::Rect],
    screens: &'a [crate::Rect],
    frontmost: &'a Option<AppInstanceId>,
    server_frames: &'a HashMap<u32, crate::Rect>,
    visible: &'a [ResolvedWorkspace],
}

impl<B: WorkspaceBackend> WorkspaceRuntime<B> {
    /// A window observed on-screen while still recorded as parked was brought back by
    /// its app or the user. Accessibility frames can be stale right after a park, so
    /// the reappearance must be confirmed by the window server before trusting it.
    /// Re-home it when its app already has a window on screen; otherwise it keeps its
    /// hidden workspace and focus follow switches there.
    pub(super) fn heal_reappeared_window(
        &mut self,
        window: &crate::ObservedWindow,
        visible: &[ResolvedWorkspace],
        server_frames: &HashMap<u32, crate::Rect>,
    ) {
        let Some(original) = self.parked.get(&window.id).copied() else {
            return;
        };
        if self.awaiting_display_repark(&window.id)
            || self
                .recently_parked
                .get(&window.id)
                .is_some_and(|parked_at| parked_at.elapsed() < PARK_SETTLE_GRACE)
        {
            // The park write may still be applying inside the app; wait it out.
            return;
        }
        // The observation may be stale; only the window server is trusted.
        let Some(frame) = server_frames.get(&window.id.window_id).copied() else {
            return;
        };
        let Some(target) = visible
            .iter()
            .find(|resolved| resolved.output.contains_center(frame))
            .map(|resolved| resolved.workspace)
        else {
            return;
        };
        // Background bounds re-assertions stay parked. So does a frontmost window
        // whose home is a hidden workspace and whose app has nothing on screen.
        if self.backend.frontmost_application().as_ref() != Some(&window.id.app)
            || !self.rehome_reappearance(&window.id, visible)
        {
            return;
        }
        self.honor_reappearance(&window.id, frame, original, target);
    }

    /// Honor a window that will not stay parked: drop the recovery frame and
    /// re-home it to the workspace it insists on appearing on. A second honor
    /// within [`PARKING_EXEMPT_WINDOW`] exempts it from further parking.
    fn honor_reappearance(
        &mut self,
        window: &crate::PhysicalWindowId,
        frame: crate::Rect,
        original: crate::Rect,
        target: WorkspaceId,
    ) {
        self.commit_unparked(window);
        log_event!(
            "heal: {:?} reappeared at {:?}; dropping recovery frame {:?}",
            window,
            frame,
            original
        );
        self.persist_removals_or_rollback(vec![(window.clone(), original)]);
        if self.parked.contains_key(window) {
            return;
        }
        self.recent_heals
            .retain(|_, healed_at| healed_at.elapsed() < PARKING_EXEMPT_WINDOW);
        let repeatedly_unparked = self
            .recent_heals
            .insert(window.clone(), Instant::now())
            .is_some();
        if self.workspaces.floating_workspace_of(window).is_some() {
            log_event!(
                "heal: re-home floating {:?} -> workspace {}",
                window,
                target
            );
            self.workspaces.own_floating(window.clone(), target);
        } else if let Some(source) = self.workspaces.move_window(window, target) {
            self.exit_fullscreen_if_split(target);
            self.clear_fullscreen_restore_if_inactive(source);
            self.clear_relocation_evidence(source, target, window);
            log_event!(
                "heal: re-home tiled {:?} workspace {} -> {}",
                window,
                source,
                target
            );
        }
        if repeatedly_unparked {
            // The app insists on managing its saved position. Keep workspace
            // ownership and tiling, but stop trying to hide this physical
            // window across workspace switches for the rest of the session.
            log_event!(
                "parking exemption: {:?} keeps un-parking itself; leaving it visible",
                window
            );
            self.parking_exempt.insert(window.clone());
        }
    }

    /// Recover a newly identified frontmost window that was relaunched at the
    /// exact one-pixel edge position produced by Spaceman parking. Its former
    /// physical identity (and recovery entry) no longer exists, so revealing it
    /// lets normal reconciliation adopt and tile it again.
    pub(super) fn recover_frontmost_stale_parked_window(
        &mut self,
        outputs: &macos::DisplayOutputs,
        frontmost: Option<&AppInstanceId>,
        focused: Option<&crate::PhysicalWindowId>,
        server_frames: &HashMap<u32, crate::Rect>,
    ) {
        let Some(window) = focused.filter(|window| frontmost == Some(&window.app)) else {
            return;
        };
        if self.parked.contains_key(window) || self.workspaces.workspace_of_any(window).is_some() {
            return;
        }
        let Some(frame) = server_frames.get(&window.window_id).copied() else {
            return;
        };
        if outputs
            .frames()
            .any(|display| display.contains_center(frame))
        {
            return;
        }
        let displays = outputs.frames().collect::<Vec<_>>();
        let screens = outputs.screens();
        let Some(output) = outputs.displays().iter().find_map(|display| {
            let parked =
                crate::geometry::parked_frame_clear_of(display.frame, &displays, &screens, frame);
            crate::geometry::parked_at(frame, parked, 1).then_some(display.frame)
        }) else {
            return;
        };
        let target = crate::Rect {
            x: output.x,
            y: output.y,
            ..frame
        };
        log_event!(
            "recover stale parked focus: {:?} from {:?} to {:?}",
            window,
            frame,
            target
        );
        let outcome = self.backend.set_position(window, target);
        if !matches!(outcome, macos::ApplyOutcome::Applied) {
            log_event!(
                "stale parked focus recovery incomplete for {:?}: {:?}",
                window,
                outcome
            );
        }
    }

    pub(super) fn park_inactive(
        &mut self,
        outputs: &macos::DisplayOutputs,
        visible: &[ResolvedWorkspace],
        server_frames: &HashMap<u32, crate::Rect>,
        cancelled: &AtomicBool,
        failed: &mut usize,
    ) {
        let visible_workspaces: HashSet<_> =
            visible.iter().map(|resolved| resolved.workspace).collect();
        let inactive = self.workspaces.inactive_windows_except(&visible_workspaces);
        let displays = outputs.frames().collect::<Vec<_>>();
        let screens = outputs.screens();
        let frontmost = self.backend.frontmost_application();
        let context = ParkContext {
            displays: &displays,
            screens: &screens,
            frontmost: &frontmost,
            server_frames,
            visible,
        };
        // Hidden scratchpad windows intentionally have no workspace owner, so
        // re-park any background bounds reassertions here before affinity sees them.
        for window in self.scratchpad.hidden() {
            if cancelled.load(Ordering::Acquire) {
                return;
            }
            if self.parking_exempt.contains(&window) {
                continue;
            }
            if let Some(original) = self.parked.get(&window).copied() {
                self.repark_drifted_window(&window, original, outputs.main().frame, &context);
            }
        }
        let mut candidates = Vec::new();
        for (workspace, window) in inactive {
            if cancelled.load(Ordering::Acquire) {
                return;
            }
            if self.parking_exempt.contains(&window) {
                continue;
            }
            let output = output_for(&self.workspaces, workspace, outputs);
            if let Some(original) = self.parked.get(&window).copied() {
                self.repark_drifted_window(&window, original, output, &context);
                continue;
            }
            let frame = match self.backend.frame(&window) {
                Ok(frame) => {
                    self.inaccessible_since.remove(&window);
                    frame
                }
                Err(macos::WindowAccessError::Gone) => {
                    if self.confirm_gone(&window) {
                        log_event!(
                            "forget {:?} (workspace {}): window closed",
                            window,
                            workspace
                        );
                        self.forget_window(workspace, &window);
                    } else {
                        *failed += 1;
                    }
                    continue;
                }
                Err(macos::WindowAccessError::Unavailable) => {
                    *failed += 1;
                    continue;
                }
            };
            let frame = server_frames
                .get(&window.window_id)
                .copied()
                .unwrap_or(frame);
            if !crate::geometry::shows_on(frame, &displays) {
                // Already off-screen by the app's own doing; leave it alone rather
                // than recording an off-screen frame as recovery data.
                continue;
            }
            let park_output = Self::display_for_frame(frame, &displays).unwrap_or(output);
            let target =
                crate::geometry::parked_frame_clear_of(park_output, &displays, &screens, frame);
            candidates.push((workspace, window, frame, target));
        }
        if candidates.is_empty() {
            return;
        }
        self.order_back_to_front(&mut candidates, |(_, window, ..)| window.window_id);
        for (_, window, frame, _) in &candidates {
            self.parked.insert(window.clone(), *frame);
        }
        if let Err(error) = self.backend.persist_parked(&self.parked) {
            for (_, window, _, _) in &candidates {
                self.parked.remove(window);
            }
            *failed += candidates.len();
            log_event!("failed to persist parked windows: {error}");
            return;
        }

        let mut restored = Vec::new();
        for (workspace, window, frame, target) in candidates {
            log_event!(
                "park inactive: {:?} (workspace {}) from {:?}",
                window,
                workspace,
                frame
            );
            let mut outcome = self.backend.set_parked_position(&window, target);
            if outcome == macos::ApplyOutcome::Gone && !self.confirm_gone(&window) {
                outcome = macos::ApplyOutcome::Unavailable;
            }
            match outcome {
                macos::ApplyOutcome::Applied => {
                    self.recently_parked.insert(window.clone(), Instant::now());
                }
                macos::ApplyOutcome::Gone => {
                    self.parked.remove(&window);
                    restored.push((window.clone(), frame));
                    self.forget_window(workspace, &window);
                }
                _ => {
                    *failed += 1;
                    // Only an applied or gone restore retires the entry.
                    self.restore_window_uncommitted(&window);
                    if !self.parked.contains_key(&window) {
                        restored.push((window, frame));
                    }
                }
            }
        }
        self.persist_removals_or_rollback(restored);
    }

    /// A parked window observed on-screen in the background re-asserted its
    /// saved bounds (Electron does this on screen-parameter changes). Push it
    /// back to its parked spot; the recovery frame is already recorded. A
    /// frontmost window is left for heal, unless its home is a hidden workspace.
    fn repark_drifted_window(
        &mut self,
        window: &crate::PhysicalWindowId,
        original: crate::Rect,
        output: crate::Rect,
        context: &ParkContext<'_>,
    ) {
        if self.awaiting_display_repark(window) {
            return;
        }
        let Some(server) = context.server_frames.get(&window.window_id).copied() else {
            return;
        };
        if !crate::geometry::shows_on(server, context.displays) {
            // Only a window that STAYED parked past the settle grace ends a
            // fight; right after a re-park write the app may still undo it.
            if self
                .recently_parked
                .get(window)
                .is_none_or(|parked_at| parked_at.elapsed() >= PARK_SETTLE_GRACE)
            {
                self.repark_attempts.remove(window);
            }
            return;
        }
        if context.frontmost.as_ref() == Some(&window.app)
            && self.rehome_reappearance(window, context.visible)
        {
            return;
        }
        if self
            .recently_parked
            .get(window)
            .is_some_and(|parked_at| parked_at.elapsed() < PARK_SETTLE_GRACE)
        {
            return;
        }
        if server.width < original.width.saturating_sub(RESTORE_CONFIRM_TOLERANCE)
            && server.height < original.height.saturating_sub(RESTORE_CONFIRM_TOLERANCE)
            && self.backend.frame(window).is_ok_and(|ax| {
                !context
                    .displays
                    .iter()
                    .any(|display| display.contains_center(ax))
            })
        {
            // Mission Control / Exposé scales parked windows into on-screen
            // thumbnails in the window server while their Accessibility
            // position stays parked off-screen.
            return;
        }
        if self.repark_attempts.get(window).copied().unwrap_or(0) >= REPARK_ATTEMPT_LIMIT {
            // The app undoes every park. Fighting on would flicker forever;
            // honor the reappearance instead (re-home, then exempt it from
            // parking if it keeps happening).
            log_event!(
                "re-park escalation: {:?} will not stay parked after {} attempts",
                window,
                REPARK_ATTEMPT_LIMIT
            );
            if let Some(target) = context
                .visible
                .iter()
                .find(|resolved| resolved.output.contains_center(server))
                .map(|resolved| resolved.workspace)
            {
                self.honor_reappearance(window, server, original, target);
            }
            return;
        }
        // Park by the size the window has now; the recovery frame's may be stale.
        let park_output = Self::display_for_frame(server, context.displays).unwrap_or(output);
        let target = crate::geometry::parked_frame_clear_of(
            park_output,
            context.displays,
            context.screens,
            server,
        );
        log_event!(
            "re-park: {:?} drifted back on-screen to {:?}",
            window,
            server
        );
        if matches!(
            self.backend.set_parked_position(window, target),
            macos::ApplyOutcome::Applied
        ) {
            // Only a confirmed park write that the app later undoes proves the
            // app is fighting parking; transient Gone/Unavailable errors during
            // display reconfiguration must not escalate and adopt the window.
            *self.repark_attempts.entry(window.clone()).or_insert(0) += 1;
            self.recently_parked.insert(window.clone(), Instant::now());
        }
    }

    /// Confirms that an Accessibility `Gone` result means `window` actually
    /// closed, and if so retires its parked recovery entry and workspace binding.
    pub(super) fn retire_confirmed_gone_window(
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
        let frame = self.parked.get(window).copied();
        self.commit_unparked(window);
        if let Some(frame) = frame {
            self.persist_removals_or_rollback(vec![(window.clone(), frame)]);
        }
        self.forget_window(workspace, window);
        true
    }

    /// Whether an Accessibility "gone" for `window` is real: the window
    /// server no longer lists it, its process exited, or Accessibility has
    /// failed to find it for [`INACCESSIBLE_GRACE`].
    fn confirm_gone(&mut self, window: &crate::PhysicalWindowId) -> bool {
        if self.backend.window_server_frame(window).is_none()
            || !self.backend.is_process_running(&window.app)
        {
            self.inaccessible_since.remove(window);
            return true;
        }
        let since = *self
            .inaccessible_since
            .entry(window.clone())
            .or_insert_with(|| {
                log_event!(
                    "keeping {:?}: Accessibility cannot find it but the window server still lists it",
                    window
                );
                Instant::now()
            });
        if since.elapsed() < INACCESSIBLE_GRACE {
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

    pub(super) fn restore_all(&mut self) -> usize {
        let parked: Vec<_> = self.parked.keys().cloned().collect();
        self.restore_windows(parked)
    }

    pub(super) fn restore_windows(&mut self, windows: Vec<crate::PhysicalWindowId>) -> usize {
        let mut restored = Vec::new();
        for window in &windows {
            let Some(frame) = self.parked.get(window).copied() else {
                continue;
            };
            self.restore_window_uncommitted(window);
            if !self.parked.contains_key(window) {
                restored.push((window.clone(), frame));
            }
        }
        self.persist_removals_or_rollback(restored);
        // Persistence failure rolls recovery entries back into this map. The
        // caller may only exit when both native restoration and durable state
        // removal have completed.
        windows
            .iter()
            .filter(|window| self.parked.contains_key(*window))
            .count()
    }

    pub(super) fn mark_parked(
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
    pub(super) fn record_recovery(
        &mut self,
        window: &crate::PhysicalWindowId,
        frame: crate::Rect,
    ) -> bool {
        let previous = self.parked.insert(window.clone(), frame);
        if previous == Some(frame) {
            return true;
        }
        if let Err(error) = self.backend.persist_parked(&self.parked) {
            match previous {
                Some(previous) => self.parked.insert(window.clone(), previous),
                None => self.parked.remove(window),
            };
            log_event!("failed to persist parked window: {error}");
            return false;
        }
        true
    }

    /// The connected display that currently owns `frame`: its center first,
    /// then the display with the largest on-screen overlap (ignoring the
    /// one-pixel parking strip).
    pub(super) fn display_for_frame(
        frame: crate::Rect,
        displays: &[crate::Rect],
    ) -> Option<crate::Rect> {
        displays
            .iter()
            .copied()
            .find(|display| display.contains_center(frame))
            .or_else(|| {
                displays
                    .iter()
                    .copied()
                    .filter_map(|display| {
                        let width = (i64::from(frame.x) + i64::from(frame.width))
                            .min(i64::from(display.x) + i64::from(display.width))
                            - i64::from(frame.x.max(display.x));
                        let height = (i64::from(frame.y) + i64::from(frame.height))
                            .min(i64::from(display.y) + i64::from(display.height))
                            - i64::from(frame.y.max(display.y));
                        (width > 1 && height > 0).then_some((display, width * height))
                    })
                    .max_by_key(|(_, area)| *area)
                    .map(|(display, _)| display)
            })
    }

    /// The connected display where `window` currently sits, falling back to
    /// its recorded recovery frame when it is already parked off-screen.
    pub(super) fn window_display_output(
        &mut self,
        window: &crate::PhysicalWindowId,
        outputs: &macos::DisplayOutputs,
    ) -> Option<crate::Rect> {
        let displays: Vec<_> = outputs.frames().collect();
        self.backend
            .window_server_frame(window)
            .or_else(|| self.backend.frame(window).ok())
            .and_then(|frame| Self::display_for_frame(frame, &displays))
            .or_else(|| {
                self.parked
                    .get(window)
                    .copied()
                    .and_then(|frame| Self::display_for_frame(frame, &displays))
            })
    }

    pub(super) fn park_window(
        &mut self,
        window: &crate::PhysicalWindowId,
        output: crate::Rect,
        outputs: &macos::DisplayOutputs,
    ) -> macos::ApplyOutcome {
        self.park_window_toward(window, output, None, outputs)
    }

    /// Park a window that is moving to a hidden workspace on another display.
    /// It parks off the edge of the display it currently occupies (falling
    /// back to `from`): macOS can clamp a write off another display's edge
    /// back onto the window's own display, which used to fail the whole move.
    /// Its recovery frame moves from `from` onto `to` (same size, same place
    /// relative to the display), so showing the workspace, or recovering after
    /// a crash, brings it back on the display its workspace lives on.
    pub(super) fn park_window_across(
        &mut self,
        window: &crate::PhysicalWindowId,
        from: crate::Rect,
        to: crate::Rect,
        outputs: &macos::DisplayOutputs,
    ) -> macos::ApplyOutcome {
        self.park_window_toward(window, from, Some((from, to)), outputs)
    }

    fn park_window_toward(
        &mut self,
        window: &crate::PhysicalWindowId,
        output: crate::Rect,
        relocate: Option<(crate::Rect, crate::Rect)>,
        outputs: &macos::DisplayOutputs,
    ) -> macos::ApplyOutcome {
        if self.parking_exempt.contains(window) {
            return macos::ApplyOutcome::Applied;
        }
        let ax_frame = match self.backend.frame(window) {
            Ok(frame) => frame,
            Err(macos::WindowAccessError::Gone) => return macos::ApplyOutcome::Gone,
            Err(macos::WindowAccessError::Unavailable) => {
                return macos::ApplyOutcome::Unavailable;
            }
        };
        // Prefer the window server's frame: Accessibility reads can be stale
        // right after a move, which would misclassify the window and record a
        // wrong recovery frame.
        let frame = self.backend.window_server_frame(window).unwrap_or(ax_frame);
        let was_parked = self.parked.contains_key(window);
        let displays = outputs.frames().collect::<Vec<_>>();
        if !crate::geometry::shows_on(frame, &displays) && !was_parked {
            // The app keeps this window off-screen itself. There is nothing to hide
            // and no on-screen frame worth recording for recovery.
            log_event!("park skip: {:?} already off-screen at {:?}", window, frame);
            return macos::ApplyOutcome::Applied;
        }
        let original = self.parked.get(window).copied().unwrap_or(frame);
        let recovery = match relocate {
            Some((from, to))
                if from != to
                    && (from.contains_center(original)
                        || Self::display_for_frame(original, &displays) == Some(from)) =>
            {
                super::move_workspace::translate_frame_to_output(original, from, to)
            }
            _ => original,
        };
        if !self.record_recovery(window, recovery) {
            return macos::ApplyOutcome::Unavailable;
        }
        // Park off the display the window currently occupies (falling back to
        // `output` when already off-screen) so macOS does not clamp a
        // cross-display write back on-screen. Use the window's current size so
        // a grown window leaves nothing showing.
        let park_output = Self::display_for_frame(frame, &displays).unwrap_or(output);
        let park_size = crate::Rect {
            width: frame.width.max(ax_frame.width),
            height: frame.height.max(ax_frame.height),
            ..frame
        };
        let parked = crate::geometry::parked_frame_clear_of(
            park_output,
            &displays,
            &outputs.screens(),
            park_size,
        );
        let outcome = self.backend.set_parked_position(window, parked);
        log_event!("park: {:?} from {:?} ({:?})", window, recovery, outcome);
        if matches!(outcome, macos::ApplyOutcome::Applied) {
            self.recently_parked.insert(window.clone(), Instant::now());
        } else {
            // Undo onto the frame it had, not the display it was heading to.
            if recovery != original && !self.record_recovery(window, original) {
                self.parked.insert(window.clone(), original);
            }
            if !was_parked {
                let _: macos::ApplyOutcome = self.restore_window(window);
            }
        }
        outcome
    }

    pub(super) fn restore_window(
        &mut self,
        window: &crate::PhysicalWindowId,
    ) -> macos::ApplyOutcome {
        let frame = self.parked.get(window).copied();
        let outcome = self.restore_window_uncommitted(window);
        if !self.parked.contains_key(window)
            && let Some(frame) = frame
        {
            self.persist_removals_or_rollback(vec![(window.clone(), frame)]);
        }
        outcome
    }

    fn restore_window_uncommitted(
        &mut self,
        window: &crate::PhysicalWindowId,
    ) -> macos::ApplyOutcome {
        let Some(frame) = self.parked.get(window).copied() else {
            return macos::ApplyOutcome::Applied;
        };
        let outcome = self.write_restore_position(window, frame);
        if let macos::ApplyOutcome::Applied = outcome {
            // Accessibility read-back is optimistic for apps that apply
            // moves asynchronously. Keep the recovery frame until the
            // window server confirms arrival; cycles retry until it does.
            // A window the window server cannot see is not confirmed.
            let target = self.restore_target(frame);
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
                    frame
                );
            }
        }
        outcome
    }

    /// Apply a restore write and update transient parking state. Confirmation
    /// and durable removal remain the caller's responsibility so bulk paths
    /// can share one window-server snapshot and one persistence transaction.
    pub(super) fn write_restore_position(
        &mut self,
        window: &crate::PhysicalWindowId,
        frame: crate::Rect,
    ) -> macos::ApplyOutcome {
        let target = self.restore_target(frame);
        let mut outcome = self.backend.set_position(window, target);
        log_event!("restore: {:?} to {:?} ({:?})", window, target, outcome);
        // Accessibility reports a window it cannot resolve as gone; only the
        // window server or the process exiting may retire its recovery frame.
        if outcome == macos::ApplyOutcome::Gone && !self.confirm_gone(window) {
            outcome = macos::ApplyOutcome::Unavailable;
        }
        match outcome {
            macos::ApplyOutcome::Applied => {
                // Also throttles the next retry and heal consideration.
                self.recently_parked.insert(window.clone(), Instant::now());
            }
            macos::ApplyOutcome::Gone => self.commit_unparked(window),
            _ => {}
        }
        outcome
    }

    /// A layout placed these windows on screen. Any still recorded as parked (a
    /// config reload made their workspace visible) are no longer hidden; retire
    /// their recovery frames, or a later park or restore would reuse them.
    pub(super) fn retire_laid_out(&mut self, results: &[macos::ApplyResult]) {
        let mut retired = Vec::new();
        for result in results {
            if matches!(
                result.outcome,
                macos::ApplyOutcome::Applied | macos::ApplyOutcome::Constrained
            ) && let Some(frame) = self.parked.get(&result.window).copied()
            {
                self.commit_unparked(&result.window);
                retired.push((result.window.clone(), frame));
            }
        }
        self.persist_removals_or_rollback(retired);
    }

    /// The window is confirmed no longer parked; retire all parking state.
    pub(super) fn commit_unparked(&mut self, window: &crate::PhysicalWindowId) {
        self.parked.remove(window);
        self.recently_parked.remove(window);
        self.repark_attempts.remove(window);
    }

    pub(super) fn persist_removals_or_rollback(
        &mut self,
        restored: Vec<(crate::PhysicalWindowId, crate::Rect)>,
    ) {
        if restored.is_empty() {
            return;
        }
        if let Err(error) = self.backend.persist_parked(&self.parked) {
            self.parked.extend(restored);
            log_event!("failed to persist restored windows: {error}");
        }
    }
}
