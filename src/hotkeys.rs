mod delivery;

use std::{
    collections::{BTreeMap, BTreeSet},
    ffi::c_void,
    ptr,
    sync::mpsc::Sender,
};

pub(crate) use delivery::DeliveryWatch;

use crate::{
    Direction, DisplayTarget, ResizeDirection, WorkspaceId,
    app::{Action, WorkerAction},
    keyboard_layout::{LayoutFingerprint, LayoutSnapshot, LogicalKey, SemanticKey},
    scratchpad::ScratchpadCommand,
    shortcuts::{self, Binding, Command, Shortcuts},
};

const EVENT_CLASS_KEYBOARD: u32 = u32::from_be_bytes(*b"keyb");
const EVENT_HOTKEY_PRESSED: u32 = 5;
pub(crate) const COMMAND_KEY: u32 = 1 << 8;
pub(crate) const SHIFT_KEY: u32 = 1 << 9;
pub(crate) const OPTION_KEY: u32 = 1 << 11;
pub(crate) const CONTROL_KEY: u32 = 1 << 12;
const SIGNATURE: u32 = u32::from_be_bytes(*b"SPMN");

#[repr(C)]
struct EventTypeSpec {
    event_class: u32,
    event_kind: u32,
}

#[repr(C)]
struct EventHotKeyId {
    signature: u32,
    id: u32,
}

type EventHandler = unsafe extern "C" fn(*mut c_void, *mut c_void, *mut c_void) -> i32;

#[link(name = "Carbon", kind = "framework")]
unsafe extern "C" {
    fn InstallEventHandler(
        target: *mut c_void,
        handler: EventHandler,
        event_type_count: u32,
        event_types: *const EventTypeSpec,
        user_data: *mut c_void,
        handler_ref: *mut *mut c_void,
    ) -> i32;
    fn RemoveEventHandler(handler_ref: *mut c_void) -> i32;
    fn GetApplicationEventTarget() -> *mut c_void;
    fn RegisterEventHotKey(
        key_code: u32,
        modifiers: u32,
        hotkey_id: EventHotKeyId,
        target: *mut c_void,
        options: u32,
        hotkey_ref: *mut *mut c_void,
    ) -> i32;
    fn UnregisterEventHotKey(hotkey_ref: *mut c_void) -> i32;
    fn GetEventParameter(
        event: *mut c_void,
        name: u32,
        desired_type: u32,
        actual_type: *mut u32,
        buffer_size: u32,
        actual_size: *mut u32,
        data: *mut c_void,
    ) -> i32;
}

pub struct Hotkeys {
    registrations: Vec<*mut c_void>,
    handler: *mut c_void,
    context: Option<Box<HandlerContext>>,
    layout_fingerprint: LayoutFingerprint,
    /// A hotkey's ID is its index here.
    bindings: Vec<Registration>,
    /// Shortcuts the plan or Carbon skipped while the rest registered, for the log.
    skipped: Vec<String>,
}

/// Read by the Carbon handler.
struct HandlerContext {
    sender: Sender<Action>,
}

#[derive(Debug)]
pub(crate) struct HotkeyPlan {
    registrations: Vec<Registration>,
    layout_fingerprint: LayoutFingerprint,
    /// Shortcuts this keyboard layout could not map. Each one is skipped on its own; the
    /// rest of the plan, and automatic tiling, keep running.
    warnings: Vec<String>,
}

#[derive(Clone, Debug)]
struct Registration {
    key_code: u32,
    modifiers: u32,
    action: WorkerAction,
    description: String,
}

impl HotkeyPlan {
    pub(crate) fn current(enabled: &[WorkspaceId], shortcuts: &Shortcuts) -> Result<Self, String> {
        let layout = LayoutSnapshot::current().map_err(|error| error.to_string())?;
        Self::from_layout_with_system(
            enabled,
            shortcuts,
            &layout,
            &crate::mission_control_shortcuts(),
        )
    }

    pub(crate) fn from_layout(
        enabled: &[WorkspaceId],
        shortcuts: &Shortcuts,
        layout: &LayoutSnapshot,
    ) -> Result<Self, String> {
        Self::from_layout_with_system(enabled, shortcuts, layout, &[])
    }

    pub(crate) fn from_layout_with_system(
        enabled: &[WorkspaceId],
        shortcuts: &Shortcuts,
        layout: &LayoutSnapshot,
        system: &[shortcuts::SystemChord],
    ) -> Result<Self, String> {
        let mut plan = PlanBuilder::new(
            layout,
            18 + enabled.len() * 2 + shortcuts.bindings.len(),
            system,
        );
        // `[shortcuts]` bindings claim their chords first, so a built-in shortcut on the same
        // chord quietly gives way instead of colliding.
        for binding in &shortcuts.bindings {
            plan.push_binding(binding, enabled);
        }
        if shortcuts.defaults {
            push_defaults(&mut plan, enabled, shortcuts);
        }
        if shortcuts.scratchpad {
            push_scratchpad(&mut plan);
        }

        if plan.registrations.is_empty() {
            return Err(match plan.warnings.first() {
                Some(warning) => format!("no shortcut could be planned: {warning}"),
                None => "no shortcut could be planned".into(),
            });
        }
        Ok(Self {
            registrations: plan.registrations,
            layout_fingerprint: layout.fingerprint().clone(),
            warnings: plan.warnings,
        })
    }

    pub(crate) fn matches(&self, hotkeys: &Hotkeys) -> bool {
        self.layout_fingerprint == hotkeys.layout_fingerprint
    }

    #[cfg(test)]
    pub(crate) fn has_registration(
        &self,
        key_code: u32,
        modifiers: u32,
        action: &WorkerAction,
    ) -> bool {
        self.registrations.iter().any(|registration| {
            registration.key_code == key_code
                && registration.modifiers == modifiers
                && registration.action == *action
        })
    }
}

