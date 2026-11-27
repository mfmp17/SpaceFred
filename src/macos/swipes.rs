use std::{
    cell::{Cell, RefCell},
    ffi::c_void,
    ptr::NonNull,
};

use objc2::rc::autoreleasepool;
use objc2_app_kit::{NSEvent, NSEventType, NSTouchPhase};
use objc2_core_foundation::{
    CFMachPort, CFRetained, CFRunLoop, CFRunLoopSource, kCFRunLoopCommonModes,
};
use objc2_core_graphics::{
    CGEvent, CGEventSource, CGEventSourceStateID, CGEventTapLocation, CGEventTapOptions,
    CGEventTapPlacement, CGEventTapProxy, CGEventType, CGMouseButton,
};
use objc2_foundation::{NSString, NSUserDefaults};

use crate::swipe::{Fingers, SpaceSlide, Swipe, SwipeTracker, TouchFilter, TouchReport};

/// A gap this long between gesture frames means the lift was missed (matches
/// `crate::swipe::SwipeTracker`).
const STALE_SECONDS: f64 = 0.5;
/// How often an active horizontal three- or four-finger slide refreshes the
/// Space-gesture hold while fingers remain on the trackpad (avoids 120 Hz
/// cancellations while still keeping long held slides covered).
const SPACE_GESTURE_REFRESH_SECONDS: f64 = 0.5;

/// Gates [`SwipeTracker`] so a pointer drag (including macOS Accessibility
/// "Three Finger Drag") never triggers a workspace swipe.
///
/// Any mouse button press or drag event while fingers are touching suppresses
/// the current touch sequence until every finger lifts (or the gesture goes
/// stale), even if the mouse button releases before the fingers leave the
/// trackpad.
#[derive(Debug, Default)]
struct SwipeGate {
    touches: TouchFilter,
    tracker: SwipeTracker,
    space_slide: SpaceSlide,
    touching: bool,
    suppressed: bool,
    sliding_spaces: bool,
    last_frame: Option<f64>,
    last_space_note: Option<f64>,
    space_gesture_note_due: bool,
}

impl SwipeGate {
    fn reset(&mut self) {
        self.touches.reset();
        self.tracker = SwipeTracker::default();
        self.touching = false;
        self.suppressed = false;
        self.sliding_spaces = false;
        self.space_slide.reset();
        self.last_frame = None;
        self.last_space_note = None;
        self.space_gesture_note_due = false;
    }

    fn pointer_pressed(&mut self) {
        if self.touching {
            self.suppressed = true;
            self.sliding_spaces = false;
            self.space_slide.reset();
            self.last_space_note = None;
            self.space_gesture_note_due = false;
            self.tracker = SwipeTracker::default();
        }
    }

    /// Feeds one gesture event as AppKit reported it. Empty and partial
    /// reports made while the fingers stay down are skipped (#781). While
    /// macOS may be sliding between desktops, a skipped report still refreshes
    /// the Space-gesture hold, as it did when it was read as a lift.
    fn report(
        &mut self,
        at: f64,
        report: TouchReport<'_>,
        pointer_down: bool,
    ) -> Option<(Swipe, Fingers)> {
        let Some(accepted) = self.touches.report(at, report) else {
            // Keep the stale timers in step: a skipped report is not a gap.
            self.last_frame = Some(at);
            self.tracker.hold(at);
            if self.sliding_spaces {
                self.space_gesture_note_due = true;
            }
            return None;
        };
        if accepted.restart {
            // The old fingers' lift was missed: end that gesture first.
            self.frame(at, &[], pointer_down);
        }
        self.frame(at, &accepted.touches, pointer_down)
    }

