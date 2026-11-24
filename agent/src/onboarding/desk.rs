//! The Done step's practice desktop and the three lessons beside it.
//!
//! The screen is a small dark display whatever the window's theme, as the Welcome mark is.
//! Tiles, the focus ring, the workspace strip and the HUD's marker all sit at absolute frames
//! and spring between them, so every shortcut reads as motion rather than a cut.

use rune::{
    Beat, Color, Cursor, Easing, El, Sequence, Shadow, Spring, TextStyle, Transition, col, el,
    icon, row, text,
};

use super::sandbox::{Lesson, Sandbox, Unit};
use crate::{
    Ui,
    controls::{DESCRIPTION, Paint},
};

/// The practice row's width: the display, a gap, and the lessons.
pub const WIDTH: f32 = 600.0;
const SCREEN_W: f32 = 368.0;
const SCREEN_H: f32 = 230.0;
const BEZEL: f32 = 6.0;
const MENU_BAR: f32 = 12.0;
const DESK_H: f32 = SCREEN_H - MENU_BAR;
/// Screen-edge padding and the gap between tiles, as tiling lays them out.
const INSET: f32 = 8.0;
const GAP: f32 = 6.0;
const COLUMN_GAP: f32 = 24.0;
const LESSONS_W: f32 = WIDTH - SCREEN_W - 2.0 * BEZEL - COLUMN_GAP;
const MARKER: f32 = 18.0;
const MARKER_GAP: f32 = 10.0;
const CAP: f32 = 22.0;
const HUD_SLOT: f32 = 16.0;
const HUD_H: f32 = 18.0;

const SLIDE: Spring = Spring::new(0.5, 0.0);
const GLIDE: Spring = Spring::new(0.34, 0.0);

const CAP_FONT: TextStyle = TextStyle::new(11.0, 560.0).lh(14.0);
const DIGIT: TextStyle = TextStyle::new(9.0, 650.0).lh(12.0);
const MARK: TextStyle = TextStyle::new(10.5, 620.0).lh(14.0);

/// A pressed keycap sinks and springs back.
const DIP: &[Beat] = &[
    Beat::new(0.0).scale(1.0),
    Beat::new(0.05).scale(0.92).easing(Easing::EASE_OUT),
    Beat::new(0.3).scale(1.0).easing(Easing::EASE_OUT),
];
/// A pressed keycap lights, then cools.
const GLOW: &[Beat] = &[
    Beat::new(0.0).opacity(0.0),
    Beat::new(0.04).opacity(1.0).easing(Easing::EASE_OUT),
    Beat::new(0.6).opacity(0.0).easing(Easing::EASE_IN_OUT),
];
/// The HUD flashes on each workspace switch.
const FLASH: &[Beat] = &[
    Beat::new(0.0).opacity(0.0),
    Beat::new(0.06).opacity(1.0).easing(Easing::EASE_OUT),
    Beat::new(0.8).opacity(0.0).easing(Easing::EASE_IN_OUT),
];

#[derive(Clone, Copy)]
enum Look {
    Code,
    Page,
    Chat,
    List,
}

/// Each workspace's windows: a hue and what they hold.
const DESKS: [&[(f32, Look)]; 2] = [
    &[(262.0, Look::Code), (165.0, Look::Page)],
    &[(40.0, Look::Chat), (300.0, Look::List), (205.0, Look::Page)],
];

fn focus_ink() -> Color {
    rune::oklch(0.76, 0.13, 252.0)
}

pub fn practice(p: Paint, s: &Sandbox) -> El {
    row()
        .key("practice")
        .w(WIDTH)
        .gap(COLUMN_GAP)
        .center_y()
        .child(bezel(p, s))
        .child(lessons(p, s))
}

