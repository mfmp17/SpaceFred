use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

mod write;
pub use write::{
    ConfigLock, lock_config, resolve_write_target, save_workspace_monitor_at, write_config_atomic,
};

use crate::{
    DisplaySelector, ObservedWindow, WorkspaceId, Workspaces,
    shortcuts::{RawShortcuts, Shortcuts},
    spacing::{Spacing, SpacingKey, check_outer},
};

#[derive(Clone, Copy, Debug)]
struct AppSettings {
    hud: bool,
    hud_style: HudStyle,
    hud_position: HudPosition,
    update_channel: UpdateChannel,
    mouse_follows_focus: MouseFollowsFocus,
    spacing: Spacing,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            hud: true,
            hud_style: HudStyle::default(),
            hud_position: HudPosition::default(),
            update_channel: UpdateChannel::default(),
            mouse_follows_focus: MouseFollowsFocus::default(),
            spacing: Spacing::default(),
        }
    }
}

/// Which release channel the in-app updater follows: `settings.update_channel`.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum UpdateChannel {
    /// Public releases.
    #[default]
    Stable,
    /// A build of every merge to `dev`.
    Dev,
}

impl UpdateChannel {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Stable => "stable",
            Self::Dev => "dev",
        }
    }

    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "stable" => Some(Self::Stable),
            "dev" => Some(Self::Dev),
            _ => None,
        }
    }
}

/// Whether Spaceman moves the pointer when an explicit focus action lands elsewhere:
/// `settings.mouse_follows_focus`.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum MouseFollowsFocus {
    /// Leave the pointer where it is.
    #[default]
    Off,
    /// Move the pointer to the focused window (or empty display) when it is on another display.
    Display,
    /// Move the pointer to the focused window (or empty display) whenever it is outside it.
    Window,
}

impl MouseFollowsFocus {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Display => "display",
            Self::Window => "window",
        }
    }

    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "off" => Some(Self::Off),
            "display" => Some(Self::Display),
            "window" => Some(Self::Window),
            _ => None,
        }
    }
}

/// How the Workspace HUD draws each workspace in its row: `settings.hud_style`.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum HudStyle {
    /// Each workspace as its layout, with an app icon in every tile; the current one larger.
    Layouts,
    /// Each workspace as its number and the icons of its apps.
    #[default]
    Icons,
}

impl HudStyle {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Layouts => "layouts",
            Self::Icons => "icons",
        }
    }

    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "layouts" => Some(Self::Layouts),
            "icons" => Some(Self::Icons),
            _ => None,
        }
    }
}

/// Where the Workspace HUD capsule sits on each display: `settings.hud_position`.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum HudPosition {
    /// Just above the bottom of the display, clear of the Dock.
    #[default]
    Bottom,
    /// Just below the top of the display, clear of the menu bar.
    Top,
}

impl HudPosition {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Bottom => "bottom",
            Self::Top => "top",
        }
    }

    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "bottom" => Some(Self::Bottom),
            "top" => Some(Self::Top),
            _ => None,
        }
    }
}

/// The `[labs]` table: experimental features, each behind its own switch and off by default.
/// With every switch off Spaceman behaves exactly as if Labs did not exist.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct LabsConfig {
    /// The i3/sway-style Scratchpad ([`crate::scratchpad`]) and its `⌥\` / `⌥⇧\` shortcuts.
    #[serde(default)]
    pub scratchpad: bool,
}

