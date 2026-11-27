//! macOS Space slides (Mission Control desktops, native fullscreen). While one
//! runs, windows from the outgoing and incoming Spaces are on screen together
//! and Accessibility writes to the incoming Space's windows can fail, so
//! background cycles wait for the slide to settle instead of laying out a mix
//! of both Spaces (#334) or rolling back a focus follow and re-homing the
//! window it revealed (#329), and a parked window macOS puts back on screen
//! when an app leaves its fullscreen Space is parked again, not re-homed (#489).

use super::focus::FAILED_FOLLOW_RETRY_WINDOW;
use super::*;

/// How long after a Space slide settles the windows it brought on screen are
/// taken as the display's whole desktop (#431).
const SLIDE_ARRIVAL_WINDOW: Duration = Duration::from_secs(2);

impl<B: WorkspaceBackend> WorkspaceRuntime<B> {
    /// When the macOS Space slide under way settles, while one is. A trackpad
    /// swipe between desktops brings the arriving desktop's windows on screen
    /// while the fingers move, before macOS reports the change and even when
    /// the swipe is reversed, so three or four fingers on the trackpad hold
    /// layout too, until a settle after they lift (#513). macOS reports the
    /// change about a second after they lift, so that settle is longer than
    /// the one after a reported change (#572).
    fn space_transition_settled_at(&mut self) -> Option<Instant> {
        let changed = self
            .backend
            .active_space_changed_at()
            .map(|at| at + SPACE_TRANSITION_SETTLE);
        let gesture = self
            .backend
            .space_gesture_at()
            .map(|at| at + SPACE_GESTURE_SETTLE);
        changed.max(gesture)
    }

    pub(super) fn space_transition_until(&mut self) -> Option<Instant> {
        let until = self.space_transition_settled_at()?;
        (self.backend.now() < until).then_some(until)
    }

    /// Whether a macOS Space slide settled moments ago. The first cycles after
    /// it see the desktop it brought on screen; a window it ordered out is on
    /// the desktop it left, not a flaky snapshot (#431).
    pub(super) fn space_slide_just_settled(&mut self) -> bool {
        let Some(settled) = self.space_transition_settled_at() else {
            return false;
        };
        self.backend.now() < settled + SLIDE_ARRIVAL_WINDOW
    }

    /// Whether a parked window of the frontmost app that is back on screen was
    /// put there by macOS rather than summoned by the user. Leaving an app's
    /// native fullscreen Space moves its other windows back to their last
    /// on-screen frames (#489); the window the user lands on is the focused one.
    pub(super) fn returned_by_space_slide(&mut self, window: &crate::PhysicalWindowId) -> bool {
        self.space_slide_just_settled() && self.backend.focused_window().as_ref() != Some(window)
    }

    /// The status of a background cycle held back by a Space slide. Nothing
    /// was observed or written; the app loop retiles once the slide settles.
    pub(super) fn defer_for_space_transition(
        &self,
        until: Instant,
        outputs: &macos::DisplayOutputs,
    ) -> CycleStatus {
        log_event!(
            "space transition: holding layout for {}ms",
            until
                .saturating_duration_since(self.backend.now())
                .as_millis()
        );
        let mut status = self.status(0, outputs);
        status.close_recheck = Some(until);
        status
    }

    /// A focus follow that failed while macOS was sliding between Spaces
    /// (Cmd+Tab out of a native fullscreen app) failed because of the slide,
    /// not the workspace. Lift the focus suppression its rollback set so the
    /// next cycle follows again, instead of leaving the revealed window for
    /// heal to re-home onto the workspace on screen. Returns whether it did.
    pub(super) fn retry_follow_after_space_change(&mut self) -> bool {
        let Some(started) = self.focus.failed_follow_at() else {
            return false;
        };
        if self.backend.now().saturating_duration_since(started) >= FAILED_FOLLOW_RETRY_WINDOW {
            self.focus.forget_failed_follow();
            return false;
        }
        let Some(changed) = self.backend.active_space_changed_at() else {
            return false;
        };
        if changed + SPACE_TRANSITION_SETTLE <= started {
            // That slide had settled before the follow began.
            return false;
        }
        self.focus.forget_failed_follow();
        log_event!("focus follow: retrying after a macOS Space change interrupted it");
        self.clear_suppressed_focus();
        true
    }
}
