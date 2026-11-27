use super::*;

mod restore;
mod table;
pub(super) use table::{ParkRecord, ParkTable};

/// Accessibility lookups fail transiently, notably right after Spaceman or an
/// application launches. A window the window server still lists keeps its
/// slot for this long before an Accessibility "gone" is believed.
pub(super) const INACCESSIBLE_GRACE: Duration = Duration::from_secs(30);
/// A restore is confirmed when the window server reports the window near its
/// target. Loose on purpose: apps may clamp the exact position, while a
/// still-parked window sits a full display dimension away.
pub(super) const RESTORE_CONFIRM_TOLERANCE: i32 = 64;
/// After this many undone re-parks in a row the window's reappearance is
/// honored (re-homed) instead of fought; repeated honors exempt it from parking.
pub(super) const REPARK_ATTEMPT_LIMIT: u8 = 3;

/// Where a parked window's recovery frame points.
#[derive(Clone, Copy)]
enum Relocate {
    /// Where it is now (or its existing recovery frame).
    Keep,
    /// Moved from the first display onto the second, same place relative to it.
    Across(crate::Rect, crate::Rect),
    /// At this position, keeping the window's size.
    At(crate::Rect),
}

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
    /// Fighting a genuinely reappeared window back off-screen loses, so drop the
    /// stale recovery frame and re-home the window to the workspace it reappeared on.
    pub(super) fn heal_reappeared_window(
        &mut self,
        window: &crate::ObservedWindow,
        visible: &[ResolvedWorkspace],
        server_frames: &HashMap<u32, crate::Rect>,
    ) -> Option<WorkspaceId> {
        let original = self.parked.get(&window.id).copied()?;
        if self.parked.awaits_display(&window.id)
            || self.parked.settling(&window.id, self.backend.now())
        {
            // The park write may still be applying inside the app; wait it out.
            return None;
        }
        // The observation may be stale; only the window server is trusted.
        let frame = server_frames.get(&window.id.window_id).copied()?;
        let target = visible
            .iter()
            .find(|resolved| resolved.output.contains_center(frame))?;
        if self.backend.frontmost_application().as_ref() != Some(&window.id.app) {
            // A background reappearance is an app re-asserting its saved bounds
            // (Electron does this on screen-parameter changes), not the user
            // summoning the window. Keep the recovery frame; parking pushes it
            // back off-screen instead of re-homing it.
            return None;
        }
        if self.returned_by_space_slide(&window.id) {
            // macOS put it back while leaving the app's fullscreen Space (#489);
            // parking pushes it back off-screen.
            return None;
        }
        self.honor_reappearance(&window.id, frame, original, target)
    }

    /// Honor a window that will not stay parked: drop the recovery frame and
    /// re-home it to the workspace it insists on appearing on. A second honor
    /// within five minutes exempts it from further parking.
    fn honor_reappearance(
        &mut self,
        window: &crate::PhysicalWindowId,
        frame: crate::Rect,
        original: crate::Rect,
        target: &ResolvedWorkspace,
    ) -> Option<WorkspaceId> {
        let record = self.commit_unparked(window);
        log_event!(
            "heal: {:?} reappeared at {:?}; dropping recovery frame {:?}",
            window,
            frame,
            original
        );
        self.persist_removals_or_rollback(record.map(|record| (window.clone(), record)));
        if self.parked.contains_key(window) {
            return None;
        }
        let repeatedly_unparked = self.parked.note_heal(window, self.backend.now());
        let workspace = target.workspace;
        let mut joined_fullscreen = None;
        if self.workspaces.floating_workspace_of(window).is_some() {
            log_event!(
                "heal: re-home floating {:?} -> workspace {}",
                window,
                workspace
            );
            self.workspaces.own_floating(window.clone(), workspace);
        } else if let Some(source) = self.move_tiled_to(window, workspace, target.output) {
            if self.workspaces.deck(workspace).is_monocle() {
                joined_fullscreen = Some(workspace);
            }
            self.clear_fullscreen_restore_if_inactive(source);
            self.clear_relocation_evidence(source, workspace, window);
            log_event!(
                "heal: re-home tiled {:?} workspace {} -> {}",
                window,
                source,
                workspace
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
        }
        joined_fullscreen
    }

    pub(super) fn park_inactive(
        &mut self,
        outputs: &macos::DisplayOutputs,
        visible: &[ResolvedWorkspace],
        server_frames: &HashMap<u32, crate::Rect>,
        cancelled: &AtomicBool,
        failed: &mut usize,
    ) -> Option<WorkspaceId> {
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
        let lost = self.displays.lost_displays().to_vec();
        let mut fullscreen_hud = None;
        // Hidden scratchpad windows intentionally have no workspace owner, so
        // re-park any background bounds reassertions here before affinity sees them.
        for window in self.workspaces.scratchpad().hidden() {
            if cancelled.load(Ordering::Acquire) {
                return fullscreen_hud;
            }
            if self.stays_on_screen(&window) {
                continue;
            }
            if let Some(original) = self.parked.get(&window).copied()
                && let Some(ws) =
                    self.repark_drifted_window(&window, original, outputs.main().frame, &context)
            {
                fullscreen_hud = Some(ws);
            }
        }
        let mut candidates = Vec::new();
        for (workspace, window) in inactive {
            if cancelled.load(Ordering::Acquire) {
                return fullscreen_hud;
            }
            // Pinned windows show on every workspace; restore_visible brings back one pinned
            // while it was parked.
            if self.stays_on_screen(&window) {
                continue;
            }
            let output = output_for(&self.workspaces, workspace, outputs);
            if let Some(original) = self.parked.get(&window).copied() {
                // A parked window the window server stopped listing closed while its app kept
                // running; forget it so its hidden workspace stops counting as occupied
                // (#553). An empty snapshot means the server could not be read, and an app
                // that quit is left to the exited-app path, which notes its departure for
                // relaunch recall.
                if !server_frames.is_empty()
                    && !server_frames.contains_key(&window.window_id)
                    && !self.parked.awaits_display(&window)
                    && self.backend.is_process_running(&window.app)
                    && matches!(
                        self.backend.frame(&window),
                        Err(macos::WindowAccessError::Gone)
                    )
                    && self.retire_confirmed_gone_window(workspace, &window)
                {
                    continue;
                }
                if let Some(ws) = self.repark_drifted_window(&window, original, output, &context) {
                    fullscreen_hud = Some(ws);
                }
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
            let recovery = if !crate::geometry::shows_on(frame, &displays) {
                if !crate::geometry::shows_on(frame, &lost)
                    || self.backend.confirms_set_aside(&window)
                {
                    // The app keeps this window off-screen itself (or it is minimized /
                    // hidden). There is nothing to hide and no on-screen frame worth
                    // recording for recovery.
                    log_event!("park skip: {:?} already off-screen at {:?}", window, frame);
                    continue;
                }
                // A live managed window left off-screen by the display that just went
                // away still needs a clamped recovery frame on `output` and an explicit
                // park write, so Pause/Quit or returning to the workspace brings it back
                // on screen.
                crate::geometry::onto_displays(frame, &[], output)
            } else {
                frame
            };
            // A window still on screen that an `ignore` rule now matches (loaded during
            // Pause, which restored it) is released where it is, not parked (#663).
            // One left off-screen by a lost display still parks with its clamped recovery
            // frame, so Pause or its workspace brings it back where the user can see it.
            if crate::geometry::shows_on(frame, &displays)
                && self.config.has_ignore_rules()
                && self.stale_window_ignored(&window, frame) == Some(true)
            {
                log_event!("release {:?} from workspace {}: ignored", window, workspace);
                self.forget_window(workspace, &window);
                continue;
            }
            let park_output = Self::display_for_frame(frame, &displays).unwrap_or(output);
            let target =
                crate::geometry::parked_frame_clear_of(park_output, &displays, &screens, frame);
            candidates.push((workspace, window, recovery, target, frame));
        }
        if candidates.is_empty() {
            return fullscreen_hud;
        }
        self.order_back_to_front(&mut candidates, |(_, window, ..)| window.window_id);
        for (_, window, frame, ..) in &candidates {
            let display = self.recovery_display(*frame);
            self.parked.insert(window.clone(), *frame, display);
        }
        if let Err(error) = self.persist_parked() {
            for (_, window, ..) in &candidates {
                self.parked.remove(window);
            }
            *failed += candidates.len();
            log_event!("failed to persist parked windows: {error}");
            return fullscreen_hud;
        }

        let mut restored = Vec::new();
        for (workspace, window, frame, target, from) in candidates {
            let record = self.parked.record(&window).cloned();
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
                    self.parked
                        .park_applied_from(&window, self.backend.now(), from);
                }
                macos::ApplyOutcome::Gone => {
                    restored.extend(self.parked.remove(&window).map(|r| (window.clone(), r)));
                    self.forget_window(workspace, &window);
                }
                _ => {
                    *failed += 1;
                    // Only an applied or gone restore retires the entry.
                    self.restore_window_uncommitted(&window);
                    if !self.parked.contains_key(&window) {
                        restored.extend(record.map(|record| (window, record)));
                    }
                }
            }
        }
        self.persist_removals_or_rollback(restored);
        fullscreen_hud
    }

    /// A parked window observed on-screen in the background re-asserted its
    /// saved bounds (Electron does this on screen-parameter changes). Push it
    /// back to its parked spot; the recovery frame is already recorded. A
    /// frontmost app's window is left for heal to re-home instead.
    fn repark_drifted_window(
        &mut self,
        window: &crate::PhysicalWindowId,
        original: crate::Rect,
        output: crate::Rect,
        context: &ParkContext<'_>,
    ) -> Option<WorkspaceId> {
        if self.parked.awaits_display(window) {
            return None;
        }
        let server = context.server_frames.get(&window.window_id).copied()?;
        let now = self.backend.now();
        if !crate::geometry::shows_on(server, context.displays) {
            // Only a window that STAYED parked past the settle grace ends a
            // fight; right after a re-park write the app may still undo it.
            self.parked.stayed_parked(window, now);
            return None;
        }
        // A stale readback right after a park still shows where the window was when the
        // park was issued (not necessarily its recovery frame, which a cross-display park
        // moves). A window that turns up somewhere else while its first park settles (a
        // background unhide, #693) moved since the park; re-park it now instead of
        // leaving it on screen for the rest of the grace.
        let moved_since_park = self.parked.settling_from(window, now).is_some_and(|from| {
            !server.position_approximately_equals(from, RESTORE_CONFIRM_TOLERANCE)
        });
        if (self.parked.settling(window, now) && !moved_since_park)
            || (context.frontmost.as_ref() == Some(&window.app)
                && !self.returned_by_space_slide(window))
        {
            return None;
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
            return None;
        }
        if self.parked.reparks(window) >= REPARK_ATTEMPT_LIMIT {
            // The app undoes every park. Fighting on would flicker forever;
            // honor the reappearance instead (re-home, then exempt it from
            // parking if it keeps happening).
            log_event!(
                "re-park escalation: {:?} will not stay parked after {} attempts",
                window,
                REPARK_ATTEMPT_LIMIT
            );
            return context
                .visible
                .iter()
                .find(|resolved| resolved.output.contains_center(server))
                .and_then(|target| self.honor_reappearance(window, server, original, target));
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
            self.parked.repark_applied(window, now);
        }
        None
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

    /// Write a parked window's off-screen position from its recovery frame
    /// alone, without first reading it (its app may not be answering). A
    /// rolled-back switch uses it to supersede a restore the app has not yet
    /// applied: the write goes out on the window's cached handle, which an
    /// unanswering app keeps (`macos::keeps_failed_handle`).
    pub(super) fn park_from_recovery(
        &mut self,
        window: &crate::PhysicalWindowId,
        output: crate::Rect,
        outputs: &macos::DisplayOutputs,
    ) -> macos::ApplyOutcome {
        let Some(original) = self.parked.get(window).copied() else {
            return macos::ApplyOutcome::Applied;
        };
        if self.stays_on_screen(window) {
            return macos::ApplyOutcome::Applied;
        }
        let displays = outputs.frames().collect::<Vec<_>>();
        let park_output = Self::display_for_frame(original, &displays).unwrap_or(output);
        let target = crate::geometry::parked_frame_clear_of(
            park_output,
            &displays,
            &outputs.screens(),
            original,
        );
        let outcome = self.backend.set_parked_position(window, target);
        log_event!(
            "park from recovery: {:?} from {:?} ({:?})",
            window,
            original,
            outcome
        );
        outcome
    }

    pub(super) fn park_window(
        &mut self,
        window: &crate::PhysicalWindowId,
        output: crate::Rect,
        outputs: &macos::DisplayOutputs,
    ) -> macos::ApplyOutcome {
        self.park_window_toward(window, output, Relocate::Keep, outputs)
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
        self.park_window_toward(window, from, Relocate::Across(from, to), outputs)
    }

    /// Park a window that is joining a hidden workspace with its recovery
    /// frame at `position` (a remembered placement), keeping its current size,
    /// so showing the workspace brings it back where it was remembered.
    pub(super) fn park_window_at(
        &mut self,
        window: &crate::PhysicalWindowId,
        from: crate::Rect,
        position: crate::Rect,
        outputs: &macos::DisplayOutputs,
    ) -> macos::ApplyOutcome {
        self.park_window_toward(window, from, Relocate::At(position), outputs)
    }

    fn park_window_toward(
        &mut self,
        window: &crate::PhysicalWindowId,
        output: crate::Rect,
        relocate: Relocate,
        outputs: &macos::DisplayOutputs,
    ) -> macos::ApplyOutcome {
        if self.stays_on_screen(window) {
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
        let recovery_frame = if !crate::geometry::shows_on(frame, &displays) && !was_parked {
            if !crate::geometry::shows_on(frame, self.displays.lost_displays())
                || self.workspaces.workspace_of_any(window).is_none()
                || self.backend.confirms_set_aside(window)
            {
                // The app keeps this window off-screen itself (or it is minimized
                // / hidden). There is nothing to hide and no on-screen frame worth
                // recording for recovery.
                log_event!("park skip: {:?} already off-screen at {:?}", window, frame);
                return macos::ApplyOutcome::Applied;
            }
            crate::geometry::onto_displays(frame, &[], output)
        } else {
            frame
        };
        let original = self.parked.get(window).copied().unwrap_or(recovery_frame);
        let recovery = match relocate {
            Relocate::At(position) => crate::Rect {
                x: position.x,
                y: position.y,
                ..original
            },
            Relocate::Across(from, to)
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
            self.parked
                .park_applied_from(window, self.backend.now(), frame);
        } else {
            // Undo onto the frame it had, not the display it was heading to.
            if recovery != original && !self.record_recovery(window, original) {
                let display = self.recovery_display(original);
                self.parked.insert(window.clone(), original, display);
            }
            if !was_parked {
                let _: macos::ApplyOutcome = self.restore_window(window);
            }
        }
        outcome
    }
}
