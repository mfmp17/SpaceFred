//! The Try it step's miniature desktop, drawn: a small display holding a dark macOS desktop, with
//! a menu bar (`menu_bar.rs`), two workspaces of two windows each (`window.rs`) and the Workspace
//! HUD (`hud.rs`), which stays up so every move shows in it too. It draws `Demo` and nothing
//! else, so every state is one build of the same elements and each change springs from where it
//! is drawn.
//!
//! | What changes      | What moves                                                          | Spring |
//! | ----------------- | ------------------------------------------------------------------- | ------ |
//! | Another workspace | both workspaces slide; the one arriving grows to full size          | .38 / .12 |
//! |                   | the wallpaper's light drifts the same way, a little and later       | .60 / 0 |
//! |                   | the HUD's screens trade sizes; its marker's leading, trailing edge  | .30 / 0, .20 / 0, .30 / 0 |
//! | Float             | the focused window lifts to the middle, the other goes wide         | .36 / .14 |
//! |                   | in the HUD, its tile lifts over the other's                         | .36 / .14 |
//! | Fullscreen Layout | the focused window fills the screen, the other tucks behind         | .36 / .14 |
//! |                   | in the HUD, one stacked tile, the app in front large                | .36 / .14 |
//! | Its number again  | the window behind comes forward over the one in front, which tucks | .36 / .14 |
//! |                   | in the HUD, the two apps' icons trade places and sizes              | .36 / .14 |
//!
//! Under Reduce Motion nothing travels: the workspace and its windows cross-fade.

mod glyphs;
mod hud;
mod menu_bar;
mod window;

use rune::{
    Color, El, Shadow, Spring, TextStyle, Transition, el, oklch, text, tok::motion::pair::SETTLE,
};

use super::demo::{self, APPS, Demo, Frame, Mode, Pose};
use crate::{Ui, apps, controls::Paint};
use window::Look;

/// The display's key: it takes the keyboard while the step is up.
pub const KEY: &str = "demo-stage";

pub const DISPLAY_W: f32 = 464.0;
const DISPLAY_H: f32 = 290.0;
const BEZEL: f32 = 6.0;
const BEZEL_RADIUS: f32 = 16.0;
const SCREEN_W: f32 = DISPLAY_W - 2.0 * BEZEL;
const SCREEN_H: f32 = DISPLAY_H - 2.0 * BEZEL;
/// Concentric with the bezel.
const SCREEN_RADIUS: f32 = BEZEL_RADIUS - BEZEL;
const MENU_H: f32 = 14.0;
/// From the screen's edge to a window, and between two windows.
const EDGE: f32 = 8.0;
const GAP: f32 = 8.0;
/// How much of the window behind shows above the one filling the screen, and how far it is
/// inset each side.
const PEEK: f32 = 5.0;
const PEEK_INSET: f32 = 12.0;

const SLIDE: Spring = Spring::new(0.38, 0.12);
const WINDOW: Spring = Spring::new(0.36, 0.14);
/// The wallpaper's light, which trails the workspaces it sits behind.
const DRIFT: Spring = Spring::new(0.6, 0.0);
const DRIFT_BY: f32 = 16.0;
/// The workspace out of view, a little smaller and dimmer, so the one arriving grows in.
const AWAY_SCALE: f32 = 0.94;
const AWAY_OPACITY: f32 = 0.35;

/// What the stage shows.
#[derive(Clone, Copy, Debug)]
pub struct Scene {
    pub demo: Demo,
    /// Reduce Motion: cross-fade instead of travelling.
    pub still: bool,
}

/// A window's rectangle on the screen from its unit frame, in a workspace laid out as `mode`.
#[must_use]
pub fn rect(frame: Frame, mode: Mode) -> (f32, f32, f32, f32) {
    let desk_w = SCREEN_W - 2.0 * EDGE + GAP;
    let desk_h = SCREEN_H - MENU_H - 2.0 * EDGE + GAP;
    let x = EDGE + frame.x * desk_w;
    let y = MENU_H + EDGE + frame.y * desk_h;
    let w = frame.w * desk_w - GAP;
    let h = frame.h * desk_h - GAP;
    match (frame.pose, mode) {
        // A tab of the window behind, above the one in front.
        (Pose::Behind, _) => (
            x + PEEK_INSET,
            y,
            w - 2.0 * PEEK_INSET,
            PEEK + window::RADIUS,
        ),
        (Pose::Tiled, Mode::Fullscreen) => (x, y + PEEK, w, h - PEEK),
        _ => (x, y, w, h),
    }
}

