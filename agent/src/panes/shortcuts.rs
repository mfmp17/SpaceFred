//! Shortcuts pane: every global shortcut with its chord from `[shortcuts]`. Pressing a
//! shortcut's keys records a new chord (`recorder`). The base modifier moves the built-in
//! shortcuts; Switch to space 1–10 stays on Control while that base is Option, and each of
//! those ten has its own recorder. Each change is saved to `config.toml` and registered live.

mod catalog;
mod recorder;
#[cfg(test)]
mod tests;
mod workspaces;

use rune::{El, Shadow, col, row, text};
use rune_kit::SegmentedControl;
use spaceman::{
    WorkspaceId,
    config::LabFeature,
    shortcuts::{self, Chord, Modifiers, Shortcuts},
};

pub use recorder::Capture;
pub(crate) use workspaces::switch_keys;

/// The first chord that switches to `workspace`, as the setup tour asks for it.
pub(crate) fn switch_chord(shortcuts: &Shortcuts, workspace: WorkspaceId) -> Option<Chord> {
    workspaces::active_chords(shortcuts, workspaces::WorkspaceKind::Switch, workspace)
        .into_iter()
        .next()
}

/// The first chord that runs the catalog's action `id` ("float", "fullscreen").
pub(crate) fn action_chord(
    shortcuts: &Shortcuts,
    named: &[WorkspaceId],
    id: &str,
) -> Option<Chord> {
    catalog::find(id)?
        .chords(shortcuts, named)
        .into_iter()
        .next()
}

/// A physical key press read as the recorder reads it: the modifiers held, and the chord it
/// spells on the active keyboard layout.
pub(crate) use catalog::PRESETS;
pub(crate) use recorder::{modifiers as held_modifiers, pressed_chord};

use crate::{
    Ui,
    controls::{self, ButtonKind, KEYS, Paint, keycap, roll, tray},
    view::scroll_body,
};
use catalog::Action;

pub fn pane(ui: &Ui, p: Paint) -> El {
    let shortcuts = ui.doc.shortcuts();
    let named = recorder::running_named_workspaces(ui);
    let mut workspace_rows = family_rows(ui, p, &shortcuts, &named);
    workspace_rows.extend(action_rows(
        ui,
        p,
        &shortcuts,
        &named,
        catalog::WORKSPACE_ACTIONS,
    ));
    if shortcuts.workspace_steps {
        workspace_rows.extend(action_rows(
            ui,
            p,
            &shortcuts,
            &named,
            catalog::WORKSPACE_STEP_ACTIONS,
        ));
    }
    workspace_rows.push(swipe_row(ui, p));
    scroll_body(
        "shortcuts-body",
        controls::page(
            "sections",
            vec![
                controls::section(p, "modifier", "Modifier", modifier_rows(ui, p, &shortcuts)),
                controls::section(
                    p,
                    "switching",
                    "Switching",
                    switching_rows(ui, p, &shortcuts),
                ),
                controls::section(p, "workspaces", "Workspaces", workspace_rows),
                controls::section(
                    p,
                    "windows-layout",
                    "Windows & layout",
                    action_rows(ui, p, &shortcuts, &named, catalog::LAYOUT_ACTIONS),
                ),
            ],
        ),
    )
}

/// The base modifier every built-in shortcut holds, and a way back to the built-ins.
fn modifier_rows(ui: &Ui, p: Paint, shortcuts: &Shortcuts) -> Vec<El> {
    let mut rows = Vec::new();
    let running = running_shortcuts(ui);
    if running.is_some() {
        rows.push(controls::setting_row(
            p,
            "not-applied",
            "Previous shortcuts still run",
            Some((
                "These rows show config.toml, which wasn't applied".into(),
                p.muted(),
            )),
            row().key("none"),
        ));
    }
    let modifier_note = running.and_then(|running| modifier_running_note(running, shortcuts));
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
        let base_note = "Moves add ⇧. On the ⌥ base, Switch to space uses ⌃1–0 instead of ⌥.";
        let description = match modifier_note {
            Some(note) => format!("{base_note} · {note}"),
            None => base_note.into(),
        };
        rows.push(controls::setting_row(
            p,
            "base-modifier",
            "Base modifier",
            Some((description, p.muted())),
            picker,
        ));
    } else {
        let description = match modifier_note {
            Some(note) => format!("defaults = false in config.toml · {note}"),
            None => "defaults = false in config.toml: only your bindings run".into(),
        };
        rows.push(controls::setting_row(
            p,
            "defaults-off",
            "Built-in shortcuts are off",
            Some((description, p.muted())),
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
                "Reset to defaults",
                ButtonKind::Secondary,
                |ui: &mut Ui, _| reset(ui),
            ),
        ));
    }
    rows
}