#[derive(Clone, Debug, Default)]
pub struct Config {
    settings: AppSettings,
    labs: LabsConfig,
    rules: Vec<WindowRule>,
    workspace_monitors: Vec<(WorkspaceId, DisplaySelector)>,
    workspace_order: Vec<WorkspaceId>,
    shortcuts: Shortcuts,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WindowDisposition {
    Managed,
    Floating,
    /// Tiled even when the window looks like a dialog or non-resizable panel.
    Tiled,
    Route(WorkspaceId),
    /// Never tiled, owned, moved, or parked. For windows whose applications
    /// insist on managing them themselves.
    Ignored,
}

#[derive(Clone, Debug)]
struct WindowRule {
    matcher: WindowMatcher,
    action: WindowDisposition,
}

#[derive(Clone, Debug)]
struct WindowMatcher {
    bundle_id: Option<String>,
    title: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawConfig {
    version: u32,
    #[serde(default)]
    settings: RawSettings,
    #[serde(default)]
    labs: LabsConfig,
    #[serde(default)]
    rules: Vec<RawWindowRule>,
    #[serde(default)]
    workspaces: Vec<RawWorkspace>,
    #[serde(default)]
    shortcuts: RawShortcuts,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawSettings {
    #[serde(default = "default_hud")]
    hud: bool,
    #[serde(default)]
    hud_style: HudStyle,
    #[serde(default)]
    hud_position: HudPosition,
    #[serde(default)]
    update_channel: UpdateChannel,
    #[serde(default)]
    mouse_follows_focus: MouseFollowsFocus,
    #[serde(default)]
    single_window_max_width: i64,
    #[serde(default)]
    workspace_order: Vec<WorkspaceId>,
    #[serde(default)]
    gaps: RawGaps,
}

impl Default for RawSettings {
    fn default() -> Self {
        Self {
            hud: true,
            hud_style: HudStyle::default(),
            hud_position: HudPosition::default(),
            update_channel: UpdateChannel::default(),
            mouse_follows_focus: MouseFollowsFocus::default(),
            single_window_max_width: 0,
            workspace_order: Vec::new(),
            gaps: RawGaps::default(),
        }
    }
}

/// `[settings.gaps]`, read wide so out-of-range values get a clear message.
#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawGaps {
    inner: Option<i64>,
    outer: Option<i64>,
    top: Option<i64>,
    bottom: Option<i64>,
    left: Option<i64>,
    right: Option<i64>,
}

impl RawSettings {
    fn spacing(&self) -> Result<Spacing, String> {
        let gaps = &self.gaps;
        let outer = gaps.outer.map(check_outer).transpose()?;
        let mut spacing = Spacing::default();
        for (key, value) in [
            (SpacingKey::Inner, gaps.inner),
            (SpacingKey::Top, gaps.top),
            (SpacingKey::Bottom, gaps.bottom),
            (SpacingKey::Left, gaps.left),
            (SpacingKey::Right, gaps.right),
            (
                SpacingKey::SingleWindowMaxWidth,
                Some(self.single_window_max_width),
            ),
        ] {
            let value = match value {
                Some(value) => key.check(value)?,
                None => key.fallback(outer),
            };
            key.set(&mut spacing, value);
        }
        Ok(spacing)
    }
}

const fn default_hud() -> bool {
    true
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawWindowRule {
    bundle_id: Option<String>,
    title: Option<String>,
    action: String,
    workspace: Option<WorkspaceId>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawWorkspace {
    workspace: WorkspaceId,
    monitor: String,
}

impl Config {
    #[must_use]
    #[cfg(test)]
    pub fn has_rules(&self) -> bool {
        !self.rules.is_empty()
    }

    pub fn parse(source: &str) -> Result<Self, String> {
        let raw: RawConfig = toml::from_str(source).map_err(|error| error.to_string())?;
        if raw.version != 1 {
            return Err(format!(
                "unsupported config version {}; expected 1",
                raw.version
            ));
        }
        let mut rules = Vec::with_capacity(raw.rules.len());
        for rule in raw.rules {
            if rule.bundle_id.is_none() && rule.title.is_none() {
                return Err("window rule must contain at least one matcher".into());
            }
            let action = match (rule.action.as_str(), rule.workspace) {
                ("float", None) => WindowDisposition::Floating,
                ("ignore", None) => WindowDisposition::Ignored,
                ("tile", None) => WindowDisposition::Tiled,
                ("route", Some(workspace)) => WindowDisposition::Route(workspace),
                ("float", Some(_)) => return Err("float rule cannot specify workspace".into()),
                ("ignore", Some(_)) => return Err("ignore rule cannot specify workspace".into()),
                ("tile", Some(_)) => return Err("tile rule cannot specify workspace".into()),
                ("route", None) => return Err("route rule requires workspace".into()),
                (action, _) => return Err(format!("unknown window rule action {action:?}")),
            };
            rules.push(WindowRule {
                matcher: WindowMatcher {
                    bundle_id: rule.bundle_id,
                    title: rule.title,
                },
                action,
            });
        }
        let mut workspace_monitors = Vec::with_capacity(raw.workspaces.len());
        for workspace in raw.workspaces {
            let id = workspace.workspace;
            if workspace.monitor.trim().is_empty() {
                return Err(format!("workspace {id} monitor cannot be empty"));
            }
            if workspace_monitors
                .iter()
                .any(|(existing, _)| *existing == id)
            {
                return Err(format!("workspace {} is configured more than once", id));
            }
            workspace_monitors.push((id, DisplaySelector::parse(&workspace.monitor)));
        }
        for (index, workspace) in raw.settings.workspace_order.iter().enumerate() {
            if raw.settings.workspace_order[..index].contains(workspace) {
                return Err(format!(
                    "workspace {workspace} appears more than once in settings.workspace_order"
                ));
            }
        }
        let mut shortcuts = raw.shortcuts.parse()?;
        shortcuts.scratchpad = raw.labs.scratchpad;
        Ok(Self {
            settings: AppSettings {
                hud: raw.settings.hud,
                hud_style: raw.settings.hud_style,
                hud_position: raw.settings.hud_position,
                update_channel: raw.settings.update_channel,
                mouse_follows_focus: raw.settings.mouse_follows_focus,
                spacing: raw.settings.spacing()?,
            },
            labs: raw.labs,
            rules,
            workspace_monitors,
            workspace_order: raw.settings.workspace_order,
            shortcuts,
        })
    }

    /// The `[shortcuts]` table: user bindings and whether built-in shortcuts stay.
    #[must_use]
    pub const fn shortcuts(&self) -> &Shortcuts {
        &self.shortcuts
    }

    /// The `[labs]` switches: experimental features, all off by default.
    #[must_use]
    pub const fn labs(&self) -> LabsConfig {
        self.labs
    }

    #[must_use]
    pub const fn hud(&self) -> bool {
        self.settings.hud
    }

    #[must_use]
    pub const fn hud_style(&self) -> HudStyle {
        self.settings.hud_style
    }

    #[must_use]
    pub const fn hud_position(&self) -> HudPosition {
        self.settings.hud_position
    }

    #[must_use]
    pub const fn update_channel(&self) -> UpdateChannel {
        self.settings.update_channel
    }

    #[must_use]
    pub const fn mouse_follows_focus(&self) -> MouseFollowsFocus {
        self.settings.mouse_follows_focus
    }

    /// `settings.workspace_order`: workspaces the HUD, menu bar and swipes list first, in order.
    #[must_use]
    pub fn workspace_order(&self) -> &[WorkspaceId] {
        &self.workspace_order
    }

    /// Every workspace in listing order: `settings.workspace_order`, then the rest by number,
    /// then A-Z. Only the order workspaces are listed in; identities and hotkeys don't change.
    pub fn ordered_workspaces(&self) -> impl Iterator<Item = WorkspaceId> + '_ {
        self.workspace_order
            .iter()
            .copied()
            .chain(WorkspaceId::all().filter(|workspace| !self.workspace_order.contains(workspace)))
    }

    /// Gaps between tiles, outer screen padding and the lone-window width cap.
    #[must_use]
    pub const fn spacing(&self) -> Spacing {
        self.settings.spacing
    }

    pub fn load() -> Result<Self, String> {
        let path = config_path().ok_or("HOME is unavailable")?;
        match std::fs::read_to_string(&path) {
            Ok(source) => parse_path(&path, &source),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(error) => Err(format!("failed to read {}: {error}", path.display())),
        }
    }

    fn matching_rule(&self, bundle_id: &str, window: &ObservedWindow) -> Option<&WindowRule> {
        self.rules
            .iter()
            .find(|rule| rule.matcher.matches(bundle_id, window))
    }

    #[must_use]
    pub fn disposition(&self, bundle_id: &str, window: &ObservedWindow) -> WindowDisposition {
        self.matching_rule(bundle_id, window)
            .map_or(WindowDisposition::Managed, |rule| rule.action)
    }

    #[must_use]
    pub fn route_matches_title(&self, bundle_id: &str, window: &ObservedWindow) -> bool {
        self.matching_rule(bundle_id, window).is_some_and(|rule| {
            matches!(rule.action, WindowDisposition::Route(_)) && rule.matcher.title.is_some()
        })
    }

    #[must_use]
    pub fn has_bundle_route(&self, bundle_id: &str) -> bool {
        self.rules
            .iter()
            .find(|rule| {
                rule.matcher.title.is_none()
                    && rule
                        .matcher
                        .bundle_id
                        .as_deref()
                        .is_some_and(|value| value == bundle_id)
            })
            .is_some_and(|rule| matches!(rule.action, WindowDisposition::Route(_)))
    }

    pub fn configure_workspaces(&self, workspaces: &mut Workspaces) {
        for workspace in self.named_workspaces() {
            workspaces.enable(workspace);
        }
        for workspace in WorkspaceId::all() {
            if !workspaces.is_enabled(workspace) {
                continue;
            }
            let target = self
                .workspace_monitors
                .iter()
                .find(|(id, _)| *id == workspace)
                .map_or_else(
                    || Workspaces::default_assignment(workspace),
                    |(_, selector)| selector.clone(),
                );
            workspaces.set_assignment(workspace, target);
        }
    }

    pub fn set_workspace_monitor(&mut self, workspace: WorkspaceId, selector: DisplaySelector) {
        let should_clear = selector == Workspaces::default_assignment(workspace)
            && (workspace.number().is_some() || self.has_workspace_route(workspace));
        if should_clear {
            self.workspace_monitors
                .retain(|(existing, _)| *existing != workspace);
            return;
        }
        if let Some((_, existing)) = self
            .workspace_monitors
            .iter_mut()
            .find(|(existing, _)| *existing == workspace)
        {
            *existing = selector;
        } else {
            self.workspace_monitors.push((workspace, selector));
        }
    }

    fn has_workspace_route(&self, workspace: WorkspaceId) -> bool {
        self.rules
            .iter()
            .any(|rule| rule.action == WindowDisposition::Route(workspace))
    }

    pub fn named_workspaces(&self) -> impl Iterator<Item = WorkspaceId> + '_ {
        self.workspace_monitors
            .iter()
            .map(|(workspace, _)| *workspace)
            .chain(self.rules.iter().filter_map(|rule| match rule.action {
                WindowDisposition::Route(workspace) => Some(workspace),
                _ => None,
            }))
            .filter(|workspace| workspace.letter().is_some())
    }
}

impl WindowMatcher {
    fn matches(&self, bundle_id: &str, window: &ObservedWindow) -> bool {
        self.bundle_id
            .as_deref()
            .is_none_or(|value| value == bundle_id)
            && self
                .title
                .as_deref()
                .is_none_or(|value| window.title.as_deref() == Some(value))
    }
}

/// Wall-clock duration since the Unix epoch.
#[must_use]
#[expect(
    clippy::disallowed_methods,
    reason = "config::now is the central wall-clock adapter"
)]
pub fn now() -> std::time::Duration {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
}

