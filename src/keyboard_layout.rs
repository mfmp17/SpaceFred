use std::{ffi::c_void, fmt, slice};

const VIRTUAL_KEY_CODES: std::ops::Range<u16> = 0..128;

// Public Carbon Events.h keypad identities; intervening codes include non-keypad keys.
const KEYPAD_KEY_CODES: [u16; 19] = [
    0x41, // kVK_ANSI_KeypadDecimal
    0x43, // kVK_ANSI_KeypadMultiply
    0x45, // kVK_ANSI_KeypadPlus
    0x47, // kVK_ANSI_KeypadClear
    0x4b, // kVK_ANSI_KeypadDivide
    0x4c, // kVK_ANSI_KeypadEnter
    0x4e, // kVK_ANSI_KeypadMinus
    0x51, // kVK_ANSI_KeypadEquals
    0x52, // kVK_ANSI_Keypad0
    0x53, // kVK_ANSI_Keypad1
    0x54, // kVK_ANSI_Keypad2
    0x55, // kVK_ANSI_Keypad3
    0x56, // kVK_ANSI_Keypad4
    0x57, // kVK_ANSI_Keypad5
    0x58, // kVK_ANSI_Keypad6
    0x59, // kVK_ANSI_Keypad7
    0x5b, // kVK_ANSI_Keypad8
    0x5c, // kVK_ANSI_Keypad9
    0x5f, // kVK_JIS_KeypadComma
];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum SemanticKey {
    Tab,
    Space,
    Workspace(u8),
    /// A key that types no character (an arrow, Return, an F-key), by virtual key code.
    KeyCode(u16),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum LogicalKey {
    Character(char),
    Semantic(SemanticKey),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct LayoutFingerprint {
    selected_source: String,
    layout_source: String,
    ascii_source: String,
    primary_layout_hash: u64,
    ascii_layout_hash: u64,
}

#[derive(Clone, Debug)]
pub(crate) struct LayoutSnapshot {
    fingerprint: LayoutFingerprint,
    primary: Vec<KeyOutput>,
    ascii_fallback: Vec<KeyOutput>,
    /// Characters produced while Option is held, for shortcut collision warnings.
    option_glyphs: Vec<crate::shortcuts::LayoutGlyph>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct KeyOutput {
    key_code: u16,
    character: char,
    shift: bool,
    dead: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ResolvedKey {
    pub(crate) key_code: u32,
    pub(crate) shift: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum ResolveError {
    InvalidSemanticKey(SemanticKey),
    MissingCharacter(char),
    System(String),
}

impl fmt::Display for ResolveError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidSemanticKey(key) => write!(formatter, "invalid semantic key {key:?}"),
            Self::MissingCharacter(character) => {
                write!(
                    formatter,
                    "current keyboard layout does not produce {character:?}"
                )
            }
            Self::System(message) => formatter.write_str(message),
        }
    }
}

impl std::error::Error for ResolveError {}

impl LayoutSnapshot {
    pub(crate) fn current() -> Result<Self, ResolveError> {
        macos::snapshot()
    }

    pub(crate) fn current_fingerprint() -> Result<LayoutFingerprint, ResolveError> {
        macos::fingerprint()
    }

    pub(crate) fn fingerprint(&self) -> &LayoutFingerprint {
        &self.fingerprint
    }

    /// The single best key for `key`. Shortcut planning walks `candidates` instead so a
    /// taken chord can fall through to another key; tests use this to pin one resolution.
    #[cfg(test)]
    pub(crate) fn resolve(&self, key: LogicalKey) -> Result<ResolvedKey, ResolveError> {
        match key {
            LogicalKey::Semantic(key) => semantic_key_code(key).map(|key_code| ResolvedKey {
                key_code,
                shift: false,
            }),
            LogicalKey::Character(character) => self
                .character_candidates(character)
                .into_iter()
                .next()
                .ok_or(ResolveError::MissingCharacter(character)),
        }
    }

    /// Every physical key that could serve `key`, most preferred first. Shortcut planning
    /// walks this list so one taken chord costs only an alternative key, not the shortcut.
    pub(crate) fn candidates(&self, key: LogicalKey) -> Result<Vec<ResolvedKey>, ResolveError> {
        match key {
            LogicalKey::Semantic(key) => semantic_key_code(key).map(|key_code| {
                vec![ResolvedKey {
                    key_code,
                    shift: false,
                }]
            }),
            LogicalKey::Character(character) => {
                let candidates = self.character_candidates(character);
                if candidates.is_empty() {
                    Err(ResolveError::MissingCharacter(character))
                } else {
                    Ok(candidates)
                }
            }
        }
    }

    /// What the key serving `key` types with Shift held, on the primary layout: `+` for `=`
    /// on a US layout. A shifted binding written with that character (`alt++`) lands on the
    /// same physical chord as `alt+shift+=`, so Settings must treat the two as one.
    pub(crate) fn shifted(&self, key: LogicalKey) -> Option<char> {
        let key_code = self
            .candidates(key)
            .ok()?
            .into_iter()
            .find(|candidate| !candidate.shift)?
            .key_code;
        let key_code = u16::try_from(key_code).ok()?;
        self.primary
            .iter()
            .find(|output| output.key_code == key_code && output.shift && !output.dead)
            .map(|output| output.character)
    }

    /// A character's keys in preference order: a live key on the primary layout, a live key
    /// on the ASCII-capable fallback, a dead key, then the physical ANSI punctuation key.
    ///
    /// A dead key is usable because `RegisterEventHotKey` matches a key code and modifiers
    /// rather than composed text, and the physical fallback covers layouts (Italian – Pro,
    /// for one) that cannot type the character at all: the shortcut still fires from the
    /// standard ANSI position, whatever that key types in the active layout.
    fn character_candidates(&self, character: char) -> Vec<ResolvedKey> {
        let mut candidates = Vec::with_capacity(4);
        let mut push = |candidate: ResolvedKey| {
            if !candidates.contains(&candidate) {
                candidates.push(candidate);
            }
        };
        let resolved = |output: KeyOutput| ResolvedKey {
            key_code: u32::from(output.key_code),
            shift: output.shift,
        };
        if let Ok(output) = reverse_lookup(&self.primary, character) {
            push(resolved(output));
        }
        if character.is_ascii()
            && let Ok(output) = reverse_lookup(&self.ascii_fallback, character)
        {
            push(resolved(output));
        }
        if let Ok(output) = reverse_lookup_dead(&self.primary, character) {
            push(resolved(output));
        }
        if character.is_ascii()
            && let Ok(output) = reverse_lookup_dead(&self.ascii_fallback, character)
        {
            push(resolved(output));
        }
        if let Some(key_code) = physical_punctuation_key_code(character) {
            push(ResolvedKey {
                key_code,
                shift: false,
            });
        }
        candidates
    }

    #[cfg(test)]
    pub(crate) fn for_test(
        primary: Vec<(u16, char, bool, bool)>,
        fallback: Vec<(u16, char, bool, bool)>,
    ) -> Self {
        let convert = |outputs: Vec<(u16, char, bool, bool)>| {
            outputs
                .into_iter()
                .map(|(key_code, character, shift, dead)| KeyOutput {
                    key_code,
                    character,
                    shift,
                    dead,
                })
                .collect()
        };
        Self {
            fingerprint: LayoutFingerprint {
                selected_source: "test".into(),
                layout_source: "test-layout".into(),
                ascii_source: "test-ascii".into(),
                primary_layout_hash: 1,
                ascii_layout_hash: 2,
            },
            primary: convert(primary),
            ascii_fallback: convert(fallback),
            option_glyphs: Vec::new(),
        }
    }

    /// Plain and Option-modified characters, so a shortcut can warn when it types one.
    pub(crate) fn layout_glyphs(&self) -> Vec<crate::shortcuts::LayoutGlyph> {
        let mut glyphs = Vec::with_capacity(self.primary.len() + self.option_glyphs.len());
        for output in &self.primary {
            glyphs.push(crate::shortcuts::LayoutGlyph {
                key_code: output.key_code,
                option: false,
                shift: output.shift,
                character: output.character,
                dead: output.dead,
            });
        }
        glyphs.extend(self.option_glyphs.iter().copied());
        glyphs
    }
}

/// Option-modified characters for the active layout, reused for a couple of seconds so
/// Settings can warn without translating the whole keyboard on every frame.
pub(crate) fn cached_layout_glyphs() -> Vec<crate::shortcuts::LayoutGlyph> {
    use std::sync::Mutex;
    use std::time::{Duration, Instant};

    static CACHE: Mutex<Option<(Instant, Vec<crate::shortcuts::LayoutGlyph>)>> = Mutex::new(None);
    if let Ok(guard) = CACHE.lock()
        && let Some((at, glyphs)) = guard.as_ref()
        && at.elapsed() < Duration::from_secs(2)
    {
        return glyphs.clone();
    }
    let glyphs = LayoutSnapshot::current()
        .map(|snapshot| snapshot.layout_glyphs())
        .unwrap_or_default();
    if let Ok(mut guard) = CACHE.lock() {
        *guard = Some((Instant::now(), glyphs.clone()));
    }
    glyphs
}

fn semantic_key_code(key: SemanticKey) -> Result<u32, ResolveError> {
    const WORKSPACE_NUMBER_ROW: [u32; 10] =
        [0x12, 0x13, 0x14, 0x15, 0x17, 0x16, 0x1a, 0x1c, 0x19, 0x1d];

    match key {
        SemanticKey::Tab => Ok(0x30),
        SemanticKey::Space => Ok(0x31),
        // Workspace shortcuts intentionally identify physical number-row keys. Their
        // Option+Shift variants therefore remain usable on layouts where digits need Shift.
        SemanticKey::Workspace(number @ 1..=10) => Ok(WORKSPACE_NUMBER_ROW[number as usize - 1]),
        SemanticKey::Workspace(_) => Err(ResolveError::InvalidSemanticKey(key)),
        SemanticKey::KeyCode(key_code) => Ok(u32::from(key_code)),
    }
}

/// Physical ANSI positions of the punctuation shortcuts, used only when the active layout
/// cannot type the character with any key at all. Global hotkeys match the key code, so an
/// Italian – Pro keyboard still fires Option plus the key above Tab even though that key
/// types a backslash there.
const PHYSICAL_PUNCTUATION_KEY_CODES: [(char, u16); 11] = [
    ('`', 0x32),  // kVK_ANSI_Grave
    ('/', 0x2c),  // kVK_ANSI_Slash
    ('-', 0x1b),  // kVK_ANSI_Minus
    ('=', 0x18),  // kVK_ANSI_Equal
    ('[', 0x21),  // kVK_ANSI_LeftBracket
    (']', 0x1e),  // kVK_ANSI_RightBracket
    (';', 0x29),  // kVK_ANSI_Semicolon
    ('\'', 0x27), // kVK_ANSI_Quote
    (',', 0x2b),  // kVK_ANSI_Comma
    ('.', 0x2f),  // kVK_ANSI_Period
    ('\\', 0x2a), // kVK_ANSI_Backslash
];

fn physical_punctuation_key_code(character: char) -> Option<u32> {
    PHYSICAL_PUNCTUATION_KEY_CODES
        .iter()
        .find_map(|(punctuation, key_code)| {
            (*punctuation == character).then_some(u32::from(*key_code))
        })
}

fn reverse_lookup(outputs: &[KeyOutput], character: char) -> Result<KeyOutput, ResolveError> {
    lookup(outputs, character, false)
}

/// A dead key still names a real physical key, and Carbon hotkeys match the key code, so a
/// layout whose only grave (German QWERTZ) is a dead key can still offer the shortcut.
fn reverse_lookup_dead(outputs: &[KeyOutput], character: char) -> Result<KeyOutput, ResolveError> {
    lookup(outputs, character, true)
}

fn lookup(outputs: &[KeyOutput], character: char, dead: bool) -> Result<KeyOutput, ResolveError> {
    outputs
        .iter()
        .filter(|output| output.dead == dead && output.character == character)
        .filter(|output| !character.is_alphabetic() || !output.shift)
        // Keep character shortcuts reachable on keyboards without a numeric keypad.
        .min_by_key(|output| (KEYPAD_KEY_CODES.contains(&output.key_code), output.shift))
        .cloned()
        .ok_or(ResolveError::MissingCharacter(character))
}

#[cfg(target_os = "macos")]
mod macos {
    use objc2_core_foundation::{CFString, CFType};

    use super::*;

    type InputSource = *const c_void;
    const KEY_ACTION_DOWN: u16 = 0;
    const NO_DEAD_KEYS_MASK: u32 = 1;

    #[link(name = "Carbon", kind = "framework")]
    unsafe extern "C" {
        static kTISPropertyInputSourceID: *const c_void;
        static kTISPropertyUnicodeKeyLayoutData: *const c_void;

        fn TISCopyCurrentKeyboardInputSource() -> InputSource;
        fn TISCopyCurrentKeyboardLayoutInputSource() -> InputSource;
        fn TISCopyCurrentASCIICapableKeyboardLayoutInputSource() -> InputSource;
        fn TISGetInputSourceProperty(source: InputSource, property: *const c_void)
        -> *const c_void;
        fn UCKeyTranslate(
            layout: *const c_void,
            virtual_key_code: u16,
            key_action: u16,
            modifier_key_state: u32,
            keyboard_type: u32,
            options: u32,
            dead_key_state: *mut u32,
            max_string_length: u32,
            actual_string_length: *mut u32,
            unicode_string: *mut u16,
        ) -> i32;
        fn LMGetKbdType() -> u8;
    }

    #[link(name = "CoreFoundation", kind = "framework")]
    unsafe extern "C" {
        fn CFRelease(value: *const c_void);
        fn CFDataGetBytePtr(data: *const c_void) -> *const u8;
        fn CFDataGetLength(data: *const c_void) -> isize;
    }

    struct OwnedInputSource(InputSource);

    impl OwnedInputSource {
        fn copy(
            copy: unsafe extern "C" fn() -> InputSource,
            description: &str,
        ) -> Result<Self, ResolveError> {
            // SAFETY: TIS Copy functions follow the Core Foundation create rule. The non-null
            // reference remains alive until `Drop` releases it.
            let source = unsafe { copy() };
            if source.is_null() {
                Err(ResolveError::System(format!(
                    "macOS returned no {description}"
                )))
            } else {
                Ok(Self(source))
            }
        }

        fn id(&self) -> Result<String, ResolveError> {
            // SAFETY: `self.0` is a retained TIS input source and `kTISPropertyInputSourceID` is
            // a valid Carbon property key.
            let value = unsafe { TISGetInputSourceProperty(self.0, kTISPropertyInputSourceID) };
            cf_string(value).ok_or_else(|| {
                ResolveError::System("keyboard input source has no valid identifier".into())
            })
        }

        fn layout_data(&self) -> Result<Option<(*const u8, usize)>, ResolveError> {
            // SAFETY: TIS owns the property value, and its CFData storage stays valid while this
            // retained input source is alive.
            let data =
                unsafe { TISGetInputSourceProperty(self.0, kTISPropertyUnicodeKeyLayoutData) };
            if data.is_null() {
                return Ok(None);
            }
            // SAFETY: `data` is a non-null `CFDataRef` returned for the Unicode key layout key.
            let length = unsafe { CFDataGetLength(data) };
            // SAFETY: `data` is a non-null `CFDataRef` owned by `self.0`.
            let layout = unsafe { CFDataGetBytePtr(data) };
            if length <= 0 || layout.is_null() {
                return Err(ResolveError::System(
                    "keyboard input source contains empty Unicode layout data".into(),
                ));
            }

            Ok(Some((
                layout,
                usize::try_from(length).map_err(|_| {
                    ResolveError::System("keyboard layout data is too large".into())
                })?,
            )))
        }

        fn outputs(&self) -> Result<Option<LayoutTables>, ResolveError> {
            let Some((layout, _)) = self.layout_data()? else {
                return Ok(None);
            };
            let mut outputs = Vec::with_capacity(256);
            let mut option_glyphs = Vec::with_capacity(256);
            for key_code in VIRTUAL_KEY_CODES {
                for shift in [false, true] {
                    if let Some(output) = translate(layout.cast(), key_code, shift)? {
                        outputs.push(output);
                    }
                }
                // Option is modifier state bit 3 (Carbon `optionKey >> 8`).
                for (shift, state) in [(false, 8), (true, 10)] {
                    if let Some(glyph) = translate_option(layout.cast(), key_code, state, shift)? {
                        option_glyphs.push(glyph);
                    }
                }
            }
            Ok(Some(LayoutTables {
                outputs,
                option_glyphs,
            }))
        }

        fn layout_hash(&self) -> Result<u64, ResolveError> {
            let Some((bytes, length)) = self.layout_data()? else {
                return Ok(0);
            };
            // SAFETY: `layout_data` validated the pointer and length, and the retained source owns
            // the backing CFData for this borrow.
            let bytes = unsafe { slice::from_raw_parts(bytes, length) };
            Ok(bytes.iter().fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
                (hash ^ u64::from(*byte)).wrapping_mul(0x0000_0100_0000_01b3)
            }))
        }
    }

    impl Drop for OwnedInputSource {
        fn drop(&mut self) {
            // SAFETY: `self.0` was non-null and retained by a TIS Copy function.
            unsafe { CFRelease(self.0) };
        }
    }

    struct Sources {
        selected: OwnedInputSource,
        layout: OwnedInputSource,
        ascii: OwnedInputSource,
    }

    impl Sources {
        fn current() -> Result<Self, ResolveError> {
            Ok(Self {
                selected: OwnedInputSource::copy(
                    TISCopyCurrentKeyboardInputSource,
                    "selected keyboard input source",
                )?,
                layout: OwnedInputSource::copy(
                    TISCopyCurrentKeyboardLayoutInputSource,
                    "keyboard layout input source",
                )?,
                ascii: OwnedInputSource::copy(
                    TISCopyCurrentASCIICapableKeyboardLayoutInputSource,
                    "ASCII-capable keyboard layout input source",
                )?,
            })
        }

        fn fingerprint(&self) -> Result<LayoutFingerprint, ResolveError> {
            Ok(LayoutFingerprint {
                selected_source: self.selected.id()?,
                layout_source: self.layout.id()?,
                ascii_source: self.ascii.id()?,
                primary_layout_hash: match self.selected.layout_hash()? {
                    0 => self.layout.layout_hash()?,
                    hash => hash,
                },
                ascii_layout_hash: self.ascii.layout_hash()?,
            })
        }
    }

    pub(super) fn fingerprint() -> Result<LayoutFingerprint, ResolveError> {
        Sources::current()?.fingerprint()
    }

    pub(super) fn snapshot() -> Result<LayoutSnapshot, ResolveError> {
        let sources = Sources::current()?;
        let fingerprint = sources.fingerprint()?;
        // IMEs commonly expose no UCKeyboardLayout. In that case, macOS's current
        // keyboard-layout source is the authoritative layout beneath the IME.
        let primary = match sources.selected.outputs()? {
            Some(tables) => tables,
            None => sources.layout.outputs()?.ok_or_else(|| {
                ResolveError::System("current input sources expose no Unicode layout data".into())
            })?,
        };
        let ascii_fallback = sources.ascii.outputs()?.ok_or_else(|| {
            ResolveError::System("ASCII-capable input source exposes no Unicode layout data".into())
        })?;
        Ok(LayoutSnapshot {
            fingerprint,
            primary: primary.outputs,
            ascii_fallback: ascii_fallback.outputs,
            option_glyphs: primary.option_glyphs,
        })
    }

    struct LayoutTables {
        outputs: Vec<KeyOutput>,
        option_glyphs: Vec<crate::shortcuts::LayoutGlyph>,
    }

    fn translate(
        layout: *const c_void,
        key_code: u16,
        shift: bool,
    ) -> Result<Option<KeyOutput>, ResolveError> {
        let modifier_state = if shift { 2 } else { 0 };
        let no_dead_key = translate_raw(layout, key_code, modifier_state, NO_DEAD_KEYS_MASK)?;
        let Some(character) = no_dead_key.text else {
            return Ok(None);
        };
        let normal = translate_raw(layout, key_code, modifier_state, 0)?;
        Ok(Some(KeyOutput {
            key_code,
            character,
            shift,
            dead: normal.dead_key_state != 0,
        }))
    }

    fn translate_option(
        layout: *const c_void,
        key_code: u16,
        modifier_state: u32,
        shift: bool,
    ) -> Result<Option<crate::shortcuts::LayoutGlyph>, ResolveError> {
        let normal = translate_raw(layout, key_code, modifier_state, 0)?;
        let no_dead_key = translate_raw(layout, key_code, modifier_state, NO_DEAD_KEYS_MASK)?;
        let Some(character) = no_dead_key.text.or(normal.text) else {
            return Ok(None);
        };
        if character.is_control() {
            return Ok(None);
        }
        Ok(Some(crate::shortcuts::LayoutGlyph {
            key_code,
            option: true,
            shift,
            character,
            dead: normal.dead_key_state != 0,
        }))
    }

    struct Translation {
        text: Option<char>,
        dead_key_state: u32,
    }

    fn translate_raw(
        layout: *const c_void,
        key_code: u16,
        modifier_state: u32,
        options: u32,
    ) -> Result<Translation, ResolveError> {
        let mut dead_key_state = 0;
        let mut actual_length = 0;
        let mut utf16 = [0_u16; 16];
        // SAFETY: `layout` points to retained `UCKeyboardLayout` bytes and all out-pointers
        // reference stack storage valid for the call.
        let status = unsafe {
            UCKeyTranslate(
                layout,
                key_code,
                KEY_ACTION_DOWN,
                modifier_state,
                u32::from(LMGetKbdType()),
                options,
                &raw mut dead_key_state,
                utf16.len() as u32,
                &raw mut actual_length,
                utf16.as_mut_ptr(),
            )
        };
        if status != 0 {
            return Err(ResolveError::System(format!(
                "UCKeyTranslate failed for virtual key {key_code:#04x} ({status})"
            )));
        }
        let length = usize::try_from(actual_length)
            .ok()
            .filter(|length| *length <= utf16.len())
            .ok_or_else(|| ResolveError::System("UCKeyTranslate returned invalid length".into()))?;
        if length == 0 {
            return Ok(Translation {
                text: None,
                dead_key_state,
            });
        }
        let text = String::from_utf16(&utf16[..length]).map_err(|_| {
            ResolveError::System(format!(
                "UCKeyTranslate returned invalid UTF-16 for virtual key {key_code:#04x}"
            ))
        })?;
        Ok(Translation {
            text: one_scalar(&text),
            dead_key_state,
        })
    }

    fn cf_string(value: *const c_void) -> Option<String> {
        // SAFETY: `value` is either null or a Core Foundation property object owned by TIS.
        unsafe { value.cast::<CFType>().as_ref() }?
            .downcast_ref::<CFString>()
            .map(CFString::to_string)
    }
}

