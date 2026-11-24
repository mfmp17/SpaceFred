//! Shortcuts pane: every global shortcut with its chord from `[shortcuts]`. Pressing a
//! shortcut's keys records a new chord (`recorder`). The base modifier moves the built-in
//! shortcuts; Switch to space 1–10 stays on Control while that base is Option, and each of
//! those ten has its own recorder. Each change is saved to `config.toml` and registered live.

mod catalog;
mod keycap;
mod recorder;
mod workspaces;

use rune::{El, Shadow, col, row, text};
use rune_kit::SegmentedControl;
use spaceman::{
    WorkspaceId,
    shortcuts::{self, Chord, Modifiers, Shortcuts},
};

pub use recorder::Capture;

use crate::{
    Ui,
    controls::{self, ButtonKind, Paint},
    view::scroll_body,
};
use catalog::Action;
use keycap::{KEY_GAP, keycap, roll, tray};

/// The recorder is as tall as a tray of keys, so a row never jumps when it starts listening.
const RECORDER_HEIGHT: f32 = keycap::KEY_HEIGHT + 2.0 * keycap::TRAY_PADDING;

pub fn pane(ui: &Ui, p: Paint) -> El {
    let shortcuts = ui.doc.shortcuts();
    let named = recorder::enabled_named_workspaces(ui);
    let mut workspace_rows = family_rows(p, &shortcuts, &named);
    workspace_rows.extend(action_rows(
        ui,
        p,
        &shortcuts,
        &named,
        catalog::WORKSPACE_ACTIONS,
    ));
    workspace_rows.push(fixed_row(
        p,
        "swipe",
        "Switch adjacent workspace",
        vec!["3-finger swipe".into()],
    ));
    scroll_body(
        "shortcuts-body",
        col()
            .key("sections")
            .gap(24.0)
            .child(controls::section(
                p,
                "modifier",
                "Modifier",
                modifier_rows(ui, p, &shortcuts),
            ))
            .child(
                col()
                    .key("switching")
                    .gap(8.0)
                    .child(controls::heading(
                        p,
                        "header",
                        "Switching",
                        Some("Click a shortcut to record · esc cancels · ⌫ clears"),
                    ))
                    .child(controls::card(p, "card", switching_rows(ui, p, &shortcuts))),
            )
            .child(
                col()
                    .key("workspaces")
                    .gap(8.0)
                    .child(controls::heading(
                        p,
                        "header",
                        "Workspaces",
                        Some("Click a shortcut to record · esc cancels · ⌫ clears"),
                    ))
                    .child(controls::card(p, "card", workspace_rows)),
            )
            .child(controls::section(
                p,
                "windows-layout",
                "Windows & Layout",
                action_rows(ui, p, &shortcuts, &named, catalog::LAYOUT_ACTIONS),
            )),
    )
}

/// The base modifier every built-in shortcut holds, and a way back to the built-ins.
fn modifier_rows(ui: &Ui, p: Paint, shortcuts: &Shortcuts) -> Vec<El> {
    let mut rows = Vec::new();
    if shortcuts.defaults {
        let labels = catalog::PRESETS.map(|(label, _)| label);
        let selected = catalog::PRESETS
            .iter()
            .position(|(_, modifier)| *modifier == shortcuts.modifier);
        let picker = SegmentedControl::new(&labels, selected)
            .label("Base modifier")
            .fit()
            .on_pick(|ui: &mut Ui, index: usize, _: &mut rune::Tx| {
                if let Some((_, modifier)) = catalog::PRESETS.get(index) {
                    set_modifier(ui, *modifier);
                }
            })
            .build(p.k)
            .key("picker");
        rows.push(controls::setting_row(
            p,
            "base-modifier",
            "Base modifier",
            Some((
                "Move shortcuts add ⇧. On the ⌥ base, Switch to space uses ⌃1–0 instead of ⌥."
                    .into(),
                p.muted(),
            )),
            picker,
        ));
    } else {
        rows.push(controls::setting_row(
            p,
            "defaults-off",
            "Built-in shortcuts are off",
            Some((
                "defaults = false in config.toml: only your bindings run.".into(),
                p.muted(),
            )),
            row().key("none"),
        ));
    }
    if ui.doc.has_custom_shortcuts() {
        rows.push(controls::setting_row(
            p,
            "reset",
            "Custom shortcuts",
            None,
            controls::button(
                p,
                "reset-button",
                "Reset to Defaults",
                ButtonKind::Secondary,
                |ui: &mut Ui, _| reset(ui),
            ),
        ));
    }
    rows
}