fn bezel(p: Paint, s: &Sandbox) -> El {
    let dark = p.k.dark;
    let mut bezel = el()
        .key("bezel")
        .size(SCREEN_W + 2.0 * BEZEL, SCREEN_H + 2.0 * BEZEL)
        .radius(14.0)
        .continuous_corners()
        .fill(rune::oklch(0.17, 0.01, 265.0))
        .border(p.hair, Color::WHITE.alpha(0.10))
        .box_shadow(Shadow {
            y: 1.0,
            blur: 2.0,
            color: Color::BLACK.alpha(if dark { 0.5 } else { 0.18 }),
            ..Shadow::default()
        })
        .box_shadow(Shadow {
            y: 14.0,
            blur: 32.0,
            color: Color::BLACK.alpha(if dark { 0.5 } else { 0.2 }),
            ..Shadow::default()
        })
        // Finishing all three lessons warms the display with a calm glow.
        .box_shadow(Shadow {
            blur: 40.0,
            color: p.success().alpha(if s.complete() { 0.26 } else { 0.0 }),
            ..Shadow::default()
        })
        .box_shadow(Shadow {
            y: 0.5,
            color: Color::WHITE.alpha(0.07),
            inset: true,
            ..Shadow::default()
        })
        .motion(GLIDE)
        .label("Practice desktop")
        .child(screen(p, s).absolute(BEZEL, BEZEL));
    if !dark {
        bezel = bezel.border(p.hair, Color::BLACK.alpha(0.14));
    }
    bezel
}

fn screen(p: Paint, s: &Sandbox) -> El {
    let strip = el()
        .key("strip")
        .absolute(-f32::from(s.workspace - 1) * SCREEN_W, MENU_BAR)
        .size(2.0 * SCREEN_W, DESK_H)
        .motion(SLIDE)
        .child(desk(p, s, 1))
        .child(desk(p, s, 2));
    el().key("screen")
        .size(SCREEN_W, SCREEN_H)
        .radius(8.0)
        .continuous_corners()
        .fill(rune::oklch(0.25, 0.035, 268.0))
        .gradient(
            rune::oklch(0.31, 0.055, 280.0),
            rune::oklch(0.21, 0.035, 235.0),
        )
        .radial_gradient(
            [0.22, 0.05],
            [260.0, 200.0],
            [
                (0.0, rune::oklch(0.52, 0.11, 290.0).alpha(0.55)),
                (1.0, rune::oklch(0.52, 0.11, 290.0).alpha(0.0)),
            ],
        )
        .clip_soft(0.0)
        .child(strip)
        .child(menu_bar())
        .child(hud(s))
}

fn menu_bar() -> El {
    let bar = |key: &'static str, x: f32, w: f32, alpha: f32| {
        el().key(key)
            .absolute(x, 4.5)
            .size(w, 3.0)
            .radius(1.5)
            .fill(Color::WHITE.alpha(alpha))
    };
    el().key("menu-bar")
        .absolute(0.0, 0.0)
        .size(SCREEN_W, MENU_BAR)
        .fill(Color::BLACK.alpha(0.28))
        .child(
            el().key("logo")
                .absolute(8.0, 3.5)
                .size(5.0, 5.0)
                .radius(2.5)
                .fill(Color::WHITE.alpha(0.75)),
        )
        .child(bar("menu-0", 19.0, 12.0, 0.4))
        .child(bar("menu-1", 35.0, 16.0, 0.24))
        .child(bar("menu-2", 55.0, 10.0, 0.24))
        .child(bar("status", SCREEN_W - 46.0, 8.0, 0.3))
        .child(bar("clock", SCREEN_W - 32.0, 24.0, 0.4))
}

/// A tile's unit frame as pixels on the desk, with padded screen edges and a gap between tiles.
fn place(unit: Unit) -> [f32; 4] {
    let span = |start: f32, length: f32, size: f32| {
        let lead = INSET + start * size + if start > 0.001 { GAP / 2.0 } else { 0.0 };
        let end = start + length;
        let trail = INSET + end * size - if end < 0.999 { GAP / 2.0 } else { 0.0 };
        (lead, trail - lead)
    };
    let (x, w) = span(unit[0], unit[2], SCREEN_W - 2.0 * INSET);
    let (y, h) = span(unit[1], unit[3], DESK_H - 2.0 * INSET);
    [x, y, w, h]
}

