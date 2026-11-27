use std::collections::HashSet;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

mod choices;
mod disposition;
mod labs;
mod write;
pub use choices::{MenuBarIcon, MouseFollowsFocus};
pub use disposition::{DispositionPatch, Management};
pub use labs::{LabFeature, LabsConfig};
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
    layout: LayoutMode,
    update_channel: UpdateChannel,
    mouse_follows_focus: MouseFollowsFocus,
    trackpad_swipe: bool,
    menu_bar_icon: MenuBarIcon,
    pause_on_builtin_display: bool,
    spacing: Spacing,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            hud: true,
            hud_style: HudStyle::default(),
            hud_position: HudPosition::default(),
            layout: LayoutMode::default(),
            update_channel: UpdateChannel::default(),
            mouse_follows_focus: MouseFollowsFocus::default(),
            trackpad_swipe: true,
            menu_bar_icon: MenuBarIcon::default(),
            pause_on_builtin_display: false,
            spacing: Spacing::default(),
        }
    }
}

/// How newly opened tiled windows split the workspace: `settings.layout`.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum LayoutMode {
    /// Binary space partition: each new window splits the active tile along its longer side.
    #[default]
    Dwindle,
    /// Side-by-side columns: each new window joins the active tile's split as a sibling.
    Columns,
}

impl LayoutMode {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Dwindle => "dwindle",
            Self::Columns => "columns",
        }
    }

    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "dwindle" => Some(Self::Dwindle),
            "columns" => Some(Self::Columns),
            _ => None,
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
    /// Native size, owned by one workspace: parks and restores with it.
    Floating,
    /// Floating, and shown on every workspace of its display: never parked by a switch.
    Pinned,
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
    /// `windows = "all"`: a `route` rule that routes every new window of its app, read only
    /// with `[labs] route_all_windows` on.
    every_window: bool,
}

/// A `route` rule's `windows` key: which of an app's new windows it routes.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum RouteWindows {
    /// Only the app's first window (the default without the key).
    First,
    /// Every new window, read only with `[labs] route_all_windows` on.
    All,
}

impl RouteWindows {
    /// The value as `config.toml` writes it.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::First => "first",
            Self::All => "all",
        }
    }

    /// Parses `"first"` or `"all"`.
    #[must_use]
    pub fn parse(raw: &str) -> Option<Self> {
        [Self::First, Self::All]
            .into_iter()
            .find(|windows| windows.as_str() == raw)
    }
}