/// Optional `SPACEMAN_MANAGED_DISPLAY` override read at startup.
#[expect(
    clippy::disallowed_methods,
    reason = "startup environment variables are read in config.rs"
)]
pub fn managed_display_env() -> Result<Option<String>, String> {
    match std::env::var("SPACEMAN_MANAGED_DISPLAY") {
        Ok(value) => Ok(Some(value)),
        Err(std::env::VarError::NotPresent) => Ok(None),
        Err(std::env::VarError::NotUnicode(_)) => {
            Err("SPACEMAN_MANAGED_DISPLAY must be valid UTF-8".into())
        }
    }
}

/// When the config file last changed, or `None` while it doesn't exist.
#[must_use]
pub fn modified() -> Option<std::time::SystemTime> {
    std::fs::metadata(config_path()?).ok()?.modified().ok()
}

pub fn config_path() -> Option<PathBuf> {
    std::env::var_os("SPACEMAN_CONFIG")
        .map(PathBuf::from)
        .or_else(|| application_support_path("config.toml"))
}

pub fn application_support_path(file: &str) -> Option<PathBuf> {
    std::env::var_os("SPACEMAN_APPLICATION_SUPPORT")
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME")
                .map(PathBuf::from)
                // Keep Spaceman's folder so an existing config, workspaces, and
                // parked-window recovery carry over to this SpaceFred fork.
                .map(|home| home.join("Library/Application Support/Spaceman"))
        })
        .map(|directory| directory.join(file))
}