fn desk(p: Paint, s: &Sandbox, workspace: u8) -> El {
    let looks = DESKS[usize::from(workspace - 1)];
    let focus = s.focus(workspace);
    let frames: Vec<[f32; 4]> = s.frames(workspace).into_iter().map(place).collect();
    let mut desk = el()
        .key(format!("desk-{workspace}"))
        .absolute(f32::from(workspace - 1) * SCREEN_W, 0.0)
        .size(SCREEN_W, DESK_H);
    for (index, (frame, (hue, look))) in frames.iter().zip(looks.iter()).enumerate() {
        desk = desk.child(tile(p, index, *frame, *hue, *look, index == focus));
    }
    if let Some(&[x, y, w, h]) = frames.get(focus) {
        let ink = focus_ink();
        desk = desk
            .child(
                el().key("focus-halo")
                    .absolute(x - 4.0, y - 4.0)
                    .size(w + 8.0, h + 8.0)
                    .radius(9.0)
                    .continuous_corners()
                    .border(3.0, ink.alpha(0.22))
                    .motion(GLIDE),
            )
            .child(
                el().key("focus-ring")
                    .absolute(x - 2.0, y - 2.0)
                    .size(w + 4.0, h + 4.0)
                    .radius(7.0)
                    .continuous_corners()
                    .border(1.5, ink)
                    .motion(GLIDE),
            );
    }
    desk
}

fn tile(p: Paint, index: usize, [x, y, w, h]: [f32; 4], hue: f32, look: Look, focused: bool) -> El {
    let lit = if focused { 0.34 } else { 0.28 };
    el().key(format!("tile-{index}"))
        .absolute(x, y)
        .size(w, h)
        .radius(5.0)
        .continuous_corners()
        .fill(rune::oklch(lit, 0.03, hue))
        .gradient(Color::WHITE.alpha(0.06), Color::WHITE.alpha(0.0))
        .border(p.hair, Color::WHITE.alpha(if focused { 0.0 } else { 0.08 }))
        .box_shadow(Shadow {
            y: 1.0,
            blur: 2.0,
            color: Color::BLACK.alpha(0.35),
            ..Shadow::default()
        })
        .box_shadow(Shadow {
            y: 6.0,
            blur: 16.0,
            color: Color::BLACK.alpha(0.28),
            ..Shadow::default()
        })
        .clip_soft(0.0)
        .motion(GLIDE)
        .child(title_bar(focused))
        .child(contents(look, hue).absolute(7.0, 17.0))
}

/// Traffic lights: coloured on the focused window, grey on the rest, as macOS draws them.
fn title_bar(focused: bool) -> El {
    let mut lights = row().key("lights").absolute(6.0, 4.0).gap(3.0);
    for (n, hue) in [25.0, 85.0, 145.0].into_iter().enumerate() {
        lights = lights.child(
            el().key(format!("light-{n}"))
                .size(4.0, 4.0)
                .radius(2.0)
                .fill(if focused {
                    rune::oklch(0.74, 0.15, hue)
                } else {
                    Color::WHITE.alpha(0.2)
                }),
        );
    }
    lights
}

fn bar(w: f32, color: Color) -> El {
    el().size(w, 3.0).radius(1.5).fill(color)
}

fn contents(look: Look, hue: f32) -> El {
    let ink = Color::WHITE.alpha(0.16);
    let tint = rune::oklch(0.74, 0.1, hue);
    let mut column = col().key("contents").gap(4.0);
    match look {
        Look::Code => {
            for (n, (indent, width, keyword)) in [
                (0.0, 26.0, true),
                (8.0, 48.0, false),
                (8.0, 34.0, false),
                (16.0, 56.0, false),
                (16.0, 30.0, true),
                (8.0, 40.0, false),
                (0.0, 14.0, false),
                (0.0, 32.0, true),
                (8.0, 44.0, false),
                (8.0, 22.0, false),
            ]
            .into_iter()
            .enumerate()
            {
                let mut line = row().key(format!("line-{n}")).pl(indent).gap(3.0);
                if keyword {
                    line = line.child(bar(12.0, tint));
                }
                column = column.child(line.child(bar(width, ink)));
            }
        }
        Look::Page => {
            column = column
                .child(bar(84.0, Color::WHITE.alpha(0.1)).key("address").h(6.0))
                .child(
                    el().key("hero")
                        .size(128.0, 30.0)
                        .radius(3.0)
                        .fill(rune::oklch(0.55, 0.08, hue).alpha(0.5)),
                )
                .child(bar(110.0, ink).key("copy-0"))
                .child(bar(92.0, ink).key("copy-1"))
                .child(bar(64.0, ink).key("copy-2"));
        }
        Look::Chat => {
            for (n, (indent, width, mine)) in [
                (0.0, 64.0, false),
                (44.0, 76.0, true),
                (0.0, 50.0, false),
                (60.0, 48.0, true),
                (0.0, 72.0, false),
            ]
            .into_iter()
            .enumerate()
            {
                column = column.child(row().key(format!("bubble-{n}")).pl(indent).child(
                    el().size(width, 9.0).radius(4.5).fill(if mine {
                        tint.alpha(0.55)
                    } else {
                        Color::WHITE.alpha(0.12)
                    }),
                ));
            }
        }
        Look::List => {
            for (n, width) in [52.0, 70.0, 40.0, 62.0, 48.0, 58.0].into_iter().enumerate() {
                column = column.child(
                    row()
                        .key(format!("item-{n}"))
                        .gap(5.0)
                        .center_y()
                        .child(el().size(6.0, 6.0).radius(3.0).fill(tint.alpha(0.7)))
                        .child(bar(width, ink)),
                );
            }
        }
    }
    column
}

