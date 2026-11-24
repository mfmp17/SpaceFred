//! Enabled Mission Control shortcuts from `com.apple.symbolichotkeys`.
//!
//! Spaceman never writes this domain. When a shortcut here is turned on, the hotkey planner
//! leaves that chord for macOS instead of registering over it.

use std::time::{Duration, Instant};

use objc2_core_foundation::{
    CFArray, CFBoolean, CFDictionary, CFNumber, CFRetained, CFString, CFType, ConcreteType,
};

use crate::shortcuts::{self, SymbolicHotkey, SystemChord};

use super::ax::cf_array_items;

pub(crate) fn mission_control_shortcuts() -> Vec<SystemChord> {
    use std::sync::Mutex;

    static CACHE: Mutex<Option<(Instant, Vec<SystemChord>)>> = Mutex::new(None);
    if let Ok(guard) = CACHE.lock()
        && let Some((at, chords)) = guard.as_ref()
        && at.elapsed() < Duration::from_secs(2)
    {
        return chords.clone();
    }
    let chords = read();
    if let Ok(mut guard) = CACHE.lock() {
        *guard = Some((Instant::now(), chords.clone()));
    }
    chords
}

fn read() -> Vec<SystemChord> {
    let Some(table) = preferences() else {
        return Vec::new();
    };
    let mut hotkeys = Vec::new();
    for id in mission_control_ids() {
        let Some(entry) = dictionary_value::<CFDictionary>(&table, &id.to_string()) else {
            continue;
        };
        if !dictionary_bool(entry, "enabled") {
            continue;
        }
        let Some(value) = dictionary_value::<CFDictionary>(entry, "value") else {
            continue;
        };
        let Some(parameters) = dictionary_value::<CFArray>(value, "parameters") else {
            continue;
        };
        let numbers = cf_array_items::<CFNumber>(parameters)
            .filter_map(CFNumber::as_i64)
            .collect::<Vec<_>>();
        let Some(key_code) = numbers
            .get(1)
            .copied()
            .and_then(|code| u16::try_from(code).ok())
        else {
            continue;
        };
        let Some(flags) = numbers
            .get(2)
            .copied()
            .and_then(|flags| u32::try_from(flags).ok())
        else {
            continue;
        };
        hotkeys.push(SymbolicHotkey {
            id,
            enabled: true,
            key_code,
            modifier_flags: flags,
        });
    }
    shortcuts::mission_control_chords(&hotkeys)
}

fn mission_control_ids() -> impl Iterator<Item = u16> {
    [32, 33, 36, 37, 79, 81].into_iter().chain(118..=133)
}

fn preferences() -> Option<CFRetained<CFDictionary>> {
    let key = CFString::from_static_str("AppleSymbolicHotKeys");
    let application = CFString::from_static_str("com.apple.symbolichotkeys");
    // SAFETY: Both strings are retained for the call. Copy returns +1 or null.
    let raw = unsafe { CFPreferencesCopyAppValue(&*key, &*application) };
    let raw = std::ptr::NonNull::new(raw.cast_mut())?;
    // SAFETY: `raw` is a +1 `CFTypeRef` from `CFPreferencesCopyAppValue`.
    let value = unsafe { CFRetained::<CFType>::from_raw(raw) };
    value.downcast().ok()
}

fn dictionary_value<'a, T: ConcreteType>(dictionary: &'a CFDictionary, key: &str) -> Option<&'a T> {
    let key = CFString::from_str(key);
    // SAFETY: `dictionary` retains its values for `'a`, and `key` is a valid `CFString`.
    let raw = unsafe { dictionary.value((&*key as *const CFString).cast::<std::ffi::c_void>()) };
    // SAFETY: `raw` is null or a Core Foundation object owned by `dictionary`.
    unsafe { raw.cast::<CFType>().as_ref() }?.downcast_ref::<T>()
}

fn dictionary_bool(dictionary: &CFDictionary, key: &str) -> bool {
    if let Some(flag) = dictionary_value::<CFBoolean>(dictionary, key) {
        // SAFETY: `flag` is a live `CFBoolean` retained by `dictionary`. `Boolean` is an
        // unsigned char, so the return is read as a byte rather than a Rust `bool`.
        return unsafe { CFBooleanGetValue(flag) } != 0;
    }
    dictionary_value::<CFNumber>(dictionary, key).is_some_and(|number| number.as_i64() != Some(0))
}

#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    fn CFPreferencesCopyAppValue(
        key: *const CFString,
        application_id: *const CFString,
    ) -> *const CFType;
    fn CFBooleanGetValue(boolean: *const CFBoolean) -> u8;
}
