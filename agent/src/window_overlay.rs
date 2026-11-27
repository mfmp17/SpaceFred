//! `[labs] window_overlay`: while the chord is held, one transparent, non-activating panel over
//! each display draws a card on every visible window. Only the cards take the pointer, so a
//! click on a card's button never activates Spaceman or moves focus, and everything else
//! passes through to the windows below. Never shows a window title.

mod card;
mod place;

use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::Duration,
};

use rune::{
    Cx, DisplayTarget, El, KeyPolicy, OverlayHandle, OverlayInput, OverlayOptions,
    OverlayPlacement, OverlaySpec, el,
};
use spaceman::app::{UiWindowOverlayDisplay, UiWindowOverlayNews, WindowOverlayCommand};

use crate::{
    apps,
    controls::Paint,
    hud::{self, Icons},
};

/// One panel per display, up to this many displays.
const PANEL_NAMES: [&str; 4] = [
    "Spaceman Window Overlay",
    "Spaceman Window Overlay 2",
    "Spaceman Window Overlay 3",
    "Spaceman Window Overlay 4",
];
/// A brief fade (>= 33 ms) so AppKit marks the panel visible before Rune's occlusion check;
/// the cards' own entrance carries the motion.
const REVEAL_FADE: Duration = Duration::from_millis(33);
/// Long enough for the cards' 100 ms exit to play before the panel goes.
const LEAVE_FADE: Duration = Duration::from_millis(120);

/// Sends a card's button press to the agent.
pub type CommandSink = Arc<dyn Fn(WindowOverlayCommand) + Send + Sync>;

/// One panel's state: the display it covers, while shown.
pub struct OverlayState {
    display: Option<UiWindowOverlayDisplay>,
    icons: Icons,
    names: HashMap<String, String>,
    reduce_motion: bool,
    send: CommandSink,
}

impl OverlayState {
    fn send(&self, command: WindowOverlayCommand) {
        (self.send)(command);
    }
}

struct Panel {
    handle: OverlayHandle<OverlayState>,
    /// The display it covers, by `UiWindowOverlayDisplay::id`, while shown.
    display: Option<String>,
}

struct Panels {
    panels: Vec<Panel>,
    apps: apps::Catalog,
}

#[derive(Clone)]
pub struct WindowOverlayDriver {
    panels: Arc<Mutex<Panels>>,
}

pub fn install<S: 'static>(app: &mut rune::App<S>, send: &CommandSink) -> WindowOverlayDriver {
    let panels = PANEL_NAMES
        .iter()
        .map(|&name| {
            let spec =
                OverlaySpec::new(name, (800.0, 600.0), overlay_view).options(OverlayOptions {
                    placement: Some(OverlayPlacement::FullScreen),
                    input: OverlayInput::Regions,
                    keys: KeyPolicy::None,
                    prewarm: false,
                    ..Default::default()
                });
            // A hidden display-sized panel would cost Mission Control its Space thumbnails, so
            // it holds nothing while hidden; the cards enter from their own pose.
            let spec = spec.animate_reveal().release_gpu_when_hidden();
            let state = OverlayState {
                display: None,
                icons: Icons::new(),
                names: HashMap::new(),
                reduce_motion: false,
                send: Arc::clone(send),
            };
            Panel {
                handle: app.overlay(spec, state),
                display: None,
            }
        })
        .collect();
    WindowOverlayDriver {
        panels: Arc::new(Mutex::new(Panels {
            panels,
            apps: apps::Catalog::default(),
        })),
    }
}

impl WindowOverlayDriver {
    /// Call on the main thread, where displays, app icons and Reduce Motion are read.
    pub fn apply(&self, news: UiWindowOverlayNews) {
        let Ok(mut panels) = self.panels.lock() else {
            return;
        };
        match news {
            UiWindowOverlayNews::Show(overlay) => panels.show(overlay.displays),
            UiWindowOverlayNews::Hide => panels.show(Vec::new()),
        }
    }
}

impl Panels {
    /// Shows `displays`, one panel each, and hides every other panel.
    fn show(&mut self, displays: Vec<UiWindowOverlayDisplay>) {
        let connected = hud::display_ids();
        let reduce_motion = hud::reduce_motion();
        let rows: Vec<(UiWindowOverlayDisplay, u32)> = displays
            .into_iter()
            .filter_map(|display| {
                let id = *connected.get(&display.id)?;
                Some((display, id))
            })
            .take(PANEL_NAMES.len())
            .collect();
        let bundles = rows
            .iter()
            .flat_map(|(display, _)| &display.windows)
            .filter_map(|window| window.bundle_id.as_ref());
        let (icons, names) = self.apps.icons_and_names(bundles);
        let current: Vec<Option<String>> = self
            .panels
            .iter()
            .map(|panel| panel.display.clone())
            .collect();
        let wanted: Vec<&str> = rows
            .iter()
            .map(|(display, _)| display.id.as_str())
            .collect();
        let assigned = hud::assign_panels(&current, &wanted);
        for (panel, display) in self.panels.iter_mut().zip(assigned) {
            let row = rows
                .iter()
                .find(|(row, _)| Some(&row.id) == display.as_ref())
                .cloned();
            let was = std::mem::replace(&mut panel.display, display);
            let retarget = was != panel.display;
            let handle = panel.handle.clone();
            let (icons, names) = (icons.clone(), names.clone());
            match row {
                Some((row, native)) => panel.handle.update(move |state, _| {
                    if retarget {
                        let target = rune::NativeDisplayId::MacOS(native);
                        handle.set_display(DisplayTarget::Native(target));
                    }
                    if was.is_none() {
                        handle.fade_in(REVEAL_FADE);
                    }
                    state.display = Some(row);
                    state.icons.extend(icons);
                    state.names.extend(names);
                    state.reduce_motion = reduce_motion;
                }),
                None if was.is_some() => panel.handle.update(move |state, _| {
                    // The cards leave as the panel fades.
                    state.display = None;
                    handle.fade_out(LEAVE_FADE);
                }),
                None => {}
            }
        }
    }
}

fn overlay_view(state: &OverlayState, cx: &mut Cx<OverlayState>) -> El {
    if state.reduce_motion {
        cx.reduced_motion(true);
    }
    let p = Paint::new(true, cx.env().scale);
    let still = cx.is_reduced_motion();
    let cards = state.display.iter().flat_map(|display| {
        display.windows.iter().map(move |window| {
            let name = window
                .bundle_id
                .as_deref()
                .map_or_else(String::new, |bundle| {
                    state
                        .names
                        .get(bundle)
                        .cloned()
                        .unwrap_or_else(|| apps::fallback_name(bundle))
                });
            card::card(window, display, &name, &state.icons, p, still)
        })
    });
    el().key("window-overlay").w_full().h_full().children(cards)
}
