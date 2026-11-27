//! `[labs]`: experimental features, each behind its own switch and off by default.

use serde::{Deserialize, Serialize};

/// The `[labs]` table: experimental features, each behind its own switch and off by default.
/// With every switch off Spaceman behaves exactly as if Labs did not exist.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct LabsConfig {
    /// The i3/sway-style Scratchpad ([`crate::scratchpad`]) and its `⌥\` / `⌥⇧\` shortcuts.
    #[serde(default)]
    pub scratchpad: bool,
    /// Settings › Activity: the live arrangement log, doctor diagnostics, and process vitals.
    #[serde(default)]
    pub activity: bool,
    /// Top and bottom display edge drop bands that make a half-height row on portrait displays.
    #[serde(default)]
    pub portrait_edge_rows: bool,
    /// The menu bar, its menu and the HUD's Layouts style follow the displays' physical
    /// arrangement: left to right, portrait displays drawn tall, same-named displays told apart.
    #[serde(default)]
    pub physical_displays: bool,
    /// Directional focus prefers the most recently focused of the tiles that
    /// border the focused one on the side pressed.
    #[serde(default)]
    pub recent_focus: bool,
    /// An off switch for the three-finger workspace swipe: `settings.trackpad_swipe` and the
    /// Settings › Shortcuts switch for it.
    #[serde(default)]
    pub swipe_switch: bool,
    /// An empty workspace chosen on another display is the focused one for commands until
    /// another window takes focus.
    #[serde(default)]
    pub empty_workspace_focus: bool,
    /// A floating window dragged to another display joins the workspace showing there.
    #[serde(default)]
    pub float_follows_display: bool,
    /// `switch next|prev` and `move next|prev [--follow]`: step through the workspaces of the
    /// focused display from the keyboard.
    #[serde(default)]
    pub workspace_steps: bool,
    /// The workspace swipe takes four fingers instead of three, leaving three-finger
    /// gestures (such as Accessibility "Three Finger Drag") to macOS.
    #[serde(default)]
    pub four_finger_swipe: bool,
    /// `settings.single_window_max_height`: a lone tile is centered at that height too, and
    /// Settings › Layout shows its row.
    #[serde(default)]
    pub single_window_height: bool,
    /// A `route` rule with `windows = "all"` routes every new window of its app, not only
    /// the first one.
    #[serde(default)]
    pub route_all_windows: bool,
    /// `settings.menu_bar_icon`: the menu bar icon can be one glyph (`compact`) or hidden
    /// (`off`), and Settings › General shows its picker.
    #[serde(default)]
    pub menu_bar_icon_style: bool,
    /// A tile the pointer rests on takes focus, without a click.
    #[serde(default)]
    pub focus_follows_mouse: bool,
    /// Holding the shortcuts' base modifier and Shift (`⌥⇧` by default) shows a card over every
    /// visible window to send it to a workspace, float, pin or swap it.
    #[serde(default)]
    pub window_overlay: bool,
}

impl LabsConfig {
    /// Builds a [`LabsConfig`] by asking `enabled` for each [`LabFeature`].
    #[must_use]
    pub fn from_fn(mut enabled: impl FnMut(LabFeature) -> bool) -> Self {
        Self {
            scratchpad: enabled(LabFeature::Scratchpad),
            activity: enabled(LabFeature::Activity),
            portrait_edge_rows: enabled(LabFeature::PortraitEdgeRows),
            physical_displays: enabled(LabFeature::PhysicalDisplays),
            recent_focus: enabled(LabFeature::RecentFocus),
            swipe_switch: enabled(LabFeature::SwipeSwitch),
            empty_workspace_focus: enabled(LabFeature::EmptyWorkspaceFocus),
            float_follows_display: enabled(LabFeature::FloatFollowsDisplay),
            workspace_steps: enabled(LabFeature::WorkspaceSteps),
            four_finger_swipe: enabled(LabFeature::FourFingerSwipe),
            single_window_height: enabled(LabFeature::SingleWindowHeight),
            route_all_windows: enabled(LabFeature::RouteAllWindows),
            menu_bar_icon_style: enabled(LabFeature::MenuBarIconStyle),
            focus_follows_mouse: enabled(LabFeature::FocusFollowsMouse),
            window_overlay: enabled(LabFeature::WindowOverlay),
        }
    }

    /// Whether `feature`'s switch is on.
    #[must_use]
    pub const fn enabled(self, feature: LabFeature) -> bool {
        match feature {
            LabFeature::Scratchpad => self.scratchpad,
            LabFeature::Activity => self.activity,
            LabFeature::PortraitEdgeRows => self.portrait_edge_rows,
            LabFeature::PhysicalDisplays => self.physical_displays,
            LabFeature::RecentFocus => self.recent_focus,
            LabFeature::SwipeSwitch => self.swipe_switch,
            LabFeature::EmptyWorkspaceFocus => self.empty_workspace_focus,
            LabFeature::FloatFollowsDisplay => self.float_follows_display,
            LabFeature::WorkspaceSteps => self.workspace_steps,
            LabFeature::FourFingerSwipe => self.four_finger_swipe,
            LabFeature::SingleWindowHeight => self.single_window_height,
            LabFeature::RouteAllWindows => self.route_all_windows,
            LabFeature::MenuBarIconStyle => self.menu_bar_icon_style,
            LabFeature::FocusFollowsMouse => self.focus_follows_mouse,
            LabFeature::WindowOverlay => self.window_overlay,
        }
    }
}

