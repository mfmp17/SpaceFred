//! The miniature desktop's windows: a dark macOS window in each app's own shape, with real,
//! whisper-small contents (`window/`): Safari on Hex's site, a Ghostty-style terminal, Notes and
//! Mail. A window's contents are laid out by its frame, so when the frame springs to a new place
//! they travel with it.
//!
//! Read closely, the four windows tell one afternoon: somebody has mounted a 90 lb speaker over
//! his chair on 15 lb anchors, and scheduled it to play at a quarter past two.

mod mail;
mod notes;
mod safari;
mod terminal;

use rune::{
    Color, El, Shadow, Spring, TextStyle, Transition, el, oklch, row, text,
    tok::motion::pair::SETTLE,
};

use super::{WINDOW, glyphs};
use crate::{
    controls::Paint,
    onboarding::demo::{Frame, Mode, Pose},
};

pub const RADIUS: f32 = 8.0;
const LIGHT: f32 = 5.0;
const TOOLBAR_H: f32 = 22.0;
/// How much an unfocused window gives up, so the focused one reads first: a little beside
/// it, more under a window floating over it.
const CALM: f32 = 0.2;
const COVERED: f32 = 0.34;
/// The window tucked behind one that fills the screen is a paler sheet's edge above it.
const TUCKED: f32 = 0.09;
const TUCK: Spring = Spring::new(0.1, 0.0);
/// Where a window's contents run past its bottom edge, they fade out over this much.
const FADE_H: f32 = 18.0;

/// The windows' type, as small as a real desktop's seen from across the room: a caption and
/// the running text. Each app adds its own cuts.
const TINY: TextStyle = TextStyle::new(4.5, 500.0).lh(6.0);
const SMALL: TextStyle = TextStyle::new(5.5, 450.0).lh(7.5);

fn ink(alpha: f32) -> Color {
    Color::WHITE.alpha(alpha)
}

/// A toolbar button's glyph: opaque, so its strokes never darken where they join.
fn tool(lit: bool) -> Color {
    oklch(if lit { 0.78 } else { 0.5 }, 0.004, 265.0)
}

/// One line of words, faded rather than cut where it runs out of room.
fn words(key: &str, label: &str, style: TextStyle, color: Color) -> El {
    text(label.to_owned())
        .key(key)
        .font(style)
        .color(color)
        .truncate_fade()
}

/// What a window shows and how it sits.
#[derive(Clone, Copy, Debug)]
pub struct Look {
    pub bundle: &'static str,
    pub frame: Frame,
    pub mode: Mode,
    /// The workspace's focused window.
    pub focused: bool,
    pub still: bool,
}

