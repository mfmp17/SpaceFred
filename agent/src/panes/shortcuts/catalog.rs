//! The shortcuts Settings › Shortcuts can rebind, each with its built-in chord, and what a
//! chord does now under `[shortcuts]`.

use spaceman::{
    WorkspaceId,
    shortcuts::{self, Chord, Command, Key, Modifiers, Shortcuts},
};

/// One rebindable shortcut.
#[derive(Debug, PartialEq, Eq)]
pub struct Action {
    /// Stable, for element keys and the recorder.
    pub id: &'static str,
    pub title: &'static str,
    /// In `spaceman ctl` syntax, as `[shortcuts.bindings]` writes it.
    pub command: &'static str,
    /// The built-in chord's key, and whether it adds Shift to the base modifier.
    pub default: Option<(bool, Key)>,
}

impl Action {
    pub fn parsed(&self) -> Option<Command> {
        shortcuts::parse_command(self.command).ok().flatten()
    }

    /// The built-in chord under `modifier`, if this shortcut has one.
    ///
    /// Switch to space 1–10 uses Control while `modifier` is Option, and `modifier` otherwise.
    pub fn default_chord(&self, modifier: Modifiers) -> Option<Chord> {
        self.default.map(|(shift, key)| {
            let base = if is_numbered_switch(self.command, key) {
                shortcuts::numbered_switch_modifiers(modifier)
            } else {
                modifier
            };
            Chord {
                modifiers: if shift { base.with_shift() } else { base },
                key,
            }
        })
    }

    /// The built-in chord under `[shortcuts]`'s modifier, while the built-ins are on and no
    /// enabled named workspace claims its letter.
    pub fn active_default(
        &self,
        shortcuts: &Shortcuts,
        named_workspaces: &[WorkspaceId],
    ) -> Option<Chord> {
        self.default_chord(shortcuts.modifier).filter(|chord| {
            shortcuts.defaults && named_workspace_for_key(named_workspaces, chord.key).is_none()
        })
    }

    /// Every chord that runs this shortcut: its bindings, then its built-in chord unless a
    /// binding or enabled named workspace took that over.
    pub fn chords(&self, shortcuts: &Shortcuts, named_workspaces: &[WorkspaceId]) -> Vec<Chord> {
        let command = self.parsed();
        let mut chords = shortcuts
            .bindings
            .iter()
            .filter(|binding| binding.command.is_some() && binding.command == command)
            .map(|binding| binding.chord)
            .collect::<Vec<_>>();
        if let Some(default) = self.active_default(shortcuts, named_workspaces)
            && shortcuts.binding(default).is_none()
            && !chords.contains(&default)
        {
            chords.push(default);
        }
        chords
    }
}

const fn action(
    id: &'static str,
    title: &'static str,
    command: &'static str,
    default: Option<(bool, Key)>,
) -> Action {
    Action {
        id,
        title,
        command,
        default,
    }
}

const TAB: Key = Key::Code(0x30);
const SPACE: Key = Key::Code(0x31);
const fn letter(shift: bool, character: char) -> Option<(bool, Key)> {
    Some((shift, Key::Character(character)))
}

fn is_numbered_switch(command: &str, key: Key) -> bool {
    matches!(key, Key::Number(1..=10)) && command.starts_with("switch ")
}

const fn switch_space(number: u8) -> Action {
    action(
        match number {
            1 => "switch-space-1",
            2 => "switch-space-2",
            3 => "switch-space-3",
            4 => "switch-space-4",
            5 => "switch-space-5",
            6 => "switch-space-6",
            7 => "switch-space-7",
            8 => "switch-space-8",
            9 => "switch-space-9",
            _ => "switch-space-10",
        },
        match number {
            1 => "Switch to space 1",
            2 => "Switch to space 2",
            3 => "Switch to space 3",
            4 => "Switch to space 4",
            5 => "Switch to space 5",
            6 => "Switch to space 6",
            7 => "Switch to space 7",
            8 => "Switch to space 8",
            9 => "Switch to space 9",
            _ => "Switch to space 10",
        },
        match number {
            1 => "switch 1",
            2 => "switch 2",
            3 => "switch 3",
            4 => "switch 4",
            5 => "switch 5",
            6 => "switch 6",
            7 => "switch 7",
            8 => "switch 8",
            9 => "switch 9",
            _ => "switch 10",
        },
        Some((false, Key::Number(number))),
    )
}

pub const SWITCH_SPACES: [Action; 10] = [
    switch_space(1),
    switch_space(2),
    switch_space(3),
    switch_space(4),
    switch_space(5),
    switch_space(6),
    switch_space(7),
    switch_space(8),
    switch_space(9),
    switch_space(10),
];

pub const WORKSPACE_ACTIONS: &[Action] = &[
    action(
        "back-and-forth",
        "Previous workspace",
        "back-and-forth",
        Some((false, TAB)),
    ),
    action(
        "move-workspace",
        "Move workspace to next display",
        "move-workspace next",
        Some((true, TAB)),
    ),
];