/// The previous table, while it still runs because the saved one (which the rows show) was
/// rejected. The agent publishes it only while those shortcuts are registered.
fn running_shortcuts(ui: &Ui) -> Option<&Shortcuts> {
    ui.agent.as_ref()?.running_shortcuts.as_ref()
}

/// What the base modifier or `defaults` row runs as while a rejected table leaves the previous
/// one running; `None` when the two agree.
fn modifier_running_note(running: &Shortcuts, saved: &Shortcuts) -> Option<String> {
    if running.defaults == saved.defaults && (!saved.defaults || running.modifier == saved.modifier)
    {
        return None;
    }
    Some(if running.defaults {
        format!("Still runs {}", running.modifier.keycaps().concat())
    } else {
        "Built-in shortcuts not running".to_owned()
    })
}

/// What a shortcut runs as instead of its saved chords, while a rejected table leaves the
/// previous one running; `None` when the two agree.
fn running_note(
    running: &Shortcuts,
    saved: &Shortcuts,
    named: &[WorkspaceId],
    action: &Action,
) -> Option<String> {
    let chords = action.chords(running, named);
    if chords == action.chords(saved, named) {
        return None;
    }
    Some(chords.first().map_or_else(
        || "Not running yet".to_owned(),
        |chord| format!("Still runs {}", chord.keycaps().concat()),
    ))
}

/// What a workspace row runs as while a rejected table leaves the previous one running; `None`
/// when the workspaces represented by `entry` still run their saved chords.
fn workspace_running_note(
    running: &Shortcuts,
    saved: &Shortcuts,
    named: &[WorkspaceId],
    running_rows: &[workspaces::WorkspaceRow],
    entry: &workspaces::WorkspaceRow,
) -> Option<String> {
    let (kind, rest) = if let Some(rest) = entry.key.strip_prefix("switch") {
        (workspaces::WorkspaceKind::Switch, rest)
    } else {
        let rest = entry.key.strip_prefix("move")?;
        (workspaces::WorkspaceKind::Move, rest)
    };
    match rest {
        "" => range_running_note(
            running,
            saved,
            kind,
            WorkspaceId::numbered().collect(),
            running_rows,
            entry,
        ),
        "-named" => {
            if named.is_empty() {
                if running.defaults == saved.defaults
                    && (!saved.defaults || running.modifier == saved.modifier)
                {
                    return None;
                }
                return Some(if running.defaults {
                    let mods = match kind {
                        workspaces::WorkspaceKind::Switch => running.modifier,
                        workspaces::WorkspaceKind::Move => running.modifier.with_shift(),
                    };
                    format!("Still runs {}A…Z", mods.keycaps().concat())
                } else {
                    "Not running yet".to_owned()
                });
            }
            range_running_note(running, saved, kind, named.to_vec(), running_rows, entry)
        }
        suffix => {
            let workspace = suffix
                .strip_prefix('-')
                .and_then(|id| WorkspaceId::parse(id).ok())?;
            let chords = workspaces::active_chords(running, kind, workspace);
            if chords == workspaces::active_chords(saved, kind, workspace) {
                return None;
            }
            Some(chords.first().map_or_else(
                || "Not running yet".to_owned(),
                |chord| format!("Still runs {}", chord.keycaps().concat()),
            ))
        }
    }
}