/// Every `[labs]` switch, one per experimental feature.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LabFeature {
    /// The i3/sway-style Scratchpad ([`crate::scratchpad`]) and its `⌥\` / `⌥⇧\` shortcuts.
    Scratchpad,
    /// Settings › Activity: the live arrangement log, doctor diagnostics, and process vitals.
    Activity,
    /// Top and bottom display edge drop bands that make a half-height row on portrait displays.
    PortraitEdgeRows,
    /// The menu bar, its menu and the HUD's Layouts style in physical display order and shape.
    PhysicalDisplays,
    /// Directional focus prefers the most recently focused bordering tile.
    RecentFocus,
    /// An off switch for the three-finger workspace swipe (`settings.trackpad_swipe`).
    SwipeSwitch,
    /// An empty workspace chosen on another display is the focused one for commands until
    /// another window takes focus.
    EmptyWorkspaceFocus,
    /// A floating window dragged to another display joins the workspace showing there.
    FloatFollowsDisplay,
    /// `switch next|prev` and `move next|prev [--follow]` on the focused display.
    WorkspaceSteps,
    /// The workspace swipe takes four fingers instead of three.
    FourFingerSwipe,
    /// A lone tile's height cap, `settings.single_window_max_height`.
    SingleWindowHeight,
    /// `windows = "all"` on a `route` rule routes every new window of its app.
    RouteAllWindows,
    /// `settings.menu_bar_icon`: a compact or hidden menu bar icon.
    MenuBarIconStyle,
    /// A tile the pointer rests on takes focus, without a click.
    FocusFollowsMouse,
    /// Hold `⌥⇧` for a card over every visible window: send, float, pin, swap.
    WindowOverlay,
}

impl LabFeature {
    /// Every switch, in the order `[labs]` lists them.
    pub const ALL: [Self; 15] = [
        Self::Scratchpad,
        Self::Activity,
        Self::PortraitEdgeRows,
        Self::PhysicalDisplays,
        Self::RecentFocus,
        Self::SwipeSwitch,
        Self::EmptyWorkspaceFocus,
        Self::FloatFollowsDisplay,
        Self::WorkspaceSteps,
        Self::FourFingerSwipe,
        Self::SingleWindowHeight,
        Self::RouteAllWindows,
        Self::MenuBarIconStyle,
        Self::FocusFollowsMouse,
        Self::WindowOverlay,
    ];

    /// The `[labs]` key that turns this feature on.
    #[must_use]
    pub const fn key(self) -> &'static str {
        match self {
            Self::Scratchpad => "scratchpad",
            Self::Activity => "activity",
            Self::PortraitEdgeRows => "portrait_edge_rows",
            Self::PhysicalDisplays => "physical_displays",
            Self::RecentFocus => "recent_focus",
            Self::SwipeSwitch => "swipe_switch",
            Self::EmptyWorkspaceFocus => "empty_workspace_focus",
            Self::FloatFollowsDisplay => "float_follows_display",
            Self::WorkspaceSteps => "workspace_steps",
            Self::FourFingerSwipe => "four_finger_swipe",
            Self::SingleWindowHeight => "single_window_height",
            Self::RouteAllWindows => "route_all_windows",
            Self::MenuBarIconStyle => "menu_bar_icon_style",
            Self::FocusFollowsMouse => "focus_follows_mouse",
            Self::WindowOverlay => "window_overlay",
        }
    }

    /// Parses a `[labs]` key such as `activity`.
    #[must_use]
    pub fn parse(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|feature| feature.key() == key)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_fn_sets_every_lab_feature_independently() {
        assert_eq!(LabsConfig::from_fn(|_| false), LabsConfig::default());
        for target in LabFeature::ALL {
            let labs = LabsConfig::from_fn(|feature| feature == target);
            for feature in LabFeature::ALL {
                assert_eq!(
                    labs.enabled(feature),
                    feature == target,
                    "target={target:?}, checked={feature:?}"
                );
            }
        }
    }

    #[test]
    fn window_overlay_round_trips_through_toml() {
        let labs: LabsConfig = toml::from_str("window_overlay = true").unwrap();
        assert!(labs.window_overlay);
        assert!(labs.enabled(LabFeature::WindowOverlay));
        assert_eq!(LabsConfig::from_fn(|_| false), LabsConfig::default());
        let written = toml::to_string(&labs).unwrap();
        assert!(written.contains("window_overlay = true"), "{written}");
        assert_eq!(toml::from_str::<LabsConfig>(&written).unwrap(), labs);
        assert_eq!(
            LabFeature::parse("window_overlay"),
            Some(LabFeature::WindowOverlay)
        );
    }
}