/// The built-in shortcuts, in priority order.
/// Move shortcuts add Shift to `[shortcuts] modifier`. Numbered Switch to space uses Control
/// while that modifier is Option, and follows it otherwise.
fn push_defaults(plan: &mut PlanBuilder<'_>, enabled: &[WorkspaceId], shortcuts: &Shortcuts) {
    let base = carbon_modifiers(shortcuts.modifier);
    let switch = carbon_modifiers(shortcuts::numbered_switch_modifiers(shortcuts.modifier));
    let claimed_letters = enabled
        .iter()
        .filter_map(|workspace| workspace.letter().map(|letter| letter.to_ascii_lowercase()))
        .collect::<Vec<_>>();
    for workspace in WorkspaceId::all().filter(|workspace| enabled.contains(workspace)) {
        let key = match (workspace.number(), workspace.letter()) {
            (Some(number), _) => LogicalKey::Semantic(SemanticKey::Workspace(number)),
            (_, Some(letter)) => LogicalKey::Character(letter.to_ascii_lowercase()),
            _ => unreachable!("workspace ID is always valid"),
        };
        let switch_modifiers = if workspace.number().is_some() {
            switch
        } else {
            base
        };
        plan.push(
            key,
            switch_modifiers,
            WorkerAction::SwitchWorkspace(workspace),
            format!("switch workspace {workspace}"),
        );
        plan.push(
            key,
            base | SHIFT_KEY,
            WorkerAction::MoveFocused(workspace),
            format!("move window to workspace {workspace}"),
        );
    }
    for (character, direction, name) in [
        ('h', Direction::Left, "left"),
        ('j', Direction::Down, "down"),
        ('k', Direction::Up, "up"),
        ('l', Direction::Right, "right"),
    ] {
        if claimed_letters.contains(&character) {
            continue;
        }
        plan.push(
            LogicalKey::Character(character),
            base,
            WorkerAction::Focus(direction),
            format!("focus {name}"),
        );
        plan.push(
            LogicalKey::Character(character),
            base | SHIFT_KEY,
            WorkerAction::Reorder(direction),
            format!("move window {name}"),
        );
    }
    for (key, modifiers, action, description) in [
        (
            LogicalKey::Character('f'),
            base | SHIFT_KEY,
            WorkerAction::ToggleFullscreenLayout,
            "toggle fullscreen layout",
        ),
        (
            LogicalKey::Semantic(SemanticKey::Space),
            base | SHIFT_KEY,
            WorkerAction::ToggleFloat,
            "toggle floating window",
        ),
        (
            LogicalKey::Character('/'),
            base,
            WorkerAction::ToggleLayout,
            "toggle layout",
        ),
        (
            LogicalKey::Character('`'),
            base,
            WorkerAction::CycleFocus,
            "cycle workspace windows",
        ),
        (
            LogicalKey::Character('`'),
            base | SHIFT_KEY,
            WorkerAction::CycleFocusPrevious,
            "cycle workspace windows backward",
        ),
        (
            LogicalKey::Semantic(SemanticKey::Tab),
            base,
            WorkerAction::WorkspaceBackAndForth,
            "workspace back-and-forth",
        ),
        (
            LogicalKey::Semantic(SemanticKey::Tab),
            base | SHIFT_KEY,
            WorkerAction::MoveWorkspaceToDisplay(DisplayTarget::Next),
            "move workspace to next display",
        ),
        (
            LogicalKey::Character('-'),
            base,
            WorkerAction::Resize(ResizeDirection::Shrink),
            "shrink layout",
        ),
        (
            LogicalKey::Character('='),
            base,
            WorkerAction::Resize(ResizeDirection::Grow),
            "grow layout",
        ),
    ] {
        if matches!(key, LogicalKey::Character(character) if claimed_letters.contains(&character)) {
            continue;
        }
        plan.push(key, modifiers, action, description);
    }
}

/// The Scratchpad Lab's shortcuts, planned only while `[labs] scratchpad` is on so a default
/// install registers exactly the built-in set.
fn push_scratchpad(plan: &mut PlanBuilder<'_>) {
    for (modifiers, command, description) in [
        (
            OPTION_KEY,
            ScratchpadCommand::Toggle,
            "show or hide the scratchpad",
        ),
        (
            OPTION_KEY | SHIFT_KEY,
            ScratchpadCommand::Mark,
            "move window to or from the scratchpad",
        ),
    ] {
        plan.push(
            LogicalKey::Character('\\'),
            modifiers,
            WorkerAction::Scratchpad(command),
            description,
        );
    }
}

/// The worker action a `[shortcuts]` command queues.
fn worker_action(command: &Command) -> WorkerAction {
    match command {
        Command::Switch(workspace) => WorkerAction::SwitchWorkspace(*workspace),
        Command::Move(workspace) => WorkerAction::MoveFocused(*workspace),
        Command::MoveFollow(workspace) => WorkerAction::MoveFocusedFollow(*workspace),
        Command::BackAndForth => WorkerAction::WorkspaceBackAndForth,
        Command::MoveWorkspace(target) => WorkerAction::MoveWorkspaceToDisplay(target.clone()),
        Command::Focus(direction) => WorkerAction::Focus(*direction),
        Command::FocusNext => WorkerAction::CycleFocus,
        Command::FocusPrev => WorkerAction::CycleFocusPrevious,
        Command::MoveDirection(direction) => WorkerAction::Reorder(*direction),
        Command::Layout => WorkerAction::ToggleLayout,
        Command::Fullscreen => WorkerAction::ToggleFullscreenLayout,
        Command::Float => WorkerAction::ToggleFloat,
        Command::Resize(direction) => WorkerAction::Resize(*direction),
        Command::ResizeTo(axis, points) => WorkerAction::ResizeTo(*axis, *points),
        Command::Balance => WorkerAction::Balance,
        Command::Retile => WorkerAction::Retile,
    }
}