fn range_running_note(
    running: &Shortcuts,
    saved: &Shortcuts,
    kind: workspaces::WorkspaceKind,
    candidates: Vec<WorkspaceId>,
    running_rows: &[workspaces::WorkspaceRow],
    entry: &workspaces::WorkspaceRow,
) -> Option<String> {
    let default_mods = match kind {
        workspaces::WorkspaceKind::Switch => saved.modifier,
        workspaces::WorkspaceKind::Move => saved.modifier.with_shift(),
    };
    let mut any_differ = false;
    let mut any_running = false;
    for workspace in candidates {
        let saved_chords = workspaces::active_chords(saved, kind, workspace);
        let in_saved_range = saved.defaults
            && saved_chords.len() == 1
            && saved_chords[0].modifiers == default_mods
            && match (workspace.number(), workspace.letter()) {
                (Some(number), _) => {
                    saved_chords[0].key == spaceman::shortcuts::Key::Number(number)
                }
                (_, Some(letter)) => {
                    saved_chords[0].key
                        == spaceman::shortcuts::Key::Character(letter.to_ascii_lowercase())
                }
                _ => false,
            };
        if !in_saved_range {
            continue;
        }
        let running_chords = workspaces::active_chords(running, kind, workspace);
        if !running_chords.is_empty() {
            any_running = true;
        }
        if running_chords != saved_chords {
            any_differ = true;
        }
    }
    if !any_differ {
        return None;
    }
    if !any_running {
        return Some("Not running yet".to_owned());
    }
    if let Some(workspaces::WorkspaceRow {
        keys: workspaces::RowKeys::Range(caps),
        ..
    }) = running_rows
        .iter()
        .find(|row| row.key == entry.key && row.title == entry.title)
    {
        return Some(format!("Still runs {}", caps.concat()));
    }
    Some("Still runs previous shortcuts".to_owned())
}

