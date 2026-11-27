use super::*;

impl MainThreadAgent {
    pub(super) fn tick_automatic(&mut self) {
        self.refresh_hotkey_layout();
        if !self.automatic {
            // Hotkey failure may have paused inside this iteration.
            return;
        }
        self.heal_missed_hotkey();
        if !self.automatic {
            return;
        }
        // `NSWorkspace` flags activations; asking LaunchServices for the
        // frontmost app on every tick cost a synchronous XPC 40 times a second.
        let check_frontmost = self.events.take_frontmost_changed()
            || self.last_frontmost_check.elapsed() >= FRONTMOST_FALLBACK_INTERVAL;
        let frontmost_changed = check_frontmost && {
            self.last_frontmost_check = Instant::now();
            let current = macos::frontmost_application();
            std::mem::replace(&mut self.frontmost_application, current)
                != self.frontmost_application
        };
        if frontmost_changed {
            self.last_activity = Instant::now();
            self.opening_until = Some(Instant::now() + WINDOW_OPENING_SETTLE);
            self.activation_refresh_at = Some(Instant::now() + APPLICATION_ACTIVATION_SETTLE);
            // Background cycles and previews yield to activation changes. Explicit
            // user commands (switch, move, focus) change the frontmost app
            // themselves; cancelling them here would abort our own work.
            if self.worker.is_interruptible_busy() {
                self.cancelled.store(true, Ordering::Release);
            }
        }
        if macos::take_active_space_changed() {
            // A desktop or fullscreen slide, or fingers that may be swiping
            // to another desktop (#513): a background cycle under way would
            // lay out both Spaces' windows together, so stop it, and retile
            // once the slide settles (#334).
            self.last_activity = Instant::now();
            if self.worker.is_interruptible_busy() {
                self.cancelled.store(true, Ordering::Release);
            }
            let settled = Instant::now() + SPACE_TRANSITION_SETTLE;
            self.observation_recheck = Some(
                self.observation_recheck
                    .map_or(settled, |recheck| recheck.min(settled)),
            );
        }
        let activation_ready = self
            .activation_refresh_at
            .is_none_or(|refresh_at| Instant::now() >= refresh_at);
        if activation_ready && self.activation_refresh_at.take().is_some() {
            self.events.invalidate();
        }
        let mouse_down = macos::mouse_down();
        if mouse_down {
            self.last_activity = Instant::now();
        }
        let pointer = (mouse_down || self.mouse_was_down)
            .then(macos::mouse_position)
            .flatten();
        if !self.mouse_was_down && mouse_down {
            // Core Graphics hit-testing and readback do not send synchronous
            // AX messages to another app from the main-thread input loop.
            self.gesture_tracker.begin_pressed(
                pointer.map(macos::windows_at_point).unwrap_or_default(),
                pointer,
                Some(Instant::now()),
            );
        } else if mouse_down
            && self.gesture_tracker.is_active()
            && let Some(point) = pointer
        {
            self.gesture_tracker.observe_point(point);
        }
        if self.mouse_was_down && !mouse_down {
            let frames = self
                .gesture_tracker
                .is_active()
                .then(macos::window_server_frames)
                .flatten();
            if let Some(gesture) = self.gesture_tracker.finish(
                |window| frames.as_ref()?.get(&window.window_id).copied(),
                pointer,
            ) {
                if self.worker.is_interruptible_busy() {
                    self.cancelled.store(true, Ordering::Release);
                }
                self.queue_worker_input(QueuedInput::Gesture(gesture));
            }
            self.events.invalidate();
        } else if mouse_down
            && !self.worker.is_busy()
            && self.pending_inputs.is_empty()
            && self.gesture_tracker.is_active()
            // One window-list copy per tick, however many windows the gesture may be about.
            && let Some(frames) = macos::window_server_frames()
            && let Some(gesture) = self.gesture_tracker.preview(
                |window| frames.get(&window.window_id).copied(),
                pointer,
            )
        {
            self.worker
                .send_with_outputs(|outputs| WorkerCommand::Gesture(gesture, true, outputs));
        }
        self.tick_hover(mouse_down || self.mouse_was_down);
        self.mouse_was_down = mouse_down;
        let held = self.pending_inputs.front().and_then(|input| {
            held_by_space_change(
                input,
                macos::last_active_space_change(),
                macos::last_space_gesture(),
                Instant::now(),
            )
        });
        if held.is_some() {
            // A command on the focused window waits out the Space slide's
            // layout hold, or a desktop swipe's before macOS reports it, then
            // runs after a retile binds the arriving desktop's windows (#770, #777).
            self.retile_before_input = true;
        } else if self.retile_before_input && !self.worker.is_busy() {
            self.cancelled.store(false, Ordering::Release);
            if self
                .worker
                .send_with_outputs(|outputs| WorkerCommand::Action(WorkerAction::Retile, outputs))
            {
                self.retile_before_input = false;
                self.last_refresh = Instant::now();
            }
        } else if !self.worker.is_busy()
            && let Some(input) = self.pending_inputs.front().cloned()
        {
            self.cancelled.store(false, Ordering::Release);
            if self.worker.send_with_outputs(|outputs| match input {
                QueuedInput::Action(action) => WorkerCommand::Action(action, outputs),
                QueuedInput::Gesture(gesture) => WorkerCommand::Gesture(gesture, false, outputs),
            }) {
                self.pending_inputs.pop();
            }
        }
        if !mouse_down
            && !self.worker.is_busy()
            && self.pending_inputs.is_empty()
            && {
                let window_created = self.events.take_window_created();
                if window_created {
                    self.opening_until = Some(Instant::now() + WINDOW_OPENING_SETTLE);
                }
                let event_dirty = (window_created || activation_ready) && self.events.take_dirty();
                if window_created || event_dirty {
                    self.last_activity = Instant::now();
                }
                refresh_due(
                    window_created,
                    event_dirty,
                    self.last_refresh.elapsed()
                        >= fallback_refresh_interval(self.last_activity.elapsed())
                        || self
                            .observation_recheck
                            .is_some_and(|at| Instant::now() >= at),
                    activation_ready,
                )
            }
            && self
                .worker
                .send_with_outputs(|outputs| WorkerCommand::Action(WorkerAction::Retile, outputs))
        {
            self.last_refresh = Instant::now();
            if self
                .observation_recheck
                .is_some_and(|at| self.last_refresh >= at)
            {
                self.observation_recheck = None;
            }
        }
    }