fn set_modifier(ui: &mut Ui, modifier: Modifiers) {
    recorder::cancel(ui);
    ui.shortcut_capture = Capture::Idle;
    recorder::edit_shortcuts(ui, |doc| doc.set_shortcut_modifier(modifier));
}

fn reset(ui: &mut Ui) {
    recorder::cancel(ui);
    ui.shortcut_capture = Capture::Idle;
    recorder::edit_shortcuts(ui, crate::rules::Doc::reset_shortcuts);
}

fn reset_switching(ui: &mut Ui) {
    recorder::cancel(ui);
    ui.shortcut_capture = Capture::Idle;
    recorder::edit_shortcuts(ui, crate::rules::Doc::reset_switch_shortcuts);
}

/// One recorder per Switch to space 1–10, plus a way back to Control+number (or the base
/// modifier, when that base is not Option).
fn switching_rows(ui: &Ui, p: Paint, shortcuts: &Shortcuts) -> Vec<El> {
    let named = recorder::enabled_named_workspaces(ui);
    let needs_layout = catalog::SWITCH_SPACES.iter().any(|action| {
        action.chords(shortcuts, &named).iter().any(|chord| {
            chord.modifiers.option && !chord.modifiers.control && !chord.modifiers.command
        })
    });
    let glyphs = if needs_layout {
        spaceman::current_layout_glyphs()
    } else {
        Vec::new()
    };
    let system = spaceman::mission_control_shortcuts();
    let mut rows = catalog::SWITCH_SPACES
        .iter()
        .map(|action| {
            let warning = action
                .chords(shortcuts, &named)
                .iter()
                .find_map(|chord| shortcuts::switch_shortcut_warning(*chord, &glyphs, &system));
            controls::setting_row(
                p,
                &format!("{}-row", action.id),
                action.title,
                warning.map(|text| (text, p.warning())),
                shortcut_button(ui, p, shortcuts, &named, action),
            )
        })
        .collect::<Vec<_>>();
    rows.push(controls::setting_row(
        p,
        "reset-switching",
        "Switch to space shortcuts",
        None,
        controls::button(
            p,
            "reset-switching-button",
            "Reset to Defaults",
            ButtonKind::Secondary,
            |ui: &mut Ui, _| reset_switching(ui),
        ),
    ));
    rows
}

/// The workspace shortcuts, reflecting `[shortcuts.bindings]`, `defaults`, and `modifier`.
fn family_rows(p: Paint, shortcuts: &Shortcuts, named: &[WorkspaceId]) -> Vec<El> {
    workspaces::rows(shortcuts, named)
        .into_iter()
        .map(|entry| match entry.keys {
            workspaces::RowKeys::Range(caps) => fixed_row(p, &entry.key, &entry.title, caps),
            workspaces::RowKeys::Chords(chords) => {
                let keys = row()
                    .key("keys")
                    .padding(2.0)
                    .mr(-3.0)
                    .child(keys_face(p, &chords, false));
                controls::setting_row(p, &format!("{}-row", entry.key), &entry.title, None, keys)
            }
        })
        .collect()
}

fn fixed_row(p: Paint, key: &str, title: &str, caps: Vec<String>) -> El {
    // Inset like a shortcut button, so every tray lines up at the card's edge.
    let keys = row()
        .key("keys")
        .padding(2.0)
        .mr(-3.0)
        .child(tray(p, "tray", &caps));
    controls::setting_row(p, &format!("{key}-row"), title, None, keys)
}

fn action_rows(
    ui: &Ui,
    p: Paint,
    shortcuts: &Shortcuts,
    named: &[WorkspaceId],
    actions: &'static [Action],
) -> Vec<El> {
    actions
        .iter()
        .map(|action| {
            controls::setting_row(
                p,
                &format!("{}-row", action.id),
                action.title,
                None,
                shortcut_button(ui, p, shortcuts, named, action),
            )
        })
        .collect()
}