pub fn load_path(path: &Path) -> Result<Config, String> {
    let source = std::fs::read_to_string(path)
        .map_err(|error| format!("failed to read {}: {error}", path.display()))?;
    parse_path(path, &source)
}

fn parse_path(path: &Path, source: &str) -> Result<Config, String> {
    Config::parse(source).map_err(|error| format!("{}: {error}", path.display()))
}

#[cfg(test)]
mod tests {
    use crate::{AppInstanceId, PhysicalWindowId, Rect};

    use super::*;

    fn window(title: Option<&str>) -> ObservedWindow {
        ObservedWindow {
            id: PhysicalWindowId {
                app: AppInstanceId {
                    pid: 1,
                    launched_at: 1,
                },
                window_id: 1,
            },
            frame: Rect::default(),
            resizable: true,
            title: title.map(std::sync::Arc::from),
        }
    }

    #[test]
    fn ordered_rules_match_all_supplied_fields() {
        let config = Config::parse(
            r#"
version = 1

[[rules]]
bundle_id = "com.example.App"
title = "Settings"
action = "float"

[[rules]]
bundle_id = "com.example.App"
action = "route"
workspace = 7
"#,
        )
        .unwrap();

        assert_eq!(
            config.disposition("com.example.App", &window(Some("Settings"))),
            WindowDisposition::Floating
        );
        assert_eq!(
            config.disposition("com.example.App", &window(Some("Document"))),
            WindowDisposition::Route(WorkspaceId::new(7).unwrap())
        );
    }

    #[test]
    fn tile_rule_parses_and_rejects_a_workspace() {
        let config = Config::parse(
            "version = 1\n[[rules]]\nbundle_id = 'com.apple.systempreferences'\naction = 'tile'",
        )
        .unwrap();
        assert_eq!(
            config.disposition("com.apple.systempreferences", &window(None)),
            WindowDisposition::Tiled
        );
        assert_eq!(
            config.disposition("com.example.Other", &window(None)),
            WindowDisposition::Managed
        );
        assert_eq!(
            Config::parse(
                "version = 1\n[[rules]]\nbundle_id = 'x'\naction = 'tile'\nworkspace = 2"
            )
            .err()
            .as_deref(),
            Some("tile rule cannot specify workspace")
        );
    }

    #[test]
    fn rejects_invalid_rules_and_workspaces() {
        assert!(Config::parse("version = 2").is_err());
        assert!(Config::parse("version = 1\n[[rules]]\naction = 'float'").is_err());
        assert!(
            Config::parse(
                "version = 1\n[[rules]]\nbundle_id = 'x'\naction = 'route'\nworkspace = 11"
            )
            .is_err()
        );
        assert!(
            Config::parse(
                "version = 1\n[[rules]]\nbundle_id = 'x'\naction = 'route'\nworkspace = 'AA'"
            )
            .is_err()
        );
    }

    #[test]
    fn workspace_order_accepts_numbers_and_letters_in_the_given_order() {
        let config = Config::parse(
            "version = 1\n[settings]\nworkspace_order = [1, \"Q\", \"w\", \"10\", \"E\"]",
        )
        .unwrap();
        assert_eq!(
            config.workspace_order(),
            [
                WorkspaceId::ONE,
                WorkspaceId::named('Q').unwrap(),
                WorkspaceId::named('W').unwrap(),
                WorkspaceId::new(10).unwrap(),
                WorkspaceId::named('E').unwrap(),
            ]
        );
        assert!(
            Config::parse("version = 1")
                .unwrap()
                .workspace_order()
                .is_empty()
        );
    }