    fn frame(
        &mut self,
        at: f64,
        touches: &[(f64, f64)],
        pointer_down: bool,
    ) -> Option<(Swipe, Fingers)> {
        if self
            .last_frame
            .is_some_and(|last| at - last > STALE_SECONDS)
        {
            self.suppressed = false;
            self.touching = false;
            self.sliding_spaces = false;
            self.space_slide.reset();
            self.last_space_note = None;
            self.space_gesture_note_due = false;
        }
        self.last_frame = Some(at);
        if touches.is_empty() {
            let was_sliding = self.sliding_spaces;
            self.touching = false;
            self.suppressed = false;
            self.sliding_spaces = false;
            self.space_slide.reset();
            self.last_space_note = None;
            if was_sliding {
                self.space_gesture_note_due = true;
            }
            return self.tracker.frame(at, touches);
        }
        self.touching = true;
        if pointer_down {
            self.suppressed = true;
            self.sliding_spaces = false;
            self.space_slide.reset();
            self.last_space_note = None;
            self.space_gesture_note_due = false;
            self.tracker = SwipeTracker::default();
            return None;
        }
        if self.suppressed {
            return None;
        }
        let swipe = self.tracker.frame(at, touches);
        // Tracked apart from the workspace swipe, which a finger-count change
        // spends while macOS goes on sliding between desktops (#677).
        let slid = self.space_slide.frame(touches);
        if !self.sliding_spaces && (swipe.is_some() || slid) {
            self.sliding_spaces = true;
            self.last_space_note = Some(at);
            self.space_gesture_note_due = true;
        } else if self.sliding_spaces
            && self
                .last_space_note
                .is_none_or(|last| at - last >= SPACE_GESTURE_REFRESH_SECONDS)
        {
            self.last_space_note = Some(at);
            self.space_gesture_note_due = true;
        }
        swipe
    }

    /// Whether the active touch sequence has moved horizontally with three or
    /// four fingers (and is not a pointer drag), so macOS may be sliding
    /// between desktops (#513). Stationary taps and resting palms do not arm
    /// the hold.
    #[cfg(test)]
    fn may_slide_spaces(&self) -> bool {
        self.sliding_spaces
    }

    /// Whether the frame just processed should publish a Space-gesture hold
    /// (on slide start, periodic refresh while held, and once on lift).
    fn take_space_gesture_note(&mut self) -> bool {
        std::mem::take(&mut self.space_gesture_note_due)
    }
}

/// Whether any pointer button is down in the login session, including synthetic
/// left-button drags generated by macOS Accessibility "Three Finger Drag".
fn pointer_button_down() -> bool {
    NSEvent::pressedMouseButtons() != 0
        || CGEventSource::button_state(
            CGEventSourceStateID::CombinedSessionState,
            CGMouseButton::Left,
        )
        || CGEventSource::button_state(CGEventSourceStateID::HIDSystemState, CGMouseButton::Left)
}

/// Whether macOS Natural scrolling is on (`com.apple.swipescrolldirection` in the
/// global domain). A missing value is the macOS default, on. Read on every swipe,
/// so toggling the setting applies to the next one.
fn natural_scrolling() -> bool {
    let defaults = NSUserDefaults::standardUserDefaults();
    let key = NSString::from_str("com.apple.swipescrolldirection");
    defaults
        .objectForKey(&key)
        .is_none_or(|_| defaults.boolForKey(&key))
}

/// Listens for three- and four-finger trackpad swipes and reports each with the pointer's
/// position. The tap only listens, so touches still reach every app, and it needs
/// only Accessibility.
pub struct SwipeMonitor {
    tap: CFRetained<CFMachPort>,
    source: CFRetained<CFRunLoopSource>,
    _context: Box<Context>,
}

/// Each swipe with the number of fingers and where the pointer was, in window-server
/// coordinates.
type SwipeReporter = Box<dyn Fn(Swipe, Fingers, (i32, i32))>;

struct Context {
    report: SwipeReporter,
    gate: RefCell<SwipeGate>,
    left_button_down: Cell<bool>,
    tap: RefCell<Option<CFRetained<CFMachPort>>>,
}

impl SwipeMonitor {
    /// Installs on the main run loop. Fails without Accessibility.
    pub fn install(report: impl Fn(Swipe, Fingers, (i32, i32)) + 'static) -> Result<Self, String> {
        let context = Box::new(Context {
            report: Box::new(report),
            gate: RefCell::default(),
            left_button_down: Cell::new(false),
            tap: RefCell::default(),
        });
        let gesture = NSEventType::Gesture.0 as u32;
        let mask = (1u64 << gesture)
            | (1u64 << CGEventType::LeftMouseDown.0)
            | (1u64 << CGEventType::LeftMouseDragged.0)
            | (1u64 << CGEventType::LeftMouseUp.0);
        // SAFETY: `tap_callback` matches `CGEventTapCallBack`, and `context` stays
        // heap-pinned in the monitor until `Drop` invalidates the tap.
        let tap = unsafe {
            CGEvent::tap_create(
                CGEventTapLocation::SessionEventTap,
                CGEventTapPlacement::HeadInsertEventTap,
                CGEventTapOptions::ListenOnly,
                mask,
                Some(tap_callback),
                (&raw const *context).cast_mut().cast(),
            )
        }
        .ok_or("trackpad swipes need Accessibility")?;
        let source = CFMachPort::new_run_loop_source(None, Some(&tap), 0)
            .ok_or("could not listen for trackpad swipes")?;
        let main = CFRunLoop::main().ok_or("the main run loop is unavailable")?;
        // SAFETY: `kCFRunLoopCommonModes` is a static Core Foundation mode constant.
        main.add_source(Some(&source), unsafe { kCFRunLoopCommonModes });
        context.tap.replace(Some(tap.clone()));
        CGEvent::tap_enable(&tap, true);
        Ok(Self {
            tap,
            source,
            _context: context,
        })
    }
}