    fn refresh_hotkey_layout(&mut self) {
        if self.last_hotkey_refresh.elapsed() < APPLICATION_REFRESH_INTERVAL {
            return;
        }
        self.last_hotkey_refresh = Instant::now();
        let layout_changed = match self.hotkeys.as_ref().map(Hotkeys::matches_current_layout) {
            Some(Ok(matches)) => !matches,
            Some(Err(error)) => {
                let error = format!("Hotkeys unavailable: {error}");
                self.last_error = Some(error.clone());
                self.ui_dirty = true;
                status_item::set_status(&error);
                false
            }
            None => false,
        };
        if !layout_changed {
            return;
        }
        match HotkeyPlan::current(&self.hotkey_workspaces, &self.shortcuts) {
            // Resolve and validate while old registrations remain active. Carbon
            // cannot register the replacement chords until those are released.
            Ok(plan)
                if !self
                    .hotkeys
                    .as_ref()
                    .is_some_and(|hotkeys| plan.matches(hotkeys)) =>
            {
                log_event!("hotkeys: keyboard layout changed");
                self.reinstall_hotkeys(plan);
            }
            Ok(_) => {}
            Err(error) => {
                let error = format!("Hotkeys unavailable: {error}");
                self.last_error = Some(error.clone());
                self.ui_dirty = true;
                status_item::set_status(&error);
            }
        }
    }
}