/// A shortcut's keys, pressed to record a new chord; while recording, the recorder in their
/// place, taking every key until it saves, refuses or cancels.
fn shortcut_button(
    ui: &Ui,
    p: Paint,
    shortcuts: &Shortcuts,
    named: &[WorkspaceId],
    action: &'static Action,
) -> El {
    let c = &p.k.c;
    let id = action.id;
    let (content, label) = match &ui.shortcut_capture {
        Capture::Listening {
            action: active,
            held,
            message,
        } if *active == id => (
            recorder_face(p, *held, message.as_deref()),
            message.clone().unwrap_or_else(|| "Press shortcut".into()),
        ),
        capture => {
            let saved = *capture == Capture::Saved { action: id };
            let chords = action.chords(shortcuts, named);
            let label = chords
                .first()
                .map_or_else(|| "Not set".to_owned(), |chord| chord.keycaps().concat());
            (
                keys_face(p, &chords, saved),
                format!("Change shortcut, {label}"),
            )
        }
    };
    let listening = ui.shortcut_capture.listening() == Some(id);
    let button = row()
        .key(format!("{id}-shortcut"))
        .role("button")
        .label(label)
        .center_y()
        .padding(2.0)
        .mr(-3.0)
        .radius(p.k.radius.md)
        .focusable(true)
        .focus_ring(2.0, 0.0, c.border_focus)
        .on_press(move |ui: &mut Ui| recorder::begin(ui, id))
        .on_physical_input(|ui: &mut Ui, event, _| recorder::on_input(ui, event))
        .child(col().key("content").child(content));
    if listening {
        // A soft glow says the keyboard belongs to this shortcut now.
        button.fill(c.bg_button_neutral).box_shadow(Shadow {
            blur: 6.0,
            spread: 1.0,
            color: c.border_focus.alpha(0.45),
            ..Shadow::default()
        })
    } else {
        button.hover(c.hover).press(c.pressed)
    }
}

fn keys_face(p: Paint, chords: &[Chord], saved: bool) -> El {
    let mut keys = row().key("keys").gap(6.0).center_y();
    if chords.is_empty() {
        keys = keys.child(
            row().key("off").h(RECORDER_HEIGHT).px(8.0).center().child(
                text("Not set")
                    .key("label")
                    .font(p.k.ty.ui)
                    .color(p.faint()),
            ),
        );
    }
    for (index, chord) in chords.iter().take(2).enumerate() {
        if index > 0 {
            keys = keys.child(text("or").key("or").font(p.k.ty.small).color(p.faint()));
        }
        keys = keys.child(tray(p, &format!("tray{index}"), &chord.keycaps()));
    }
    row()
        .key("face")
        .gap(4.0)
        .center_y()
        .children(saved.then(|| {
            rune::icon(&rune_theme_opencode::icons::CHECK_SMALL, 16.0)
                .key("saved")
                .color(p.success())
        }))
        .child(keys)
}

/// Listening: a breathing dot, then the held modifiers sunk into their keys (or why the last
/// chord was refused), then Esc.
fn recorder_face(p: Paint, held: Modifiers, message: Option<&str>) -> El {
    let style = p.k.ty.ui_medium.lh(16.0);
    let held = held.keycaps();
    let prompt = match message {
        Some(message) => text(message.to_owned())
            .key("refused")
            .font(style)
            .color(p.danger()),
        None if held.is_empty() => text("Press shortcut")
            .key("prompt")
            .font(style)
            .color(p.text()),
        None => row()
            .key("held")
            .gap(4.0)
            .center_y()
            .child(
                row().key("keys").gap(KEY_GAP).children(
                    held.iter()
                        .enumerate()
                        .map(|(index, cap)| keycap(p, index, cap, true)),
                ),
            )
            .child(text("…").key("ellipsis").font(style).color(p.muted())),
    };
    row()
        .key("listening")
        .h(RECORDER_HEIGHT)
        .gap(8.0)
        .center_y()
        .pl(11.0)
        .pr(keycap::TRAY_PADDING)
        // The prompt rolls in from below and out through the top, inside the recorder.
        .clip_rounded(p.k.radius.md)
        .child(controls::pulsing_dot(p.danger()))
        .child(prompt.transition(roll()))
        .child(
            keycap(p, 0, "esc", false)
                .key("escape")
                .label("Esc to cancel"),
        )
}

/// Playground: Previous workspace listening, holding ⌃⌥, or refused with `message`.
pub(crate) fn fixture_recording(ui: &mut Ui, message: Option<&str>) {
    ui.shortcut_capture = Capture::Listening {
        action: "back-and-forth",
        held: catalog::PRESETS[1].1,
        message: message.map(str::to_owned),
    };
}

/// Playground: the Control + Option base, Previous workspace on ⌘⌥P just saved, and Move
/// workspace to next display unbound.
pub(crate) fn fixture_custom(ui: &mut Ui) {
    ui.edit(|doc| doc.set_shortcut_modifier(catalog::PRESETS[1].1));
    let shortcuts = ui.doc.shortcuts();
    for (id, source) in [
        ("back-and-forth", Some("cmd+alt+p")),
        ("move-workspace", None),
    ] {
        if let Some(action) = catalog::find(id) {
            let chord = source.and_then(|source| spaceman::shortcuts::parse_chord(source).ok());
            let default = action.default_chord(shortcuts.modifier);
            ui.edit(|doc| doc.set_shortcut(action.command, default, chord));
        }
    }
    ui.shortcut_capture = Capture::Saved {
        action: "back-and-forth",
    };
}