impl Drop for SwipeMonitor {
    fn drop(&mut self) {
        if let Some(main) = CFRunLoop::main() {
            // SAFETY: `kCFRunLoopCommonModes` is a static Core Foundation mode constant.
            main.remove_source(Some(&self.source), unsafe { kCFRunLoopCommonModes });
        }
        self.tap.invalidate();
    }
}

unsafe extern "C-unwind" fn tap_callback(
    _: CGEventTapProxy,
    event_type: CGEventType,
    event: NonNull<CGEvent>,
    user_info: *mut c_void,
) -> *mut CGEvent {
    // SAFETY: `user_info` points to the `Context` boxed in `SwipeMonitor::install`, which
    // outlives the tap, and the tap calls back only on the main thread.
    let context = unsafe { &*user_info.cast::<Context>() };
    // SAFETY: the tap passes a live event for the duration of the callback.
    let cg_event = unsafe { event.as_ref() };
    match event_type {
        CGEventType::TapDisabledByTimeout | CGEventType::TapDisabledByUserInput => {
            context.left_button_down.set(false);
            context.gate.borrow_mut().reset();
            if let Some(tap) = context.tap.borrow().as_ref() {
                CGEvent::tap_enable(tap, true);
            }
            return event.as_ptr();
        }
        CGEventType::LeftMouseDown | CGEventType::LeftMouseDragged => {
            context.left_button_down.set(true);
            context.gate.borrow_mut().pointer_pressed();
            return event.as_ptr();
        }
        CGEventType::LeftMouseUp => {
            context.left_button_down.set(false);
            return event.as_ptr();
        }
        _ => {}
    }
    autoreleasepool(|_| {
        let Some(ns_event) = NSEvent::eventWithCGEvent(cg_event) else {
            return;
        };
        let mut touching = Vec::new();
        let mut began = 0;
        let mut lifted = false;
        for touch in ns_event.allTouches().iter() {
            let phase = touch.phase();
            if phase.intersects(NSTouchPhase::Ended | NSTouchPhase::Cancelled) {
                lifted = true;
            } else if phase.intersects(NSTouchPhase::Touching) {
                if phase.intersects(NSTouchPhase::Began) {
                    began += 1;
                }
                let point = touch.normalizedPosition();
                touching.push((point.x, point.y));
            }
        }
        let live_button_down = pointer_button_down();
        if touching.is_empty() && !live_button_down {
            // Self-heal in case a LeftMouseUp was missed outside the tap.
            context.left_button_down.set(false);
        }
        let button_down = context.left_button_down.get() || live_button_down;
        let mut gate = context.gate.borrow_mut();
        let report = TouchReport {
            touching: &touching,
            began,
            lifted,
        };
        let swipe = gate.report(ns_event.timestamp(), report, button_down);
        let note_space_gesture = gate.take_space_gesture_note();
        drop(gate);
        if note_space_gesture {
            super::spaces::note_space_gesture();
        }
        if let Some((swipe, fingers)) = swipe {
            let point = CGEvent::location(Some(cg_event));
            let point = (point.x.round() as i32, point.y.round() as i32);
            (context.report)(
                swipe.for_scroll_direction(natural_scrolling()),
                fingers,
                point,
            );
        }
    });
    event.as_ptr()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fingers(x: f64, y: f64, count: usize) -> Vec<(f64, f64)> {
        (0..count)
            .map(|finger| (x + finger as f64 * 0.05, y))
            .collect()
    }

    fn sweep(
        gate: &mut SwipeGate,
        start: f64,
        from: (f64, f64),
        to: (f64, f64),
        pointer_down: impl Fn(u32) -> bool,
    ) -> Vec<(Swipe, Fingers)> {
        (0..=20)
            .filter_map(|step| {
                let t = f64::from(step) / 20.0;
                let x = from.0 + (to.0 - from.0) * t;
                let y = from.1 + (to.1 - from.1) * t;
                gate.frame(
                    start + f64::from(step) * 0.01,
                    &fingers(x, y, 3),
                    pointer_down(step),
                )
            })
            .collect()
    }

    #[test]
    fn three_finger_drag_with_left_button_down_never_fires_a_swipe() {
        let mut gate = SwipeGate::default();
        // macOS Three Finger Drag starts with 3 touching fingers and posts
        // LeftMouseDown / LeftMouseDragged once horizontal motion begins.
        let swipes = sweep(&mut gate, 0.0, (0.2, 0.5), (0.6, 0.5), |step| step >= 2);
        assert!(swipes.is_empty());

        // Lifting during macOS's drag-lock window, then continuing the drag,
        // still suppresses workspace swipes.
        assert_eq!(gate.frame(0.25, &[], true), None);
        let continued = sweep(&mut gate, 0.30, (0.2, 0.5), (0.6, 0.5), |_| true);
        assert!(continued.is_empty());

        // Once the drag ends and fingers lift, normal three-finger swipes work.
        assert_eq!(gate.frame(0.55, &[], false), None);
        assert_eq!(
            sweep(&mut gate, 0.65, (0.2, 0.5), (0.6, 0.5), |_| false),
            [(Swipe::Right, Fingers::Three)]
        );
    }

    #[test]
    fn mouse_release_before_fingers_lift_keeps_current_gesture_suppressed() {
        let mut gate = SwipeGate::default();
        assert_eq!(gate.frame(0.0, &fingers(0.2, 0.5, 3), false), None);
        gate.pointer_pressed();
        // Button released while the same three fingers keep sliding.
        let swipes = sweep(&mut gate, 0.02, (0.25, 0.5), (0.7, 0.5), |_| false);
        assert!(swipes.is_empty());

        // Lifting all fingers re-arms the next normal swipe.
        assert_eq!(gate.frame(0.3, &[], false), None);
        assert_eq!(
            sweep(&mut gate, 0.4, (0.6, 0.5), (0.2, 0.5), |_| false),
            [(Swipe::Left, Fingers::Three)]
        );
    }

    #[test]
    fn an_ordinary_click_before_a_swipe_does_not_suppress_the_swipe() {
        let mut gate = SwipeGate::default();
        // Click with no trackpad fingers touching.
        gate.pointer_pressed();
        assert_eq!(
            sweep(&mut gate, 0.1, (0.6, 0.5), (0.2, 0.5), |_| false),
            [(Swipe::Left, Fingers::Three)]
        );
    }

    /// #513: macOS shows the arriving desktop once three or four fingers move
    /// horizontally, so horizontal slides arm the hold (and stay latched until
    /// lift), while a pointer drag, a two-finger scroll, a vertical swipe and
    /// lifted fingers do not.
    #[test]
    fn horizontal_three_or_four_finger_slides_arm_space_hold_until_lift() {
        let mut gate = SwipeGate::default();
        gate.frame(0.0, &fingers(0.3, 0.5, 3), false);
        assert!(!gate.may_slide_spaces());
        assert!(!gate.take_space_gesture_note());

        // Horizontal movement >= 0.015 latches sliding_spaces and notes once.
        gate.frame(0.01, &fingers(0.32, 0.5, 3), false);
        assert!(gate.may_slide_spaces());
        assert!(gate.take_space_gesture_note());

        // Subsequent 120 Hz frames stay latched without re-emitting every frame.
        gate.frame(0.02, &fingers(0.34, 0.5, 3), false);
        assert!(gate.may_slide_spaces());
        assert!(!gate.take_space_gesture_note());

        // Refresh fires only after SPACE_GESTURE_REFRESH_SECONDS.
        gate.frame(0.52, &fingers(0.35, 0.5, 3), false);
        assert!(gate.may_slide_spaces());
        assert!(gate.take_space_gesture_note());

        // Lifting emits one final note to anchor the post-lift settle and clears.
        gate.frame(0.55, &[], false);
        assert!(!gate.may_slide_spaces());
        assert!(gate.take_space_gesture_note());

        // Four-finger horizontal slide works the same way.
        gate.frame(1.0, &fingers(0.3, 0.5, 4), false);
        assert!(!gate.may_slide_spaces());
        gate.frame(1.01, &fingers(0.28, 0.5, 4), false);
        assert!(gate.may_slide_spaces());
        assert!(gate.take_space_gesture_note());
        gate.frame(1.02, &[], false);
        assert!(!gate.may_slide_spaces());
        assert!(gate.take_space_gesture_note());

        // Two-finger scroll and pointer drag never arm the hold.
        gate.frame(1.1, &fingers(0.3, 0.5, 2), false);
        gate.frame(1.11, &fingers(0.4, 0.5, 2), false);
        assert!(!gate.may_slide_spaces());
        assert!(!gate.take_space_gesture_note());

        gate.frame(1.2, &[], false);
        gate.frame(1.3, &fingers(0.3, 0.5, 3), true);
        gate.frame(1.31, &fingers(0.4, 0.5, 3), false);
        assert!(!gate.may_slide_spaces());
        assert!(!gate.take_space_gesture_note());
    }

    #[test]
    fn stationary_three_finger_touch_and_vertical_swipe_never_freeze_layout() {
        let mut gate = SwipeGate::default();
        // Resting three fingers or palm with small jitter for 120 frames (~1 s).
        for step in 0..120 {
            let jitter = if step % 2 == 0 { 0.0 } else { 0.004 };
            let touches = fingers(0.4 + jitter, 0.5 + jitter, 3);
            assert_eq!(gate.frame(f64::from(step) * 0.008, &touches, false), None);
            assert!(!gate.may_slide_spaces());
            assert!(!gate.take_space_gesture_note());
        }
        gate.frame(1.0, &[], false);
        assert!(!gate.take_space_gesture_note());

        // Upward three-finger Mission Control swipe with small horizontal drift.
        gate.frame(1.5, &fingers(0.5, 0.3, 3), false);
        for step in 1..=20 {
            let t = f64::from(step) / 20.0;
            let touches = fingers(0.5 + 0.02 * t, 0.3 + 0.3 * t, 3);
            assert_eq!(
                gate.frame(1.5 + f64::from(step) * 0.01, &touches, false),
                None
            );
            assert!(!gate.may_slide_spaces());
            assert!(!gate.take_space_gesture_note());
        }
        gate.frame(1.75, &[], false);
        assert!(!gate.take_space_gesture_note());
    }

    /// #677: a resting thumb that lifts mid-swipe (four fingers to three)
    /// spends the workspace swipe, but macOS still slides between desktops,
    /// so the remaining three fingers' horizontal slide arms the hold.
    #[test]
    fn a_thumb_lifting_mid_swipe_still_arms_space_hold() {
        let mut gate = SwipeGate::default();
        gate.frame(0.0, &fingers(0.5, 0.5, 4), false);
        gate.frame(0.01, &fingers(0.5, 0.5, 3), false);
        assert!(!gate.may_slide_spaces());
        assert!(!gate.take_space_gesture_note());

        gate.frame(0.02, &fingers(0.47, 0.5, 3), false);
        assert!(gate.may_slide_spaces());
        assert!(gate.take_space_gesture_note());

        gate.frame(0.03, &[], false);
        assert!(gate.take_space_gesture_note());
    }

    fn touching(touches: &[(f64, f64)], began: usize) -> TouchReport<'_> {
        TouchReport {
            touching: touches,
            began,
            lifted: false,
        }
    }

    /// #781: an empty report mid-swipe is not a lift, but it keeps a desktop
    /// slide's hold fresh the way the lift it used to be read as did.
    #[test]
    fn an_empty_report_mid_slide_refreshes_the_space_hold_without_lifting() {
        let mut gate = SwipeGate::default();
        let landed = fingers(0.3, 0.5, 3);
        assert_eq!(gate.report(0.0, touching(&landed, 3), false), None);
        let moved = fingers(0.33, 0.5, 3);
        assert_eq!(gate.report(0.01, touching(&moved, 0), false), None);
        assert!(gate.take_space_gesture_note());

        assert_eq!(gate.report(0.02, touching(&[], 0), false), None);
        assert!(gate.may_slide_spaces());
        assert!(gate.take_space_gesture_note());
    }
}
