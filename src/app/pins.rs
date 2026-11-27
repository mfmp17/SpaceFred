//! Pinned floats at runtime: `spaceman ctl pin` and the parking exemption a pin carries.
//!
//! Ownership lives in [`crate::workspace::Workspaces`]; this module only decides what a pin
//! means for parking and focus: a pinned window is never parked by a switch (so it shows on
//! every workspace of its display) and never pulls focus to its owning workspace. Pause, quit
//! and restart still restore every parked window, a pinned one included.

use super::*;
use crate::workspace::FloatCause;

impl<B: WorkspaceBackend> WorkspaceRuntime<B> {
    /// Whether `window` stays on screen through workspace switches: it is pinned, or its app
    /// keeps un-parking it.
    pub(super) fn stays_on_screen(&self, window: &crate::PhysicalWindowId) -> bool {
        self.parked.is_exempt(window) || self.workspaces.is_pinned(window)
    }

    /// What rules and defaults say about an unobserved window, judged by its app's bundle alone
    /// (a title rule cannot be checked without a title). `None` for an app never observed.
    pub(super) fn bundle_disposition(
        &self,
        window: &crate::PhysicalWindowId,
        frame: crate::Rect,
    ) -> Option<crate::config::DispositionPatch> {
        self.disposition_with_title(window, frame, None)
    }

    /// What rules and defaults say about a parked window: by its bundle and the title it last
    /// showed, or by its bundle alone if it was never observed with one.
    fn parked_disposition(
        &self,
        window: &crate::PhysicalWindowId,
        frame: crate::Rect,
    ) -> Option<crate::config::DispositionPatch> {
        let title = self.memory.last_title(window).map(std::sync::Arc::from);
        self.disposition_with_title(window, frame, title)
    }

    /// Read a parked window's bundle and title from its running app when no observation in
    /// this run has listed it, as after a restart with every window of its app minimized
    /// (#697), and remember them as an observation would. Judged the way
    /// `stale_window_ignored` judges a window: `false` while a title rule could decide but the
    /// title could not be read yet, so the window is not judged this cycle and is read again
    /// on the next.
    fn learn_unobserved_window(&mut self, window: &crate::PhysicalWindowId) -> bool {
        if self.memory.identities.contains_key(window) {
            return true;
        }
        if let Some(bundle_id) = self.memory.bundles.get(&window.app)
            && !self.config.has_title_rules(bundle_id)
        {
            return true;
        }
        let Some(probe) = self.backend.window_rule_probe(window) else {
            return true;
        };
        if !probe.resolved && self.config.has_title_rules(&probe.bundle_id) {
            return false;
        }
        if probe.resolved {
            self.memory.note_probed_title(window, probe.title);
        }
        self.memory
            .bundles
            .entry(window.app.clone())
            .or_insert(probe.bundle_id);
        true
    }

    fn disposition_with_title(
        &self,
        window: &crate::PhysicalWindowId,
        frame: crate::Rect,
        title: Option<std::sync::Arc<str>>,
    ) -> Option<crate::config::DispositionPatch> {
        let bundle_id = self.memory.bundles.get(&window.app)?;
        let probe = crate::ObservedWindow {
            id: window.clone(),
            frame,
            resizable: true,
            title,
            document: None,
        };
        Some(self.config.patch(bundle_id, &probe))
    }

    /// A `pin` rule that arrived while a window was parked: no observation lists a parked
    /// window, so judge it by its app's bundle and its last observed title (an earlier title
    /// rule keeps precedence, #549), and let restore_visible bring it back. A window never
    /// observed in this run (its app's windows all minimized since a restart) is read from its
    /// running app instead (#697). A parked tile leaves its workspace's tiles and floats there
    /// first, as a shown tile would (#678).
    pub(super) fn pin_parked_windows_by_rule(&mut self) {
        let parked: Vec<_> = self
            .parked
            .iter()
            .filter(|(window, _)| {
                self.workspaces.workspace_of_any(window).is_some()
                    && !self.workspaces.is_pinned(window)
            })
            .map(|(window, frame)| (window.clone(), *frame))
            .collect();
        for (window, frame) in parked {
            let ready = self.learn_unobserved_window(&window);
            if !ready
                || !self
                    .parked_disposition(&window, frame)
                    .is_some_and(crate::config::DispositionPatch::pins)
            {
                continue;
            }
            if let Some(workspace) = self.workspaces.workspace_of(&window) {
                // As when a rule floats a shown tile: it keeps its app's first-window route
                // used (#664).
                self.forget_window(workspace, &window);
                self.workspaces
                    .float_former_tile(window.clone(), workspace, FloatCause::Automatic);
            }
            if self.workspaces.floating_workspace_of(&window).is_none() {
                continue;
            }
            log_event!("pin rule now matches parked {:?}; showing it", window);
            self.workspaces.set_rule_pinned(&window, true);
        }
    }

    /// Pin the focused window on every workspace (floating it first if it is tiled), or drop
    /// its hand pin.
    pub(super) fn toggle_pin(
        &mut self,
        outputs: macos::DisplayOutputs,
        cancelled: &AtomicBool,
    ) -> Result<CycleStatus, String> {
        let window = self
            .current_focus()
            .ok_or("Focused window is unavailable")?;
        self.toggle_pin_window(window, outputs, cancelled)
    }

    /// [`Self::toggle_pin`] for `window`, focused or not (the Window overlay Lab's card).
    pub(super) fn toggle_pin_window(
        &mut self,
        window: crate::PhysicalWindowId,
        outputs: macos::DisplayOutputs,
        cancelled: &AtomicBool,
    ) -> Result<CycleStatus, String> {
        if self.workspaces.workspace_of_any(&window).is_none() {
            // A window observed for the first time may not be bound yet.
            self.cycle(outputs.clone(), cancelled)?;
        }
        let workspace = self
            .workspaces
            .workspace_of_any(&window)
            .ok_or("Focused window is not managed by SpaceFred")?;
        if self.workspaces.is_pinned_by_hand(&window) {
            self.workspaces.unpin(&window);
            // Keep it where it is seen: the workspace now showing it on its display owns it.
            if let Some(output) = self.window_display_output(&window, &outputs)
                && let Some(shown) = self
                    .visible_workspaces(&outputs)
                    .into_iter()
                    .find(|resolved| resolved.output == output)
            {
                self.workspaces
                    .own_floating(window.clone(), shown.workspace);
            }
            log_event!("unpin {:?}", window);
            self.persist_workspace_state_if_changed()?;
            return Ok(self.status(0, &outputs));
        }
        if self.workspaces.is_pinned(&window) {
            return Err("Focused window is pinned by a window rule".into());
        }
        if !self.is_visible(workspace, &outputs) {
            return Err("Focused window is not on an active workspace".into());
        }
        if self.workspaces.workspace_of(&window).is_some() {
            self.float_window(window.clone(), workspace, outputs.clone(), cancelled)?;
        }
        if !self.workspaces.pin(window.clone()) {
            return Err("Focused window could not float, so it cannot be pinned".into());
        }
        log_event!("pin {:?} (workspace {})", window, workspace);
        self.persist_workspace_state_if_changed()?;
        Ok(self.status(0, &outputs))
    }
}