fn logical_key(key: shortcuts::Key) -> LogicalKey {
    match key {
        shortcuts::Key::Character(character) => LogicalKey::Character(character),
        shortcuts::Key::Number(number) => LogicalKey::Semantic(SemanticKey::Workspace(number)),
        shortcuts::Key::Code(code) => LogicalKey::Semantic(SemanticKey::KeyCode(code)),
    }
}

fn carbon_modifiers(modifiers: shortcuts::Modifiers) -> u32 {
    [
        (modifiers.command, COMMAND_KEY),
        (modifiers.control, CONTROL_KEY),
        (modifiers.option, OPTION_KEY),
        (modifiers.shift, SHIFT_KEY),
    ]
    .into_iter()
    .filter(|(held, _)| *held)
    .fold(0, |bits, (_, bit)| bits | bit)
}

/// Builds a plan in priority order, keeping the first shortcut that claims a chord and
/// skipping later ones that would collide. Unusual layouts (Italian, AZERTY) reset two
/// characters onto one physical key with different Shift states, and that must cost one
/// shortcut rather than every shortcut and automatic tiling.
struct PlanBuilder<'a> {
    layout: &'a LayoutSnapshot,
    registrations: Vec<Registration>,
    /// Enabled Mission Control chords, by Carbon key code and modifiers.
    system: BTreeMap<(u32, u32), String>,
    occupied: BTreeMap<(u32, u32), String>,
    /// Chords a `[shortcuts]` binding claimed (or unbound); built-in shortcuts skip them quietly.
    user_chords: BTreeSet<(u32, u32)>,
    /// Shortcuts this layout could not map, kept for diagnosis instead of pausing the agent.
    warnings: Vec<String>,
}

impl<'a> PlanBuilder<'a> {
    fn new(layout: &'a LayoutSnapshot, capacity: usize, system: &[shortcuts::SystemChord]) -> Self {
        let system = system
            .iter()
            .map(|entry| {
                (
                    (u32::from(entry.key_code), carbon_modifiers(entry.modifiers)),
                    entry.name.clone(),
                )
            })
            .collect();
        Self {
            layout,
            registrations: Vec::with_capacity(capacity),
            system,
            occupied: BTreeMap::new(),
            user_chords: BTreeSet::new(),
            warnings: Vec::new(),
        }
    }

    /// Claims a `[shortcuts]` binding's chord: registers its command, or for `none` only
    /// reserves the chord so no built-in shortcut takes it.
    fn push_binding(&mut self, binding: &Binding, enabled: &[WorkspaceId]) {
        let source = &binding.source;
        let candidates = match self.layout.candidates(logical_key(binding.chord.key)) {
            Ok(candidates) => candidates,
            Err(error) => {
                self.warnings
                    .push(format!("skipped shortcut {source}: {error}"));
                return;
            }
        };
        let base = carbon_modifiers(binding.chord.modifiers);
        let chords = candidates
            .into_iter()
            .map(|candidate| {
                let shift = if candidate.shift { SHIFT_KEY } else { 0 };
                (candidate.key_code, base | shift)
            })
            .collect::<Vec<_>>();
        let Some(chord) = chords
            .iter()
            .copied()
            .find(|chord| !self.occupied.contains_key(chord) && !self.system.contains_key(chord))
        else {
            let owner = chords.iter().find_map(|chord| self.system.get(chord));
            self.warnings.push(match owner {
                Some(owner) => {
                    format!("skipped shortcut {source}: {owner} uses this shortcut, left for macOS")
                }
                None => {
                    format!("skipped shortcut {source}: another shortcut already uses that key")
                }
            });
            return;
        };
        self.user_chords.insert(chord);
        let Some(command) = &binding.command else {
            self.occupied.insert(chord, format!("{source} (unbound)"));
            return;
        };
        if let Some(workspace) = command.workspace()
            && !enabled.contains(&workspace)
        {
            self.occupied
                .insert(chord, format!("{source} (unavailable)"));
            self.warnings.push(format!(
                "skipped shortcut {source}: workspace {workspace} is not enabled on a managed display"
            ));
            return;
        }
        let description = command.description();
        self.occupied.insert(chord, description.clone());
        self.registrations.push(Registration {
            key_code: chord.0,
            modifiers: chord.1,
            action: worker_action(command),
            description,
        });
    }

    fn push(
        &mut self,
        key: LogicalKey,
        modifiers: u32,
        action: WorkerAction,
        description: impl Into<String>,
    ) {
        let description = description.into();
        let candidates = match self.layout.candidates(key) {
            Ok(candidates) => candidates,
            Err(error) => {
                self.warnings
                    .push(format!("skipped {description}: {error}"));
                return;
            }
        };
        let mut left_for_macos = None;
        for candidate in candidates {
            let modifiers = modifiers | if candidate.shift { SHIFT_KEY } else { 0 };
            let chord = (candidate.key_code, modifiers);
            if self.user_chords.contains(&chord) {
                // A `[shortcuts]` binding replaced this built-in shortcut.
                return;
            }
            if let Some(owner) = self.system.get(&chord) {
                left_for_macos = Some(owner.clone());
                continue;
            }
            if self.occupied.contains_key(&chord) {
                continue;
            }
            self.occupied.insert(chord, description.clone());
            self.registrations.push(Registration {
                key_code: chord.0,
                modifiers,
                action,
                description,
            });
            return;
        }
        if let Some(owner) = left_for_macos {
            self.warnings.push(format!(
                "skipped {description}: {owner} uses this shortcut, left for macOS"
            ));
            return;
        }
        self.warnings.push(format!(
            "skipped {description}: every key it can use is already taken"
        ));
    }
}

