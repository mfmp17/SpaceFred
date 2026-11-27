//! Built-in window dispositions: the small utility, note and password apps whose windows float
//! at their native size without any configuration.
//!
//! The table is deliberately short, static and keyed only by bundle identifier, so every default
//! is predictable and visible in one place. Size guesses are avoided on purpose: a small terminal
//! window must still tile, and a Sticky that was once stretched must still float.
//!
//! These defaults apply only after the user's own `[[rules]]`: a matching rule wins wherever it
//! decides, so `action = "tile"` tiles a built-in float again, `action = "pin"` pins one
//! everywhere, and `action = "route"` floats one on the routed workspace.

use crate::config::{DispositionPatch, Management, WindowDisposition};

/// Bundle identifiers whose windows float by default.
pub const FLOATING_BUNDLES: &[&str] = &[
    "com.apple.Stickies",
    "com.apple.calculator",
    "com.apple.Dictionary",
    "com.apple.clock",
    "com.apple.weather",
    "com.apple.systempreferences",
    "com.apple.Passwords",
    "com.apple.PhotoBooth",
    "com.apple.VoiceMemos",
    "com.apple.DigitalColorMeter",
    "com.apple.archiveutility",
    "com.apple.keychainaccess",
    "com.1password.1password",
    "com.agilebits.onepassword7",
    "com.bitwarden.desktop",
    "com.raycast.macos",
    "com.runningwithcrayons.Alfred",
    "pl.maketheweb.cleanshotx",
    "anomaly.hex",
    "com.kitlangton.hex2",
];

/// Whether `bundle_id` floats by default. Bundle identifiers compare case-insensitively, as
/// Launch Services compares them.
#[must_use]
pub fn floats_by_default(bundle_id: &str) -> bool {
    FLOATING_BUNDLES
        .iter()
        .any(|candidate| candidate.eq_ignore_ascii_case(bundle_id))
}

/// The built-in disposition for a window no user rule matched.
#[must_use]
pub fn disposition(bundle_id: &str) -> WindowDisposition {
    if floats_by_default(bundle_id) {
        WindowDisposition::Floating
    } else {
        WindowDisposition::Managed
    }
}

/// The built-in default as a patch under the user's rules: a float for the bundles above,
/// nothing otherwise.
#[must_use]
pub fn patch(bundle_id: &str) -> DispositionPatch {
    if floats_by_default(bundle_id) {
        DispositionPatch::managed(Management::Float)
    } else {
        DispositionPatch::NONE
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utilities_float_and_everything_else_is_managed() {
        assert_eq!(
            disposition("com.apple.Stickies"),
            WindowDisposition::Floating
        );
        assert_eq!(
            disposition("com.apple.stickies"),
            WindowDisposition::Floating
        );
        assert_eq!(disposition("com.apple.Safari"), WindowDisposition::Managed);
        assert_eq!(
            disposition("com.mitchellh.ghostty"),
            WindowDisposition::Managed
        );
        assert_eq!(disposition(""), WindowDisposition::Managed);
    }

    #[test]
    fn the_table_has_no_duplicates() {
        for (index, bundle_id) in FLOATING_BUNDLES.iter().enumerate() {
            assert!(
                !FLOATING_BUNDLES[..index]
                    .iter()
                    .any(|earlier| earlier.eq_ignore_ascii_case(bundle_id)),
                "{bundle_id} is listed twice"
            );
        }
    }
}
