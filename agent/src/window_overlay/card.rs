//! One window's card: its app, mode pills, swap arrows, then a chip for each workspace it can
//! be sent to. Dark frosted capsule styling, matching the Workspace HUD.

use rune::{Cursor, El, Spring, SpringPair, TextStyle, Transition, col, el, row, text};
use spaceman::{
    Direction, WorkspaceId,
    app::{UiWindowOverlayDisplay, UiWindowOverlayWindow, WindowOverlayCommand},
};

use super::{OverlayState, place};
use crate::{
    controls::Paint,
    hud::{Icons, rgba, srgb},
};

/// Cards spring in from .96 and leave the same way, 100 ms each; a card that moves (a swap)
/// springs to its new place.
const PRESENCE: Spring = Spring::new(0.10, 0.0);
const MOVE: Spring = Spring::new(0.12, 0.0);
const REST_SCALE: f32 = 0.96;

const PAD: f32 = 10.0;
const ROW_GAP: f32 = 8.0;
const RADIUS: f32 = 12.0;
const ICON: f32 = 16.0;
const TOP_H: f32 = 22.0;
const NAME_MAX: f32 = 120.0;
const PILL_H: f32 = 20.0;
const PILL_PX: f32 = 7.0;
const GAP: f32 = 4.0;
const ARROW: f32 = 20.0;
const CHIP: f32 = 22.0;
/// Chips per row before they wrap.
const CHIPS_PER_ROW: usize = 10;

fn name_style() -> TextStyle {
    TextStyle::new(12.0, 600.0).lh(16.0)
}

fn pill_style() -> TextStyle {
    TextStyle::new(11.0, 600.0).lh(14.0)
}

/// Tabular figures, so every chip's digit sits the same.
fn chip_style() -> TextStyle {
    TextStyle {
        features: "'tnum' 1",
        ..TextStyle::new(12.0, 600.0).lh(16.0)
    }
}

/// The mode pills: `Float` (`Tile` for a float), `Pin`, and for a tile `FS`.
fn pills(window: &UiWindowOverlayWindow) -> Vec<&'static str> {
    if window.floating {
        vec!["Tile", "Pin"]
    } else {
        vec!["Float", "Pin", "FS"]
    }
}

fn pill_width(label: &str) -> f32 {
    (rune::text::advance(label, pill_style()) + 2.0 * PILL_PX).ceil()
}

/// The card's size: its top row or its chip rows, whichever is wider.
#[must_use]
pub fn card_size(window: &UiWindowOverlayWindow, name: &str, workspaces: usize) -> (f32, f32) {
    let name_w = rune::text::advance(name, name_style()).ceil().min(NAME_MAX);
    let pills: f32 = pills(window)
        .iter()
        .map(|label| pill_width(label) + GAP)
        .sum();
    let arrows = if window.swaps.is_empty() {
        0.0
    } else {
        2.0 * GAP + window.swaps.len() as f32 * (ARROW + GAP)
    };
    let top = ICON + 6.0 + name_w + 12.0 + pills + arrows;
    let per_row = workspaces.clamp(1, CHIPS_PER_ROW);
    let chips = per_row as f32 * CHIP + (per_row - 1) as f32 * GAP;
    let rows = workspaces.div_ceil(CHIPS_PER_ROW).max(1);
    let chips_h = rows as f32 * CHIP + (rows - 1) as f32 * GAP;
    (
        (top.max(chips) + 2.0 * PAD).ceil(),
        2.0 * PAD + TOP_H + ROW_GAP + chips_h,
    )
}

/// `window`'s card on `display`'s overlay, placed over the window.
pub fn card(
    window: &UiWindowOverlayWindow,
    display: &UiWindowOverlayDisplay,
    name: &str,
    icons: &Icons,
    p: Paint,
    still: bool,
) -> El {
    let size = card_size(window, name, display.workspaces.len());
    let (x, y) = place::card_origin(window.frame, display.screen, size);
    let springs = SpringPair::new(PRESENCE, PRESENCE);
    let presence = if still {
        Transition::opacity().springs(springs)
    } else {
        Transition::scale(REST_SCALE).springs(springs)
    };
    let mut card = col()
        .key(format!(
            "card-{}-{}",
            window.window.app.pid, window.window.window_id
        ))
        .absolute(x, y)
        .w(size.0)
        .h(size.1)
        .padding(PAD)
        .gap(ROW_GAP)
        .radius(RADIUS)
        .fill(srgb(0x161618))
        .border(p.hair, srgb(0x303036))
        .motion(MOVE)
        .transition(presence)
        .hit()
        .child(top_row(window, display.fullscreen, name, icons, p))
        .child(chips(window, display));
    for shadow in &p.k.elevation.overlay {
        card = card.box_shadow(*shadow);
    }
    card
}

