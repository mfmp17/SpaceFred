//! Writing a tile frame to one window and reading back how it landed.

use objc2_application_services::{AXError, AXUIElement};

use crate::{
    PhysicalWindowId, Rect, Settings,
    macos::ax::{set_position, set_size, window_frame},
};

use super::{ApplyOutcome, ApplyResult};

pub(super) fn apply_element(
    id: &PhysicalWindowId,
    window: &AXUIElement,
    target: Rect,
    settings: &Settings,
) -> ApplyResult {
    let before = window_frame(window);
    if before.is_some_and(|frame| frame.approximately_equals(target, settings.frame_tolerance)) {
        return ApplyResult {
            window: id.clone(),
            target,
            actual: before,
            outcome: ApplyOutcome::Applied,
        };
    }
    let write = write_frame(|| set_size(window, target), || set_position(window, target));
    let mut actual = window_frame(window);
    if write.written() && arrived_short(before, target, actual, settings) {
        // By the readback the app has seen its new screen; size it again.
        let _: AXError = set_size(window, target);
        actual = window_frame(window);
    }
    ApplyResult {
        window: id.clone(),
        target,
        actual,
        outcome: frame_outcome(write, target, actual, settings.frame_tolerance),
    }
}

/// Which parts of a tile frame write the window accepted.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct FrameWrite {
    pub(super) moved: bool,
    pub(super) sized: bool,
}

impl FrameWrite {
    pub(super) const fn written(self) -> bool {
        self.moved && self.sized
    }
}

/// Size, move, then size again. The first size write only helps a window
/// that is leaving a smaller display fit where it lands, so it is best effort:
/// a window that refuses it on its old display (a clamped or slow app) must
/// still move. Whether the frame was written rests on the move and the final
/// size alone.
pub(super) fn write_frame(
    mut set_size: impl FnMut() -> AXError,
    set_position: impl FnOnce() -> AXError,
) -> FrameWrite {
    let _: AXError = set_size();
    let moved = set_position() == AXError::Success;
    let sized = set_size() == AXError::Success;
    FrameWrite { moved, sized }
}

/// The outcome of a tile frame write read back as `actual`. A window whose
/// size cannot be set (a fixed-size window under a `tile` rule) refuses every
/// size write, yet one that moved to its tile's corner keeps its own size
/// inside the tile: constrained like a clamped window, not unavailable, which
/// would roll back every switch to its workspace (#726).
pub(super) fn frame_outcome(
    write: FrameWrite,
    target: Rect,
    actual: Option<Rect>,
    tolerance: i32,
) -> ApplyOutcome {
    match actual {
        Some(actual) if actual.approximately_equals(target, tolerance) => ApplyOutcome::Applied,
        Some(_) if write.written() => ApplyOutcome::Constrained,
        Some(actual) if write.moved && actual.position_approximately_equals(target, tolerance) => {
            ApplyOutcome::Constrained
        }
        _ => ApplyOutcome::Unavailable,
    }
}

/// Whether a window that came from another display landed at its target but
/// short of its size. macOS fits the final size write to the display the
/// window is leaving until the app sees its new screen, so a window moved onto
/// a taller display stops at the old display's bottom edge (#428).
pub(super) fn arrived_short(
    before: Option<Rect>,
    target: Rect,
    actual: Option<Rect>,
    settings: &Settings,
) -> bool {
    let (Some(before), Some(actual)) = (before, actual) else {
        return false;
    };
    let tolerance = settings.frame_tolerance;
    !settings.output.contains_center(before)
        && actual.position_approximately_equals(target, tolerance)
        && (actual.height + tolerance < target.height || actual.width + tolerance < target.width)
}
