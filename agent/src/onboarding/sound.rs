//! The Try it step's two sounds: a tick as a lesson is learned and a chime after the last.
//! They are macOS's own alert sounds, played quietly, and only by the real setup window: a
//! scene, a headless render and a test never reach this, so nothing plays unasked.

/// What just happened on the miniature desktop.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Cue {
    /// A lesson was learned.
    Tick,
    /// The last lesson was learned.
    Chime,
}

impl Cue {
    /// The system sound's name and how loud to play it, out of 1.
    const fn sound(self) -> (&'static str, f32) {
        match self {
            Self::Tick => ("Tink", 0.3),
            Self::Chime => ("Glass", 0.3),
        }
    }
}

#[cfg(all(target_os = "macos", not(test)))]
pub fn play(cue: Cue) {
    use objc2_app_kit::NSSound;
    use objc2_foundation::NSString;

    let (name, volume) = cue.sound();
    if let Some(sound) = NSSound::soundNamed(&NSString::from_str(name)) {
        // One shared sound per name: a quick second lesson restarts it.
        sound.stop();
        sound.setVolume(volume);
        sound.play();
    }
}

#[cfg(not(all(target_os = "macos", not(test))))]
pub fn play(cue: Cue) {
    let _ = cue.sound();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn both_cues_are_quiet_system_sounds() {
        for cue in [Cue::Tick, Cue::Chime] {
            let (name, volume) = cue.sound();
            assert!(["Tink", "Glass"].contains(&name));
            assert!((0.25..=0.35).contains(&volume), "{name} at {volume}");
        }
    }
}