impl Hotkeys {
    pub fn install(sender: Sender<Action>, plan: HotkeyPlan) -> Result<Self, String> {
        let context = Box::new(HandlerContext { sender });
        let context_pointer = (&raw const *context).cast_mut();
        let event_type = EventTypeSpec {
            event_class: EVENT_CLASS_KEYBOARD,
            event_kind: EVENT_HOTKEY_PRESSED,
        };
        let mut handler = ptr::null_mut();
        // SAFETY: `event_type` lives across installation, `context` stays heap-pinned in `self`
        // (or is leaked if removal fails), and `handler` is a valid out-pointer.
        let status = unsafe {
            InstallEventHandler(
                GetApplicationEventTarget(),
                hotkey_handler,
                1,
                &raw const event_type,
                context_pointer.cast(),
                &raw mut handler,
            )
        };
        if status != 0 {
            return Err(format!("failed to install hotkey handler ({status})"));
        }

        let HotkeyPlan {
            registrations,
            layout_fingerprint,
            warnings,
        } = plan;
        let mut hotkeys = Self {
            registrations: Vec::with_capacity(registrations.len()),
            handler,
            context: Some(context),
            layout_fingerprint,
            bindings: Vec::new(),
            skipped: Vec::new(),
        };
        // Shortcuts the layout already skipped are the first reasons a plan can be thin.
        let mut failures = warnings;
        for registration in registrations {
            // A chord another app already owns (or macOS refuses) costs only that shortcut.
            let id = u32::try_from(hotkeys.bindings.len())
                .map_err(|_| "too many hotkeys to register".to_owned())?;
            match hotkeys.register(registration.key_code, registration.modifiers, id) {
                Ok(()) => hotkeys.bindings.push(registration),
                Err(error) => {
                    failures.push(format!("skipped {}: {error}", registration.description))
                }
            }
        }
        if hotkeys.bindings.is_empty() {
            return Err(match failures.first() {
                Some(failure) => failure.clone(),
                None => "no shortcut could be registered".into(),
            });
        }
        hotkeys.skipped = failures;
        Ok(hotkeys)
    }

    pub(crate) fn len(&self) -> usize {
        self.bindings.len()
    }

    /// Why each skipped shortcut was skipped, such as a `[shortcuts]` binding this keyboard
    /// layout cannot type or a chord another app already owns.
    pub(crate) fn skipped(&self) -> &[String] {
        &self.skipped
    }

    /// The action and description of the hotkey Carbon delivered as `id`.
    pub(crate) fn binding(&self, id: u32) -> Option<(&WorkerAction, &str)> {
        let registration = self.bindings.get(usize::try_from(id).ok()?)?;
        Some((&registration.action, &registration.description))
    }

    /// The hotkey registered for this key and these Carbon modifiers.
    pub(crate) fn id_for(&self, key_code: u32, modifiers: u32) -> Option<u32> {
        let index = self.bindings.iter().position(|registration| {
            registration.key_code == key_code && registration.modifiers == modifiers
        })?;
        u32::try_from(index).ok()
    }

    pub fn matches_current_layout(&self) -> Result<bool, String> {
        LayoutSnapshot::current_fingerprint()
            .map(|fingerprint| self.layout_fingerprint == fingerprint)
            .map_err(|error| error.to_string())
    }

    fn register(&mut self, key_code: u32, modifiers: u32, id: u32) -> Result<(), String> {
        let mut registration = ptr::null_mut();
        // SAFETY: Carbon's application event target is valid on the main thread and `registration`
        // is a valid out-pointer.
        let status = unsafe {
            RegisterEventHotKey(
                key_code,
                modifiers,
                EventHotKeyId {
                    signature: SIGNATURE,
                    id,
                },
                GetApplicationEventTarget(),
                0,
                &raw mut registration,
            )
        };
        if status != 0 {
            return Err(format!("hotkey is unavailable ({status})"));
        }
        self.registrations.push(registration);
        Ok(())
    }
}

impl Drop for Hotkeys {
    fn drop(&mut self) {
        for registration in self.registrations.drain(..) {
            // SAFETY: Each registration handle came from a successful `RegisterEventHotKey` call.
            unsafe { UnregisterEventHotKey(registration) };
        }
        // SAFETY: `self.handler` came from a successful `InstallEventHandler` call.
        let removed = unsafe { RemoveEventHandler(self.handler) };
        if removed != 0
            && let Some(context) = self.context.take()
        {
            Box::leak(context);
        }
    }
}