fn top_row(
    window: &UiWindowOverlayWindow,
    fullscreen: bool,
    name: &str,
    icons: &Icons,
    p: Paint,
) -> El {
    let target = window.window.clone();
    let mut row = row()
        .key("top")
        .h(TOP_H)
        .w_full()
        .gap(6.0)
        .center_y()
        .child(app_icon(icons, window.bundle_id.as_deref(), name))
        .child(
            text(name.to_owned())
                .key("name")
                .font(name_style())
                .color(rgba(0xffffff, 0.92))
                .max_w(NAME_MAX)
                .truncate(),
        )
        .child(el().key("spacer").grow(1.0));
    let pills = pills(window).into_iter().map(|label| {
        let (on, command) = match label {
            "Pin" => (
                window.pinned,
                WindowOverlayCommand::TogglePin(target.clone()),
            ),
            "FS" => (
                fullscreen,
                WindowOverlayCommand::ToggleFullscreen(target.clone()),
            ),
            _ => (false, WindowOverlayCommand::ToggleFloat(target.clone())),
        };
        pill(label, on, p, command)
    });
    row = row.child(row_of("pills").children(pills));
    if !window.swaps.is_empty() {
        let arrows = window.swaps.iter().map(|side| arrow(&target, *side));
        row = row
            .child(
                el().key("divider")
                    .size(p.hair.max(1.0), 14.0)
                    .fill(rgba(0xffffff, 0.12)),
            )
            .child(row_of("swaps").children(arrows));
    }
    row
}

fn row_of(key: &'static str) -> El {
    row().key(key).gap(GAP).center_y()
}

fn app_icon(icons: &Icons, bundle: Option<&str>, name: &str) -> El {
    let base = el().key("icon").size(ICON, ICON);
    match bundle.and_then(|bundle| icons.get(bundle)) {
        Some(pixels) => base.pixels(pixels),
        None => base
            .radius(ICON * 0.25)
            .fill(rgba(0xffffff, 0.16))
            .center()
            .child(
                text(name.chars().next().map(String::from).unwrap_or_default())
                    .font(TextStyle::new(10.0, 600.0))
                    .color(rgba(0xffffff, 0.7)),
            ),
    }
}

/// A button that sends `command` for this card's window.
fn button(key: impl Into<String>, label: String, command: WindowOverlayCommand) -> El {
    el().key(key.into())
        .role("button")
        .label(label)
        .cursor(Cursor::Pointer)
        .hover(rgba(0xffffff, 0.08))
        .press(rgba(0xffffff, 0.14))
        .on_press_tx(move |state: &mut OverlayState, _| state.send(command.clone()))
}

fn pill(label: &'static str, on: bool, p: Paint, command: WindowOverlayCommand) -> El {
    let (fill, border, ink) = if on {
        (p.accent(), p.accent(), rgba(0xffffff, 1.0))
    } else {
        (
            rgba(0xffffff, 0.06),
            rgba(0xffffff, 0.10),
            rgba(0xffffff, 0.86),
        )
    };
    button(label, label.to_owned(), command)
        .w(pill_width(label))
        .h(PILL_H)
        .radius(PILL_H / 2.0)
        .fill(fill)
        .border(p.hair, border)
        .center()
        .child(text(label).font(pill_style()).color(ink))
}

