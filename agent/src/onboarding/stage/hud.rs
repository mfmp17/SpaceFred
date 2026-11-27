//! The Workspace HUD in miniature, in its Layouts style (`hud/grow.rs`): each workspace drawn
//! as its own small screen with a tile and an app icon for every window, the one showing at
//! full size inside a marker and the other smaller and dimmer. It stays up the whole step, so
//! every move on the desktop shows in it as it happens: a switch springs the marker and the
//! screens' sizes, Float lifts a tile over the other, and Fullscreen Layout stacks the two with
//! the window in front at full size and the one behind it small beside it.

use rune::{Color, El, Shadow, Spring, Transition, el, oklch, tok::motion::pair::SETTLE};

use super::{
    SCREEN_H as DESKTOP_H, SCREEN_W as DESKTOP_W, app_badge,
    demo::{self, APPS, Demo, Frame, Pose},
};
use crate::{Ui, controls::Paint};

/// The workspace showing; the other draws at `SMALL` of it.
const SCREEN_W: f32 = 50.0;
const SCREEN_H: f32 = 31.0;
const SMALL: f32 = 0.62;
const GAP: f32 = 6.0;
/// From the capsule's inside edge to the first screen.
const EDGE: f32 = 6.0;
/// The marker's reach past the screen showing.
const PAD: f32 = 2.5;
const HUD_H: f32 = SCREEN_H + 2.0 * PAD + 6.0;
const HUD_W: f32 = 2.0 * EDGE + SCREEN_W * (1.0 + SMALL) + GAP;
/// From the bottom of the desktop.
const REST: f32 = 9.0;
const DIM: f32 = 0.5;
const INSET: f32 = 2.0;
const TILE_GAP: f32 = 0.7;
const ICON_MAX: f32 = 14.0;
/// Fullscreen Layout's window behind, beside the one in front.
const BEHIND_SCALE: f32 = 0.62;
const BEHIND_OPACITY: f32 = 0.55;
const BEHIND_GAP: f32 = 2.0;
/// How far the tile behind shows above the one in front, and how far it is inset each side.
const PEEK: f32 = 2.2;
const PEEK_INSET: f32 = 4.0;

/// Sizes, places and brightness spring together, so the row stays one row mid-flight.
const GROW: Spring = Spring::new(0.3, 0.0);
const MARKER: Spring = Spring::new(0.24, 0.0);
const MARKER_LEAD: Spring = Spring::new(0.2, 0.0);
const MARKER_TRAIL: Spring = Spring::new(0.3, 0.0);
/// Tiles and icons inside a screen move with the windows they stand for.
const TILE: Spring = Spring::new(0.36, 0.14);

pub(super) fn hud(ui: &Ui, p: Paint, demo: Demo, still: bool) -> El {
    let showing = demo.workspace();
    let small = SCREEN_W * SMALL;
    // Each screen's left edge and scale.
    let rest = |index: usize| {
        let x = if index == 0 {
            EDGE
        } else {
            EDGE + if showing == 0 { SCREEN_W } else { small } + GAP
        };
        (x, if index == showing { 1.0 } else { SMALL })
    };
    let (marker_x, _) = rest(showing);
    let mut marker = el()
        .key(if still {
            format!("marker{showing}")
        } else {
            "marker".to_owned()
        })
        .absolute(marker_x - PAD, (HUD_H - SCREEN_H) / 2.0 - PAD)
        .size(SCREEN_W + 2.0 * PAD, SCREEN_H + 2.0 * PAD)
        .radius(5.5)
        .fill(Color::WHITE.alpha(0.1))
        .border(p.hair, Color::WHITE.alpha(0.18))
        .transition(Transition::opacity().springs(SETTLE));
    if !still {
        marker = marker.motion(MARKER).lead(MARKER_LEAD, MARKER_TRAIL);
    }
    let cast = |y: f32, blur: f32, alpha: f32| Shadow {
        y,
        blur,
        color: Color::BLACK.alpha(alpha),
        ..Shadow::default()
    };
    let mut capsule = el()
        .key("hud")
        .absolute((DESKTOP_W - HUD_W) / 2.0, DESKTOP_H - REST - HUD_H)
        .size(HUD_W, HUD_H)
        .radius(9.0)
        .fill(oklch(0.2, 0.004, 285.0).alpha(0.9))
        .backdrop_blur(6.0)
        .border(p.hair, Color::WHITE.alpha(0.16))
        .box_shadow(cast(1.0, 2.0, 0.45))
        .box_shadow(cast(6.0, 16.0, 0.45))
        .a11y_hidden()
        .child(marker);
    for index in 0..APPS.len() {
        let (x, scale) = rest(index);
        let current = index == showing;
        // The screen keeps its full size and draws scaled about its centre, which sits in the
        // middle of the room its scaled width takes.
        let mut screen = screen(ui, p, demo, index, still)
            .absolute(
                x + SCREEN_W * scale / 2.0 - SCREEN_W / 2.0,
                (HUD_H - SCREEN_H) / 2.0,
            )
            .scale(scale)
            .opacity(if current { 1.0 } else { DIM });
        if !still {
            screen = screen.motion(GROW).scale_motion(GROW).opacity_motion(GROW);
        }
        capsule = capsule.child(screen);
    }
    capsule
}