unsafe extern "C" fn hotkey_handler(
    _: *mut c_void,
    event: *mut c_void,
    user_data: *mut c_void,
) -> i32 {
    // SAFETY: `user_data` points to the `HandlerContext` boxed in `Hotkeys::install`, which
    // outlives the installed Carbon event handler.
    let context = unsafe { &*user_data.cast::<HandlerContext>() };
    let mut hotkey_id = EventHotKeyId {
        signature: 0,
        id: 0,
    };
    // SAFETY: `event` is provided by Carbon for `EVENT_HOTKEY_PRESSED`, and `hotkey_id` matches
    // the expected `EventHotKeyId` layout and byte count.
    let status = unsafe {
        GetEventParameter(
            event,
            u32::from_be_bytes(*b"----"),
            u32::from_be_bytes(*b"hkid"),
            ptr::null_mut(),
            u32::try_from(std::mem::size_of::<EventHotKeyId>()).unwrap_or_default(),
            ptr::null_mut(),
            (&raw mut hotkey_id).cast(),
        )
    };
    if status == 0 && hotkey_id.signature == SIGNATURE {
        drop(context.sender.send(Action::Hotkey(hotkey_id.id)));
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn layout() -> LayoutSnapshot {
        LayoutSnapshot::for_test(
            vec![
                (60, 'h', false, false),
                (61, 'j', false, false),
                (62, 'k', false, false),
                (63, 'l', false, false),
                (64, 'f', false, false),
                (65, '/', false, false),
                (66, '-', false, false),
                (67, '=', false, false),
                (68, '`', false, false),
            ],
            vec![],
        )
    }

    fn action<'a>(plan: &'a HotkeyPlan, description: &str) -> Option<&'a WorkerAction> {
        plan.registrations
            .iter()
            .find(|registration| registration.description == description)
            .map(|registration| &registration.action)
    }

    #[test]
    fn registrations_carry_switch_move_layout_and_float_actions_including_zero_key() {
        let enabled = WorkspaceId::numbered().collect::<Vec<_>>();
        let plan = HotkeyPlan::from_layout(&enabled, &Shortcuts::default(), &layout()).unwrap();
        let one = WorkspaceId::new(1).unwrap();
        let ten = WorkspaceId::new(10).unwrap();
        for (description, expected) in [
            ("switch workspace 1", WorkerAction::SwitchWorkspace(one)),
            (
                "toggle fullscreen layout",
                WorkerAction::ToggleFullscreenLayout,
            ),
            ("toggle floating window", WorkerAction::ToggleFloat),
            ("toggle layout", WorkerAction::ToggleLayout),
            (
                "workspace back-and-forth",
                WorkerAction::WorkspaceBackAndForth,
            ),
            (
                "move workspace to next display",
                WorkerAction::MoveWorkspaceToDisplay(DisplayTarget::Next),
            ),
            (
                "shrink layout",
                WorkerAction::Resize(ResizeDirection::Shrink),
            ),
            ("grow layout", WorkerAction::Resize(ResizeDirection::Grow)),
            ("cycle workspace windows", WorkerAction::CycleFocus),
            (
                "cycle workspace windows backward",
                WorkerAction::CycleFocusPrevious,
            ),
            ("focus left", WorkerAction::Focus(Direction::Left)),
            ("focus right", WorkerAction::Focus(Direction::Right)),
            ("move window left", WorkerAction::Reorder(Direction::Left)),
            ("move window right", WorkerAction::Reorder(Direction::Right)),
            ("switch workspace 10", WorkerAction::SwitchWorkspace(ten)),
            (
                "move window to workspace 10",
                WorkerAction::MoveFocused(ten),
            ),
        ] {
            assert_eq!(action(&plan, description), Some(&expected), "{description}");
        }
    }

    #[test]
    fn scratchpad_shortcuts_register_only_while_the_lab_is_on() {
        let enabled = WorkspaceId::numbered().collect::<Vec<_>>();
        let layout = LayoutSnapshot::for_test(vec![(42, '\\', false, false)], vec![]);
        let scratchpad = |plan: &HotkeyPlan| {
            plan.registrations
                .iter()
                .filter(|registration| matches!(registration.action, WorkerAction::Scratchpad(_)))
                .map(|registration| {
                    (
                        registration.key_code,
                        registration.modifiers,
                        registration.action.clone(),
                    )
                })
                .collect::<Vec<_>>()
        };

        let off = HotkeyPlan::from_layout(&enabled, &Shortcuts::default(), &layout).unwrap();
        assert!(scratchpad(&off).is_empty());

        let shortcuts = Shortcuts {
            scratchpad: true,
            ..Shortcuts::default()
        };
        let on = HotkeyPlan::from_layout(&enabled, &shortcuts, &layout).unwrap();
        assert_eq!(
            scratchpad(&on),
            vec![
                (
                    42,
                    OPTION_KEY,
                    WorkerAction::Scratchpad(ScratchpadCommand::Toggle)
                ),
                (
                    42,
                    OPTION_KEY | SHIFT_KEY,
                    WorkerAction::Scratchpad(ScratchpadCommand::Mark)
                ),
            ]
        );
        assert_eq!(on.registrations.len(), off.registrations.len() + 2);
    }

    #[test]
    fn full_plan_contains_all_thirty_seven_registrations() {
        let enabled = WorkspaceId::numbered().collect::<Vec<_>>();
        let plan = HotkeyPlan::from_layout(&enabled, &Shortcuts::default(), &layout()).unwrap();
        assert_eq!(plan.registrations.len(), 37);
        assert!(plan.registrations.capacity() >= 37);
        let move_ws = plan
            .registrations
            .iter()
            .find(|registration| {
                registration.action == WorkerAction::MoveWorkspaceToDisplay(DisplayTarget::Next)
            })
            .unwrap();
        assert_eq!(move_ws.modifiers, OPTION_KEY | SHIFT_KEY);
    }

    #[test]
    fn float_shortcut_is_option_shift_space_and_collides_with_nothing() {
        let enabled = WorkspaceId::numbered().collect::<Vec<_>>();
        let plan = HotkeyPlan::from_layout(&enabled, &Shortcuts::default(), &layout()).unwrap();
        let float = plan
            .registrations
            .iter()
            .filter(|registration| registration.action == WorkerAction::ToggleFloat)
            .collect::<Vec<_>>();
        assert_eq!(float.len(), 1);
        // Physical kVK_Space, so every keyboard layout can press it.
        assert_eq!(float[0].key_code, 0x31);
        assert_eq!(float[0].modifiers, OPTION_KEY | SHIFT_KEY);
        assert_eq!(float[0].description, "toggle floating window");

        let mut chords = plan
            .registrations
            .iter()
            .map(|registration| (registration.key_code, registration.modifiers))
            .collect::<Vec<_>>();
        chords.sort_unstable();
        let mut unique = chords.clone();
        unique.dedup();
        assert_eq!(
            chords, unique,
            "every planned chord must be registered once"
        );
    }

    #[test]
    fn named_workspace_claims_its_letter_shortcuts() {
        let mut enabled = WorkspaceId::numbered().collect::<Vec<_>>();
        enabled.push(WorkspaceId::named('H').unwrap());
        let plan = HotkeyPlan::from_layout(&enabled, &Shortcuts::default(), &layout()).unwrap();
        let h = WorkspaceId::named('H').unwrap();
        assert_eq!(
            action(&plan, "switch workspace H"),
            Some(&WorkerAction::SwitchWorkspace(h))
        );
        assert_eq!(
            action(&plan, "move window to workspace H"),
            Some(&WorkerAction::MoveFocused(h))
        );
        assert_eq!(action(&plan, "focus left"), None);
        assert_eq!(action(&plan, "move window left"), None);
    }

    #[test]
    fn layout_required_shift_is_composed_with_command_modifiers() {
        let layout = LayoutSnapshot::for_test(
            vec![
                (60, 'h', false, false),
                (61, 'j', false, false),
                (62, 'k', false, false),
                (63, 'l', false, false),
                (64, 'f', false, false),
                (65, '/', true, false),
                (66, '-', false, false),
                (67, '=', false, false),
                (68, '`', false, false),
            ],
            vec![],
        );
        let plan = HotkeyPlan::from_layout(&[], &Shortcuts::default(), &layout).unwrap();
        let toggle_layout = plan
            .registrations
            .iter()
            .find(|registration| registration.action == WorkerAction::ToggleLayout)
            .unwrap();
        assert_eq!(toggle_layout.modifiers, OPTION_KEY | SHIFT_KEY);
    }

    #[test]
    fn duplicate_chords_skip_only_the_conflicting_shortcut() {
        let layout = LayoutSnapshot::for_test(
            vec![
                (60, 'h', false, false),
                (60, 'j', false, false),
                (62, 'k', false, false),
                (63, 'l', false, false),
                (64, 'f', false, false),
                (65, '/', false, false),
                (66, '-', false, false),
                (67, '=', false, false),
                (68, '`', false, false),
            ],
            vec![],
        );
        let plan = HotkeyPlan::from_layout(&[], &Shortcuts::default(), &layout).unwrap();
        assert_eq!(
            action(&plan, "focus left"),
            Some(&WorkerAction::Focus(Direction::Left))
        );
        assert_eq!(action(&plan, "focus down"), None);
        assert_eq!(
            action(&plan, "toggle layout"),
            Some(&WorkerAction::ToggleLayout)
        );
        assert!(
            plan.warnings
                .iter()
                .any(|warning| warning.contains("skipped focus down")),
            "{:?}",
            plan.warnings
        );
    }

    #[test]
    fn italian_pro_without_a_grave_keeps_tiling_and_remaps_shifted_punctuation() {
        // Italian – Pro: no key types a plain grave, `/` is Shift+7 (virtual key 0x1a, the
        // physical 7 key), and `=` is Shift+0 (0x1d, the physical 0 key).
        let italian = LayoutSnapshot::for_test(
            vec![
                (4, 'h', false, false),
                (38, 'j', false, false),
                (40, 'k', false, false),
                (37, 'l', false, false),
                (3, 'f', false, false),
                (0x1a, '/', true, false),
                (0x1b, '-', false, false),
                (0x1d, '=', true, false),
            ],
            vec![],
        );
        let enabled = WorkspaceId::numbered().collect::<Vec<_>>();
        let plan = HotkeyPlan::from_layout(&enabled, &Shortcuts::default(), &italian).unwrap();
        let registration = |description: &str| {
            plan.registrations
                .iter()
                .find(|registration| registration.description == description)
                .unwrap_or_else(|| panic!("missing {description}: {:?}", plan.warnings))
        };

        // The semantic number row still owns Option+Shift+7 and Option+Shift+0.
        assert_eq!(registration("move window to workspace 7").key_code, 0x1a);
        assert_eq!(registration("move window to workspace 10").key_code, 0x1d);

        // Each shifted punctuation shortcut moves to its plain ANSI physical key instead of
        // colliding with a workspace chord or disappearing.
        for (description, key_code, modifiers) in [
            ("toggle layout", 0x2c, OPTION_KEY),
            ("grow layout", 0x18, OPTION_KEY),
            ("shrink layout", 0x1b, OPTION_KEY),
            // The missing grave falls back to the key above Tab.
            ("cycle workspace windows", 0x32, OPTION_KEY),
        ] {
            let registration = registration(description);
            assert_eq!(registration.key_code, key_code, "{description}");
            assert_eq!(registration.modifiers, modifiers, "{description}");
        }
        let focus_left = registration("focus left");
        assert_eq!(focus_left.key_code, 4);
        assert_eq!(focus_left.modifiers, OPTION_KEY);
        assert_eq!(focus_left.action, WorkerAction::Focus(Direction::Left));
        assert_eq!(
            registration("switch workspace 1").key_code,
            0x12,
            "the semantic number row keeps tiling reachable"
        );
        assert!(plan.warnings.is_empty(), "{:?}", plan.warnings);
    }

    #[test]
    fn german_dead_grave_key_still_plans_the_cycle_shortcut() {
        // German QWERTZ exposes no live grave; the only one is a dead key on 0x0a.
        let german = LayoutSnapshot::for_test(
            vec![
                (4, 'h', false, false),
                (38, 'j', false, false),
                (40, 'k', false, false),
                (37, 'l', false, false),
                (3, 'f', false, false),
                (0x2c, '/', true, false),
                (0x1b, '-', false, false),
                (0x18, '=', false, false),
                (0x0a, '`', false, true),
            ],
            vec![],
        );
        let plan = HotkeyPlan::from_layout(&[], &Shortcuts::default(), &german).unwrap();
        let cycle = plan
            .registrations
            .iter()
            .find(|registration| registration.action == WorkerAction::CycleFocus)
            .expect("cycle shortcut is planned from the dead key");
        assert_eq!(cycle.key_code, 0x0a);
        assert_eq!(cycle.modifiers, OPTION_KEY);
        assert!(plan.warnings.is_empty(), "{:?}", plan.warnings);
    }

    #[test]
    fn layout_without_letters_keeps_semantic_and_punctuation_shortcuts() {
        // Cyrillic without an ASCII-capable fallback cannot type h, j, k or l at all.
        let non_latin_without_ascii_fallback = LayoutSnapshot::for_test(
            vec![
                (60, 'ж', false, false),
                (61, 'д', false, false),
                (62, 'л', false, false),
                (63, 'о', false, false),
                (65, '/', false, false),
                (66, '-', false, false),
                (67, '=', false, false),
            ],
            vec![],
        );

        let plan = HotkeyPlan::from_layout(
            &[],
            &Shortcuts::default(),
            &non_latin_without_ascii_fallback,
        )
        .unwrap();
        assert_eq!(action(&plan, "focus left"), None);
        assert_eq!(
            action(&plan, "toggle layout"),
            Some(&WorkerAction::ToggleLayout)
        );
        assert_eq!(
            action(&plan, "workspace back-and-forth"),
            Some(&WorkerAction::WorkspaceBackAndForth)
        );
        assert_eq!(
            action(&plan, "cycle workspace windows"),
            Some(&WorkerAction::CycleFocus)
        );
        assert!(
            plan.warnings
                .iter()
                .any(|warning| warning.contains("skipped focus left")
                    && warning.contains("does not produce 'h'")),
            "{:?}",
            plan.warnings
        );
    }

    fn shortcuts(defaults: bool, bindings: &str) -> Shortcuts {
        crate::config::Config::parse(&format!(
            "version = 1\n[shortcuts]\ndefaults = {defaults}\n[shortcuts.bindings]\n{bindings}"
        ))
        .unwrap()
        .shortcuts()
        .clone()
    }

    fn registration<'a>(plan: &'a HotkeyPlan, action: &WorkerAction) -> Vec<&'a Registration> {
        plan.registrations
            .iter()
            .filter(|registration| registration.action == *action)
            .collect()
    }

    #[test]
    fn configured_binding_replaces_the_built_in_shortcut_on_its_chord() {
        let enabled = WorkspaceId::numbered().collect::<Vec<_>>();
        let shortcuts = shortcuts(true, r#""alt+j" = "layout""#);
        let plan = HotkeyPlan::from_layout(&enabled, &shortcuts, &layout()).unwrap();
        // Option+J now flips the split instead of focusing down, with no collision warning.
        let toggles = registration(&plan, &WorkerAction::ToggleLayout);
        assert!(toggles.iter().any(
            |registration| registration.key_code == 61 && registration.modifiers == OPTION_KEY
        ));
        assert_eq!(action(&plan, "focus down"), None);
        // The built-in Option+/ stays, and so does Option+Shift+J.
        assert!(
            toggles
                .iter()
                .any(|registration| registration.key_code == 65)
        );
        assert_eq!(
            action(&plan, "move window down"),
            Some(&WorkerAction::Reorder(Direction::Down))
        );
        assert!(plan.warnings.is_empty(), "{:?}", plan.warnings);
    }

    #[test]
    fn arrow_keys_and_extra_modifiers_register_their_virtual_key_codes() {
        let shortcuts = shortcuts(
            true,
            r#"
            "alt+left" = "move-direction left"
            "alt+shift+ctrl+right" = "focus right"
            "alt+shift+b" = "balance"
            "ctrl+alt+w" = "resize width 1200"
            "#,
        );
        let layout_with_b = LayoutSnapshot::for_test(
            vec![(11, 'b', false, false), (13, 'w', false, false)],
            vec![],
        );
        let plan = HotkeyPlan::from_layout(&[], &shortcuts, &layout_with_b).unwrap();
        let reorder = registration(&plan, &WorkerAction::Reorder(Direction::Left));
        assert!(
            reorder
                .iter()
                .any(|registration| registration.key_code == 0x7b
                    && registration.modifiers == OPTION_KEY)
        );
        let focus = registration(&plan, &WorkerAction::Focus(Direction::Right));
        assert!(
            focus
                .iter()
                .any(|registration| registration.key_code == 0x7c
                    && registration.modifiers == OPTION_KEY | SHIFT_KEY | CONTROL_KEY)
        );
        assert_eq!(registration(&plan, &WorkerAction::Balance).len(), 1);
        let resize = registration(
            &plan,
            &WorkerAction::ResizeTo(crate::Axis::Horizontal, 1200),
        );
        assert!(resize.iter().any(|registration| registration.key_code == 13
            && registration.modifiers == OPTION_KEY | CONTROL_KEY));
    }

    #[test]
    fn none_unbinds_a_built_in_shortcut_without_registering_anything() {
        let shortcuts = shortcuts(true, r#""alt+h" = "none""#);
        let plan = HotkeyPlan::from_layout(&[], &shortcuts, &layout()).unwrap();
        assert_eq!(action(&plan, "focus left"), None);
        assert!(!plan.registrations.iter().any(
            |registration| registration.key_code == 60 && registration.modifiers == OPTION_KEY
        ));
        assert!(plan.warnings.is_empty(), "{:?}", plan.warnings);
    }

    #[test]
    fn defaults_false_registers_only_the_configured_bindings() {
        let enabled = WorkspaceId::numbered().collect::<Vec<_>>();
        let shortcuts = shortcuts(
            false,
            r#"
            "alt+1" = "switch 1"
            "alt+shift+1" = "move 1"
            "alt+v" = "float"
            "#,
        );
        let with_v = LayoutSnapshot::for_test(vec![(69, 'v', false, false)], vec![]);
        let plan = HotkeyPlan::from_layout(&enabled, &shortcuts, &with_v).unwrap();
        assert_eq!(plan.registrations.len(), 3);
        assert_eq!(
            action(&plan, "switch workspace 1"),
            Some(&WorkerAction::SwitchWorkspace(WorkspaceId::new(1).unwrap()))
        );
        assert_eq!(
            registration(&plan, &WorkerAction::ToggleFloat)[0].key_code,
            69
        );
    }

    #[test]
    fn binding_a_workspace_that_is_not_enabled_is_skipped_with_a_warning() {
        let shortcuts = shortcuts(true, r#""alt+s" = "switch S""#);
        let enabled = WorkspaceId::numbered().collect::<Vec<_>>();
        let layout = LayoutSnapshot::for_test(vec![(1, 's', false, false)], vec![]);
        let plan = HotkeyPlan::from_layout(&enabled, &shortcuts, &layout).unwrap();
        assert_eq!(action(&plan, "switch workspace S"), None);
        assert!(
            plan.warnings
                .iter()
                .any(|warning| warning.contains("alt+s") && warning.contains("workspace S")),
            "{:?}",
            plan.warnings
        );
    }

    #[test]
    fn modifier_moves_every_built_in_shortcut_and_keeps_shift_for_move() {
        let shortcuts = crate::config::Config::parse(
            "version = 1\n[shortcuts]\nmodifier = \"ctrl+alt\"\n[shortcuts.bindings]\n\"alt+h\" = \"layout\"",
        )
        .unwrap()
        .shortcuts()
        .clone();
        let enabled = WorkspaceId::numbered().collect::<Vec<_>>();
        let plan = HotkeyPlan::from_layout(&enabled, &shortcuts, &layout()).unwrap();
        let base = CONTROL_KEY | OPTION_KEY;
        for (description, modifiers) in [
            ("focus left", base),
            ("move window left", base | SHIFT_KEY),
            ("switch workspace 3", base),
            ("move window to workspace 3", base | SHIFT_KEY),
            ("toggle floating window", base | SHIFT_KEY),
            ("cycle workspace windows", base),
        ] {
            let found = plan
                .registrations
                .iter()
                .find(|registration| registration.description == description)
                .unwrap_or_else(|| panic!("missing {description}"));
            assert_eq!(found.modifiers, modifiers, "{description}");
        }
        // A binding is an exact chord: plain Option+H flips the split beside Ctrl+Option+H.
        assert!(
            registration(&plan, &WorkerAction::ToggleLayout)
                .iter()
                .any(|registration| registration.key_code == 60
                    && registration.modifiers == OPTION_KEY)
        );
        assert!(plan.warnings.is_empty(), "{:?}", plan.warnings);
    }

    #[test]
    fn keeping_fixed_scratchpad_unbind_under_new_base_modifier_keeps_toggle_disabled() {
        let layout = LayoutSnapshot::for_test(vec![(42, '\\', false, false)], vec![]);
        let config = crate::config::Config::parse(
            "version = 1\n[labs]\nscratchpad = true\n[shortcuts]\nmodifier = 'ctrl+alt'\n[shortcuts.bindings]\n'alt+\\' = 'none'\n",
        )
        .unwrap();
        let enabled = [WorkspaceId::new(1).unwrap()];
        let plan = HotkeyPlan::from_layout(&enabled, config.shortcuts(), &layout).unwrap();
        assert_eq!(action(&plan, "show or hide the scratchpad"), None);
        assert_eq!(
            action(&plan, "move window to or from the scratchpad"),
            Some(&WorkerAction::Scratchpad(ScratchpadCommand::Mark))
        );
        let switch_1 = plan
            .registrations
            .iter()
            .find(|registration| registration.description == "switch workspace 1")
            .unwrap();
        assert_eq!(switch_1.modifiers, CONTROL_KEY | OPTION_KEY);
    }

    #[test]
    fn default_switch_to_space_uses_control_and_leaves_option_number_free() {
        let enabled = WorkspaceId::numbered().collect::<Vec<_>>();
        let plan = HotkeyPlan::from_layout(&enabled, &Shortcuts::default(), &layout()).unwrap();
        let switch_1 = plan
            .registrations
            .iter()
            .find(|registration| registration.description == "switch workspace 1")
            .unwrap();
        assert_eq!(switch_1.key_code, 0x12);
        assert_eq!(switch_1.modifiers, CONTROL_KEY);
        assert!(!plan.registrations.iter().any(|registration| {
            registration.description.starts_with("switch workspace")
                && registration
                    .description
                    .split_whitespace()
                    .next_back()
                    .is_some_and(|word| word.parse::<u8>().is_ok())
                && registration.modifiers & OPTION_KEY != 0
        }));
        let move_1 = plan
            .registrations
            .iter()
            .find(|registration| registration.description == "move window to workspace 1")
            .unwrap();
        assert_eq!(move_1.modifiers, OPTION_KEY | SHIFT_KEY);
    }

    #[test]
    fn an_enabled_mission_control_shortcut_is_not_registered() {
        let enabled = WorkspaceId::numbered().collect::<Vec<_>>();
        let reserved = [shortcuts::SystemChord {
            key_code: 0x12,
            modifiers: shortcuts::Modifiers::CONTROL,
            name: "Switch to Desktop 1".into(),
        }];
        let plan = HotkeyPlan::from_layout_with_system(
            &enabled,
            &Shortcuts::default(),
            &layout(),
            &reserved,
        )
        .unwrap();
        assert_eq!(action(&plan, "switch workspace 1"), None);
        assert!(
            plan.warnings.iter().any(|warning| {
                warning.contains("switch workspace 1") && warning.contains("Switch to Desktop 1")
            }),
            "{:?}",
            plan.warnings
        );
        assert!(action(&plan, "switch workspace 2").is_some());
    }
}
