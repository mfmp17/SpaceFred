//! Spaceman's single-process macOS agent (`spaceman-agent`): runs the window
//! manager, native status bar item, updater, Settings window, and Rune
//! Playground (`⇧⌘D` / `--playground`) on one AppKit + Rune event loop.

#[cfg(not(test))]
#[global_allocator]
static ALLOCATOR: procwatch::alloc::Counting = procwatch::alloc::Counting::system();

mod activity;
mod apps;
mod controls;
#[cfg(target_os = "macos")]
mod host;
mod hud;
mod map;
mod onboarding;
mod panes;
mod playground;
mod rules;
mod updates;
mod view;

use std::sync::Arc;

pub use panes::Pane;
use rules::{Doc, Place};
use rune::{AppSpec, KeyEvent, PointerPress, Tx, Waker};
use spaceman::app::{UiAction, UiAgentSnapshot, UiBridge};

/// One fixed size: tiling window managers float such a window, and every pane fits it.
pub const WINDOW_SIZE: (f32, f32) = (860.0, 580.0);

/// The one anchored menu that is open.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Menu {
    /// Pick a running app to send to this place.
    Add(Place),
    /// A rule's context menu, at the secondary press.
    Rule(usize, PointerPress),
}

pub struct Ui {
    pub pane: Pane,
    /// Someone picked a pane, so the agent's first report doesn't move them.
    pub navigated: bool,
    pub doc: Doc,
    pub apps: apps::Catalog,
    /// `None` until the runtime publishes its first snapshot.
    pub agent: Option<UiAgentSnapshot>,
    pub activity: activity::ActivityPane,
    bridge: Option<UiBridge>,
    window: Option<rune::AppHandle<Ui>>,
    hud_driver: Option<hud::HudDriver>,
    pub hud_preview: Option<hud::HudPreview>,
    frame_remembered: bool,
    pub tour: Option<onboarding::Tour>,
    pub drag: Option<map::Drag>,
    pub menu: Option<Menu>,
    pub running: Vec<apps::Running>,
    /// Every connected display, as Settings › Workspaces offers them.
    pub(crate) displays: Vec<panes::workspaces::ConnectedDisplay>,
    /// When the display list was last read, on Rune's clock.
    pub(crate) displays_at: Option<f64>,
    /// Lab slug requested by a `spaceman://labs/<slug>` deep link, preserved
    /// cleanly for Labs cards to highlight.
    pub highlight_lab: Option<String>,
    /// Setting or feature slug requested by a `spaceman://` deep link.
    pub highlight_feature: Option<String>,
    /// The last save that failed.
    pub error: Option<String>,
    /// Settings › Layout while a slider is held: the preview follows it, the file waits for
    /// the release.
    pub(crate) spacing_draft: Option<spaceman::spacing::Spacing>,
    /// Whether the four edge sliders show, once someone opened or closed them.
    pub(crate) per_edge: Option<bool>,
    /// Settings › Shortcuts' recorder.
    pub(crate) shortcut_capture: panes::shortcuts::Capture,
    /// Scenes render fixed data and ignore the live agent and file.
    pub(crate) fixture: bool,
}

impl Ui {
    #[cfg(target_os = "macos")]
    fn load_live(
        doc: Doc,
        bridge: UiBridge,
        initial_tour: Option<onboarding::Step>,
        accessibility: bool,
        initial_target: Option<spaceman::deep_link::OpenTarget>,
    ) -> Ui {
        let mut ui = Ui {
            pane: Pane::Rules,
            navigated: false,
            doc,
            apps: apps::Catalog::default(),
            agent: None,
            activity: activity::ActivityPane::default(),
            bridge: Some(bridge),
            window: None,
            hud_driver: None,
            hud_preview: None,
            frame_remembered: false,
            tour: None,
            drag: None,
            menu: None,
            running: Vec::new(),
            displays: Vec::new(),
            displays_at: None,
            highlight_lab: None,
            highlight_feature: None,
            error: None,
            spacing_draft: None,
            per_edge: None,
            shortcut_capture: panes::shortcuts::Capture::Idle,
            fixture: false,
        };
        ui.look_up_apps();
        if let Some(step) = initial_tour {
            ui.tour = Some(onboarding::Tour::from_launch(step, accessibility, &mut ui));
        }
        if let Some(target) = initial_target {
            ui.open_target(target);
        }
        ui
    }

