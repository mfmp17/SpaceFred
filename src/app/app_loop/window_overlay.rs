//! `[labs] window_overlay`, the main thread's half: samples the chord each tick from the event
//! system's modifier and button state (no Accessibility), hides on any key or hotkey, and asks
//! the worker for what the overlay shows while it is up.

use super::*;
use crate::window_overlay::{HoldChange, OverlayHold};

pub(in crate::app) struct WindowOverlay {
    /// `[labs] window_overlay`.
    pub(in crate::app) lab: bool,
    hold: OverlayHold,
    /// A snapshot is queued for the worker and has not come back.
    snapshot_pending: bool,
}

impl WindowOverlay {
    pub(in crate::app) const fn new(lab: bool) -> Self {
        Self {
            lab,
            hold: OverlayHold::new(),
            snapshot_pending: false,
        }
    }
}

impl MainThreadAgent {
    /// Samples the chord: shows the overlay once it has been held alone long enough, and hides
    /// it when let go or when tiling or the Lab turns off.
    pub(super) fn tick_window_overlay(&mut self) {
        let enabled = self.automatic && self.window_overlay.lab;
        if !enabled && self.window_overlay.hold.is_idle() {
            return;
        }
        let change = self.window_overlay.hold.sample(
            Instant::now(),
            enabled,
            // The built-in Move shortcuts' chord: the base modifier with Shift.
            self.shortcuts.modifier.with_shift(),
            macos::held_modifiers(),
            macos::any_mouse_button_down(),
        );
        self.window_overlay_changed(change);
    }

    /// A key press or hotkey: the chord was part of a shortcut.
    pub(super) fn interrupt_window_overlay(&mut self) {
        if self.window_overlay.lab {
            let change = self.window_overlay.hold.interrupt();
            self.window_overlay_changed(change);
        }
    }

    /// A worker result while tiling: shows what it carries for the overlay, or asks for a fresh
    /// reading while the overlay is up, since the result may have moved its windows.
    pub(super) fn window_overlay_result(&mut self, overlay: Option<&UiWindowOverlay>) {
        match overlay {
            Some(overlay) => {
                self.window_overlay.snapshot_pending = false;
                if self.window_overlay.hold.is_shown() {
                    let news = UiWindowOverlayNews::Show(overlay.clone());
                    self.bridge.publish_window_overlay(news);
                }
            }
            None if self.window_overlay.hold.is_shown() => self.request_window_overlay(),
            None => {}
        }
    }

    fn window_overlay_changed(&mut self, change: Option<HoldChange>) {
        match change {
            Some(HoldChange::Show) => self.request_window_overlay(),
            Some(HoldChange::Hide) => {
                self.window_overlay.snapshot_pending = false;
                self.bridge
                    .publish_window_overlay(UiWindowOverlayNews::Hide);
            }
            None => {}
        }
    }

    fn request_window_overlay(&mut self) {
        if !std::mem::replace(&mut self.window_overlay.snapshot_pending, true) {
            let snapshot = WorkerAction::WindowOverlay(WindowOverlayCommand::Snapshot);
            self.queue_worker_input(QueuedInput::Action(snapshot));
        }
    }
}
