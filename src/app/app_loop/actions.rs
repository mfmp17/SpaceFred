use super::*;

impl MainThreadAgent {
    /// Starts automatic tiling once the worker, Accessibility, and hotkeys are available.
    ///
    /// `manual` distinguishes an explicit user resume from internal starts (Accessibility
    /// granted, setup tour finished, failed-update recovery, or auto-resume). An internal start
    /// respects `pause_on_builtin_display` immediately instead of briefly tiling on an undocked
    /// laptop or suppressing auto-pause for the session.
    ///
    /// Hotkeys may already be registered while a first-run setup tour holds
    /// tiling, so an installed set is kept rather than registered twice.
    pub(super) fn start_automatic(&mut self, manual: bool) {
        if self.worker.is_gone() {
            self.awaiting_accessibility = false;
            self.ui_dirty = true;
            status_item::set_status("Paused · worker unavailable");
            return;
        }
        let trusted = macos::accessibility_trusted();
        self.accessibility = trusted;
        if !trusted {
            self.awaiting_accessibility = true;
            self.last_error = Some("Accessibility permission is required".into());
            self.ui_dirty = true;
            status_item::set_status("Paused · Accessibility required");
            return;
        }
        self.awaiting_accessibility = false;
        let topology = self
            .pause_on_builtin_display
            .then(macos::display_topology)
            .flatten();
        if self
            .builtin_display
            .on_start(manual, self.pause_on_builtin_display, topology)
            == builtin_display::StartDecision::AutoPause
        {
            self.hotkeys = None;
            self.release_hotkey_hold_on_stop();
            self.hotkey_delivery.clear();
            self.setup.clear_arrangement();
            if !is_update_failed_error(self.last_error.as_deref())
                && !is_shortcuts_not_applied_error(self.last_error.as_deref())
            {
                self.last_error = None;
            }
            self.ui_dirty = true;
            status_item::set_automatic(false);
            status_item::set_status(self.paused_status());
            log_event!("auto-paused tiling: built-in display only");
            self.bridge
                .publish_toast(UiHudToast::paused(builtin_display::TOAST_PAUSED));
            return;
        }
        if self.hotkeys.is_none() {
            match HotkeyPlan::current(&self.hotkey_workspaces, &self.shortcuts)
                .and_then(|plan| Hotkeys::install(self.actions_sender.clone(), plan))
            {
                Ok(registered) => {
                    log_event!("hotkeys: registered {} shortcuts", registered.len());
                    for skipped in registered.skipped() {
                        log_event!("hotkeys: {skipped}");
                    }
                    self.hotkeys = Some(registered);
                    self.hotkey_delivery.clear();
                }
                Err(error) => {
                    let error = format!("Hotkeys unavailable: {error}");
                    self.last_error = Some(error.clone());
                    self.ui_dirty = true;
                    self.worker.request_restore(RestoreKind::Restore);
                    status_item::set_status(&error);
                    return;
                }
            }
        }
        self.listen_for_input();
        self.automatic = true;
        self.cancelled.store(false, Ordering::Release);
        self.events.invalidate();
        self.ui_dirty = true;
        status_item::set_automatic(true);
        let status = automatic_started_status(&mut self.last_error, self.scope.automatic_status());
        status_item::set_status(status);
        self.queue_arrangement();
    }