    pub fn start_tour(&mut self) {
        self.menu = None;
        self.drag = None;
        self.flush_pending_reload();
        // Re-running setup from Settings replaces a first-run tour, and that
        // tour's hold on automatic tiling ends with it.
        if self.tour.as_ref().is_some_and(|tour| !tour.flow.manual()) {
            onboarding::dismiss(self);
        }
        self.tour = Some(onboarding::Tour::manual(self));
    }

    pub fn select_pane(&mut self, pane: Pane) {
        self.pane = pane;
        self.navigated = true;
        self.menu = None;
        self.drag = None;
        self.flush_pending_reload();
        self.highlight_lab = None;
        self.highlight_feature = None;
        if pane == Pane::Workspaces {
            // Read the displays before the next frame paints them.
            self.displays_at = None;
        }
    }

    /// Opens Settings onto a validated `spaceman://` deep-link target.
    ///
    /// Security invariant: only changes UI pane selection and highlight state;
    /// never mutates `self.doc` or enables any setting/Lab.
    pub fn open_target(&mut self, target: spaceman::deep_link::OpenTarget) {
        onboarding::dismiss(self);
        self.hud_preview = None;
        let pane = match target.pane {
            Some(deep_pane) => match Pane::from_deep_link(deep_pane) {
                Some(pane) => pane,
                None => {
                    let line = format!(
                        "spaceman: deep link pane '{}' is not registered in this build; opening General\n",
                        deep_pane.as_str()
                    );
                    drop(std::io::Write::write_all(
                        &mut std::io::stderr().lock(),
                        line.as_bytes(),
                    ));
                    Pane::General
                }
            },
            None => {
                if let Some(note) = &target.diagnostic {
                    let line = format!("spaceman: deep link: {note}\n");
                    drop(std::io::Write::write_all(
                        &mut std::io::stderr().lock(),
                        line.as_bytes(),
                    ));
                }
                Pane::General
            }
        };
        self.select_pane(pane);
        self.highlight_lab = target.highlight_lab;
        self.highlight_feature = target.highlight_feature;
    }

    /// Releases the first-run setup tour's hold on automatic tiling.
    pub fn finish_setup(&self) {
        self.send_action(UiAction::FinishSetup);
    }

    fn send_action(&self, action: UiAction) {
        if !self.fixture
            && let Some(bridge) = &self.bridge
        {
            bridge.send(action);
        }
    }

    pub fn toggle_automatic(&self) {
        self.send_action(UiAction::ToggleAutomatic);
    }

    pub fn open_accessibility(&self) {
        self.send_action(UiAction::OpenAccessibility);
    }

    pub fn toggle_launch_at_login(&self) {
        self.send_action(UiAction::ToggleLaunchAtLogin);
    }

    pub fn check_for_updates(&self) {
        self.send_action(UiAction::CheckForUpdates);
    }

    /// Restores every window, then installs the ready update and relaunches.
    pub fn install_update(&self) {
        self.send_action(UiAction::InstallUpdate);
    }

    /// Switches to the update channel's release on offer.
    pub fn confirm_update_switch(&self) {
        self.send_action(UiAction::ConfirmUpdateSwitch);
    }

    /// Opens the release's download, for a copy that can't update itself.
    pub fn download_update(&self) {
        self.send_action(UiAction::DownloadUpdate);
    }

    /// Saves the channel, then checks it at once.
    pub fn set_update_channel(&mut self, channel: spaceman::config::UpdateChannel) {
        self.edit(|doc| doc.set_update_channel(channel));
        if self.error.is_none() {
            self.check_for_updates();
        }
    }

    /// Sets the Workspace HUD off (`None`) or on with `style` (`Some(style)`).
    pub fn set_hud(&mut self, mode: Option<spaceman::config::HudStyle>) {
        let enabled = mode.is_some();
        self.edit(|doc| {
            doc.set_hud(enabled);
            if let Some(style) = mode {
                doc.set_hud_style(style);
            }
        });
        if self.error.is_none() {
            if let Some(driver) = &self.hud_driver {
                driver.set_enabled(enabled);
                if let Some(style) = mode {
                    driver.set_style(style);
                }
            }
            self.send_action(UiAction::SetHud(enabled));
        }
    }

