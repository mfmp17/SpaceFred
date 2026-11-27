//! `[labs] window_overlay`: holding the shortcuts' base modifier and Shift (`⌥⇧` by default)
//! shows a card over every visible window. Pure and cross-platform: the main thread samples the
//! modifier flags and mouse buttons each tick (no Accessibility), and reports key presses and
//! hotkeys as interruptions.

use std::time::{Duration, Instant};

use crate::shortcuts::Modifiers;

/// How long the chord must be held, alone, before the overlay shows. A shortcut such as
/// `⌥⇧1` lands well inside it, so it never flashes the overlay.
pub const HOLD_DELAY: Duration = Duration::from_millis(180);

/// What a sample or an interruption changed.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HoldChange {
    Show,
    Hide,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum Phase {
    /// The chord isn't held.
    #[default]
    Idle,
    /// The chord is held alone, since this moment.
    Armed(Instant),
    Shown,
    /// This hold went to something else (a key, another modifier, a mouse button): the overlay
    /// stays away until the chord is let go.
    Spent,
}

/// Whether the chord has been held long enough, alone, to show the overlay.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct OverlayHold {
    phase: Phase,
}

impl OverlayHold {
    #[must_use]
    pub const fn new() -> Self {
        Self { phase: Phase::Idle }
    }

    /// Whether the overlay is up.
    #[must_use]
    pub const fn is_shown(self) -> bool {
        matches!(self.phase, Phase::Shown)
    }

    /// Whether a sample can change anything: the chord is held, or something is.
    #[must_use]
    pub const fn is_idle(self) -> bool {
        matches!(self.phase, Phase::Idle)
    }

    /// One tick's reading: whether the overlay may show now (`enabled`: tiling and the Lab are
    /// on), the chord, the modifiers held, and whether a mouse button is down.
    pub fn sample(
        &mut self,
        now: Instant,
        enabled: bool,
        chord: Modifiers,
        held: Modifiers,
        mouse_down: bool,
    ) -> Option<HoldChange> {
        let was_shown = self.is_shown();
        self.phase = match self.phase {
            _ if !enabled || !contains(held, chord) => Phase::Idle,
            Phase::Idle if held == chord && !mouse_down => Phase::Armed(now),
            Phase::Idle => Phase::Spent,
            Phase::Armed(_) if held != chord || mouse_down => Phase::Spent,
            Phase::Armed(since) if now.duration_since(since) >= HOLD_DELAY => Phase::Shown,
            // A click on a card holds a button: the overlay stays up.
            phase @ (Phase::Armed(_) | Phase::Shown | Phase::Spent) => phase,
        };
        change(was_shown, self.is_shown())
    }

    /// A key was pressed or a hotkey fired: this hold was a shortcut, so the overlay hides, or
    /// never shows, until the chord is pressed again.
    pub fn interrupt(&mut self) -> Option<HoldChange> {
        let was_shown = self.is_shown();
        self.phase = Phase::Spent;
        change(was_shown, false)
    }
}

const fn change(was_shown: bool, shown: bool) -> Option<HoldChange> {
    match (was_shown, shown) {
        (false, true) => Some(HoldChange::Show),
        (true, false) => Some(HoldChange::Hide),
        _ => None,
    }
}

/// Whether every modifier in `chord` is held.
const fn contains(held: Modifiers, chord: Modifiers) -> bool {
    (!chord.command || held.command)
        && (!chord.control || held.control)
        && (!chord.option || held.option)
        && (!chord.shift || held.shift)
}

#[cfg(test)]
mod tests {
    use super::*;

    const OPTION_SHIFT: Modifiers = Modifiers::OPTION.with_shift();
    const NONE: Modifiers = Modifiers {
        command: false,
        control: false,
        option: false,
        shift: false,
    };

    /// Samples `held` at `millis` after `start`, with the Lab on and no button down.
    fn at(
        hold: &mut OverlayHold,
        start: Instant,
        millis: u64,
        held: Modifiers,
    ) -> Option<HoldChange> {
        hold.sample(
            start + Duration::from_millis(millis),
            true,
            OPTION_SHIFT,
            held,
            false,
        )
    }

    #[test]
    fn holding_the_chord_alone_shows_after_the_delay_and_letting_go_hides() {
        let start = Instant::now();
        let mut hold = OverlayHold::default();
        assert_eq!(at(&mut hold, start, 0, OPTION_SHIFT), None);
        assert_eq!(at(&mut hold, start, 179, OPTION_SHIFT), None);
        assert_eq!(
            at(&mut hold, start, 180, OPTION_SHIFT),
            Some(HoldChange::Show)
        );
        // Clicking a card holds a button: still up.
        let later = start + Duration::from_millis(300);
        assert_eq!(
            hold.sample(later, true, OPTION_SHIFT, OPTION_SHIFT, true),
            None
        );
        assert!(hold.is_shown());
        // Lifting Shift alone is letting go.
        assert_eq!(
            at(&mut hold, start, 400, Modifiers::OPTION),
            Some(HoldChange::Hide)
        );
        assert!(hold.is_idle());
        // Pressed again, it waits the whole delay again.
        assert_eq!(at(&mut hold, start, 500, OPTION_SHIFT), None);
        assert_eq!(at(&mut hold, start, 679, OPTION_SHIFT), None);
        assert_eq!(
            at(&mut hold, start, 680, OPTION_SHIFT),
            Some(HoldChange::Show)
        );
    }