/// The Workspace HUD, small: a marker glides to the visible workspace and the pill flashes.
fn hud(s: &Sandbox) -> El {
    let width = 2.0 * HUD_SLOT + 6.0;
    let mut pill = el()
        .key("hud")
        .absolute((SCREEN_W - width) / 2.0, SCREEN_H - HUD_H - 8.0)
        .size(width, HUD_H)
        .radius(HUD_H / 2.0)
        .fill(Color::BLACK.alpha(0.5))
        .border(0.5, Color::WHITE.alpha(0.14))
        .box_shadow(Shadow {
            y: 2.0,
            blur: 8.0,
            color: Color::BLACK.alpha(0.35),
            ..Shadow::default()
        })
        .motion(GLIDE)
        .child(
            el().key("hud-marker")
                .absolute(3.0 + f32::from(s.workspace - 1) * HUD_SLOT, 3.0)
                .size(HUD_SLOT, HUD_H - 6.0)
                .radius((HUD_H - 6.0) / 2.0)
                .fill(Color::WHITE.alpha(0.92)),
        );
    for workspace in 1..=2u8 {
        let visible = workspace == s.workspace;
        pill = pill.child(
            text(workspace.to_string())
                .key(format!("hud-{workspace}"))
                .absolute(3.0 + f32::from(workspace - 1) * HUD_SLOT, 3.0)
                .size(HUD_SLOT, HUD_H - 6.0)
                .font(DIGIT)
                .align(rune::Align::Center)
                .color(if visible {
                    rune::oklch(0.2, 0.01, 265.0)
                } else {
                    Color::WHITE.alpha(0.62)
                }),
        );
    }
    if s.switches > 0 {
        pill = pill.child(
            el().key(format!("hud-flash-{}", s.switches))
                .absolute(-3.0, -3.0)
                .size(width + 6.0, HUD_H + 6.0)
                .radius(HUD_H / 2.0 + 3.0)
                .fill(focus_ink().alpha(0.12))
                .border(1.0, focus_ink().alpha(0.75))
                .transition(Transition::none())
                .sequence(Sequence::new(FLASH)),
        );
    }
    pill
}

fn lessons(p: Paint, s: &Sandbox) -> El {
    let mut column = col().key("lessons").w(LESSONS_W).gap(2.0);
    for lesson in Lesson::ALL {
        column = column.child(lesson_row(p, s, lesson));
    }
    column.child(
        row()
            .key("shift-hint")
            .pt(10.0)
            .pl(MARKER + MARKER_GAP)
            .child(
                text("Add ⇧ to move the window.")
                    .key("copy")
                    .font(DESCRIPTION)
                    .color(p.faint())
                    .wrap()
                    .w(LESSONS_W - MARKER - MARKER_GAP),
            ),
    )
}

fn lesson_row(p: Paint, s: &Sandbox, lesson: Lesson) -> El {
    let current = s.current() == Some(lesson);
    row()
        .key(lesson.key())
        .gap(MARKER_GAP)
        .py(8.0)
        .items_start()
        .motion(GLIDE)
        .child(marker(p, lesson, s.done(lesson), current))
        .child(
            col()
                .key("copy")
                .gap(7.0)
                .child(
                    text(lesson.title())
                        .key("title")
                        .font(p.k.ty.ui_medium.lh(MARKER))
                        .color(if current { p.text() } else { p.muted() }),
                )
                .child(chords(p, s, lesson)),
        )
}