    /// Saves where the Workspace HUD capsule sits, then moves every panel to that edge.
    pub fn set_hud_position(&mut self, position: spaceman::config::HudPosition) {
        self.edit(|doc| doc.set_hud_position(position));
        if self.error.is_none()
            && let Some(driver) = &self.hud_driver
        {
            driver.set_position(position);
        }
    }

    /// Saves gaps, edge padding and the single-window width, then retiles so every display
    /// takes them at once.
    pub fn set_spacing(&mut self, spacing: spaceman::spacing::Spacing) {
        self.spacing_draft = None;
        if self.doc.spacing() == spacing {
            return;
        }
        self.edit(|doc| doc.set_spacing(spacing));
        if self.error.is_none() {
            self.send_action(UiAction::Retile);
        }
    }

    pub fn mark_wrong(&self) {
        self.send_action(UiAction::MarkWrong);
    }

    /// Names and icons for every app the rules mention, before the view needs them.
    pub(crate) fn look_up_apps(&mut self) {
        for rule in self.doc.rules() {
            if let Some(bundle_id) = &rule.bundle_id {
                self.apps.get(bundle_id);
            }
        }
    }

    /// Applies `edit` and saves against the latest file on disk under the config lock.
    pub fn edit(&mut self, edit: impl FnOnce(&mut Doc)) {
        self.try_edit(|doc| {
            edit(doc);
            Ok(())
        });
    }

    /// Applies a fallible `edit` against the latest file on disk under the config lock.
    pub fn try_edit(&mut self, edit: impl FnOnce(&mut Doc) -> Result<(), String>) {
        if self.fixture {
            let before = self.doc.clone();
            match edit(&mut self.doc) {
                Ok(()) => self.error = None,
                Err(error) => {
                    self.error = Some(error);
                    self.doc = before;
                }
            }
            self.look_up_apps();
            return;
        }
        match self.doc.edit_and_save(edit) {
            Ok(()) => self.error = None,
            Err(error) => self.error = Some(error),
        }
        self.sync_hud_from_doc();
        self.look_up_apps();
    }

    fn sync_hud_from_doc(&self) {
        if let Some(driver) = &self.hud_driver {
            driver.set_enabled(self.doc.hud());
            driver.set_style(self.doc.hud_style());
            driver.set_position(self.doc.hud_position());
        }
    }

    pub(crate) fn reload_doc(&mut self) {
        if self.fixture {
            return;
        }
        self.doc = self.doc.reloaded();
        self.look_up_apps();
        self.sync_hud_from_doc();
    }

    /// Handles a config-changed notification from the agent pump: reloads immediately when
    /// idle, or defers until an active Rules drag finishes.
    pub(crate) fn on_config_changed(&mut self) -> bool {
        self.doc.mark_reload_pending();
        if self.drag.is_none() {
            self.reload_doc();
            return true;
        }
        false
    }

    /// Applies a config reload that arrived while a Rules drag was in progress.
    pub(crate) fn flush_pending_reload(&mut self) {
        if self.drag.is_none() && self.doc.reload_pending() {
            self.reload_doc();
        }
    }

    pub fn open_add_menu(&mut self, place: Place) {
        let ruled: Vec<String> = self
            .doc
            .rules()
            .into_iter()
            .filter(|rule| rule.title.is_none())
            .filter_map(|rule| rule.bundle_id)
            .collect();
        self.running = apps::running()
            .into_iter()
            .filter(|app| !ruled.contains(&app.bundle_id))
            .collect();
        for app in &self.running {
            self.apps.get(&app.bundle_id);
        }
        self.menu = Some(Menu::Add(place));
    }
}

fn setup(ui: &mut Ui, waker: Waker) {
    if !ui.fixture
        && let Some(bridge) = &ui.bridge
    {
        bridge.set_waker(Arc::new(move || waker()));
    }
}