fn one_scalar(text: &str) -> Option<char> {
    let mut characters = text.chars();
    let character = characters.next()?;
    characters.next().is_none().then_some(character)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn output(key_code: u16, character: char) -> KeyOutput {
        KeyOutput {
            key_code,
            character,
            shift: false,
            dead: false,
        }
    }

    #[test]
    fn shifted_names_what_the_same_key_types_with_shift() {
        // US: `=` and `+` share 0x18; `1` and `!` share the number row's 0x12.
        let snapshot = LayoutSnapshot::for_test(
            vec![
                (0x18, '=', false, false),
                (0x18, '+', true, false),
                (0x45, '+', false, false),
                (0x12, '1', false, false),
                (0x12, '!', true, false),
                (0x04, 'h', false, false),
                (0x04, 'H', true, false),
            ],
            vec![],
        );
        assert_eq!(snapshot.shifted(LogicalKey::Character('=')), Some('+'));
        assert_eq!(
            snapshot.shifted(LogicalKey::Semantic(SemanticKey::Workspace(1))),
            Some('!')
        );
        assert_eq!(snapshot.shifted(LogicalKey::Character('h')), Some('H'));
        // `+` resolves to Shift+`=` itself, so it has no shifted twin.
        assert_eq!(snapshot.shifted(LogicalKey::Character('+')), None);
        assert_eq!(snapshot.shifted(LogicalKey::Character('x')), None);
    }

    #[test]
    fn reverse_lookup_resolves_exact_unmodified_character() {
        let outputs = [output(4, 'h'), output(38, 'j')];
        assert_eq!(
            reverse_lookup(&outputs, 'j').map(|key| key.key_code),
            Ok(38)
        );
        assert_eq!(
            reverse_lookup(&outputs, 'H'),
            Err(ResolveError::MissingCharacter('H'))
        );
    }

    #[test]
    fn reverse_lookup_handles_unicode_strings_safely() {
        let outputs = [output(12, 'λ'), output(13, '😀')];
        assert_eq!(
            reverse_lookup(&outputs, 'λ').map(|key| key.key_code),
            Ok(12)
        );
        assert_eq!(
            reverse_lookup(&outputs, '😀').map(|key| key.key_code),
            Ok(13)
        );
    }

    #[test]
    fn translations_must_be_exactly_one_unicode_scalar() {
        assert_eq!(one_scalar("😀"), Some('😀'));
        assert_eq!(one_scalar(""), None);
        assert_eq!(one_scalar("ab"), None);
        assert_eq!(one_scalar("e\u{301}"), None);
    }

    #[test]
    fn semantic_keys_are_independent_of_character_outputs() {
        let snapshot = LayoutSnapshot {
            fingerprint: fingerprint("test"),
            primary: vec![output(99, '1'), output(98, '\t')],
            ascii_fallback: vec![],
            option_glyphs: vec![],
        };
        assert_eq!(
            snapshot
                .resolve(LogicalKey::Semantic(SemanticKey::Workspace(1)))
                .unwrap()
                .key_code,
            0x12
        );
        assert_eq!(
            snapshot
                .resolve(LogicalKey::Semantic(SemanticKey::Tab))
                .unwrap()
                .key_code,
            0x30
        );
        assert_eq!(
            snapshot
                .resolve(LogicalKey::Semantic(SemanticKey::Space))
                .unwrap()
                .key_code,
            0x31,
            "Space stays physical kVK_Space (0x31) in every layout"
        );
        assert_eq!(
            snapshot
                .resolve(LogicalKey::Character('1'))
                .unwrap()
                .key_code,
            99
        );
    }

    #[test]
    fn ascii_fallback_only_applies_when_primary_character_is_missing() {
        let snapshot = LayoutSnapshot {
            fingerprint: fingerprint("test"),
            primary: vec![output(1, 'ж'), output(2, '-')],
            ascii_fallback: vec![output(3, 'h'), output(4, '-')],
            option_glyphs: vec![],
        };
        assert_eq!(
            snapshot
                .resolve(LogicalKey::Character('h'))
                .unwrap()
                .key_code,
            3
        );
        assert_eq!(
            snapshot
                .resolve(LogicalKey::Character('-'))
                .unwrap()
                .key_code,
            2
        );
        assert_eq!(
            snapshot.resolve(LogicalKey::Character('λ')),
            Err(ResolveError::MissingCharacter('λ'))
        );
    }

    #[test]
    fn punctuation_can_require_shift_but_letters_cannot() {
        let mut shifted_slash = output(9, '/');
        shifted_slash.shift = true;
        let mut shifted_h = output(10, 'h');
        shifted_h.shift = true;
        assert!(reverse_lookup(&[shifted_slash], '/').unwrap().shift);
        assert_eq!(
            reverse_lookup(&[shifted_h], 'h'),
            Err(ResolveError::MissingCharacter('h'))
        );
    }

    #[test]
    fn dead_key_candidates_are_rejected() {
        let mut dead = output(9, '=');
        dead.dead = true;
        assert_eq!(
            reverse_lookup(&[dead], '='),
            Err(ResolveError::MissingCharacter('='))
        );
    }

    #[test]
    fn candidates_prefer_live_keys_then_dead_then_physical_position() {
        let layout = LayoutSnapshot::for_test(
            vec![(0x0a, '°', true, false), (0x0a, '`', false, true)],
            vec![(0x32, '`', false, false)],
        );
        assert_eq!(
            layout.candidates(LogicalKey::Character('`')),
            Ok(vec![
                ResolvedKey {
                    key_code: 0x32,
                    shift: false
                },
                ResolvedKey {
                    key_code: 0x0a,
                    shift: false
                },
            ]),
            "the live ASCII key wins, then the dead key; the physical key is deduplicated"
        );
        assert_eq!(
            layout.resolve(LogicalKey::Character('`')),
            Ok(ResolvedKey {
                key_code: 0x32,
                shift: false
            })
        );
    }

    #[test]
    fn dead_key_alone_resolves_a_character() {
        // German QWERTZ has no live grave at all; its only grave is a dead key.
        let layout = LayoutSnapshot::for_test(vec![(0x0a, '`', false, true)], vec![]);
        assert_eq!(
            layout.resolve(LogicalKey::Character('`')),
            Ok(ResolvedKey {
                key_code: 0x0a,
                shift: false
            })
        );
    }

    #[test]
    fn missing_punctuation_falls_back_to_its_physical_ansi_key() {
        // Italian – Pro cannot type a plain grave, and a layout may hide `/` behind Shift.
        let layout = LayoutSnapshot::for_test(vec![], vec![]);
        for (character, key_code) in [('`', 0x32), ('/', 0x2c), ('-', 0x1b), ('=', 0x18)] {
            assert_eq!(
                layout.resolve(LogicalKey::Character(character)),
                Ok(ResolvedKey {
                    key_code,
                    shift: false
                }),
                "{character:?}"
            );
        }
    }

    #[test]
    fn physical_fallback_never_invents_letter_keys() {
        let layout = LayoutSnapshot::for_test(vec![], vec![]);
        assert_eq!(
            layout.resolve(LogicalKey::Character('h')),
            Err(ResolveError::MissingCharacter('h'))
        );
        assert_eq!(
            layout.candidates(LogicalKey::Character('h')),
            Err(ResolveError::MissingCharacter('h'))
        );
    }

    #[test]
    fn main_keyboard_slash_precedes_keypad_even_when_shifted() {
        // Russian-PC uses Shift+Backslash; Russian uses the unshifted Slash key.
        for (key_code, shift) in [(0x2a, true), (0x2c, false)] {
            let layout = LayoutSnapshot::for_test(
                vec![
                    (key_code, '/', shift, false),
                    (0x4b, '/', false, false),
                    (0x4b, '/', true, false),
                    (0x4d, '/', true, false),
                ],
                vec![(0x2c, '/', false, false)],
            );
            assert_eq!(
                layout.resolve(LogicalKey::Character('/')),
                Ok(ResolvedKey {
                    key_code: u32::from(key_code),
                    shift
                })
            );
        }
    }

    #[test]
    fn keypad_fallback_keeps_primary_precedence_and_rejects_dead_keys() {
        for primary in [
            vec![(0x4b, '/', true, false), (0x4b, '/', false, false)],
            vec![(0x2a, '/', true, true), (0x4b, '/', false, false)],
        ] {
            let layout = LayoutSnapshot::for_test(primary, vec![(0x2c, '/', false, false)]);
            assert_eq!(
                layout.resolve(LogicalKey::Character('/')),
                Ok(ResolvedKey {
                    key_code: 0x4b,
                    shift: false
                })
            );
        }
    }

    #[test]
    fn non_keypad_candidates_still_prefer_unshifted_output() {
        let layout = LayoutSnapshot::for_test(
            vec![(0x2a, '/', true, false), (0x2c, '/', false, false)],
            vec![],
        );
        assert_eq!(
            layout.resolve(LogicalKey::Character('/')),
            Ok(ResolvedKey {
                key_code: 0x2c,
                shift: false
            })
        );
    }

    #[test]
    fn keypad_codes_exclude_function_media_and_jis_main_keys() {
        // F17, volume up/down, mute, F18/F19/F20, JIS Yen and Underscore.
        for key_code in [0x40, 0x48, 0x49, 0x4a, 0x4f, 0x50, 0x5a, 0x5d, 0x5e] {
            assert!(!KEYPAD_KEY_CODES.contains(&key_code));
        }
        assert!(KEYPAD_KEY_CODES.contains(&0x5f)); // JIS KeypadComma
    }

    #[test]
    fn fingerprint_changes_with_any_input_source_identity() {
        assert_eq!(fingerprint("a"), fingerprint("a"));
        assert_ne!(fingerprint("a"), fingerprint("b"));
        let mut changed_bytes = fingerprint("a");
        changed_bytes.primary_layout_hash += 1;
        assert_ne!(fingerprint("a"), changed_bytes);
    }

    fn fingerprint(id: &str) -> LayoutFingerprint {
        LayoutFingerprint {
            selected_source: id.into(),
            layout_source: format!("{id}-layout"),
            ascii_source: format!("{id}-ascii"),
            primary_layout_hash: 1,
            ascii_layout_hash: 2,
        }
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn current_dvorak_resolves_logical_hotkey_characters() {
        let snapshot = LayoutSnapshot::current().unwrap();
        let mapping = ['h', 'j', 'k', 'l', '/', 'f', '-', '=']
            .into_iter()
            .map(|character| {
                (
                    character,
                    snapshot.resolve(LogicalKey::Character(character)).unwrap(),
                )
            })
            .collect::<std::collections::BTreeMap<_, _>>();
        eprintln!(
            "current keyboard source {:?}: {mapping:?}",
            snapshot.fingerprint
        );

        if snapshot
            .fingerprint
            .selected_source
            .to_ascii_lowercase()
            .contains("dvorak")
            || snapshot
                .fingerprint
                .layout_source
                .to_ascii_lowercase()
                .contains("dvorak")
        {
            assert_eq!(mapping[&'-'].key_code, 0x27);
            assert_eq!(mapping[&'='].key_code, 0x1e);
        }
    }
}
