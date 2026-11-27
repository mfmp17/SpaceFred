//! Auto-pause when only the built-in display is connected, and auto-resume when an external
//! display reconnects (`settings.pause_on_builtin_display`).

use std::time::{Duration, Instant};

use crate::macos::{self, DisplayTopology};

use super::*;

pub(super) const TOAST_PAUSED: &str = "Tiling paused — built-in display";
pub(super) const TOAST_RESUMED: &str = "Tiling resumed — external display";
pub(super) const PAUSED_STATUS: &str = "Paused · built-in display";
/// How often the main loop checks connected display topology when the feature is active.
const DISPLAY_TOPOLOGY_INTERVAL: Duration = Duration::from_millis(500);
/// Consecutive built-in-only samples required before auto-pausing an active session, preventing
/// single-tick KVM/dock/mirror reconfiguration glitches from triggering restore + retile churn.
const AUTO_PAUSE_SAMPLES: u8 = 2;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum BuiltinDisplayAction {
    None,
    AutoPause,
    AutoResume,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum StartDecision {
    /// Proceed with starting automatic tiling.
    Start,
    /// Enter auto-pause instead of tiling (internal start on a built-in-only display).
    AutoPause,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(super) struct BuiltinDisplayPolicy {
    /// True only when Spaceman itself paused tiling because only a built-in display was attached.
    pub(super) auto_paused_for_builtin: bool,
    /// Built-in-only topology where the user explicitly resumed tiling; suppresses immediate
    /// re-pausing until the display topology actually changes.
    manual_resume_topology: Option<DisplayTopology>,
    /// Consecutive `evaluate` samples that observed a built-in-only topology while tiling.
    builtin_only_samples: u8,
}

impl BuiltinDisplayPolicy {
    /// Decides whether the observed `topology` should auto-pause or auto-resume tiling.
    #[must_use]
    pub(super) fn evaluate(
        &mut self,
        enabled: bool,
        automatic: bool,
        trusted: bool,
        restoring: bool,
        topology: DisplayTopology,
    ) -> BuiltinDisplayAction {
        if self
            .manual_resume_topology
            .is_some_and(|saved| saved != topology)
        {
            self.manual_resume_topology = None;
        }
        if !enabled {
            self.auto_paused_for_builtin = false;
            self.builtin_only_samples = 0;
            return BuiltinDisplayAction::None;
        }
        if automatic && topology.is_builtin_only() {
            if self.manual_resume_topology == Some(topology) {
                self.builtin_only_samples = 0;
                return BuiltinDisplayAction::None;
            }
            self.builtin_only_samples = self.builtin_only_samples.saturating_add(1);
            if self.builtin_only_samples < AUTO_PAUSE_SAMPLES {
                return BuiltinDisplayAction::None;
            }
            self.builtin_only_samples = 0;
            self.auto_paused_for_builtin = true;
            return BuiltinDisplayAction::AutoPause;
        }
        self.builtin_only_samples = 0;
        if !automatic && self.auto_paused_for_builtin && topology.has_external() {
            if !trusted || restoring {
                return BuiltinDisplayAction::None;
            }
            self.auto_paused_for_builtin = false;
            self.manual_resume_topology = None;
            return BuiltinDisplayAction::AutoResume;
        }
        BuiltinDisplayAction::None
    }

    /// Startup gate: when `enabled` and only a built-in display is connected at launch, start
    /// already auto-paused so windows are not briefly tiled and immediately restored.
    #[must_use]
    pub(super) fn should_start_auto_paused(
        &mut self,
        enabled: bool,
        topology: Option<DisplayTopology>,
    ) -> bool {
        self.builtin_only_samples = 0;
        if enabled && topology.is_some_and(DisplayTopology::is_builtin_only) {
            self.auto_paused_for_builtin = true;
            true
        } else {
            false
        }
    }

    /// Decides whether starting automatic tiling should tile now or enter auto-pause.
    ///
    /// A manual user start (`manual = true`) always tiles and suppresses immediate re-pausing
    /// while the current built-in-only topology holds. An internal start (`manual = false` — e.g.
    /// Accessibility granted, setup tour finished, failed-update recovery, or auto-resume) enters
    /// auto-pause immediately when `enabled` and only a built-in display is connected.
    #[must_use]
    pub(super) fn on_start(
        &mut self,
        manual: bool,
        enabled: bool,
        topology: Option<DisplayTopology>,
    ) -> StartDecision {
        self.builtin_only_samples = 0;
        if manual {
            self.on_manual_resume(enabled, topology);
            StartDecision::Start
        } else if self.should_start_auto_paused(enabled, topology) {
            StartDecision::AutoPause
        } else {
            self.auto_paused_for_builtin = false;
            StartDecision::Start
        }
    }

    pub(super) fn mark_auto_paused(&mut self) {
        self.auto_paused_for_builtin = true;
        self.builtin_only_samples = 0;
    }

    pub(super) fn clear_auto_pause(&mut self) {
        self.auto_paused_for_builtin = false;
        self.builtin_only_samples = 0;
    }

    /// User manually paused tiling: clear auto-pause so reconnecting displays never resumes it.
    pub(super) fn on_manual_pause(&mut self) {
        self.auto_paused_for_builtin = false;
        self.manual_resume_topology = None;
        self.builtin_only_samples = 0;
    }

    /// User manually resumed tiling: clear auto-pause and suppress immediate re-pausing while
    /// the current built-in-only display topology stays unchanged.
    pub(super) fn on_manual_resume(&mut self, enabled: bool, topology: Option<DisplayTopology>) {
        self.auto_paused_for_builtin = false;
        self.builtin_only_samples = 0;
        self.manual_resume_topology = if enabled {
            topology.filter(|t| t.is_builtin_only())
        } else {
            None
        };
    }

    /// Updates state when `settings.pause_on_builtin_display` changes at runtime.
    pub(super) fn on_setting_changed(&mut self, enabled: bool) {
        self.manual_resume_topology = None;
        self.builtin_only_samples = 0;
        if !enabled {
            self.auto_paused_for_builtin = false;
        }
    }
}

impl MainThreadAgent {
    pub(super) fn set_pause_on_builtin_display(&mut self, enabled: bool) {
        if self.pause_on_builtin_display == enabled {
            return;
        }
        self.pause_on_builtin_display = enabled;
        self.builtin_display.on_setting_changed(enabled);
        self.display_topology_dirty = true;
        if !self.automatic {
            status_item::set_status(self.paused_status());
        }
    }

    pub(super) fn tick_builtin_display(&mut self) {
        if self.exit_intent.is_some()
            || self.startup_tiling.held()
            || (!self.pause_on_builtin_display && !self.builtin_display.auto_paused_for_builtin)
        {
            return;
        }
        let due = self.display_topology_dirty
            || self.last_display_topology_check.elapsed() >= DISPLAY_TOPOLOGY_INTERVAL;
        if !due {
            return;
        }
        self.display_topology_dirty = false;
        self.last_display_topology_check = Instant::now();
        let Some(topology) = macos::display_topology() else {
            return;
        };
        let trusted = macos::accessibility_trusted();
        let action = self.builtin_display.evaluate(
            self.pause_on_builtin_display,
            self.automatic,
            trusted,
            self.worker.is_restoring_or_pending(),
            topology,
        );
        match action {
            BuiltinDisplayAction::None => {}
            BuiltinDisplayAction::AutoPause => {
                self.awaiting_accessibility = false;
                self.stop_automatic_and_restore(RestoreKind::Restore);
                self.builtin_display.mark_auto_paused();
                log_event!("auto-paused tiling: built-in display only");
                status_item::set_status(if self.worker.is_busy() {
                    PAUSED_STATUS
                } else {
                    "Paused · worker unavailable"
                });
                self.bridge.publish_toast(UiHudToast::paused(TOAST_PAUSED));
            }
            BuiltinDisplayAction::AutoResume => {
                self.start_automatic(false);
                if self.automatic {
                    log_event!("auto-resumed tiling: external display connected");
                    self.bridge
                        .publish_toast(UiHudToast::resumed(TOAST_RESUMED));
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BUILTIN_ONLY: DisplayTopology = DisplayTopology {
        builtin: 1,
        external: 0,
    };
    const DOCKED: DisplayTopology = DisplayTopology {
        builtin: 1,
        external: 1,
    };
    const CLAMSHELL: DisplayTopology = DisplayTopology {
        builtin: 0,
        external: 1,
    };
    const STUDIO_SINGLE: DisplayTopology = DisplayTopology {
        builtin: 0,
        external: 1,
    };
    const STUDIO_DUAL: DisplayTopology = DisplayTopology {
        builtin: 0,
        external: 2,
    };

    fn unplug(policy: &mut BuiltinDisplayPolicy) -> BuiltinDisplayAction {
        assert_eq!(
            policy.evaluate(true, true, true, false, BUILTIN_ONLY),
            BuiltinDisplayAction::None,
            "first built-in-only sample should wait for confirmation"
        );
        policy.evaluate(true, true, true, false, BUILTIN_ONLY)
    }

    #[test]
    fn disconnect_external_auto_pauses_and_reconnect_auto_resumes() {
        let mut policy = BuiltinDisplayPolicy::default();

        // Docked laptop stays active.
        assert_eq!(
            policy.evaluate(true, true, true, false, DOCKED),
            BuiltinDisplayAction::None
        );
        assert!(!policy.auto_paused_for_builtin);

        // Unplugging external display triggers auto-pause after 2 consecutive samples.
        assert_eq!(unplug(&mut policy), BuiltinDisplayAction::AutoPause);
        assert!(policy.auto_paused_for_builtin);

        // Staying on built-in display while paused does nothing.
        assert_eq!(
            policy.evaluate(true, false, true, false, BUILTIN_ONLY),
            BuiltinDisplayAction::None
        );
        assert!(policy.auto_paused_for_builtin);

        // Reconnecting external while still restoring waits until restoration completes.
        assert_eq!(
            policy.evaluate(true, false, true, true, DOCKED),
            BuiltinDisplayAction::None
        );
        assert!(policy.auto_paused_for_builtin);

        // Reconnecting external without Accessibility permission does not resume.
        assert_eq!(
            policy.evaluate(true, false, false, false, DOCKED),
            BuiltinDisplayAction::None
        );
        assert!(policy.auto_paused_for_builtin);

        // Once Accessibility is trusted and restoration is done, reconnect auto-resumes on 1 sample.
        assert_eq!(
            policy.evaluate(true, false, true, false, DOCKED),
            BuiltinDisplayAction::AutoResume
        );
        assert!(!policy.auto_paused_for_builtin);
    }

    #[test]
    fn single_builtin_only_glitch_does_not_auto_pause_and_resets_debounce() {
        let mut policy = BuiltinDisplayPolicy::default();

        // One transient built-in-only sample (KVM / display reconfiguration) does not pause.
        assert_eq!(
            policy.evaluate(true, true, true, false, BUILTIN_ONLY),
            BuiltinDisplayAction::None
        );
        assert!(!policy.auto_paused_for_builtin);

        // Returning to docked resets the consecutive sample counter.
        assert_eq!(
            policy.evaluate(true, true, true, false, DOCKED),
            BuiltinDisplayAction::None
        );

        // Another isolated single-tick glitch still does not pause.
        assert_eq!(
            policy.evaluate(true, true, true, false, BUILTIN_ONLY),
            BuiltinDisplayAction::None
        );
        assert!(!policy.auto_paused_for_builtin);

        // A second consecutive built-in-only sample confirms the disconnect and pauses.
        assert_eq!(
            policy.evaluate(true, true, true, false, BUILTIN_ONLY),
            BuiltinDisplayAction::AutoPause
        );
        assert!(policy.auto_paused_for_builtin);
    }

    #[test]
    fn clamshell_reconnect_auto_resumes_too() {
        let mut policy = BuiltinDisplayPolicy::default();
        assert_eq!(unplug(&mut policy), BuiltinDisplayAction::AutoPause);
        assert_eq!(
            policy.evaluate(true, false, true, false, CLAMSHELL),
            BuiltinDisplayAction::AutoResume
        );
        assert!(!policy.auto_paused_for_builtin);
    }

    #[test]
    fn manual_pause_on_external_is_never_overridden_on_reconnect() {
        let mut policy = BuiltinDisplayPolicy::default();

        // User manually pauses while docked.
        policy.on_manual_pause();
        assert!(!policy.auto_paused_for_builtin);

        // Unplugging while manually paused does not mark auto-paused.
        assert_eq!(
            policy.evaluate(true, false, true, false, BUILTIN_ONLY),
            BuiltinDisplayAction::None
        );
        assert_eq!(
            policy.evaluate(true, false, true, false, BUILTIN_ONLY),
            BuiltinDisplayAction::None
        );
        assert!(!policy.auto_paused_for_builtin);

        // Reconnecting external display NEVER auto-resumes a manual pause.
        assert_eq!(
            policy.evaluate(true, false, true, false, DOCKED),
            BuiltinDisplayAction::None
        );
        assert!(!policy.auto_paused_for_builtin);
    }

    #[test]
    fn manual_resume_on_builtin_stays_active_until_display_topology_changes() {
        let mut policy = BuiltinDisplayPolicy::default();

        // Auto-paused after unplugging external monitor.
        assert_eq!(unplug(&mut policy), BuiltinDisplayAction::AutoPause);
        assert!(policy.auto_paused_for_builtin);

        // User manually resumes while still on the built-in display.
        assert_eq!(
            policy.on_start(true, true, Some(BUILTIN_ONLY)),
            StartDecision::Start
        );
        assert!(!policy.auto_paused_for_builtin);

        // Subsequent ticks on the same built-in display must NOT re-pause the user.
        for _ in 0..3 {
            assert_eq!(
                policy.evaluate(true, true, true, false, BUILTIN_ONLY),
                BuiltinDisplayAction::None
            );
            assert!(!policy.auto_paused_for_builtin);
        }

        // Connecting an external display changes topology and clears the suppression.
        assert_eq!(
            policy.evaluate(true, true, true, false, DOCKED),
            BuiltinDisplayAction::None
        );

        // Disconnecting the external display again auto-pauses cleanly.
        assert_eq!(unplug(&mut policy), BuiltinDisplayAction::AutoPause);
        assert!(policy.auto_paused_for_builtin);
    }

    #[test]
    fn internal_start_auto_pauses_immediately_on_builtin_without_suppressing_policy() {
        // Granting Accessibility or finishing onboarding on an undocked laptop enters auto-pause
        // immediately rather than tiling or marking a manual resume.
        let mut policy = BuiltinDisplayPolicy::default();
        assert_eq!(
            policy.on_start(false, true, Some(BUILTIN_ONLY)),
            StartDecision::AutoPause
        );
        assert!(policy.auto_paused_for_builtin);

        // Connecting an external display resumes tiling immediately.
        assert_eq!(
            policy.evaluate(true, false, true, false, DOCKED),
            BuiltinDisplayAction::AutoResume
        );
        assert!(!policy.auto_paused_for_builtin);

        // An internal start while docked (e.g. failed-update recovery) starts tiling without
        // suppressing future auto-pause when the external display is later unplugged.
        assert_eq!(
            policy.on_start(false, true, Some(DOCKED)),
            StartDecision::Start
        );
        assert!(!policy.auto_paused_for_builtin);
        assert_eq!(unplug(&mut policy), BuiltinDisplayAction::AutoPause);
        assert!(policy.auto_paused_for_builtin);

        // When the setting is off, an internal start tiles normally on built-in display.
        policy.on_setting_changed(false);
        assert_eq!(
            policy.on_start(false, false, Some(BUILTIN_ONLY)),
            StartDecision::Start
        );
        assert!(!policy.auto_paused_for_builtin);
    }

    #[test]
    fn desktop_mac_with_zero_builtin_displays_never_auto_pauses() {
        let mut policy = BuiltinDisplayPolicy::default();

        for topology in [STUDIO_DUAL, STUDIO_SINGLE, DisplayTopology::default()] {
            assert_eq!(
                policy.evaluate(true, true, true, false, topology),
                BuiltinDisplayAction::None
            );
            assert!(!policy.auto_paused_for_builtin);
        }
        assert!(!policy.should_start_auto_paused(true, Some(STUDIO_SINGLE)));
        assert!(!policy.should_start_auto_paused(true, Some(STUDIO_DUAL)));
    }

    #[test]
    fn disabled_setting_never_auto_pauses_and_clears_auto_pause_state() {
        let mut policy = BuiltinDisplayPolicy::default();

        assert_eq!(
            policy.evaluate(false, true, true, false, BUILTIN_ONLY),
            BuiltinDisplayAction::None
        );
        assert!(!policy.should_start_auto_paused(false, Some(BUILTIN_ONLY)));

        // Auto-pause, then disable the setting: reconnecting must not auto-resume.
        assert_eq!(unplug(&mut policy), BuiltinDisplayAction::AutoPause);
        policy.on_setting_changed(false);
        assert!(!policy.auto_paused_for_builtin);
        assert_eq!(
            policy.evaluate(false, false, true, false, DOCKED),
            BuiltinDisplayAction::None
        );
    }

    #[test]
    fn startup_auto_pause_sets_flag_only_when_enabled_and_builtin_only() {
        let mut policy = BuiltinDisplayPolicy::default();
        assert!(!policy.should_start_auto_paused(true, Some(DOCKED)));
        assert!(!policy.auto_paused_for_builtin);

        assert!(policy.should_start_auto_paused(true, Some(BUILTIN_ONLY)));
        assert!(policy.auto_paused_for_builtin);

        // Plugging in an external display later resumes cleanly.
        assert_eq!(
            policy.evaluate(true, false, true, false, DOCKED),
            BuiltinDisplayAction::AutoResume
        );
        assert!(!policy.auto_paused_for_builtin);
    }
}
