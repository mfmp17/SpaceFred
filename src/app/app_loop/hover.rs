//! Focus follows mouse (`[labs] focus_follows_mouse`), the main thread's half:
//! notice the pointer coming to rest after it moved, ask the window server
//! (Core Graphics only, no Accessibility) which window is in front there, and
//! queue that window for the worker, which decides and focuses.

use super::*;

/// How long the pointer must rest before the tile under it takes focus. Long
/// enough that passing over a tile on the way to the menu bar never switches
/// the app the menu bar belongs to.
pub(super) const HOVER_DELAY: Duration = Duration::from_millis(150);

/// Where the pointer rests and whether it moved there since the last hover.
/// Only a pointer that moved and then rested counts: a resting pointer never
/// pulls focus back after a keyboard focus change or a workspace switch.
#[derive(Debug, Default)]
pub(super) struct HoverTracker {
    /// The pointer's last position and since when it has been there.
    rest: Option<((i32, i32), Instant)>,
    /// The pointer moved since the last hover or interruption.
    armed: bool,
}

impl HoverTracker {
    /// One pointer sample. Returns the point once the pointer has rested there
    /// for [`HOVER_DELAY`] after moving, and not again until it moves.
    pub(super) fn observe(
        &mut self,
        point: Option<(i32, i32)>,
        now: Instant,
    ) -> Option<(i32, i32)> {
        let Some(point) = point else {
            self.interrupt();
            return None;
        };
        match self.rest {
            Some((rested, since)) if rested == point => {
                if self.armed && now.saturating_duration_since(since) >= HOVER_DELAY {
                    self.armed = false;
                    return Some(point);
                }
            }
            Some(_) => {
                self.rest = Some((point, now));
                self.armed = true;
            }
            // The first sample, or the first after an interruption, is where
            // the pointer already was: no movement yet.
            None => self.rest = Some((point, now)),
        }
        None
    }

    /// A held button, a drag or a Lab change: wait for the pointer to move again.
    pub(super) const fn interrupt(&mut self) {
        self.rest = None;
        self.armed = false;
    }

    /// `action` was just queued. A keyboard or `ctl` focus change, or a
    /// workspace switch, chose focus after the pointer last moved: a rest still
    /// pending from that movement never takes focus back (#732).
    pub(super) const fn note_action(&mut self, action: &WorkerAction) {
        if matches!(
            super::super::commands::CommandOrigin::of(action),
            super::super::commands::CommandOrigin::Explicit
        ) {
            self.interrupt();
        }
    }
}

impl MainThreadAgent {
    /// Queues a hover focus when the pointer comes to rest over another window.
    pub(super) fn tick_hover(&mut self, mouse_down: bool) {
        if !self.focus_follows_mouse {
            return;
        }
        if mouse_down || self.gesture_tracker.is_active() || macos::any_mouse_button_down() {
            self.hover.interrupt();
            return;
        }
        let Some(point) = self.hover.observe(macos::mouse_position(), Instant::now()) else {
            return;
        };
        if let Some(window_id) = macos::hover_window_at(point) {
            self.queue_worker_input(QueuedInput::Action(WorkerAction::FocusHovered {
                window_id,
                point,
            }));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_pointer_that_moved_and_rested_hovers_once() {
        let start = Instant::now();
        let at = |ms| start + Duration::from_millis(ms);
        let mut hover = HoverTracker::default();
        // Resting where it started: no movement, no hover.
        assert_eq!(hover.observe(Some((10, 10)), at(0)), None);
        assert_eq!(hover.observe(Some((10, 10)), at(500)), None);
        // Moving, then passing through quickly.
        assert_eq!(hover.observe(Some((50, 10)), at(525)), None);
        assert_eq!(hover.observe(Some((90, 10)), at(550)), None);
        assert_eq!(hover.observe(Some((90, 10)), at(650)), None);
        // Rested long enough: one hover, then nothing until it moves again.
        assert_eq!(hover.observe(Some((90, 10)), at(700)), Some((90, 10)));
        assert_eq!(hover.observe(Some((90, 10)), at(2000)), None);
        assert_eq!(hover.observe(Some((95, 10)), at(2025)), None);
        assert_eq!(hover.observe(Some((95, 10)), at(2175)), Some((95, 10)));
    }

    #[test]
    fn an_interruption_waits_for_the_pointer_to_move_again() {
        let start = Instant::now();
        let at = |ms| start + Duration::from_millis(ms);
        let mut hover = HoverTracker::default();
        hover.observe(Some((0, 0)), at(0));
        hover.observe(Some((5, 0)), at(25));
        // A button went down (a click or a drag) before the rest finished.
        hover.interrupt();
        assert_eq!(hover.observe(Some((40, 0)), at(50)), None);
        assert_eq!(hover.observe(Some((40, 0)), at(400)), None);
        assert_eq!(hover.observe(Some((41, 0)), at(425)), None);
        assert_eq!(hover.observe(Some((41, 0)), at(575)), Some((41, 0)));
    }

    #[test]
    fn a_focus_command_during_the_rest_cancels_the_pending_hover() {
        // #732: the pointer moved onto a tile, and a direction command chose
        // focus before the rest finished.
        let start = Instant::now();
        let at = |ms| start + Duration::from_millis(ms);
        let mut hover = HoverTracker::default();
        hover.observe(Some((350, 620)), at(0));
        assert_eq!(hover.observe(Some((1770, 575)), at(25)), None);
        hover.note_action(&WorkerAction::Focus(crate::Direction::Right));
        hover.note_action(&WorkerAction::Focus(crate::Direction::Left));
        assert_eq!(hover.observe(Some((1770, 575)), at(50)), None);
        assert_eq!(hover.observe(Some((1770, 575)), at(700)), None);
        // A fresh move hovers as usual.
        assert_eq!(hover.observe(Some((1780, 575)), at(725)), None);
        assert_eq!(hover.observe(Some((1780, 575)), at(875)), Some((1780, 575)));
        // Actions that choose no focus leave a pending rest alone.
        assert_eq!(hover.observe(Some((1790, 575)), at(900)), None);
        hover.note_action(&WorkerAction::Balance);
        assert_eq!(
            hover.observe(Some((1790, 575)), at(1050)),
            Some((1790, 575))
        );
    }
}