fn arrow(window: &spaceman::PhysicalWindowId, side: Direction) -> El {
    let glyph = match side {
        Direction::Left => "←",
        Direction::Down => "↓",
        Direction::Up => "↑",
        Direction::Right => "→",
    };
    button(
        format!("swap-{side}"),
        format!("Swap {side}"),
        WindowOverlayCommand::Swap(window.clone(), side),
    )
    .size(ARROW, ARROW)
    .radius(6.0)
    .fill(rgba(0xffffff, 0.06))
    .center()
    .child(
        text(glyph)
            .font(TextStyle::new(12.0, 600.0))
            .color(rgba(0xffffff, 0.86)),
    )
}

fn chips(window: &UiWindowOverlayWindow, display: &UiWindowOverlayDisplay) -> El {
    let rows = display
        .workspaces
        .chunks(CHIPS_PER_ROW)
        .enumerate()
        .map(|(index, chunk)| {
            row()
                .key(format!("chips-{index}"))
                .gap(GAP)
                .children(chunk.iter().map(|entry| {
                    chip(
                        &window.window,
                        entry.workspace,
                        entry.workspace == window.workspace,
                        entry.occupied,
                        entry.on_this_display,
                    )
                }))
        });
    col().key("chips").gap(GAP).children(rows)
}

fn chip(
    window: &spaceman::PhysicalWindowId,
    workspace: WorkspaceId,
    current: bool,
    occupied: bool,
    here: bool,
) -> El {
    let label = workspace.to_string();
    let base = if current {
        el().key(format!("ws-{workspace}"))
            .role("button")
            .label(format!("On workspace {workspace}"))
            .fill(rgba(0xffffff, 0.18))
            .border(1.0, rgba(0xffffff, 0.24))
    } else {
        button(
            format!("ws-{workspace}"),
            format!("Send to workspace {workspace}"),
            WindowOverlayCommand::MoveTo {
                window: window.clone(),
                workspace,
            },
        )
        .fill(rgba(0xffffff, 0.05))
    };
    let ink = match (current, here) {
        (true, _) => rgba(0xffffff, 1.0),
        (false, true) => rgba(0xffffff, 0.78),
        (false, false) => rgba(0xffffff, 0.45),
    };
    let mut chip = base
        .size(CHIP, CHIP)
        .radius(6.0)
        .center()
        .child(text(label).font(chip_style()).color(ink));
    if occupied {
        chip = chip.child(
            el().key("dot")
                .absolute((CHIP - 3.0) / 2.0, CHIP - 5.0)
                .size(3.0, 3.0)
                .radius(1.5)
                .fill(rgba(0xffffff, if current { 0.85 } else { 0.5 })),
        );
    }
    chip
}

#[cfg(test)]
mod tests {
    use super::*;
    use spaceman::{PhysicalWindowId, Rect};

    fn window(floating: bool, swaps: Vec<Direction>) -> UiWindowOverlayWindow {
        UiWindowOverlayWindow {
            window: PhysicalWindowId {
                app: spaceman::AppInstanceId {
                    pid: 7,
                    launched_at: 1,
                },
                window_id: 70,
            },
            bundle_id: Some("com.example.Editor".into()),
            frame: Rect {
                x: 0,
                y: 0,
                width: 800,
                height: 600,
            },
            workspace: WorkspaceId::ONE,
            floating,
            pinned: false,
            swaps,
        }
    }

    #[test]
    fn a_float_offers_tile_and_pin_and_a_tile_adds_fullscreen() {
        assert_eq!(pills(&window(true, Vec::new())), vec!["Tile", "Pin"]);
        assert_eq!(
            pills(&window(false, Vec::new())),
            vec!["Float", "Pin", "FS"]
        );
    }

    #[test]
    fn chips_wrap_past_ten_and_the_card_grows_to_hold_them() {
        let tile = window(false, vec![Direction::Right]);
        let (ten_w, ten_h) = card_size(&tile, "Editor", 10);
        let (more_w, more_h) = card_size(&tile, "Editor", 14);
        assert_eq!(ten_w, more_w, "a second row, not a wider card");
        assert_eq!(more_h - ten_h, CHIP + GAP);
        let (three_w, _) = card_size(&tile, "Editor", 3);
        assert!(three_w < ten_w);
        // Swap arrows widen the top row.
        let (no_swaps, _) = card_size(&window(false, Vec::new()), "Editor", 3);
        assert!(no_swaps < three_w);
    }
}