    #[test]
    fn workspace_order_rejects_duplicate_and_unknown_workspaces() {
        let duplicate =
            Config::parse("version = 1\n[settings]\nworkspace_order = [\"Q\", \"W\", \"q\"]")
                .unwrap_err();
        assert!(
            duplicate.contains("workspace Q appears more than once in settings.workspace_order"),
            "{duplicate}"
        );
        let numbered =
            Config::parse("version = 1\n[settings]\nworkspace_order = [3, \"3\"]").unwrap_err();
        assert!(
            numbered.contains("workspace 3 appears more than once"),
            "{numbered}"
        );
        let unknown =
            Config::parse("version = 1\n[settings]\nworkspace_order = [\"AA\"]").unwrap_err();
        assert!(
            unknown.contains("workspace must be 1 through 10 or A through Z"),
            "{unknown}"
        );
    }

    #[test]
    fn accepts_named_workspace_routes_and_assignments() {
        let config = Config::parse(
            r#"
version = 1
[[rules]]
bundle_id = "ai.opencode.desktop"
action = "route"
workspace = "D"
[[workspaces]]
workspace = "D"
monitor = "main"
"#,
        )
        .unwrap();
        assert_eq!(
            config.disposition("ai.opencode.desktop", &window(None)),
            WindowDisposition::Route(WorkspaceId::named('D').unwrap())
        );
        assert_eq!(
            config.named_workspaces().collect::<Vec<_>>(),
            vec![
                WorkspaceId::named('D').unwrap(),
                WorkspaceId::named('D').unwrap()
            ]
        );
    }

    #[test]
    fn rejects_obsolete_keyboard_mapping_override() {
        assert!(Config::parse("version = 1\nkey_mapping = 'dvorak'").is_err());
    }

    #[test]
    fn parses_hud_setting_with_enabled_default() {
        assert!(Config::default().hud());
        assert!(Config::parse("version = 1").unwrap().hud());
        assert!(Config::parse("version = 1\n[settings]\n").unwrap().hud());
        assert!(
            !Config::parse("version = 1\n[settings]\nhud = false\n")
                .unwrap()
                .hud()
        );
        assert!(Config::parse("version = 1\n[settings]\nunknown = true\n").is_err());
    }

    #[test]
    fn parses_hud_style_with_icons_default() {
        assert_eq!(Config::default().hud_style(), HudStyle::Icons);
        assert_eq!(
            Config::parse("version = 1").unwrap().hud_style(),
            HudStyle::Icons
        );
        assert_eq!(
            Config::parse("version = 1\n[settings]\nhud = false\n")
                .unwrap()
                .hud_style(),
            HudStyle::Icons
        );
        for style in [HudStyle::Layouts, HudStyle::Icons] {
            let source = format!(
                "version = 1\n[settings]\nhud_style = {:?}\n",
                style.as_str()
            );
            assert_eq!(Config::parse(&source).unwrap().hud_style(), style);
            assert_eq!(HudStyle::parse(style.as_str()), Some(style));
        }
        assert!(Config::parse("version = 1\n[settings]\nhud_style = \"grid\"\n").is_err());
        assert!(Config::parse("version = 1\n[settings]\nhud_style = true\n").is_err());
        assert_eq!(HudStyle::parse("Icons"), None);
    }

    #[test]
    fn parses_hud_position_with_bottom_default() {
        assert_eq!(Config::default().hud_position(), HudPosition::Bottom);
        assert_eq!(
            Config::parse("version = 1").unwrap().hud_position(),
            HudPosition::Bottom
        );
        assert_eq!(
            Config::parse("version = 1\n[settings]\nhud = false\n")
                .unwrap()
                .hud_position(),
            HudPosition::Bottom
        );
        for position in [HudPosition::Bottom, HudPosition::Top] {
            let source = format!(
                "version = 1\n[settings]\nhud_position = {:?}\n",
                position.as_str()
            );
            assert_eq!(Config::parse(&source).unwrap().hud_position(), position);
            assert_eq!(HudPosition::parse(position.as_str()), Some(position));
        }
        assert!(Config::parse("version = 1\n[settings]\nhud_position = \"center\"\n").is_err());
        assert!(Config::parse("version = 1\n[settings]\nhud_position = true\n").is_err());
        assert_eq!(HudPosition::parse("Top"), None);
    }