    #[test]
    fn a_shortcut_inside_the_delay_never_shows_the_overlay() {
        let start = Instant::now();
        let mut hold = OverlayHold::default();
        at(&mut hold, start, 0, OPTION_SHIFT);
        // ⌥⇧1 fires.
        assert_eq!(hold.interrupt(), None);
        for millis in [200, 400, 1000] {
            assert_eq!(
                at(&mut hold, start, millis, OPTION_SHIFT),
                None,
                "spent until let go"
            );
        }
        at(&mut hold, start, 1100, NONE);
        at(&mut hold, start, 1200, OPTION_SHIFT);
        assert_eq!(
            at(&mut hold, start, 1380, OPTION_SHIFT),
            Some(HoldChange::Show)
        );
    }

    #[test]
    fn a_shortcut_reported_before_the_chord_is_sampled_still_spends_it() {
        // ⌥⇧1 pressed and its hotkey delivered within one tick, before the first sample.
        let start = Instant::now();
        let mut hold = OverlayHold::default();
        assert_eq!(hold.interrupt(), None);
        at(&mut hold, start, 0, OPTION_SHIFT);
        assert_eq!(at(&mut hold, start, 500, OPTION_SHIFT), None);
    }

    #[test]
    fn a_key_while_shown_hides_until_let_go() {
        let start = Instant::now();
        let mut hold = OverlayHold::default();
        at(&mut hold, start, 0, OPTION_SHIFT);
        at(&mut hold, start, 200, OPTION_SHIFT);
        assert_eq!(hold.interrupt(), Some(HoldChange::Hide));
        assert!(!hold.is_shown());
        assert_eq!(at(&mut hold, start, 600, OPTION_SHIFT), None);
    }

    #[test]
    fn other_modifiers_or_a_held_button_spend_the_hold() {
        let start = Instant::now();
        let command = Modifiers {
            command: true,
            ..OPTION_SHIFT
        };
        let control = Modifiers {
            control: true,
            ..OPTION_SHIFT
        };
        for extra in [command, control] {
            let mut hold = OverlayHold::default();
            at(&mut hold, start, 0, extra);
            assert_eq!(
                at(&mut hold, start, 300, extra),
                None,
                "{extra:?} from the start"
            );
            let mut hold = OverlayHold::default();
            at(&mut hold, start, 0, OPTION_SHIFT);
            at(&mut hold, start, 50, extra);
            assert_eq!(
                at(&mut hold, start, 300, OPTION_SHIFT),
                None,
                "{extra:?} added inside the delay"
            );
        }
        // A drag started with the chord held is not a hold.
        let mut hold = OverlayHold::default();
        hold.sample(start, true, OPTION_SHIFT, OPTION_SHIFT, true);
        assert_eq!(at(&mut hold, start, 300, OPTION_SHIFT), None);
        // Option alone is not the chord.
        let mut hold = OverlayHold::default();
        at(&mut hold, start, 0, Modifiers::OPTION);
        assert_eq!(at(&mut hold, start, 300, Modifiers::OPTION), None);
    }

    #[test]
    fn the_lab_off_or_tiling_paused_never_shows_and_hides_a_shown_overlay() {
        let start = Instant::now();
        let mut hold = OverlayHold::default();
        let later = |millis| start + Duration::from_millis(millis);
        hold.sample(start, false, OPTION_SHIFT, OPTION_SHIFT, false);
        assert_eq!(
            hold.sample(later(500), false, OPTION_SHIFT, OPTION_SHIFT, false),
            None
        );
        at(&mut hold, start, 600, OPTION_SHIFT);
        at(&mut hold, start, 800, OPTION_SHIFT);
        assert!(hold.is_shown());
        assert_eq!(
            hold.sample(later(900), false, OPTION_SHIFT, OPTION_SHIFT, false),
            Some(HoldChange::Hide)
        );
    }

    #[test]
    fn any_base_modifier_with_shift_is_a_chord() {
        let chord = Modifiers {
            control: true,
            ..NONE
        }
        .with_shift();
        let start = Instant::now();
        let mut hold = OverlayHold::default();
        hold.sample(start, true, chord, chord, false);
        let later = start + HOLD_DELAY;
        assert_eq!(
            hold.sample(later, true, chord, chord, false),
            Some(HoldChange::Show)
        );
    }
}