fn poll(ui: &mut Ui, clock: f64, tx: &mut Tx) -> bool {
    if !ui.fixture && !ui.frame_remembered {
        ui.frame_remembered = rune_mac::remember_frame("SpaceFred");
    }
    let mut changed = onboarding::poll(ui, tx);
    changed |= panes::workspaces::poll_displays(ui, clock);
    changed |= ui.activity.poll(
        ui.pane == Pane::Activity && ui.tour.is_none() && ui.hud_preview.is_none(),
        ui.fixture,
    );
    let Some(bridge) = ui.bridge.clone() else {
        return changed;
    };
    let news = bridge.take_news();
    if let Some(snapshot) = news.snapshot {
        if let Some(driver) = &ui.hud_driver {
            driver.set_enabled(snapshot.hud);
        }
        if !ui.navigated && ui.agent.is_none() && !snapshot.accessibility && ui.tour.is_none() {
            ui.pane = Pane::General;
        }
        ui.agent = Some(snapshot);
        changed = true;
        if ui.tour.is_some() {
            changed |= onboarding::poll(ui, tx);
        }
    }
    if let Some(hud_event) = news.hud
        && let Some(driver) = &ui.hud_driver
    {
        driver.trigger(hud_event);
    }
    if news.config_changed {
        changed |= ui.on_config_changed();
    } else if ui.doc.reload_pending() && ui.drag.is_none() {
        ui.reload_doc();
        changed = true;
    }
    changed
}

fn on_key(ui: &mut Ui, event: &KeyEvent, _tx: &mut Tx) -> bool {
    if event.key == rune::Key::Escape {
        if ui.drag.is_some() || ui.menu.is_some() {
            ui.drag = None;
            ui.menu = None;
            ui.flush_pending_reload();
            return true;
        }
        if ui.tour.is_some() {
            onboarding::dismiss(ui);
            return true;
        }
    }
    if ui.tour.is_none()
        && ui.hud_preview.is_none()
        && !event.mods.shift
        && !event.mods.alt
        && let rune::Key::Command(digit) = event.key
        && let Some(pane) = Pane::from_shortcut(digit)
    {
        ui.select_pane(pane);
        return true;
    }
    false
}

fn on_close(ui: &mut Ui, _tx: &mut Tx) {
    ui.drag = None;
    ui.menu = None;
    ui.flush_pending_reload();
    // Closing the setup window dismisses the tour, so a first-run tour starts
    // tiling now rather than holding it behind a window nobody can see.
    onboarding::dismiss(ui);
    if let Some(window) = &ui.window {
        window.set_dock_icon(false);
    }
}

#[cfg(target_os = "macos")]
fn on_reopen(ui: &mut Ui, _has_visible_windows: bool, _tx: &mut Tx) -> bool {
    if let Some(window) = &ui.window {
        host::bring_window_forward(window);
    }
    true
}

#[cfg(target_os = "macos")]
fn on_open_url(ui: &mut Ui, link: &str, _tx: &mut Tx) {
    match spaceman::deep_link::parse_url(link) {
        Ok(parsed) => {
            ui.open_target(parsed.resolve());
            if let Some(window) = &ui.window {
                host::bring_window_forward(window);
            }
        }
        Err(error) => {
            let line = format!("spaceman: ignored deep link ({error})\n");
            drop(std::io::Write::write_all(
                &mut std::io::stderr().lock(),
                line.as_bytes(),
            ));
        }
    }
}

fn spec() -> AppSpec<Ui> {
    AppSpec {
        title: "SpaceFred",
        size: WINDOW_SIZE,
        min_size: Some(WINDOW_SIZE),
        init: playground::blank_fixture,
        view: view::view,
        scenes: playground::scenes(),
        fonts: rune_theme_opencode::FONTS,
        families: rune_theme_opencode::FAMILIES,
        setup: Some(setup),
        poll: Some(poll),
        on_key: Some(on_key),
    }
}

fn base_app() -> rune::App<Ui> {
    let app = playground::attach(
        rune::App::new(spec())
            .chrome(rune::Chrome {
                full_size_content: true,
                traffic_lights: Some((18.0, 18.0)),
                drag_regions: true,
            })
            .window_options(rune::WindowOptions {
                min_size: Some(WINDOW_SIZE),
                max_size: Some(WINDOW_SIZE),
                resizable: false,
                ..Default::default()
            })
            .on_close(on_close)
            .dev_stats(activity::process_vitals)
            .standard_menus(),
    );
    let window = app.handle();
    window.update_if({
        let window = window.clone();
        move |ui, _| {
            ui.window = Some(window);
            false
        }
    });
    app
}