    #[test]
    fn saving_a_workspace_monitor_keeps_hud_position() {
        let dir = std::env::temp_dir().join(format!(
            "spaceman-config-hud-position-{}-{}",
            std::process::id(),
            now().as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.toml");
        std::fs::write(&path, "version = 1\n[settings]\nhud_position = \"top\"\n").unwrap();
        let saved = save_workspace_monitor_at(&path, WorkspaceId::TWO, &DisplaySelector::Secondary)
            .unwrap();
        assert_eq!(saved.hud_position(), HudPosition::Top);
        assert!(
            std::fs::read_to_string(&path)
                .unwrap()
                .contains("hud_position = \"top\"")
        );
        drop(std::fs::remove_dir_all(dir));
    }

    #[test]
    fn parses_labs_with_the_scratchpad_off_by_default() {
        for source in ["version = 1", "version = 1\n[labs]\n"] {
            let config = Config::parse(source).unwrap();
            assert!(!config.labs().scratchpad);
            assert!(!config.shortcuts().scratchpad);
        }
        assert!(!Config::default().labs().scratchpad);
        let config = Config::parse("version = 1\n[labs]\nscratchpad = true\n").unwrap();
        assert!(config.labs().scratchpad);
        assert!(
            config.shortcuts().scratchpad,
            "the Lab's shortcuts follow its switch"
        );
        assert!(Config::parse("version = 1\n[labs]\nwarp_drive = true\n").is_err());
        assert!(Config::parse("version = 1\n[labs]\nscratchpad = 1\n").is_err());
    }

    #[test]
    fn saving_a_workspace_monitor_keeps_the_labs_table() {
        let directory = std::env::temp_dir().join(format!(
            "spaceman-labs-{}-{}",
            std::process::id(),
            now().as_nanos()
        ));
        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join("config.toml");
        std::fs::write(&path, "version = 1\n[labs]\nscratchpad = true\n").unwrap();

        let saved = save_workspace_monitor_at(&path, WorkspaceId::ONE, &DisplaySelector::Secondary)
            .unwrap();

        assert!(saved.labs().scratchpad);
        assert!(load_path(&path).unwrap().labs().scratchpad);
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn parses_update_channel_with_stable_default() {
        assert_eq!(Config::default().update_channel(), UpdateChannel::Stable);
        assert_eq!(
            Config::parse("version = 1").unwrap().update_channel(),
            UpdateChannel::Stable
        );
        for channel in [UpdateChannel::Stable, UpdateChannel::Dev] {
            let source = format!(
                "version = 1\n[settings]\nupdate_channel = {:?}\n",
                channel.as_str()
            );
            assert_eq!(Config::parse(&source).unwrap().update_channel(), channel);
            assert_eq!(UpdateChannel::parse(channel.as_str()), Some(channel));
        }
        assert!(Config::parse("version = 1\n[settings]\nupdate_channel = \"beta\"\n").is_err());
    }

    #[test]
    fn parses_mouse_follows_focus_with_off_default() {
        assert_eq!(
            Config::default().mouse_follows_focus(),
            MouseFollowsFocus::Off
        );
        assert_eq!(
            Config::parse("version = 1").unwrap().mouse_follows_focus(),
            MouseFollowsFocus::Off
        );
        for mode in [
            MouseFollowsFocus::Off,
            MouseFollowsFocus::Display,
            MouseFollowsFocus::Window,
        ] {
            let source = format!(
                "version = 1\n[settings]\nmouse_follows_focus = {:?}\n",
                mode.as_str()
            );
            assert_eq!(Config::parse(&source).unwrap().mouse_follows_focus(), mode);
            assert_eq!(MouseFollowsFocus::parse(mode.as_str()), Some(mode));
        }
        assert!(
            Config::parse("version = 1\n[settings]\nmouse_follows_focus = \"monitor\"\n").is_err()
        );
        assert!(Config::parse("version = 1\n[settings]\nmouse_follows_focus = true\n").is_err());
        assert_eq!(MouseFollowsFocus::parse("Display"), None);
    }

    #[test]
    fn saving_a_workspace_monitor_keeps_mouse_follows_focus() {
        let dir = std::env::temp_dir().join(format!(
            "spaceman-config-mff-{}-{}",
            std::process::id(),
            now().as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.toml");
        std::fs::write(
            &path,
            "version = 1\n[settings]\nmouse_follows_focus = \"display\"\n",
        )
        .unwrap();
        let saved = save_workspace_monitor_at(&path, WorkspaceId::TWO, &DisplaySelector::Secondary)
            .unwrap();
        assert_eq!(saved.mouse_follows_focus(), MouseFollowsFocus::Display);
        drop(std::fs::remove_dir_all(dir));
    }

    #[test]
    fn saving_a_workspace_monitor_keeps_the_workspace_order() {
        let dir = std::env::temp_dir().join(format!(
            "spaceman-config-order-{}-{}",
            std::process::id(),
            now().as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.toml");
        std::fs::write(
            &path,
            "version = 1\n[settings]\nworkspace_order = [1, \"Q\", \"W\"]\n",
        )
        .unwrap();
        let saved = save_workspace_monitor_at(&path, WorkspaceId::TWO, &DisplaySelector::Secondary)
            .unwrap();
        assert_eq!(
            saved.workspace_order(),
            [
                WorkspaceId::ONE,
                WorkspaceId::named('Q').unwrap(),
                WorkspaceId::named('W').unwrap(),
            ]
        );
        drop(std::fs::remove_dir_all(dir));
    }

    #[test]
    fn save_workspace_monitor_persists_overrides_and_clears_built_in_defaults() {
        let dir = std::env::temp_dir().join(format!(
            "spaceman-config-save-{}-{}",
            std::process::id(),
            now().as_nanos()
        ));
        let path = dir.join("config.toml");

        let saved = save_workspace_monitor_at(&path, WorkspaceId::ONE, &DisplaySelector::Secondary)
            .unwrap();
        let mut workspaces = Workspaces::default();
        saved.configure_workspaces(&mut workspaces);
        assert_eq!(
            workspaces.assignment(WorkspaceId::ONE),
            &DisplaySelector::Secondary
        );
        let written = std::fs::read_to_string(&path).unwrap();
        assert!(written.contains("monitor = \"secondary\""), "{written}");

        let reset =
            save_workspace_monitor_at(&path, WorkspaceId::ONE, &DisplaySelector::Main).unwrap();
        reset.configure_workspaces(&mut workspaces);
        assert_eq!(
            workspaces.assignment(WorkspaceId::ONE),
            &DisplaySelector::Main
        );
        let written = std::fs::read_to_string(&path).unwrap();
        assert!(!written.contains("[[workspaces]]"), "{written}");

        let named = WorkspaceId::named('D').unwrap();
        let kept = save_workspace_monitor_at(&path, named, &DisplaySelector::Main).unwrap();
        assert_eq!(kept.named_workspaces().collect::<Vec<_>>(), vec![named]);
        let written = std::fs::read_to_string(&path).unwrap();
        assert!(written.contains("workspace = \"D\""), "{written}");

        drop(std::fs::remove_dir_all(dir));
    }

    #[test]
    fn save_workspace_monitor_preserves_symlink_target_without_sidecar_files() {
        let dir = std::env::temp_dir().join(format!(
            "spaceman-config-symlink-{}-{}",
            std::process::id(),
            now().as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let target = dir.join("dotfiles.toml");
        let link = dir.join("config.toml");
        std::fs::write(&target, "version = 1\n").unwrap();
        std::os::unix::fs::symlink("dotfiles.toml", &link).unwrap();

        save_workspace_monitor_at(&link, WorkspaceId::TWO, &DisplaySelector::Secondary).unwrap();

        assert!(
            std::fs::symlink_metadata(&link)
                .unwrap()
                .file_type()
                .is_symlink()
        );
        let written = std::fs::read_to_string(&target).unwrap();
        assert!(written.contains("monitor = \"secondary\""), "{written}");
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 2);

        drop(std::fs::remove_dir_all(dir));
    }

    #[test]
    fn spacing_defaults_to_none_and_parses_every_key() {
        assert!(Config::default().spacing().is_none());
        assert!(Config::parse("version = 1").unwrap().spacing().is_none());
        assert!(
            Config::parse("version = 1\n[settings]\nhud = false\n")
                .unwrap()
                .spacing()
                .is_none()
        );
        let config = Config::parse(
            r#"
version = 1
[settings]
single_window_max_width = 1600
[settings.gaps]
inner = 8
top = 32
bottom = 4
left = 0
right = 128
"#,
        )
        .unwrap();
        assert_eq!(
            config.spacing(),
            Spacing {
                inner: 8,
                top: 32,
                bottom: 4,
                left: 0,
                right: 128,
                single_window_max_width: 1600,
            }
        );
        let partial = Config::parse("version = 1\n[settings.gaps]\ninner = 12\n").unwrap();
        assert_eq!(partial.spacing().inner, 12);
        assert_eq!(partial.spacing().top, 0);
    }

    #[test]
    fn spacing_rejects_out_of_range_and_unknown_keys_with_clear_messages() {
        for (source, message) in [
            (
                "[settings.gaps]\ninner = -1",
                "settings.gaps.inner must be between 0 and 48 px, got -1",
            ),
            (
                "[settings.gaps]\ninner = 49",
                "settings.gaps.inner must be between 0 and 48 px, got 49",
            ),
            (
                "[settings.gaps]\ntop = 129",
                "settings.gaps.top must be between 0 and 128 px, got 129",
            ),
            (
                "[settings.gaps]\nright = -4",
                "settings.gaps.right must be between 0 and 128 px, got -4",
            ),
            (
                "[settings]\nsingle_window_max_width = 100",
                "settings.single_window_max_width must be 0 (off) or between 320 and 16384 px, got 100",
            ),
            (
                "[settings]\nsingle_window_max_width = 99999",
                "settings.single_window_max_width must be 0 (off) or between 320 and 16384 px, got 99999",
            ),
        ] {
            let error = Config::parse(&format!("version = 1\n{source}\n")).unwrap_err();
            assert_eq!(error, message);
        }
        assert!(Config::parse("version = 1\n[settings.gaps]\nvertical = 4\n").is_err());
        assert!(Config::parse("version = 1\n[settings.gaps]\ninner = 1.5\n").is_err());
    }

    /// Top, bottom, left and right as `gaps` (the keys under `[settings.gaps]`) resolve them.
    fn edges(gaps: &str) -> [i32; 4] {
        let source = format!("version = 1\n[settings.gaps]\n{gaps}\n");
        let spacing = Config::parse(&source).unwrap().spacing();
        SpacingKey::EDGES.map(|key| key.get(&spacing))
    }

    #[test]
    fn outer_pads_only_the_edges_the_file_leaves_out() {
        assert_eq!(edges("inner = 6\nouter = 8\ntop = 0"), [0, 8, 8, 8]);
        assert_eq!(edges("outer = 8"), [8, 8, 8, 8]);
        assert_eq!(edges("outer = 128"), [128, 128, 128, 128]);
        assert_eq!(
            edges("outer = 8\ntop = 1\nbottom = 2\nleft = 3\nright = 4"),
            [1, 2, 3, 4]
        );
        assert_eq!(
            edges("outer = 8\ntop = 0\nbottom = 0\nleft = 0\nright = 0"),
            [0, 0, 0, 0]
        );
        // It pads edges only, and an unset or zero `outer` leaves them where they were.
        let gaps = "version = 1\n[settings.gaps]\nouter = 8\n";
        let spacing = Config::parse(gaps).unwrap().spacing();
        assert_eq!((spacing.inner, spacing.single_window_max_width), (0, 0));
        assert_eq!(edges("top = 32\nright = 12"), [32, 0, 0, 12]);
        assert_eq!(edges("outer = 0\ntop = 32\nright = 12"), [32, 0, 0, 12]);
    }

    #[test]
    fn outer_takes_the_same_range_as_an_edge() {
        for (gaps, message) in [
            (
                "outer = -1",
                "settings.gaps.outer must be between 0 and 128 px, got -1",
            ),
            (
                "outer = 129",
                "settings.gaps.outer must be between 0 and 128 px, got 129",
            ),
            (
                "outer = 129\ntop = 1\nbottom = 2\nleft = 3\nright = 4",
                "settings.gaps.outer must be between 0 and 128 px, got 129",
            ),
            (
                "outer = 8\ntop = 129",
                "settings.gaps.top must be between 0 and 128 px, got 129",
            ),
        ] {
            let error = Config::parse(&format!("version = 1\n[settings.gaps]\n{gaps}\n"));
            assert_eq!(error.unwrap_err(), message);
        }
        for outer in ["8.0", "\"8\"", "true"] {
            let source = format!("version = 1\n[settings.gaps]\nouter = {outer}\n");
            assert!(Config::parse(&source).is_err(), "{source}");
        }
    }

    #[test]
    fn saving_a_workspace_monitor_keeps_spacing() {
        let dir = std::env::temp_dir().join(format!(
            "spaceman-config-spacing-{}-{}",
            std::process::id(),
            now().as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.toml");
        std::fs::write(
            &path,
            "version = 1\n[settings]\nsingle_window_max_width = 1440\n[settings.gaps]\ninner = 6\nouter = 8\ntop = 0\nleft = 20\n",
        )
        .unwrap();
        let saved = save_workspace_monitor_at(&path, WorkspaceId::TWO, &DisplaySelector::Secondary)
            .unwrap();
        assert_eq!(saved.spacing().inner, 6);
        assert_eq!(saved.spacing().left, 20);
        assert_eq!(saved.spacing().single_window_max_width, 1440);
        let written = std::fs::read_to_string(&path).unwrap();
        let written: toml::Table = toml::from_str(&written).unwrap();
        assert_eq!(
            serde_json::to_value(&written["settings"]["gaps"]).unwrap(),
            serde_json::json!({ "inner": 6, "outer": 8, "top": 0, "left": 20 }),
            "the shorthand stays a shorthand"
        );
        drop(std::fs::remove_dir_all(dir));
    }

    #[test]
    fn saving_a_workspace_monitor_keeps_shortcuts() {
        let dir = std::env::temp_dir().join(format!(
            "spaceman-config-shortcuts-{}-{}",
            std::process::id(),
            now().as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.toml");
        std::fs::write(
            &path,
            "version = 1\n[shortcuts]\ndefaults = false\n[shortcuts.bindings]\n\"alt+left\" = \"move-direction left\"\n",
        )
        .unwrap();
        let saved = save_workspace_monitor_at(&path, WorkspaceId::TWO, &DisplaySelector::Secondary)
            .unwrap();
        assert!(!saved.shortcuts().defaults);
        assert_eq!(saved.shortcuts().bindings.len(), 1);
        drop(std::fs::remove_dir_all(dir));
    }

    #[test]
    fn shortcuts_default_to_built_ins_and_reject_bad_bindings() {
        assert_eq!(
            Config::parse("version = 1").unwrap().shortcuts(),
            &crate::shortcuts::Shortcuts::default()
        );
        for (source, message) in [
            (
                "[shortcuts.bindings]\n\"super+q\" = \"float\"",
                "unknown modifier",
            ),
            (
                "[shortcuts.bindings]\n\"alt+q\" = \"close\"",
                "unknown command",
            ),
            (
                "[shortcuts]\ndefaults = false",
                "needs at least one binding",
            ),
            ("[shortcuts]\nmode = \"replace\"", "unknown field"),
        ] {
            let error = Config::parse(&format!("version = 1\n{source}")).unwrap_err();
            assert!(error.contains(message), "{source}: {error}");
        }
    }

    /// A global shortcut fires in every app, so a chord without cmd, ctrl, or alt would eat
    /// ordinary typing. Unbinding is the only exception, and it never registers a chord.
    #[test]
    fn shortcuts_never_bind_a_plain_or_shift_only_key() {
        for chord in [
            "\"h\"",
            "\"shift+h\"",
            "\"shift+left\"",
            "\"shift+/\"",
            "\"shift+1\"",
            "\"space\"",
        ] {
            let source = format!("version = 1\n[shortcuts.bindings]\n{chord} = \"float\"");
            let error = Config::parse(&source).unwrap_err();
            assert!(
                error.contains("needs at least one of alt, cmd, or ctrl"),
                "{chord}: {error}"
            );
        }
        assert!(Config::parse("version = 1\n[shortcuts.bindings]\n\"alt+h\" = \"none\"").is_ok());
    }
}