/// One window.
pub fn window(p: Paint, look: Look) -> El {
    let Look {
        bundle,
        frame,
        mode,
        focused,
        still,
    } = look;
    let (x, y, w, h) = super::rect(frame, mode);
    let lifted = frame.pose == Pose::Lifted;
    let behind = frame.pose == Pose::Behind;
    let lift = if lifted { 1.0 } else { 0.0 };
    let key = if still {
        // Under Reduce Motion a window fades into its new place.
        format!("w-{bundle}-{mode:?}-{:?}", frame.pose)
    } else {
        format!("w-{bundle}")
    };
    let veil = if behind {
        Color::WHITE.alpha(TUCKED)
    } else if focused {
        Color::BLACK.alpha(0.0)
    } else if mode == Mode::Floated {
        Color::BLACK.alpha(COVERED)
    } else {
        Color::BLACK.alpha(CALM)
    };
    let ring = if lifted {
        // Off the grid, a lifted window is edged by light instead of the tiles' focus ring.
        Color::WHITE.alpha(0.2)
    } else {
        oklch(0.82, 0.08, 250.0).alpha(if focused && !behind { 0.32 } else { 0.0 })
    };
    let cast = |y: f32, blur: f32, alpha: f32| Shadow {
        y,
        blur,
        color: Color::BLACK.alpha(alpha),
        ..Shadow::default()
    };
    let surface = surface(bundle);
    let mut window = el()
        .key(key)
        .absolute(x, y)
        .size(w, h)
        .radius(RADIUS)
        .clip_rounded(RADIUS)
        .fill(surface)
        // A hairline of light inside the edge, a touch brighter along the top, and a dark one
        // outside it, as macOS draws a dark window.
        .border_gradient(p.hair, ink(if behind { 0.3 } else { 0.17 }), ink(0.1))
        .outline(1.0, 0.0, ring)
        .box_shadow(Shadow {
            spread: p.hair,
            color: Color::BLACK.alpha(0.7),
            ..Shadow::default()
        })
        // Shadow offsets never spring, so the resting and lifted casts trade strength.
        .box_shadow(cast(1.0, 2.0, 0.4))
        .box_shadow(cast(4.0, 12.0, 0.34 * (1.0 - lift)))
        .box_shadow(cast(10.0, 30.0, 0.22 * (1.0 - lift)))
        .box_shadow(cast(4.0, 10.0, 0.38 * lift))
        .box_shadow(cast(14.0, 28.0, 0.46 * lift))
        .box_shadow(cast(28.0, 56.0, 0.36 * lift))
        .transition(Transition::opacity().springs(SETTLE))
        .a11y_hidden();
    if !still {
        window = window.motion(WINDOW);
    }
    let lit = focused && !behind;
    window
        // Tucked behind, a window is a plain sheet's edge: its contents fade, quickly, before
        // the sliver left of it closes over them, and come back as it opens.
        .child(
            chrome(bundle, lit, lifted)
                .opacity(if behind { 0.0 } else { 1.0 })
                .opacity_motion(if behind { TUCK } else { WINDOW }),
        )
        .child(
            el().key("fade")
                .pin(None, Some(0.0), Some(0.0), Some(0.0))
                .h(FADE_H)
                .fill(surface.alpha(0.0))
                .gradient(surface.alpha(0.0), surface),
        )
        .child(
            el().key("veil")
                .pin(Some(0.0), Some(0.0), Some(0.0), Some(0.0))
                .fill(veil),
        )
}

/// The window's own colour, under its contents: Safari's page, the terminal's background,
/// which lets a little of the desktop through, and Notes' and Mail's reading panes.
fn surface(bundle: &str) -> Color {
    match bundle {
        "com.apple.Safari" => safari::page_color(),
        "com.apple.Terminal" => oklch(0.17, 0.006, 270.0).alpha(0.93),
        "com.apple.Notes" => oklch(0.205, 0.002, 80.0),
        _ => oklch(0.205, 0.003, 265.0),
    }
}

fn chrome(bundle: &str, focused: bool, lifted: bool) -> El {
    let body = match bundle {
        "com.apple.Safari" => safari::safari(focused, lifted),
        "com.apple.Terminal" => terminal::terminal(focused),
        "com.apple.Notes" => notes::notes(focused),
        _ => mail::mail(focused),
    };
    body.key("chrome").w_full().h_full()
}

/// The traffic lights, each with the darker rim the real ones have; grey when the window is
/// not the one in front.
fn lights(focused: bool) -> El {
    let colors = if focused {
        [
            oklch(0.68, 0.2, 27.0),
            oklch(0.82, 0.16, 82.0),
            oklch(0.74, 0.19, 145.0),
        ]
    } else {
        [oklch(0.36, 0.003, 265.0); 3]
    };
    let mut lamps = row().key("lights").gap(3.2).center_y();
    for (index, color) in colors.into_iter().enumerate() {
        lamps = lamps.child(
            el().key(format!("light{index}"))
                .size(LIGHT, LIGHT)
                .radius(LIGHT / 2.0)
                .fill(color)
                .border(0.4, Color::BLACK.alpha(0.22)),
        );
    }
    lamps
}

/// A toolbar's row: the traffic lights, then `items`, over a hairline.
fn toolbar(focused: bool, fill: Color, items: Vec<El>) -> El {
    row()
        .key("toolbar")
        .w_full()
        .h(TOOLBAR_H)
        .px(8.0)
        .gap(7.0)
        .center_y()
        .fill(fill)
        .border_sides([0.0, 0.0, 1.0, 0.0], Color::BLACK.alpha(0.5))
        .child(lights(focused))
        .children(items)
}

fn space(key: &str) -> El {
    el().key(key).grow(1.0)
}