    pub(super) fn drain_actions(&mut self) {
        while let Ok(action) = self.actions.try_recv() {
            if self.exit_intent.is_some() {
                if action == Action::Terminate {
                    termination::cancel_pending();
                }
                continue;
            }
            match action {
                Action::ToggleAutomatic if self.automatic => {
                    self.awaiting_accessibility = false;
                    self.builtin_display.on_manual_pause();
                    self.stop_automatic_and_restore(RestoreKind::Restore);
                    status_item::set_status(if self.worker.is_busy() {
                        "Paused"
                    } else {
                        "Paused · worker unavailable"
                    });
                }
                Action::ToggleAutomatic if self.worker.is_restoring_or_pending() => {
                    status_item::set_status("Restoring windows...");
                }
                Action::ToggleAutomatic => {
                    // An explicit start also ends a first-run tour's hold.
                    self.startup_tiling.setup_finished();
                    self.start_automatic(true);
                }
                Action::FinishSetup => {
                    self.startup_tiling.setup_finished();
                    if !self.automatic {
                        if self.worker.is_restoring_or_pending() {
                            // The normal gate starts tiling once restoration ends.
                            self.awaiting_accessibility = true;
                            self.ui_dirty = true;
                        } else {
                            self.start_automatic(false);
                        }
                    }
                    self.ui_dirty = true;
                }
                Action::SkipSetup => self.skip_setup(),
                Action::PlanSetup => self.want_inventory(),
                Action::Arrange(arrangement) => self.arrange(arrangement),
                Action::UndoArrangement => self.undo_arrangement(),
                Action::OpenSettings => {
                    if let Err(error) = (self.callbacks.open_settings)() {
                        status_item::set_status(&format!("Could not open Settings: {error}"));
                    }
                }
                Action::OpenTarget(target) => {
                    if let Some(note) = &target.diagnostic {
                        log_event!("deep link: {note}");
                    }
                    if let Err(error) = (self.callbacks.open_target)(target) {
                        status_item::set_status(&format!("Could not open Settings: {error}"));
                    }
                }
                Action::OpenSetup => {
                    if let Err(error) = (self.callbacks.open_setup)() {
                        status_item::set_status(&format!("Could not open Setup: {error}"));
                    }
                }
                Action::OpenAccessibility => {
                    if permission_guide::prompt_accessibility_trust() {
                        permission_guide::close();
                        self.accessibility_requested = false;
                        self.ui_dirty = true;
                    } else {
                        self.accessibility_requested = true;
                        self.last_accessibility_poll = Instant::now();
                        permission_guide::show();
                    }
                }
                Action::ToggleHud => {
                    let next = !self.hud;
                    match (self.callbacks.save_hud)(next) {
                        Ok(()) => {
                            // Leave `config_stamp` for the probe: the save keeps any hand edit
                            // already on disk, and the probe's reading of it applies the rest
                            // (shortcuts, Labs) along with this value.
                            self.hud = next;
                            status_item::set_hud(next);
                            self.publish_ui(true);
                        }
                        Err(error) => {
                            status_item::set_status(&format!(
                                "Could not change Workspace HUD: {error}"
                            ));
                        }
                    }
                }
                Action::SetHud(enabled) => {
                    // As for `ToggleHud`, the probe's next reading applies the whole saved file.
                    self.hud = enabled;
                    status_item::set_hud(enabled);
                    self.ui_dirty = true;
                }
                Action::SetPauseOnBuiltinDisplay(enabled) => {
                    // As for `ToggleHud`, the probe's next reading applies the whole saved file.
                    self.set_pause_on_builtin_display(enabled);
                    self.ui_dirty = true;
                }
                Action::CheckForUpdates => {
                    self.clear_update_failed_error();
                    self.updater.check_now();
                    self.ui_dirty = true;
                }
                Action::InstallUpdate if !self.updater.ready() => {
                    status_item::set_status("No update is ready to install");
                }
                Action::ConfirmUpdateSwitch => {
                    self.clear_update_failed_error();
                    self.updater.confirm_switch();
                    self.ui_dirty = true;
                }
                Action::DownloadUpdate => match self.updater.open_download() {
                    Ok(()) => self.clear_update_failed_error(),
                    Err(error) => {
                        status_item::set_status(&format!("Could not open the update: {error}"));
                    }
                },
                Action::ToggleLaunchAtLogin => {
                    let result = login_item::toggle();
                    self.launch_at_login = UiLaunchAtLogin::from_result(result.clone());
                    self.ui_dirty = true;
                    match result {
                        Ok(status) => {
                            status_item::set_launch_at_login(status);
                            if status == login_item::Status::RequiresApproval {
                                status_item::set_status(
                                    "Launch at Login requires approval in System Settings",
                                );
                            }
                        }
                        Err(error) => {
                            status_item::set_status(&format!(
                                "Could not change Launch at Login: {error}"
                            ));
                        }
                    }
                }
                Action::Restart | Action::Quit | Action::InstallUpdate | Action::Terminate => {
                    let intent = match action {
                        Action::Restart => ExitIntent::Restart,
                        Action::InstallUpdate => ExitIntent::Update,
                        Action::Terminate => ExitIntent::Terminate,
                        _ => ExitIntent::Quit,
                    };
                    permission_guide::close();
                    self.resume_after_update = intent == ExitIntent::Update && self.automatic;
                    self.exit_intent = Some(intent);
                    self.stop_automatic_and_restore(RestoreKind::Restore);
                    if self.worker.is_busy() {
                        status_item::set_status(match intent {
                            ExitIntent::Quit => "Restoring windows...",
                            ExitIntent::Restart => "Restarting · restoring windows...",
                            ExitIntent::Update => "Update ready · restoring windows...",
                            ExitIntent::Terminate => "Terminating · restoring windows...",
                        });
                    } else {
                        self.cancel_unavailable_exit();
                    }
                }
                Action::Hotkey(id) => {
                    self.interrupt_window_overlay();
                    self.hotkey_delivered(id);
                }
                Action::OptionKeyDown(key_code, modifiers) => {
                    self.interrupt_window_overlay();
                    self.option_key_down(key_code, modifiers);
                }
                Action::Worker(action) => {
                    if self.automatic
                        && crate::app::input::admits_action(&action, self.swipe_fingers, || {
                            (
                                macos::mouse_down(),
                                self.mouse_was_down,
                                self.gesture_tracker.is_active(),
                            )
                        })
                    {
                        self.queue_worker_input(QueuedInput::Action(action));
                    }
                }
            }
        }
        if self.exit_intent.is_none()
            && std::mem::take(&mut self.pending_automatic_start)
            && !self.automatic
            && !self.worker.is_restoring_or_pending()
        {
            self.start_automatic(false);
        }
    }
}
