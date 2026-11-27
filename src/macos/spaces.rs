//! When macOS last changed the active Space (Mission Control desktops and
//! native fullscreen Spaces). `NSWorkspace` posts the change on the main thread
//! while the slide animation can still be running; the worker reads the time
//! to hold layout writes until the slide has settled, because windows from the
//! outgoing and incoming Spaces are both on screen while it runs.
//!
//! A trackpad swipe between desktops shows the arriving desktop's windows as
//! soon as the fingers move, often well before `NSWorkspace` posts the change
//! and sometimes with no change at all (a swipe dragged partway and reversed),
//! so the latest three- or four-finger touch is recorded too (#513).

use std::{
    sync::{
        OnceLock,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

/// Process-relative reference for the stored timestamps.
static EPOCH: OnceLock<Instant> = OnceLock::new();
/// Nanoseconds after `EPOCH` of the latest change, plus one (zero means none).
static CHANGED_AT: AtomicU64 = AtomicU64::new(0);
/// Nanoseconds after `EPOCH` of the latest multi-finger touch, plus one.
static GESTURE_AT: AtomicU64 = AtomicU64::new(0);
/// Set by each change or touch until the main thread takes it.
static PENDING: AtomicBool = AtomicBool::new(false);

fn epoch() -> Instant {
    *EPOCH.get_or_init(Instant::now)
}

fn store_now(into: &AtomicU64) {
    let elapsed = u64::try_from(epoch().elapsed().as_nanos()).unwrap_or(u64::MAX - 1);
    into.store(elapsed.saturating_add(1), Ordering::Release);
    PENDING.store(true, Ordering::Release);
}

fn load(from: &AtomicU64) -> Option<Instant> {
    match from.load(Ordering::Acquire) {
        0 => None,
        stored => Some(epoch() + Duration::from_nanos(stored - 1)),
    }
}

/// Records an active-Space change happening now.
pub(super) fn note_active_space_change() {
    store_now(&CHANGED_AT);
}

/// Records three or four fingers on the trackpad now: macOS may be sliding
/// between desktops before it reports any Space change.
pub(super) fn note_space_gesture() {
    store_now(&GESTURE_AT);
}

/// When macOS last changed the active Space, if it has since launch.
#[must_use]
pub fn last_active_space_change() -> Option<Instant> {
    load(&CHANGED_AT)
}

/// When three or four fingers last touched the trackpad, if they have since launch.
#[must_use]
pub fn last_space_gesture() -> Option<Instant> {
    load(&GESTURE_AT)
}

/// Whether the active Space changed, or a desktop swipe may be under way,
/// since the last call.
#[must_use]
pub fn take_active_space_changed() -> bool {
    PENDING.swap(false, Ordering::AcqRel)
}