#[cfg(target_os = "macos")]
pub(crate) fn live_app(
    bridge: spaceman::app::UiBridge,
    initial_tour: Option<onboarding::Step>,
    accessibility: bool,
    initial_target: Option<spaceman::deep_link::OpenTarget>,
) -> rune::App<Ui> {
    let doc = Doc::load();
    let initial_hud = doc.hud();
    let initial_style = doc.hud_style();
    let initial_position = doc.hud_position();
    let hud_bridge = bridge.clone();
    let mut app = base_app()
        .init(move || Ui::load_live(doc, bridge, initial_tour, accessibility, initial_target))
        .on_reopen(on_reopen)
        .on_open_url(on_open_url);
    let hud_sink = Arc::new(hud::LiveHudSink::new(hud_bridge, app.handle()));
    let hud_driver = hud::install(
        &mut app,
        initial_hud,
        initial_style,
        initial_position,
        hud_sink,
    );
    app.handle().update_if(move |ui, _| {
        ui.hud_driver = Some(hud_driver);
        false
    });
    app
}

fn preview_mode() -> bool {
    rune::tooling::is_headless()
        || std::env::args()
            .skip(1)
            .any(|arg| matches!(arg.as_str(), "--playground" | "--help" | "-h"))
}

fn main() {
    if preview_mode() {
        base_app().run();
        return;
    }
    #[cfg(target_os = "macos")]
    if let Err(error) = host::run() {
        drop(std::io::Write::write_all(
            &mut std::io::stderr(),
            format!("SpaceFred failed: {error}\n").as_bytes(),
        ));
        std::process::exit(1);
    }
    #[cfg(not(target_os = "macos"))]
    base_app().run();
}

#[cfg(test)]
mod tests {
    use super::*;
    use spaceman::deep_link::parse_url;

    #[test]
    fn open_target_selects_pane_and_preserves_highlights_without_mutating_config() {
        let mut ui = playground::blank_fixture();
        let initial_hud = ui.doc.hud();
        let initial_rules = ui.doc.rules().len();

        let workspaces = parse_url("spaceman://settings/workspaces")
            .unwrap()
            .resolve();
        ui.open_target(workspaces);
        assert_eq!(ui.pane, Pane::Workspaces);
        assert_eq!(ui.highlight_lab, None);
        assert_eq!(ui.highlight_feature, None);

        let lab = parse_url("spaceman://labs/scrolling-strip")
            .unwrap()
            .resolve();
        ui.open_target(lab);
        // Labs pane isn't registered yet, so it falls back to General while
        // preserving highlight_lab for when Labs cards are present.
        assert_eq!(ui.pane, Pane::General);
        assert_eq!(ui.highlight_lab.as_deref(), Some("scrolling-strip"));
        assert_eq!(ui.highlight_feature.as_deref(), Some("scrolling-strip"));

        for (url, expected_feature) in [
            ("spaceman://settings/layout", None),
            ("spaceman://features/gaps", Some("gaps")),
            ("spaceman://features/max-width", Some("max-width")),
        ] {
            let target = parse_url(url).unwrap().resolve();
            ui.open_target(target);
            assert_eq!(ui.pane, Pane::Layout, "{url}");
            assert_eq!(ui.highlight_lab, None, "{url}");
            assert_eq!(ui.highlight_feature.as_deref(), expected_feature, "{url}");
        }

        let hud_feature = parse_url("spaceman://features/hud").unwrap().resolve();
        ui.open_target(hud_feature);
        assert_eq!(ui.pane, Pane::General);
        assert_eq!(ui.highlight_lab, None);
        assert_eq!(ui.highlight_feature.as_deref(), Some("hud"));

        // Manual pane switching clears deep-link highlights.
        ui.select_pane(Pane::Shortcuts);
        assert_eq!(ui.pane, Pane::Shortcuts);
        assert_eq!(ui.highlight_lab, None);
        assert_eq!(ui.highlight_feature, None);

        // Security invariant: config doc is untouched.
        assert_eq!(ui.doc.hud(), initial_hud);
        assert_eq!(ui.doc.rules().len(), initial_rules);
    }
}