pub const LAYOUT_ACTIONS: &[Action] = &[
    action(
        "focus-next",
        "Next window",
        "focus next",
        letter(false, '`'),
    ),
    action(
        "focus-prev",
        "Previous window",
        "focus prev",
        letter(true, '`'),
    ),
    action("focus-left", "Focus left", "focus left", letter(false, 'h')),
    action("focus-down", "Focus down", "focus down", letter(false, 'j')),
    action("focus-up", "Focus up", "focus up", letter(false, 'k')),
    action(
        "focus-right",
        "Focus right",
        "focus right",
        letter(false, 'l'),
    ),
    action(
        "move-left",
        "Move left",
        "move-direction left",
        letter(true, 'h'),
    ),
    action(
        "move-down",
        "Move down",
        "move-direction down",
        letter(true, 'j'),
    ),
    action("move-up", "Move up", "move-direction up", letter(true, 'k')),
    action(
        "move-right",
        "Move right",
        "move-direction right",
        letter(true, 'l'),
    ),
    action("layout", "Flip split", "layout", letter(false, '/')),
    action("shrink", "Shrink tile", "resize shrink", letter(false, '-')),
    action("grow", "Grow tile", "resize grow", letter(false, '=')),
    action(
        "float",
        "Toggle floating window",
        "float",
        Some((true, SPACE)),
    ),
    action(
        "fullscreen",
        "Fullscreen layout",
        "fullscreen",
        letter(true, 'f'),
    ),
    action("balance", "Balance layout", "balance", None),
    action("retile", "Retile", "retile", None),
];

pub fn all() -> impl Iterator<Item = &'static Action> {
    SWITCH_SPACES
        .iter()
        .chain(WORKSPACE_ACTIONS)
        .chain(LAYOUT_ACTIONS)
}

pub fn find(id: &str) -> Option<&'static Action> {
    all().find(|action| action.id == id)
}

/// What `chord` runs now: a shortcut in the catalog, or another command by its name.
#[derive(Debug, PartialEq, Eq)]
pub enum Occupant {
    Action(&'static Action),
    Other(String),
}

impl Occupant {
    pub fn title(&self) -> &str {
        match self {
            Self::Action(action) => action.title,
            Self::Other(title) => title,
        }
    }
}

/// The enabled named workspace whose letter `key` types, if any.
pub fn named_workspace_for_key(named_workspaces: &[WorkspaceId], key: Key) -> Option<WorkspaceId> {
    let Key::Character(character) = key else {
        return None;
    };
    named_workspaces.iter().copied().find(|workspace| {
        workspace
            .letter()
            .is_some_and(|letter| letter.eq_ignore_ascii_case(&character))
    })
}

/// What `chord` runs under `shortcuts` and `named_workspaces`, if anything.
pub fn occupant(
    shortcuts: &Shortcuts,
    named_workspaces: &[WorkspaceId],
    chord: Chord,
) -> Option<Occupant> {
    if let Some(bound) = shortcuts.binding(chord) {
        let command = bound?;
        return Some(
            all()
                .find(|action| action.parsed().as_ref() == Some(command))
                .map_or_else(
                    || Occupant::Other(sentence(&command.description())),
                    Occupant::Action,
                ),
        );
    }
    if shortcuts.scratchpad
        && chord.key == Key::Character('\\')
        && !chord.modifiers.control
        && !chord.modifiers.command
        && chord.modifiers.option
    {
        return Some(Occupant::Other(
            if chord.modifiers.shift {
                "Move window to or from the scratchpad"
            } else {
                "Show or hide the scratchpad"
            }
            .into(),
        ));
    }
    if !shortcuts.defaults {
        return None;
    }
    // Enabled named workspaces take precedence over built-in letter shortcuts when they overlap.
    if let Some(workspace) = named_workspace_for_key(named_workspaces, chord.key) {
        if chord.modifiers == shortcuts.modifier {
            return Some(Occupant::Other(format!("Switch to workspace {workspace}")));
        }
        if chord.modifiers == shortcuts.modifier.with_shift() {
            return Some(Occupant::Other(format!(
                "Move window to workspace {workspace}"
            )));
        }
    }
    if let Some(action) =
        all().find(|action| action.default_chord(shortcuts.modifier) == Some(chord))
    {
        return Some(Occupant::Action(action));
    }
    let Key::Number(number) = chord.key else {
        return None;
    };
    // Switch to space is a catalog action (Control+number by default). Only Move still
    // falls through to the base modifier plus Shift.
    if chord.modifiers == shortcuts.modifier.with_shift() {
        Some(Occupant::Other(format!(
            "Move window to workspace {number}"
        )))
    } else {
        let _ = number;
        None
    }
}

fn sentence(description: &str) -> String {
    let mut characters = description.chars();
    characters.next().map_or_else(String::new, |first| {
        first.to_uppercase().chain(characters).collect()
    })
}

/// The base modifiers Settings offers, with their segment labels.
pub const PRESETS: [(&str, Modifiers); 3] = [
    ("⌥ Option", Modifiers::OPTION),
    (
        "⌃⌥ Control",
        Modifiers {
            command: false,
            control: true,
            option: true,
            shift: false,
        },
    ),
    (
        "⌃⌥⌘ Hyper",
        Modifiers {
            command: true,
            control: true,
            option: true,
            shift: false,
        },
    ),
];

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(bindings: &str) -> Shortcuts {
        spaceman::config::Config::parse(&format!("version = 1\n[shortcuts.bindings]\n{bindings}"))
            .unwrap()
            .shortcuts()
            .clone()
    }

