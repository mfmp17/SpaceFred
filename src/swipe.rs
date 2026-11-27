//! Three- and four-finger horizontal trackpad swipes, as workspace steps.

/// The most fingers a swipe may have; a fifth finger spends the gesture.
const MAX_FINGERS: usize = 4;
/// How far the fingers travel, as a fraction of the trackpad's width.
const DISTANCE: f64 = 0.15;
/// Horizontal travel must beat vertical travel by this much, so Mission Control's
/// upward swipe never switches.
const DOMINANCE: f64 = 2.0;
/// How far three or four fingers must move horizontally (as a fraction of the
/// trackpad's width) before Spaceman treats the touch as a macOS Space slide,
/// ignoring stationary taps and resting palms.
pub const SPACE_SLIDE_DISTANCE: f64 = 0.015;
/// A gap this long between frames means the swipe's end was missed.
const STALE_SECONDS: f64 = 0.5;
/// How long reports with fewer contacts than the gesture had (or none), and no
/// contact ending, are taken as partial reports and skipped before the lower
/// count is believed (a lift whose end event was lost).
const PARTIAL_HOLD_SECONDS: f64 = 0.15;

fn centroid(touches: &[(f64, f64)]) -> (f64, f64) {
    let count = touches.len() as f64;
    let x = touches.iter().map(|touch| touch.0).sum::<f64>() / count;
    let y = touches.iter().map(|touch| touch.1).sum::<f64>() / count;
    (x, y)
}

/// How many fingers made a swipe. Spaceman steps workspaces on the count
/// `Config::swipe_fingers` picks and leaves the other to macOS.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Fingers {
    Three,
    Four,
}

impl Fingers {
    const fn of(count: usize) -> Option<Self> {
        match count {
            3 => Some(Self::Three),
            4 => Some(Self::Four),
            _ => None,
        }
    }
}

/// Which way a swipe steps: `Left` goes to the next workspace and `Right` to the
/// previous one. The tracker reports the fingers' direction; see
/// [`Swipe::for_scroll_direction`] for how that becomes a step.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Swipe {
    Left,
    Right,
}