pub fn stage(ui: &Ui, p: Paint, scene: Scene) -> El {
    let Scene { demo, still } = scene;
    let desk = if still {
        // One workspace at a time, fading into the next.
        workspace(p, demo, demo.workspace(), still)
            .absolute(0.0, 0.0)
            .transition(Transition::opacity().springs(SETTLE))
    } else {
        el().key("strip")
            .absolute(-(demo.workspace() as f32) * SCREEN_W, 0.0)
            .size(2.0 * SCREEN_W, SCREEN_H)
            .motion(SLIDE)
            .children((0..APPS.len()).map(|index| {
                let showing = index == demo.workspace();
                workspace(p, demo, index, still)
                    .absolute(index as f32 * SCREEN_W, 0.0)
                    .scale(if showing { 1.0 } else { AWAY_SCALE })
                    .scale_motion(SLIDE)
                    .opacity(if showing { 1.0 } else { AWAY_OPACITY })
                    .opacity_motion(SLIDE)
            }))
    };
    let screen = el()
        .key("screen")
        .size(SCREEN_W, SCREEN_H)
        .radius(SCREEN_RADIUS)
        .clip_rounded(SCREEN_RADIUS)
        .fill(oklch(0.13, 0.04, 272.0))
        .gradient(oklch(0.17, 0.05, 266.0), oklch(0.09, 0.03, 290.0))
        .child(wallpaper(demo.workspace(), still))
        .child(desk)
        .child(menu_bar::menu_bar(demo))
        .child(hud::hud(ui, p, demo, still));
    let dark = Paint::new(true, 1.0 / p.hair);
    let mut display = el()
        .key(KEY)
        .role("group")
        .label("Practice desktop")
        .size(DISPLAY_W, DISPLAY_H)
        .padding(BEZEL)
        .radius(BEZEL_RADIUS)
        .fill(oklch(0.1, 0.004, 270.0))
        // The bezel catches the light along its top edge.
        .border_gradient(
            p.hair.max(0.5),
            Color::WHITE.alpha(0.3),
            Color::WHITE.alpha(0.07),
        );
    for shadow in &dark.k.elevation.overlay {
        display = display.box_shadow(Shadow {
            color: shadow
                .color
                .alpha(shadow.color.a * if p.k.dark { 1.0 } else { 0.4 }),
            ..*shadow
        });
    }
    display.child(screen)
}

/// The desktop picture, after macOS's own dark ones: a deep night blue with one broad band of
/// light swept across it, violet low on the left and an ember glow in the far corner. The
/// lights drift a little with the workspaces, behind them.
fn wallpaper(workspace: usize, still: bool) -> El {
    // Eased toward nothing, so a light has no edge.
    let glow = |color: Color, strength: f32| {
        [
            (0.0, color.alpha(strength)),
            (0.3, color.alpha(strength * 0.7)),
            (0.6, color.alpha(strength * 0.26)),
            (0.82, color.alpha(strength * 0.07)),
            (1.0, color.alpha(0.0)),
        ]
    };
    let light = |key: &str, center: [f32; 2], radius: [f32; 2], color: Color, strength: f32| {
        el().key(key)
            .pin(Some(0.0), Some(0.0), Some(0.0), Some(0.0))
            .radial_gradient(center, radius, glow(color, strength))
    };
    let drift = if still {
        0.0
    } else {
        -(workspace as f32) * DRIFT_BY
    };
    el().key("wallpaper")
        .absolute(-DRIFT_BY, 0.0)
        .size(SCREEN_W + 2.0 * DRIFT_BY, SCREEN_H)
        .offset(DRIFT_BY / 2.0 + drift, 0.0)
        .offset_motion(DRIFT)
        .a11y_hidden()
        .child(light(
            "band",
            [0.36, 0.58],
            [380.0, 110.0],
            oklch(0.5, 0.17, 262.0),
            0.6,
        ))
        .child(light(
            "violet",
            [0.06, 0.95],
            [240.0, 140.0],
            oklch(0.5, 0.2, 305.0),
            0.5,
        ))
        .child(light(
            "ember",
            [0.95, 1.02],
            [230.0, 110.0],
            oklch(0.66, 0.16, 42.0),
            0.36,
        ))
        .child(light(
            "haze",
            [0.82, 0.1],
            [210.0, 120.0],
            oklch(0.6, 0.1, 232.0),
            0.22,
        ))
}

/// One workspace's windows. The focused window comes last, so it draws over the other while
/// it lifts off the grid and settles back, and comes forward when its number cycles to it.
fn workspace(p: Paint, demo: Demo, index: usize, still: bool) -> El {
    let desk = demo.desk(index);
    let frames = demo::frames(desk);
    let look = |slot: usize| Look {
        bundle: APPS[index][slot],
        frame: frames[slot],
        mode: desk.mode,
        focused: slot == desk.focus,
        still,
    };
    let behind = 1 - desk.focus;
    el().key(format!("ws{index}"))
        .size(SCREEN_W, SCREEN_H)
        .child(window::window(p, look(behind)))
        .child(window::window(p, look(desk.focus)))
}

/// An app's icon, or its initial on a quiet tile when this Mac doesn't have the app.
fn app_badge(ui: &Ui, bundle: &str, size: f32) -> El {
    match ui.apps.cached(bundle).and_then(|app| app.icon.as_ref()) {
        Some(pixels) => el().size(size, size).pixels(pixels),
        None => el()
            .size(size, size)
            .radius(size * 0.24)
            .fill(oklch(0.42, 0.02, 265.0))
            .center()
            .child(
                text(
                    apps::fallback_name(bundle)
                        .chars()
                        .next()
                        .map(String::from)
                        .unwrap_or_default(),
                )
                .font(TextStyle::new((size * 0.56).max(6.0), 650.0))
                .color(Color::WHITE.alpha(0.95)),
            ),
    }
}