/// One workspace's screen: a tile for each window where it sits, with its app's icon.
fn screen(ui: &Ui, p: Paint, demo: Demo, index: usize, still: bool) -> El {
    let desk = demo.desk(index);
    let frames = demo::frames(desk);
    let inner_w = SCREEN_W - 2.0 * INSET;
    let inner_h = SCREEN_H - 2.0 * INSET;
    let place = |frame: Frame| {
        let (x, y, w, h) = (
            INSET + frame.x * inner_w + TILE_GAP,
            INSET + frame.y * inner_h + TILE_GAP,
            frame.w * inner_w - 2.0 * TILE_GAP,
            frame.h * inner_h - 2.0 * TILE_GAP,
        );
        match frame.pose {
            Pose::Behind => (x + PEEK_INSET, y, w - 2.0 * PEEK_INSET, h),
            _ if desk.mode == demo::Mode::Fullscreen => (x, y + PEEK, w, h - PEEK),
            _ => (x, y, w, h),
        }
    };
    let icon_size = |w: f32, h: f32| ICON_MAX.min(w.min(h) - 3.0).max(4.0);
    let mut base = el()
        .key(format!("hws{index}"))
        .size(SCREEN_W, SCREEN_H)
        .radius(3.5)
        .fill(Color::WHITE.alpha(0.05))
        .border(p.hair, Color::WHITE.alpha(0.22));
    // The window behind first, so the focused one draws over it.
    for slot in [1 - desk.focus, desk.focus] {
        let bundle = APPS[index][slot];
        let frame = frames[slot];
        let (x, y, w, h) = place(frame);
        let lifted = frame.pose == Pose::Lifted;
        let mut tile = el()
            .key(format!("t-{bundle}"))
            .absolute(x, y)
            .size(w, h)
            .radius(if lifted { 2.5 } else { 2.0 });
        tile = match frame.pose {
            Pose::Lifted => tile
                .fill(oklch(0.4, 0.008, 265.0))
                .border(p.hair, Color::WHITE.alpha(0.34))
                .box_shadow(Shadow {
                    y: 1.0,
                    blur: 3.0,
                    color: Color::BLACK.alpha(0.6),
                    ..Shadow::default()
                }),
            Pose::Behind => tile.fill(Color::WHITE.alpha(0.07)),
            Pose::Tiled => tile.fill(Color::WHITE.alpha(0.12)),
        };
        // Its icon: in the middle of its tile, or in Fullscreen Layout one row of the window
        // in front at full size and the one behind it smaller and dimmer.
        let front = place(frames[desk.focus]);
        let (size, opacity, ix, iy) = if desk.mode == demo::Mode::Fullscreen {
            let big = icon_size(front.2, front.3);
            let small = big * BEHIND_SCALE;
            let left = front.0 + (front.2 - (big + BEHIND_GAP + small)) / 2.0;
            let mid = front.1 + front.3 / 2.0;
            if slot == desk.focus {
                (big, 1.0, left, mid - big / 2.0)
            } else {
                (
                    small,
                    BEHIND_OPACITY,
                    left + big + BEHIND_GAP,
                    mid - small / 2.0,
                )
            }
        } else {
            let size = icon_size(w, h);
            (size, 1.0, x + (w - size) / 2.0, y + (h - size) / 2.0)
        };
        let mut icon = app_badge(ui, bundle, size)
            .key(format!("i-{bundle}"))
            .absolute(ix, iy)
            .opacity(opacity);
        if !still {
            tile = tile.motion(TILE);
            icon = icon.motion(TILE).opacity_motion(TILE);
        }
        base = base.child(tile).child(icon);
    }
    base
}