#[derive(Clone, Debug)]
struct WindowMatcher {
    bundle_id: Option<String>,
    title: Option<String>,
    /// Lowercased; matched case-insensitively against the live title, in memory only.
    title_contains: Option<String>,
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
    layout: LayoutMode,
    #[serde(default)]
    update_channel: UpdateChannel,
    #[serde(default)]
    mouse_follows_focus: MouseFollowsFocus,
    #[serde(default = "default_trackpad_swipe")]
    trackpad_swipe: bool,
    #[serde(default)]
    menu_bar_icon: MenuBarIcon,
    #[serde(default)]
    pause_on_builtin_display: bool,
    #[serde(default)]
    single_window_max_width: i64,
    #[serde(default)]
    single_window_max_height: i64,
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
            layout: LayoutMode::default(),
            update_channel: UpdateChannel::default(),
            mouse_follows_focus: MouseFollowsFocus::default(),
            trackpad_swipe: true,
            menu_bar_icon: MenuBarIcon::default(),
            pause_on_builtin_display: false,
            single_window_max_width: 0,
            single_window_max_height: 0,
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
            (
                SpacingKey::SingleWindowMaxHeight,
                Some(self.single_window_max_height),
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

const fn default_trackpad_swipe() -> bool {
    true
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawWindowRule {
    bundle_id: Option<String>,
    title: Option<String>,
    title_contains: Option<String>,
    action: String,
    workspace: Option<WorkspaceId>,
    windows: Option<RouteWindows>,
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
            if rule.bundle_id.is_none() && rule.title.is_none() && rule.title_contains.is_none() {
                return Err("window rule must contain at least one matcher".into());
            }
            if rule.title_contains.as_deref().is_some_and(str::is_empty) {
                return Err("title_contains cannot be empty".into());
            }
            let action = match (rule.action.as_str(), rule.workspace) {
                ("float", None) => WindowDisposition::Floating,
                ("pin", None) => WindowDisposition::Pinned,
                ("ignore", None) => WindowDisposition::Ignored,
                ("tile", None) => WindowDisposition::Tiled,
                ("route", Some(workspace)) => WindowDisposition::Route(workspace),
                ("float", Some(_)) => return Err("float rule cannot specify workspace".into()),
                ("pin", Some(_)) => return Err("pin rule cannot specify workspace".into()),
                ("ignore", Some(_)) => return Err("ignore rule cannot specify workspace".into()),
                ("tile", Some(_)) => return Err("tile rule cannot specify workspace".into()),
                ("route", None) => return Err("route rule requires workspace".into()),
                (action, _) => return Err(format!("unknown window rule action {action:?}")),
            };
            if rule.windows.is_some() && !matches!(action, WindowDisposition::Route(_)) {
                return Err(format!("{} rule cannot specify windows", rule.action));
            }
            rules.push(WindowRule {
                matcher: WindowMatcher {
                    bundle_id: rule.bundle_id,
                    title: rule.title,
                    title_contains: rule.title_contains.map(|value| value.to_lowercase()),
                },
                action,
                every_window: rule.windows == Some(RouteWindows::All),
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
        shortcuts.workspace_steps = raw.labs.workspace_steps;
        Ok(Self {
            settings: AppSettings {
                hud: raw.settings.hud,
                hud_style: raw.settings.hud_style,
                hud_position: raw.settings.hud_position,
                layout: raw.settings.layout,
                update_channel: raw.settings.update_channel,
                mouse_follows_focus: raw.settings.mouse_follows_focus,
                trackpad_swipe: raw.settings.trackpad_swipe,
                menu_bar_icon: raw.settings.menu_bar_icon,
                pause_on_builtin_display: raw.settings.pause_on_builtin_display,
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
    pub const fn layout(&self) -> LayoutMode {
        self.settings.layout
    }

    #[must_use]
    pub const fn update_channel(&self) -> UpdateChannel {
        self.settings.update_channel
    }

    #[must_use]
    pub const fn mouse_follows_focus(&self) -> MouseFollowsFocus {
        self.settings.mouse_follows_focus
    }

    /// Whether a three-finger horizontal swipe steps through workspaces. Always on unless
    /// `[labs] swipe_switch` is on, which hands the choice to `settings.trackpad_swipe`
    /// (default: on); off leaves the gesture to macOS alone.
    #[must_use]
    pub const fn trackpad_swipe(&self) -> bool {
        !self.labs.swipe_switch || self.settings.trackpad_swipe
    }

    /// How the menu bar icon looks: always [`MenuBarIcon::Full`] unless
    /// `[labs] menu_bar_icon_style` is on, which hands the choice to `settings.menu_bar_icon`.
    #[must_use]
    pub const fn menu_bar_icon(&self) -> MenuBarIcon {
        if self.labs.menu_bar_icon_style {
            self.settings.menu_bar_icon
        } else {
            MenuBarIcon::Full
        }
    }

    #[must_use]
    pub const fn pause_on_builtin_display(&self) -> bool {
        self.settings.pause_on_builtin_display
    }

    /// How many fingers the workspace swipe takes: three, or four with
    /// `[labs] four_finger_swipe` on. Swipes with the other count are left to macOS.
    #[must_use]
    pub const fn swipe_fingers(&self) -> crate::swipe::Fingers {
        if self.labs.four_finger_swipe {
            crate::swipe::Fingers::Four
        } else {
            crate::swipe::Fingers::Three
        }
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

    /// Gaps between tiles, outer screen padding and the lone-window caps. The height cap
    /// counts only with `[labs] single_window_height` on; with it off it reads as `0` (off).
    #[must_use]
    pub const fn spacing(&self) -> Spacing {
        let mut spacing = self.settings.spacing;
        if !self.labs.single_window_height {
            spacing.single_window_max_height = 0;
        }
        spacing
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

    /// How `window` is handled: the first matching user rule, else the built-in default
    /// ([`crate::defaults`]), else managed.
    #[must_use]
    pub fn disposition(&self, bundle_id: &str, window: &ObservedWindow) -> WindowDisposition {
        self.matching_rule(bundle_id, window).map_or_else(
            || crate::defaults::disposition(bundle_id),
            |rule| rule.action,
        )
    }

    /// What the rules and built-in defaults say about `window`: the first matching user rule
    /// over the built-in default. Callers add the window's traits with
    /// [`DispositionPatch::over`].
    #[must_use]
    pub fn patch(&self, bundle_id: &str, window: &ObservedWindow) -> DispositionPatch {
        let rule = self
            .matching_rule(bundle_id, window)
            .map_or(DispositionPatch::NONE, |rule| rule.action.into());
        rule.over(crate::defaults::patch(bundle_id))
    }

    /// Whether the `route` rule matching `window` routes it even when its app already has a
    /// window: a title-matched route, or one with `windows = "all"` while
    /// `[labs] route_all_windows` is on. A bundle-level route otherwise takes only an app's
    /// first window.
    #[must_use]
    pub fn routes_every_window(&self, bundle_id: &str, window: &ObservedWindow) -> bool {
        self.matching_rule(bundle_id, window).is_some_and(|rule| {
            matches!(rule.action, WindowDisposition::Route(_))
                && (rule.matcher.matches_titles()
                    || (rule.every_window && self.labs.route_all_windows))
        })
    }

    #[must_use]
    pub fn has_bundle_route(&self, bundle_id: &str) -> bool {
        self.rules
            .iter()
            .find(|rule| {
                !rule.matcher.matches_titles()
                    && rule
                        .matcher
                        .bundle_id
                        .as_deref()
                        .is_some_and(|value| value == bundle_id)
            })
            .is_some_and(|rule| matches!(rule.action, WindowDisposition::Route(_)))
    }

    pub fn configure_workspaces(&self, workspaces: &mut Workspaces) {
        self.configure_workspaces_with(workspaces, |_, _, _| false);
    }

    /// Applies the named workspaces and display assignments. keeps_front says
    /// whether a workspace is shown and its old and new selectors name the one
    /// connected display showing it, so it stays in front when only its
    /// selector changes.
    pub(crate) fn configure_workspaces_with(
        &self,
        workspaces: &mut Workspaces,
        keeps_front: impl Fn(WorkspaceId, &DisplaySelector, &DisplaySelector) -> bool,
    ) {
        let named: HashSet<_> = self.named_workspaces().collect();
        workspaces.retain_named(|workspace| named.contains(&workspace));
        for workspace in named {
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
            workspaces.set_assignment_with(workspace, target, &keeps_front);
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
            && self.title_contains.as_deref().is_none_or(|needle| {
                window
                    .title
                    .as_deref()
                    .is_some_and(|title| title.to_lowercase().contains(needle))
            })
    }

    /// Whether this matcher looks at window titles, so it covers only some of an app's windows.
    const fn matches_titles(&self) -> bool {
        self.title.is_some() || self.title_contains.is_some()
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
mod tests;