impl Swipe {
    /// Maps the fingers' direction to a workspace step the way macOS Spaces does.
    /// With Natural scrolling on the fingers drag the desktop strip, so swiping
    /// left goes to the next workspace; with it off they move the view, so
    /// swiping right does.
    #[must_use]
    pub const fn for_scroll_direction(self, natural: bool) -> Self {
        match (self, natural) {
            (swipe, true) => swipe,
            (Self::Left, false) => Self::Right,
            (Self::Right, false) => Self::Left,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
enum State {
    #[default]
    Ready,
    Tracking {
        fingers: Fingers,
        x: f64,
        y: f64,
    },
    /// Fired, a fifth finger landed, or a finger lifted from four: nothing more
    /// until every finger lifts.
    Spent,
}

/// Fires at most once per swipe, from the touching fingers' normalized positions.
#[derive(Debug, Default)]
pub struct SwipeTracker {
    state: State,
    last_frame: Option<f64>,
}

impl SwipeTracker {
    /// Notes an event at `at` whose touches were skipped as a partial report,
    /// so the gap before the next frame is not mistaken for a missed lift.
    pub fn hold(&mut self, at: f64) {
        self.last_frame = Some(at);
    }

    /// `at` is the frame's timestamp in seconds; `touches` are the fingers on the
    /// trackpad, each `(x, y)` from 0 to 1. Three fingers joined by a fourth
    /// start over as a four-finger swipe; four dropping to three never fires.
    pub fn frame(&mut self, at: f64, touches: &[(f64, f64)]) -> Option<(Swipe, Fingers)> {
        if self
            .last_frame
            .is_some_and(|last| at - last > STALE_SECONDS)
        {
            self.state = State::Ready;
        }
        self.last_frame = Some(at);
        if touches.is_empty() {
            self.state = State::Ready;
            return None;
        }
        if touches.len() > MAX_FINGERS {
            self.state = State::Spent;
            return None;
        }
        if self.state == State::Spent {
            return None;
        }
        let Some(fingers) = Fingers::of(touches.len()) else {
            self.state = State::Ready;
            return None;
        };
        let (x, y) = centroid(touches);
        let (start_x, start_y) = match self.state {
            State::Tracking {
                fingers: tracked,
                x,
                y,
            } if tracked == fingers => (x, y),
            State::Tracking {
                fingers: Fingers::Four,
                ..
            } => {
                self.state = State::Spent;
                return None;
            }
            _ => {
                self.state = State::Tracking { fingers, x, y };
                return None;
            }
        };
        let (dx, dy) = (x - start_x, y - start_y);
        if dx.abs() < DISTANCE || dx.abs() < DOMINANCE * dy.abs() {
            return None;
        }
        self.state = State::Spent;
        let swipe = if dx < 0.0 { Swipe::Left } else { Swipe::Right };
        Some((swipe, fingers))
    }
}

/// Watches a touch for the horizontal three- or four-finger slide macOS uses to
/// change desktops. It runs apart from [`SwipeTracker`], whose spent state (a
/// fired swipe, a fifth contact, four fingers dropping to three) must not stop
/// macOS's own slide from holding layout (#677): a resting thumb that lifts or
/// a palm that brushes the trackpad mid-swipe still leaves macOS sliding.
#[derive(Debug, Default)]
pub struct SpaceSlide {
    /// Finger count and centroid where the current three- or four-finger set
    /// landed.
    anchor: Option<(usize, f64, f64)>,
}

impl SpaceSlide {
    /// Forgets the current touch (fingers lifted, gesture stale, pointer drag).
    pub fn reset(&mut self) {
        self.anchor = None;
    }

    /// Feeds one frame of touching fingers. Returns whether three or four
    /// fingers have moved horizontally past [`SPACE_SLIDE_DISTANCE`] since that
    /// many landed, with horizontal travel exceeding vertical travel. Any other
    /// count forgets the anchor, and a change of count starts a new one there.
    pub fn frame(&mut self, touches: &[(f64, f64)]) -> bool {
        if !matches!(touches.len(), 3 | 4) {
            self.anchor = None;
            return false;
        }
        let (x, y) = centroid(touches);
        match self.anchor {
            Some((count, start_x, start_y)) if count == touches.len() => {
                let (dx, dy) = ((x - start_x).abs(), (y - start_y).abs());
                dx >= SPACE_SLIDE_DISTANCE && dx > dy
            }
            _ => {
                self.anchor = Some((touches.len(), x, y));
                false
            }
        }
    }
}

/// One trackpad gesture event, as AppKit reports its touches.
#[derive(Clone, Copy, Debug)]
pub struct TouchReport<'a> {
    /// Positions of the contacts the event reports as touching.
    pub touching: &'a [(f64, f64)],
    /// How many of those contacts just began.
    pub began: usize,
    /// Whether the event reports any contact ending or being cancelled.
    pub lifted: bool,
}

/// A report the gesture should act on.
#[derive(Clone, Debug, PartialEq)]
pub struct AcceptedTouches {
    /// The fingers on the trackpad were replaced by freshly landed ones without
    /// the old ones' lift being reported: end the old gesture first.
    pub restart: bool,
    pub touches: Vec<(f64, f64)>,
}

/// Turns AppKit's gesture events into the frames [`SwipeTracker`] and
/// [`SpaceSlide`] read (#781). Mid-swipe, macOS sends events with no touching
/// contacts, or only some of them, while the fingers stay down. Taking those as
/// lifts restarted the swipe from wherever the fingers were next, so it rarely
/// travelled far enough to switch. A reported end lifts a contact at once; an
/// empty or short report is skipped until it persists for
/// `PARTIAL_HOLD_SECONDS`, so a lift whose end was lost still frees the next
/// swipe. Fingers that all just landed while others were thought down also mean
/// the old lift was missed.
#[derive(Debug, Default)]
pub struct TouchFilter {
    /// Contacts in the last accepted report.
    count: usize,
    /// When reports started showing fewer contacts with none ending.
    short_since: Option<f64>,
    last_report: Option<f64>,
}

impl TouchFilter {
    /// Forgets the current touch.
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    /// Returns the touches to act on, or `None` to skip a partial report.
    pub fn report(&mut self, at: f64, report: TouchReport<'_>) -> Option<AcceptedTouches> {
        if self
            .last_report
            .is_some_and(|last| at - last > STALE_SECONDS)
        {
            self.count = 0;
            self.short_since = None;
        }
        self.last_report = Some(at);
        let count = report.touching.len();
        let restart = self.count > 0 && count > 0 && report.began == count;
        if !(restart || report.lifted || count >= self.count) {
            let since = *self.short_since.get_or_insert(at);
            if at - since < PARTIAL_HOLD_SECONDS {
                return None;
            }
        }
        self.count = count;
        self.short_since = None;
        Some(AcceptedTouches {
            restart,
            touches: report.touching.to_vec(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fingers(x: f64, y: f64, count: usize) -> Vec<(f64, f64)> {
        (0..count)
            .map(|finger| (x + finger as f64 * 0.05, y))
            .collect()
    }

    /// Feeds a swipe from `from` to `to` in small steps, 10 ms apart.
    fn drag(
        tracker: &mut SwipeTracker,
        start: f64,
        from: (f64, f64),
        to: (f64, f64),
        count: usize,
    ) -> Vec<(Swipe, Fingers)> {
        (0..=20)
            .filter_map(|step| {
                let t = f64::from(step) / 20.0;
                let x = from.0 + (to.0 - from.0) * t;
                let y = from.1 + (to.1 - from.1) * t;
                tracker.frame(start + f64::from(step) * 0.01, &fingers(x, y, count))
            })
            .collect()
    }

    #[test]
    fn three_fingers_left_and_right_fire_once_each() {
        let mut tracker = SwipeTracker::default();
        assert_eq!(
            drag(&mut tracker, 0.0, (0.6, 0.5), (0.2, 0.5), 3),
            [(Swipe::Left, Fingers::Three)]
        );
        assert_eq!(tracker.frame(0.3, &[]), None);
        assert_eq!(
            drag(&mut tracker, 0.4, (0.2, 0.5), (0.6, 0.5), 3),
            [(Swipe::Right, Fingers::Three)]
        );
    }

    #[test]
    fn four_finger_swipes_report_four_fingers() {
        let mut tracker = SwipeTracker::default();
        assert_eq!(
            drag(&mut tracker, 0.0, (0.6, 0.5), (0.1, 0.5), 4),
            [(Swipe::Left, Fingers::Four)]
        );
        tracker.frame(0.3, &[]);
        // Three fingers land first, then the fourth: one four-finger swipe.
        tracker.frame(0.4, &fingers(0.2, 0.5, 3));
        assert_eq!(
            drag(&mut tracker, 0.41, (0.2, 0.5), (0.6, 0.5), 4),
            [(Swipe::Right, Fingers::Four)]
        );
    }

    #[test]
    fn short_vertical_and_five_finger_swipes_do_nothing() {
        let mut tracker = SwipeTracker::default();
        assert!(drag(&mut tracker, 0.0, (0.5, 0.5), (0.4, 0.5), 3).is_empty());
        tracker.frame(0.3, &[]);
        assert!(drag(&mut tracker, 0.4, (0.5, 0.2), (0.3, 0.8), 3).is_empty());
        tracker.frame(0.7, &[]);
        assert!(drag(&mut tracker, 0.8, (0.5, 0.2), (0.3, 0.8), 4).is_empty());
        tracker.frame(1.1, &[]);
        assert!(drag(&mut tracker, 1.2, (0.6, 0.5), (0.1, 0.5), 5).is_empty());
    }

    #[test]
    fn lifting_from_four_fingers_to_three_does_not_fire() {
        let mut tracker = SwipeTracker::default();
        tracker.frame(0.0, &fingers(0.5, 0.5, 4));
        assert!(drag(&mut tracker, 0.01, (0.6, 0.5), (0.1, 0.5), 3).is_empty());
    }

    #[test]
    fn a_missed_lift_does_not_block_the_next_swipe() {
        let mut tracker = SwipeTracker::default();
        assert_eq!(drag(&mut tracker, 0.0, (0.6, 0.5), (0.2, 0.5), 3).len(), 1);
        assert_eq!(
            drag(&mut tracker, 2.0, (0.6, 0.5), (0.2, 0.5), 3),
            [(Swipe::Left, Fingers::Three)]
        );
    }

    #[test]
    fn swipes_follow_the_natural_scrolling_setting() {
        assert_eq!(Swipe::Left.for_scroll_direction(true), Swipe::Left);
        assert_eq!(Swipe::Right.for_scroll_direction(true), Swipe::Right);
        assert_eq!(Swipe::Left.for_scroll_direction(false), Swipe::Right);
        assert_eq!(Swipe::Right.for_scroll_direction(false), Swipe::Left);
    }

    #[test]
    fn horizontal_space_slide_requires_horizontal_travel_above_threshold() {
        let mut slide = SpaceSlide::default();
        assert!(!slide.frame(&fingers(0.5, 0.5, 3)));

        // Sub-threshold horizontal jitter (stationary tap / resting palm).
        assert!(!slide.frame(&fingers(0.51, 0.5, 3)));

        // Upward vertical motion (Mission Control) does not count as horizontal.
        assert!(!slide.frame(&fingers(0.52, 0.55, 3)));

        // Horizontal motion >= 0.015 exceeding vertical motion arms the slide.
        assert!(slide.frame(&fingers(0.52, 0.505, 3)));
    }

    /// #677: a resting thumb lifting (four to three) or a palm brushing the
    /// trackpad (five contacts) spends the workspace swipe, but macOS keeps
    /// sliding between desktops, so the slide still arms from where the new
    /// finger set landed.
    #[test]
    fn space_slide_survives_finger_count_changes_that_spend_a_swipe() {
        let mut tracker = SwipeTracker::default();
        let mut slide = SpaceSlide::default();
        for (at, count) in [(0.0, 4), (0.01, 3), (0.02, 5), (0.03, 3)] {
            let touches = fingers(0.5, 0.5, count);
            assert_eq!(tracker.frame(at, &touches), None);
            assert!(!slide.frame(&touches));
        }
        let moved = fingers(0.45, 0.5, 3);
        assert_eq!(tracker.frame(0.05, &moved), None);
        assert!(slide.frame(&moved));
    }

    /// Feeds one gesture event through the filter to the tracker, ending the
    /// old gesture first when the filter says the fingers were replaced.
    fn feed(
        filter: &mut TouchFilter,
        tracker: &mut SwipeTracker,
        at: f64,
        touching: &[(f64, f64)],
        began: usize,
        lifted: bool,
    ) -> Option<(Swipe, Fingers)> {
        let accepted = filter.report(
            at,
            TouchReport {
                touching,
                began,
                lifted,
            },
        )?;
        if accepted.restart {
            assert_eq!(tracker.frame(at, &[]), None);
        }
        tracker.frame(at, &accepted.touches)
    }

    /// Three fingers land at `from_x` and slide to `to_x` in 0.02 steps,
    /// 10 ms apart, with complete reports.
    fn filtered_sweep(
        filter: &mut TouchFilter,
        tracker: &mut SwipeTracker,
        start: f64,
        from_x: f64,
        to_x: f64,
        landing: bool,
    ) -> Vec<(Swipe, Fingers)> {
        (0..=10)
            .filter_map(|step| {
                let x = from_x + (to_x - from_x) * f64::from(step) / 10.0;
                let began = if step == 0 && landing { 3 } else { 0 };
                feed(
                    filter,
                    tracker,
                    start + f64::from(step) * 0.01,
                    &fingers(x, 0.5, 3),
                    began,
                    false,
                )
            })
            .collect()
    }

    fn lift(filter: &mut TouchFilter, tracker: &mut SwipeTracker, at: f64) {
        assert_eq!(feed(filter, tracker, at, &[], 0, true), None);
    }

    /// #781: macOS reports no touching contacts, or only some, between complete
    /// reports while the fingers stay down. Those must not restart the swipe.
    #[test]
    fn empty_and_partial_reports_mid_swipe_keep_its_origin() {
        let reports: Vec<_> = (1..=20)
            .map(|step| {
                let x = 0.6 - f64::from(step) * 0.01;
                match step % 3 {
                    0 => Vec::new(),
                    1 => fingers(x, 0.5, 2),
                    _ => fingers(x, 0.5, 3),
                }
            })
            .collect();

        let mut filter = TouchFilter::default();
        let mut tracker = SwipeTracker::default();
        let mut swipes: Vec<_> = feed(
            &mut filter,
            &mut tracker,
            0.0,
            &fingers(0.6, 0.5, 3),
            3,
            false,
        )
        .into_iter()
        .collect();
        for (step, touches) in (1..).zip(&reports) {
            let at = f64::from(step) * 0.01;
            swipes.extend(feed(&mut filter, &mut tracker, at, touches, 0, false));
        }
        assert_eq!(swipes, [(Swipe::Left, Fingers::Three)]);

        // Read as lifts, the same reports restart the swipe on every complete
        // one and it never travels far enough.
        let mut raw = SwipeTracker::default();
        assert_eq!(raw.frame(0.0, &fingers(0.6, 0.5, 3)), None);
        for (step, touches) in (1..).zip(&reports) {
            assert_eq!(raw.frame(f64::from(step) * 0.01, touches), None);
        }
    }

    #[test]
    fn fresh_fingers_after_a_missed_lift_start_a_new_swipe() {
        let mut filter = TouchFilter::default();
        let mut tracker = SwipeTracker::default();
        assert_eq!(
            filtered_sweep(&mut filter, &mut tracker, 0.0, 0.6, 0.4, true),
            [(Swipe::Left, Fingers::Three)]
        );
        // The lift was never reported; three fingers land again 50 ms later.
        assert_eq!(
            filtered_sweep(&mut filter, &mut tracker, 0.15, 0.4, 0.6, true),
            [(Swipe::Right, Fingers::Three)]
        );
    }

    #[test]
    fn the_same_fingers_resuming_after_a_pause_switch_once_until_they_lift() {
        let mut filter = TouchFilter::default();
        let mut tracker = SwipeTracker::default();
        assert_eq!(
            filtered_sweep(&mut filter, &mut tracker, 0.0, 0.6, 0.4, true),
            [(Swipe::Left, Fingers::Three)]
        );
        // A short pause, then the same fingers slide on: still one switch.
        assert!(filtered_sweep(&mut filter, &mut tracker, 0.4, 0.4, 0.2, false).is_empty());
        lift(&mut filter, &mut tracker, 0.55);
        assert_eq!(
            filtered_sweep(&mut filter, &mut tracker, 0.65, 0.6, 0.4, true),
            [(Swipe::Left, Fingers::Three)]
        );
    }

    #[test]
    fn fewer_contacts_are_believed_once_they_persist() {
        let mut filter = TouchFilter::default();
        fn report(touching: &[(f64, f64)]) -> TouchReport<'_> {
            TouchReport {
                touching,
                began: 0,
                lifted: false,
            }
        }
        let three = fingers(0.5, 0.5, 3);
        let two = fingers(0.5, 0.5, 2);
        assert!(filter.report(0.0, report(&three)).is_some());
        assert_eq!(filter.report(0.01, report(&two)), None);
        assert_eq!(filter.report(0.1, report(&two)), None);
        assert_eq!(
            filter.report(0.17, report(&two)),
            Some(AcceptedTouches {
                restart: false,
                touches: two.clone(),
            })
        );
        // A brief empty report does not lift; a reported end does.
        assert_eq!(filter.report(0.2, report(&[])), None);
        assert!(
            filter
                .report(
                    0.21,
                    TouchReport {
                        touching: &[],
                        began: 0,
                        lifted: true,
                    },
                )
                .is_some_and(|accepted| accepted.touches.is_empty())
        );
    }

    /// A lift whose end was lost, followed by a landing whose start was lost
    /// too, must not swallow the next quick swipe: empty reports that persist
    /// count as the lift.
    #[test]
    fn persistent_empty_reports_count_as_the_missed_lift() {
        let mut filter = TouchFilter::default();
        let mut tracker = SwipeTracker::default();
        assert_eq!(
            filtered_sweep(&mut filter, &mut tracker, 0.0, 0.6, 0.4, true),
            [(Swipe::Left, Fingers::Three)]
        );
        for step in 1..=20 {
            let at = 0.1 + f64::from(step) * 0.01;
            assert_eq!(feed(&mut filter, &mut tracker, at, &[], 0, false), None);
        }
        assert_eq!(
            filtered_sweep(&mut filter, &mut tracker, 0.32, 0.4, 0.6, false),
            [(Swipe::Right, Fingers::Three)]
        );
    }
}