/// Saves the base modifier every built-in shortcut holds.
pub(crate) fn set_modifier(ui: &mut Ui, modifier: Modifiers) {
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
    let running = running_shortcuts(ui);
    let mut rows = catalog::SWITCH_SPACES
        .iter()
        .map(|action| {
            let warning = action
                .chords(shortcuts, &named)
                .iter()
                .find_map(|chord| shortcuts::switch_shortcut_warning(*chord, &glyphs, &system));
            let note = warning.map(|text| (text, p.warning())).or_else(|| {
                running
                    .and_then(|running| running_note(running, shortcuts, &named, action))
                    .map(|text| (text, p.muted()))
            });
            controls::setting_row(
                p,
                &format!("{}-row", action.id),
                action.title,
                note,
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
fn family_rows(ui: &Ui, p: Paint, shortcuts: &Shortcuts, named: &[WorkspaceId]) -> Vec<El> {
    let running = running_shortcuts(ui);
    let running_rows = running.map(|running| workspaces::rows(running, named));
    workspaces::rows(shortcuts, named)
        .into_iter()
        .map(|entry| {
            let note = running
                .and_then(|running| {
                    workspace_running_note(
                        running,
                        shortcuts,
                        named,
                        running_rows.as_deref().unwrap_or(&[]),
                        &entry,
                    )
                })
                .map(|note| (note, p.muted()));
            match entry.keys {
                workspaces::RowKeys::Range(caps) => controls::setting_row(
                    p,
                    &format!("{}-row", entry.key),
                    &entry.title,
                    note,
                    tray(p, "tray", &caps),
                ),
                workspaces::RowKeys::Chords(chords) => controls::setting_row(
                    p,
                    &format!("{}-row", entry.key),
                    &entry.title,
                    note,
                    keys_face(p, &chords, false),
                ),
            }
        })
        .chain(
            running
                .into_iter()
                .flat_map(|running| running_only_rows(p, running, shortcuts, named)),
        )
        .collect()
}

/// Workspaces the saved table has no row for (it turned the built-ins off and binds nothing to
/// them) but the previous table still runs: shown unset, with the chord that still runs.
fn running_only_rows(
    p: Paint,
    running: &Shortcuts,
    saved: &Shortcuts,
    named: &[WorkspaceId],
) -> Vec<El> {
    workspaces::running_only_rows(running, saved, named)
        .into_iter()
        .map(|entry| {
            let note = format!("Still runs {}", workspace_row_caps(&entry.keys));
            controls::setting_row(
                p,
                &format!("{}-row", entry.key),
                &entry.title,
                Some((note, p.muted())),
                keys_face(p, &[], false),
            )
        })
        .collect()
}

/// A workspace row's keys as one run of keycaps: the whole range, or its first chord.
fn workspace_row_caps(keys: &workspaces::RowKeys) -> String {
    match keys {
        workspaces::RowKeys::Range(caps) => caps.concat(),
        workspaces::RowKeys::Chords(chords) => chords
            .first()
            .map(|chord| chord.keycaps().concat())
            .unwrap_or_default(),
    }
}

fn fixed_row(p: Paint, key: &str, title: &str, caps: Vec<String>) -> El {
    controls::setting_row(
        p,
        &format!("{key}-row"),
        title,
        None,
        tray(p, "tray", &caps),
    )
}

/// What the workspace swipe's row shows.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SwipeRow {
    /// The fixed keycap tray, as always while `[labs] swipe_switch` is off.
    Fixed,
    /// With the Lab on, a switch for `settings.trackpad_swipe`.
    Switch { on: bool },
}

impl SwipeRow {
    fn of(doc: &crate::rules::Doc) -> Self {
        if doc.lab(LabFeature::SwipeSwitch) {
            Self::Switch {
                on: doc.trackpad_swipe(),
            }
        } else {
            Self::Fixed
        }
    }
}

/// The workspace swipe's row: a fixed keycap tray, or with `[labs] swipe_switch` on, a
/// switch for `settings.trackpad_swipe` (off leaves the gesture to macOS alone). It names
/// four fingers while `[labs] four_finger_swipe` is on.
fn swipe_row(ui: &Ui, p: Paint) -> El {
    let fingers = if ui.doc.lab(LabFeature::FourFingerSwipe) {
        4
    } else {
        3
    };
    let on = match SwipeRow::of(&ui.doc) {
        SwipeRow::Fixed => {
            return fixed_row(
                p,
                "swipe",
                "Switch adjacent workspace",
                vec![format!("{fingers}-finger swipe")],
            );
        }
        SwipeRow::Switch { on } => on,
    };
    let description = if on {
        format!("{fingers}-finger swipe left or right")
    } else {
        format!("Off: {fingers}-finger swipes are left to macOS")
    };
    controls::toggle_row(
        p,
        "swipe-row",
        "Swipe to switch workspace",
        Some((description, p.muted())),
        on,
        move |ui| ui.edit(|doc| doc.set_trackpad_swipe(!on)),
    )
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
            let note = running_shortcuts(ui)
                .and_then(|running| running_note(running, shortcuts, named, action))
                .map(|note| (note, p.muted()));
            controls::setting_row(
                p,
                &format!("{}-row", action.id),
                action.title,
                note,
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
        // The tray, not the pad round it, lands on the control rail; the pad's corner is the
        // tray's plus the pad, so the hover fill is concentric with it.
        .padding(KEYS.focus_pad)
        .mr(-KEYS.focus_pad)
        .radius(KEYS.button_radius())
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
    let keys = controls::chords(p, chords);
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
/// chord was refused), then the two keys that end it: ⌫ clears the shortcut, esc keeps it.
fn recorder_face(p: Paint, held: Modifiers, message: Option<&str>) -> El {
    let style = p.label();
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
                row().key("keys").gap(KEYS.gap).children(
                    held.iter()
                        .enumerate()
                        .map(|(index, cap)| keycap(p, index, cap, true)),
                ),
            )
            .child(text("…").key("ellipsis").font(style).color(p.muted())),
    };
    row()
        .key("listening")
        .h(KEYS.tray_h())
        .gap(8.0)
        .center_y()
        .pl(12.0)
        .pr(KEYS.tray_pad)
        // The prompt rolls in from below and out through the top, inside the recorder.
        .clip_rounded(KEYS.tray_radius())
        .child(controls::pulsing_dot(p.danger()))
        .child(prompt.transition(roll()))
        .child(
            row()
                .key("ends")
                .gap(KEYS.gap)
                .child(
                    keycap(p, 0, "⌫", false)
                        .key("clear")
                        .label("Delete to clear"),
                )
                .child(
                    keycap(p, 1, "esc", false)
                        .key("escape")
                        .label("Esc to cancel"),
                ),
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

#[cfg(test)]
mod running_note_tests {
    use spaceman::config::Config;

    use super::*;

    fn shortcuts(source: &str) -> Shortcuts {
        Config::parse(source).unwrap().shortcuts().clone()
    }

    #[test]
    fn modifier_row_notes_when_running_modifier_or_defaults_differ() {
        let default = shortcuts("version = 1\n");
        let ctrl_alt = shortcuts("version = 1\n[shortcuts]\nmodifier = \"ctrl+alt\"\n");
        let no_defaults = shortcuts(
            "version = 1\n[shortcuts]\ndefaults = false\n[shortcuts.bindings]\n\"ctrl+alt+y\" = \"float\"\n",
        );

        assert_eq!(modifier_running_note(&default, &default), None);
        assert_eq!(
            modifier_running_note(&default, &ctrl_alt).as_deref(),
            Some("Still runs ⌥")
        );
        assert_eq!(
            modifier_running_note(&ctrl_alt, &no_defaults).as_deref(),
            Some("Still runs ⌃⌥")
        );
        assert_eq!(
            modifier_running_note(&no_defaults, &default).as_deref(),
            Some("Built-in shortcuts not running")
        );
        assert_eq!(modifier_running_note(&no_defaults, &no_defaults), None);
    }

    #[test]
    fn workspace_rows_note_only_rows_whose_running_shortcuts_differ() {
        let default = shortcuts("version = 1\n");
        let ctrl_alt = shortcuts("version = 1\n[shortcuts]\nmodifier = \"ctrl+alt\"\n");
        let rebound_two =
            shortcuts("version = 1\n[shortcuts.bindings]\n\"ctrl+alt+2\" = \"switch 2\"\n");
        let no_defaults = shortcuts(
            "version = 1\n[shortcuts]\ndefaults = false\n[shortcuts.bindings]\n\"ctrl+alt+y\" = \"float\"\n",
        );

        // Changing base modifier marks every range row with what still runs.
        let running_rows = workspaces::rows(&default, &[]);
        for row in workspaces::rows(&ctrl_alt, &[]) {
            let note = workspace_running_note(&default, &ctrl_alt, &[], &running_rows, &row);
            assert!(
                note.as_deref()
                    .is_some_and(|n| n.starts_with("Still runs ⌥")),
                "{}: {note:?}",
                row.key
            );
        }

        // Rebinding only workspace 2 marks switch-2, while unchanged range rows stay quiet.
        let saved_rows = workspaces::rows(&rebound_two, &[]);
        let switch_two = saved_rows.iter().find(|r| r.key == "switch-2").unwrap();
        assert_eq!(
            workspace_running_note(&default, &rebound_two, &[], &running_rows, switch_two)
                .as_deref(),
            Some("Still runs ⌥2")
        );
        let switch_rest = saved_rows.iter().find(|r| r.key == "switch").unwrap();
        assert_eq!(
            workspace_running_note(&default, &rebound_two, &[], &running_rows, switch_rest),
            None
        );

        // When previous table had defaults = false with no workspace chords, new defaults say not running yet.
        let no_default_rows = workspaces::rows(&no_defaults, &[]);
        for row in workspaces::rows(&default, &[]) {
            assert_eq!(
                workspace_running_note(&no_defaults, &default, &[], &no_default_rows, &row)
                    .as_deref(),
                Some("Not running yet"),
                "{}",
                row.key
            );
        }
    }
}
