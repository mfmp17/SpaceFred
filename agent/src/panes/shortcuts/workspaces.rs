//! Workspace shortcut rows for Settings › Shortcuts: reflects `[shortcuts.bindings]`,
//! `defaults = false`, and `modifier` for numbered (1–10) and enabled named (A–Z) workspaces,
//! matching the hotkey planner's precedence (`src/hotkeys.rs`).

use spaceman::{
    WorkspaceId,
    shortcuts::{Chord, Command, Key, Modifiers, Shortcuts},
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WorkspaceKind {
    Switch,
    Move,
}

impl WorkspaceKind {
    const fn key_prefix(self) -> &'static str {
        match self {
            Self::Switch => "switch",
            Self::Move => "move",
        }
    }

    const fn title_prefix(self) -> &'static str {
        match self {
            Self::Switch => "Switch to workspace",
            Self::Move => "Move window to workspace",
        }
    }

    const fn named_default_title(self) -> &'static str {
        match self {
            Self::Switch => "Switch to a named workspace",
            Self::Move => "Move window to a named workspace",
        }
    }

    const fn modifiers(self, base: Modifiers) -> Modifiers {
        match self {
            Self::Switch => base,
            Self::Move => base.with_shift(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RowKeys {
    /// A compact built-in range (for example `⌥` + `1…9, 0` or `⌥` + `A…Z`).
    Range(Vec<String>),
    /// Zero, one, or more explicit chords (rendered identically to catalog action rows, showing
    /// `Not set` when empty).
    Chords(Vec<Chord>),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorkspaceRow {
    pub key: String,
    pub title: String,
    pub keys: RowKeys,
}

fn command_for(kind: WorkspaceKind, workspace: WorkspaceId) -> Command {
    match kind {
        WorkspaceKind::Switch => Command::Switch(workspace),
        WorkspaceKind::Move => Command::Move(workspace),
    }
}

fn default_chord(kind: WorkspaceKind, workspace: WorkspaceId, base: Modifiers) -> Chord {
    let key = match (workspace.number(), workspace.letter()) {
        (Some(number), _) => Key::Number(number),
        (_, Some(letter)) => Key::Character(letter.to_ascii_lowercase()),
        _ => unreachable!("workspace ID is always valid"),
    };
    let modifiers = if kind == WorkspaceKind::Switch && workspace.number().is_some() {
        spaceman::shortcuts::numbered_switch_modifiers(base)
    } else {
        kind.modifiers(base)
    };
    Chord { modifiers, key }
}

/// Active chords that run `kind` on `workspace` under `shortcuts`, matching `HotkeyPlan`.
pub fn active_chords(
    shortcuts: &Shortcuts,
    kind: WorkspaceKind,
    workspace: WorkspaceId,
) -> Vec<Chord> {
    let command = command_for(kind, workspace);
    let mut chords: Vec<Chord> = shortcuts
        .bindings
        .iter()
        .filter(|binding| binding.command.as_ref() == Some(&command))
        .map(|binding| binding.chord)
        .collect();
    let default = default_chord(kind, workspace, shortcuts.modifier);
    if shortcuts.defaults && shortcuts.binding(default).is_none() && !chords.contains(&default) {
        chords.push(default);
    }
    chords
}

/// All workspace rows for the Workspaces card in Settings › Shortcuts: numbered Switch and Move,
/// then named Switch and Move.
pub fn rows(shortcuts: &Shortcuts, named: &[WorkspaceId]) -> Vec<WorkspaceRow> {
    // Numbered Switch to space rows are recorders in the Switching section, not this card.
    let mut out = numbered_rows(shortcuts, WorkspaceKind::Move);
    out.extend(named_rows(shortcuts, named, WorkspaceKind::Switch));
    out.extend(named_rows(shortcuts, named, WorkspaceKind::Move));
    out
}

fn numbered_rows(shortcuts: &Shortcuts, kind: WorkspaceKind) -> Vec<WorkspaceRow> {
    let mut default_numbers = Vec::new();
    let mut entries: Vec<(u8, WorkspaceRow)> = Vec::new();

    for workspace in WorkspaceId::numbered() {
        let Some(number) = workspace.number() else {
            continue;
        };
        let default = default_chord(kind, workspace, shortcuts.modifier);
        let chords = active_chords(shortcuts, kind, workspace);
        if shortcuts.defaults && chords == [default] {
            default_numbers.push(number);
        } else if shortcuts.defaults || !chords.is_empty() {
            entries.push((
                number,
                WorkspaceRow {
                    key: format!("{}-{number}", kind.key_prefix()),
                    title: format!("{} {number}", kind.title_prefix()),
                    keys: RowKeys::Chords(chords),
                },
            ));
        }
    }

    if let Some(&first) = default_numbers.first() {
        let mut caps = kind.modifiers(shortcuts.modifier).keycaps();
        caps.push(format_number_keycap(&default_numbers));
        entries.push((
            first,
            WorkspaceRow {
                key: kind.key_prefix().to_owned(),
                title: format!(
                    "{} {}",
                    kind.title_prefix(),
                    format_number_ranges(&default_numbers)
                ),
                keys: RowKeys::Range(caps),
            },
        ));
    }

    entries.sort_by_key(|(first, _)| *first);
    entries.into_iter().map(|(_, row)| row).collect()
}

fn named_rows(
    shortcuts: &Shortcuts,
    named: &[WorkspaceId],
    kind: WorkspaceKind,
) -> Vec<WorkspaceRow> {
    let mut default_letters: Vec<char> = Vec::new();
    let mut entries: Vec<(char, WorkspaceRow)> = Vec::new();

    for &workspace in named {
        let Some(letter) = workspace.letter() else {
            continue;
        };
        let default = default_chord(kind, workspace, shortcuts.modifier);
        let chords = active_chords(shortcuts, kind, workspace);
        if shortcuts.defaults && chords == [default] {
            default_letters.push(letter);
        } else if shortcuts.defaults || !chords.is_empty() {
            entries.push((
                letter,
                WorkspaceRow {
                    key: format!("{}-{letter}", kind.key_prefix()),
                    title: format!("{} {letter}", kind.title_prefix()),
                    keys: RowKeys::Chords(chords),
                },
            ));
        }
    }

    if shortcuts.defaults && entries.is_empty() {
        let mut caps = kind.modifiers(shortcuts.modifier).keycaps();
        caps.push("A…Z".into());
        return vec![WorkspaceRow {
            key: format!("{}-named", kind.key_prefix()),
            title: kind.named_default_title().to_owned(),
            keys: RowKeys::Range(caps),
        }];
    }

    if let Some(&first) = default_letters.first() {
        let joined = default_letters
            .iter()
            .map(char::to_string)
            .collect::<Vec<_>>()
            .join(", ");
        let mut caps = kind.modifiers(shortcuts.modifier).keycaps();
        caps.push(joined.clone());
        entries.push((
            first,
            WorkspaceRow {
                key: format!("{}-named", kind.key_prefix()),
                title: format!("{} {joined}", kind.title_prefix()),
                keys: RowKeys::Range(caps),
            },
        ));
    }

    entries.sort_by_key(|(first, _)| *first);
    entries.into_iter().map(|(_, row)| row).collect()
}

/// Formats sorted workspace numbers `1..=10` as compact title ranges: `1–10`, `2–10`, `1–2, 4–10`.
fn format_number_ranges(numbers: &[u8]) -> String {
    consecutive_runs(numbers)
        .into_iter()
        .map(|(start, end)| {
            if start == end {
                start.to_string()
            } else {
                format!("{start}–{end}")
            }
        })
        .collect::<Vec<_>>()
        .join(", ")
}

/// Formats sorted workspace numbers `1..=10` as physical number-row keycaps: `1…9, 0`, `2…9, 0`.
fn format_number_keycap(numbers: &[u8]) -> String {
    let digits: Vec<u8> = numbers.iter().copied().filter(|&n| n < 10).collect();
    let mut parts: Vec<String> = consecutive_runs(&digits)
        .into_iter()
        .map(|(start, end)| match end - start {
            0 => start.to_string(),
            1 => format!("{start}, {end}"),
            _ => format!("{start}…{end}"),
        })
        .collect();
    if numbers.contains(&10) {
        parts.push("0".into());
    }
    parts.join(", ")
}

fn consecutive_runs(numbers: &[u8]) -> Vec<(u8, u8)> {
    let mut runs: Vec<(u8, u8)> = Vec::new();
    for &n in numbers {
        if let Some((_, end)) = runs.last_mut()
            && n == *end + 1
        {
            *end = n;
            continue;
        }
        runs.push((n, n));
    }
    runs
}

#[cfg(test)]
mod tests {
    use spaceman::shortcuts::parse_chord;

    use super::*;

    fn parse(body: &str) -> Shortcuts {
        spaceman::config::Config::parse(&format!("version = 1\n{body}"))
            .unwrap()
            .shortcuts()
            .clone()
    }

    fn chord(source: &str) -> Chord {
        parse_chord(source).unwrap()
    }

    #[test]
    fn default_config_keeps_compact_workspace_family_rows() {
        let rows = rows(&Shortcuts::default(), &[]);
        assert_eq!(
            rows,
            vec![
                WorkspaceRow {
                    key: "move".into(),
                    title: "Move window to workspace 1–10".into(),
                    keys: RowKeys::Range(vec!["⌥".into(), "⇧".into(), "1…9, 0".into()]),
                },
                WorkspaceRow {
                    key: "switch-named".into(),
                    title: "Switch to a named workspace".into(),
                    keys: RowKeys::Range(vec!["⌥".into(), "A…Z".into()]),
                },
                WorkspaceRow {
                    key: "move-named".into(),
                    title: "Move window to a named workspace".into(),
                    keys: RowKeys::Range(vec!["⌥".into(), "⇧".into(), "A…Z".into()]),
                },
            ]
        );
    }

    #[test]
    fn rebinding_space_1_keeps_control_on_the_others_and_leaves_move_alone() {
        let rebound =
            parse("[shortcuts.bindings]\n\"ctrl+1\" = \"none\"\n\"ctrl+alt+1\" = \"switch 1\"\n");
        let ws1 = WorkspaceId::new(1).unwrap();
        let ws2 = WorkspaceId::new(2).unwrap();
        assert_eq!(
            active_chords(&rebound, WorkspaceKind::Switch, ws1),
            vec![chord("ctrl+alt+1")]
        );
        assert_eq!(
            active_chords(&rebound, WorkspaceKind::Switch, ws2),
            vec![chord("ctrl+2")]
        );
        // Numbered switches are recorders in Switching, not rows on this card.
        let list = rows(&rebound, &[]);
        assert_eq!(list[0].title, "Move window to workspace 1–10");
        assert!(list.iter().all(|row| !row.key.starts_with("switch")));

        let restored = active_chords(&parse(""), WorkspaceKind::Switch, ws1);
        assert_eq!(restored, vec![chord("ctrl+1")]);
    }

    #[test]
    fn unbinding_or_stealing_a_workspace_chord_shows_not_set_and_splits_the_range() {
        let unbound = parse("[shortcuts.bindings]\n\"ctrl+3\" = \"none\"\n");
        let ws3 = WorkspaceId::new(3).unwrap();
        assert!(active_chords(&unbound, WorkspaceKind::Switch, ws3).is_empty());
        assert_eq!(
            active_chords(
                &unbound,
                WorkspaceKind::Switch,
                WorkspaceId::new(2).unwrap()
            ),
            vec![chord("ctrl+2")]
        );
        // Move still shows the compact family; unbinding Control+3 does not split it.
        assert_eq!(
            rows(&unbound, &[])[0].title,
            "Move window to workspace 1–10"
        );
    }

    #[test]
    fn stealing_workspace_10_chord_omits_the_default_switch() {
        let stolen = parse("[shortcuts.bindings]\n\"ctrl+0\" = \"float\"\n");
        assert!(
            active_chords(
                &stolen,
                WorkspaceKind::Switch,
                WorkspaceId::new(10).unwrap()
            )
            .is_empty()
        );
        assert_eq!(
            active_chords(&stolen, WorkspaceKind::Switch, WorkspaceId::new(9).unwrap()),
            vec![chord("ctrl+9")]
        );
    }

    #[test]
    fn defaults_false_still_shows_explicit_workspace_bindings() {
        let custom = parse(
            "[shortcuts]\ndefaults = false\n[shortcuts.bindings]\n\"ctrl+alt+1\" = \"switch 1\"\n\"ctrl+alt+shift+1\" = \"move 1\"\n",
        );
        assert_eq!(
            rows(&custom, &[]),
            vec![WorkspaceRow {
                key: "move-1".into(),
                title: "Move window to workspace 1".into(),
                keys: RowKeys::Chords(vec![chord("ctrl+alt+shift+1")]),
            }]
        );
        assert_eq!(
            active_chords(&custom, WorkspaceKind::Switch, WorkspaceId::new(1).unwrap()),
            vec![chord("ctrl+alt+1")]
        );
    }

    #[test]
    fn rebinding_an_enabled_named_workspace_updates_the_named_rows() {
        let rebound =
            parse("[shortcuts.bindings]\n\"alt+a\" = \"none\"\n\"ctrl+alt+a\" = \"switch A\"\n");
        let named = [
            WorkspaceId::named('A').unwrap(),
            WorkspaceId::named('D').unwrap(),
        ];
        let list = rows(&rebound, &named);
        assert_eq!(list[0].title, "Move window to workspace 1–10");
        assert_eq!(
            list[1],
            WorkspaceRow {
                key: "switch-A".into(),
                title: "Switch to workspace A".into(),
                keys: RowKeys::Chords(vec![chord("ctrl+alt+a")]),
            }
        );
        assert_eq!(
            list[2],
            WorkspaceRow {
                key: "switch-named".into(),
                title: "Switch to workspace D".into(),
                keys: RowKeys::Range(vec!["⌥".into(), "D".into()]),
            }
        );
        assert_eq!(list[3].title, "Move window to a named workspace");
    }

    #[test]
    fn shortcuts_pane_renders_rebound_and_unbound_workspace_rows() {
        let mut ui = crate::playground::blank_fixture();
        ui.doc = crate::rules::Doc::parse(
            "version = 1\n[shortcuts.bindings]\n\"alt+1\" = \"none\"\n\"ctrl+alt+1\" = \"switch 1\"\n\"alt+3\" = \"none\"\n",
        );
        let p = crate::controls::Paint::new(true, 2.0);
        let _ = super::super::pane(&ui, p);
    }
}