    fn chord(source: &str) -> Chord {
        shortcuts::parse_chord(source).unwrap()
    }

    #[test]
    fn every_action_command_parses_and_ids_are_unique() {
        let mut ids = all().map(|action| action.id).collect::<Vec<_>>();
        for action in all() {
            assert!(action.parsed().is_some(), "{}", action.command);
        }
        let count = ids.len();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), count);
    }

    #[test]
    fn chords_follow_bindings_and_the_base_modifier() {
        let focus_left = find("focus-left").unwrap();
        assert_eq!(
            focus_left.chords(&Shortcuts::default(), &[]),
            [chord("alt+h")]
        );
        let moved = parse("\"alt+h\" = \"none\"\n\"ctrl+alt+h\" = \"focus left\"");
        assert_eq!(focus_left.chords(&moved, &[]), [chord("ctrl+alt+h")]);
        let taken = parse("\"alt+h\" = \"layout\"");
        assert!(focus_left.chords(&taken, &[]).is_empty());
        let hyper = Shortcuts {
            modifier: PRESETS[2].1,
            ..Shortcuts::default()
        };
        let move_left = find("move-left").unwrap();
        assert_eq!(
            move_left.chords(&hyper, &[]),
            [chord("ctrl+alt+shift+cmd+h")]
        );
        assert!(find("balance").unwrap().chords(&hyper, &[]).is_empty());
        // An enabled named workspace H takes precedence over built-in Focus left.
        let h = [WorkspaceId::named('H').unwrap()];
        assert!(focus_left.chords(&Shortcuts::default(), &h).is_empty());
    }

    #[test]
    fn occupant_names_built_ins_bindings_and_workspace_chords() {
        let defaults = Shortcuts::default();
        assert_eq!(
            occupant(&defaults, &[], chord("alt+l")),
            Some(Occupant::Action(find("focus-right").unwrap()))
        );
        assert_eq!(
            occupant(&defaults, &[], chord("alt+shift+3"))
                .unwrap()
                .title(),
            "Move window to workspace 3"
        );
        assert_eq!(occupant(&defaults, &[], chord("ctrl+alt+l")), None);
        let bound = parse("\"alt+l\" = \"none\"\n\"ctrl+alt+v\" = \"switch 4\"");
        assert_eq!(occupant(&bound, &[], chord("alt+l")), None);
        assert_eq!(
            occupant(&bound, &[], chord("ctrl+alt+v")).unwrap().title(),
            "Switch to space 4"
        );
        assert_eq!(
            occupant(&defaults, &[], chord("ctrl+2")).unwrap().title(),
            "Switch to space 2"
        );
        assert_eq!(occupant(&defaults, &[], chord("alt+2")), None);
        let scratchpad = Shortcuts {
            scratchpad: true,
            ..Shortcuts::default()
        };
        assert_eq!(
            occupant(&scratchpad, &[], chord("alt+\\")).unwrap().title(),
            "Show or hide the scratchpad"
        );
        assert_eq!(occupant(&defaults, &[], chord("alt+\\")), None);
    }

    #[test]
    fn occupant_names_enabled_named_workspaces_and_gives_them_precedence() {
        let defaults = Shortcuts::default();
        let named = [
            WorkspaceId::named('A').unwrap(),
            WorkspaceId::named('H').unwrap(),
        ];
        assert_eq!(
            occupant(&defaults, &named, chord("alt+a")).unwrap().title(),
            "Switch to workspace A"
        );
        assert_eq!(
            occupant(&defaults, &named, chord("alt+shift+a"))
                .unwrap()
                .title(),
            "Move window to workspace A"
        );
        assert_eq!(occupant(&defaults, &[], chord("alt+a")), None);
        // Workspace H wins over Focus left on alt+h.
        assert_eq!(
            occupant(&defaults, &named, chord("alt+h")).unwrap().title(),
            "Switch to workspace H"
        );
        // Explicit unbind frees the named workspace chord.
        let unbound = parse("\"alt+a\" = \"none\"");
        assert_eq!(occupant(&unbound, &named, chord("alt+a")), None);
        // With defaults = false, built-in named workspace shortcuts are inactive.
        let no_defaults = Shortcuts {
            defaults: false,
            ..Shortcuts::default()
        };
        assert_eq!(occupant(&no_defaults, &named, chord("alt+a")), None);
    }
}