fn marker(p: Paint, lesson: Lesson, done: bool, current: bool) -> El {
    let c = &p.k.c;
    let marker = row()
        .key("marker")
        .size(MARKER, MARKER)
        .radius(MARKER / 2.0)
        .center();
    if done {
        return marker.fill(c.success.bg).child(
            icon(p.k.glyph.check_small, 14.0)
                .key("check")
                .color(p.success())
                .transition(Transition::scale(0.6)),
        );
    }
    marker
        .border(
            1.0,
            if current {
                c.border_strong
            } else {
                c.border_base
            },
        )
        .child(
            text((lesson.index() + 1).to_string())
                .key("number")
                .font(MARK)
                .color(if current { p.text() } else { p.faint() })
                // The check takes its place outright rather than crossing it.
                .transition(Transition::none()),
        )
}

/// The lesson's modifier, then each key it plays. A press remounts the pressed caps under a
/// new key so their dip and glow replay; with no transition, nothing fades between the two.
fn chords(p: Paint, s: &Sandbox, lesson: Lesson) -> El {
    let keys = s.keys(lesson);
    let pressed = s.last.filter(|key| keys.contains(key));
    let (modifier_id, modifier, name) = if lesson == Lesson::Switch {
        ("control", "⌃", "Control")
    } else {
        ("option", "⌥", "Option")
    };
    let mut chords = row().key("chords").gap(4.0).center_y().child(cap(
        p,
        s,
        modifier_id,
        modifier,
        pressed.is_some(),
    ));
    for key in keys {
        let lit = pressed == Some(key);
        let id = format!("key-{}", key.legend());
        chords = chords.child(
            cap(p, s, &id, key.legend(), lit)
                .cursor(Cursor::Pointer)
                .label(format!("{name} {}", key.legend()))
                .on_press(move |ui: &mut Ui| {
                    super::press(ui, key);
                }),
        );
    }
    chords
}

fn cap(p: Paint, s: &Sandbox, id: &str, legend: &str, lit: bool) -> El {
    let cap = keycap(p, legend, lit)
        .key(if lit {
            format!("{id}-{}", s.presses)
        } else {
            id.to_owned()
        })
        .transition(Transition::none());
    if lit {
        cap.sequence(Sequence::new(DIP))
    } else {
        cap
    }
}

fn keycap(p: Paint, legend: &str, lit: bool) -> El {
    let c = &p.k.c;
    let dark = p.k.dark;
    let base = if dark {
        c.surface_raised.mix(c.layer_02, 0.32)
    } else {
        c.white.mix(c.bg_base, 0.06)
    };
    let rim = if dark {
        c.border_weak.mix(c.white, 0.05)
    } else {
        c.border_weak
    };
    let mut cap = row()
        .size(CAP, CAP)
        .radius(5.0)
        .continuous_corners()
        .center()
        .fill(base)
        .hover(base.mix(c.white, if dark { 0.05 } else { 0.4 }))
        .border(p.hair, rim)
        .box_shadow(Shadow {
            y: 1.0,
            blur: 1.8,
            color: c.bg_deep.alpha(if dark { 0.5 } else { 0.12 }),
            ..Shadow::default()
        })
        .box_shadow(Shadow {
            y: 0.5,
            color: c.white.alpha(if dark { 0.06 } else { 0.6 }),
            inset: true,
            ..Shadow::default()
        })
        // The lip along the bottom edge that makes it read as a key.
        .box_shadow(Shadow {
            y: -1.5,
            color: c.bg_deep.alpha(if dark { 0.3 } else { 0.07 }),
            inset: true,
            ..Shadow::default()
        });
    if lit {
        cap = cap.child(
            el().key("glow")
                .absolute(0.0, 0.0)
                .size(CAP, CAP)
                .radius(5.0)
                .continuous_corners()
                .fill(p.accent().alpha(0.2))
                .border(1.0, p.accent().alpha(0.7))
                .transition(Transition::none())
                .sequence(Sequence::new(GLOW)),
        );
    }
    cap.child(
        text(legend.to_owned())
            .key("legend")
            .font(CAP_FONT)
            .color(c.text_base)
            .transition(Transition::none()),
    )
}
