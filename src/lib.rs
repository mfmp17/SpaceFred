#[cfg(target_os = "macos")]
pub mod app;
pub mod config;
#[cfg(target_os = "macos")]
pub mod control;
pub mod deep_link;
pub mod defaults;
pub mod geometry;
#[cfg(target_os = "macos")]
#[allow(unsafe_code)]
mod hotkeys;
pub mod journal;
#[cfg(target_os = "macos")]
#[allow(unsafe_code)]
mod keyboard_layout;
mod layout_tree;
#[cfg(target_os = "macos")]
pub mod live;
#[cfg(target_os = "macos")]
#[allow(unsafe_code)]
mod login_item;
#[cfg(target_os = "macos")]
#[allow(unsafe_code)]
pub mod macos;
pub mod model;
pub mod motion;
pub mod navigation;
pub mod observation;
pub mod onboarding_plan;
#[cfg(target_os = "macos")]
#[allow(unsafe_code)]
pub mod permission_guide;
pub mod reconcile;
pub mod runtime;
pub mod scratchpad;
mod serde_map;
pub mod shortcuts;
pub mod spacing;
#[allow(unsafe_code)]
pub mod state_file;
#[cfg(target_os = "macos")]
#[allow(unsafe_code)]
mod status_item;
pub mod swipe;
#[cfg(target_os = "macos")]
#[allow(unsafe_code)]
mod termination;
#[cfg(target_os = "macos")]
mod updater;
pub use updater::build_version;
#[cfg(target_os = "macos")]
#[allow(unsafe_code)]
mod url_handler;
pub mod window_memory;
pub mod window_overlay;
pub mod workspace;

pub use geometry::{Axis, Rect};
pub use model::{AppInstanceId, BindingState, Model, PhysicalWindowId, Slot, SlotId};
pub use navigation::{Direction, NavigationTile};
pub use observation::{AppObservation, NativeObservation, ObservedApplication, ObservedWindow};
pub use reconcile::{
    Decision, Placement, Reconciliation, ReconciliationEvidence, Settings, reconcile,
};
pub use runtime::{CycleTicket, Engine, StaleCycle};
pub use workspace::{DisplaySelector, DisplayTarget, WorkspaceId, WorkspaceStep, Workspaces};

/// Characters the active keyboard layout types, including Option combinations, so Settings
/// can warn when a shortcut collides with one. Empty when the layout can't be read.
#[cfg(target_os = "macos")]
pub fn current_layout_glyphs() -> Vec<shortcuts::LayoutGlyph> {
    keyboard_layout::cached_layout_glyphs()
}

/// Characters the active keyboard layout types. Always empty off macOS.
#[cfg(not(target_os = "macos"))]
pub fn current_layout_glyphs() -> Vec<shortcuts::LayoutGlyph> {
    Vec::new()
}

/// Enabled Mission Control shortcuts. Spaceman does not register a chord one of these owns.
#[cfg(target_os = "macos")]
pub fn mission_control_shortcuts() -> Vec<shortcuts::SystemChord> {
    macos::mission_control_shortcuts()
}

/// Enabled Mission Control shortcuts. Always empty off macOS.
#[cfg(not(target_os = "macos"))]
pub fn mission_control_shortcuts() -> Vec<shortcuts::SystemChord> {
    Vec::new()
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ResizeDirection {
    Shrink,
    Grow,
}

/// Largest `resize width|height` size, in points.
pub const MAX_RESIZE_POINTS: i32 = 16_384;

/// Parses the arguments of `resize width|height POINTS`: the axis that `width` or `height` runs
/// along, and the window size in points.
pub fn parse_resize_to(dimension: &str, points: &str) -> Result<(Axis, i32), String> {
    let axis = match dimension {
        "width" => Axis::Horizontal,
        "height" => Axis::Vertical,
        other => return Err(format!("expected width or height, got {other:?}")),
    };
    let points = points
        .parse::<i32>()
        .ok()
        .filter(|points| (1..=MAX_RESIZE_POINTS).contains(points))
        .ok_or_else(|| {
            format!("resize {dimension} takes a size from 1 to {MAX_RESIZE_POINTS} points, got {points:?}")
        })?;
    Ok((axis, points))
}
