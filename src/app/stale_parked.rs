//! Windows an application reopened at a Spaceman parking spot.
//!
//! Quitting an app while some of its windows are parked lets macOS save those parked frames, so
//! the relaunched app reopens them one pixel inside a display corner. Their centers are off
//! screen, so no observation lists them, and their physical identity is new, so no recovery
//! frame covers them. Revealing every such window (not only the focused one) lets observation,
//! recall and reconciliation adopt it like any other window, on the workspace it remembers.

use super::*;

/// Rescued windows on one display cascade by this much, so none hides another.
const RESCUE_CASCADE: i32 = 24;

/// First wait before retrying a window recovery could not move; it doubles per failure.
pub(super) const RESCUE_RETRY_FIRST: Duration = Duration::from_secs(2);
/// Longest wait between retries of a window recovery could not move.
pub(super) const RESCUE_RETRY_CAP: Duration = Duration::from_secs(32);

/// Windows recovery could not move, by window ID. A failed write can wait out the
/// Accessibility timeout, so retrying every cycle would slow every retile; a window that only
/// failed because its app was still launching must still be rescued, so retries back off
/// rather than stop.
#[derive(Default)]
pub(super) struct UnmovableStale {
    /// Display frames when the failures were recorded: a display change retries every window.
    displays: Vec<crate::Rect>,
    windows: HashMap<u32, RescueFailure>,
    /// Windows an `ignore` rule matched or the window server left unowned (such as an
    /// always-on-top overlay off the normal layer, #581/#582), with their window-server frame
    /// then. Judging one reads its title through Accessibility or queries window owners, so each
    /// is checked once until it moves, closes, the displays change or the rules do (#562).
    ignored: HashMap<u32, crate::Rect>,
}

struct RescueFailure {
    /// Window-server frame at the failure: a window that moves is retried at once.
    frame: crate::Rect,
    failures: u32,
    retry_at: Instant,
}

impl UnmovableStale {
    /// Forget failures and ignore verdicts for windows that closed or moved, and all of them
    /// when the displays changed.
    fn refresh(&mut self, displays: &[crate::Rect], server_frames: &HashMap<u32, crate::Rect>) {
        if self.displays != displays {
            self.displays = displays.to_vec();
            self.windows.clear();
            self.ignored.clear();
        }
        self.windows
            .retain(|id, failure| server_frames.get(id) == Some(&failure.frame));
        self.ignored
            .retain(|id, frame| server_frames.get(id) == Some(frame));
    }

    /// Judge every window again: the rules changed.
    pub(super) fn forget_ignored(&mut self) {
        self.ignored.clear();
    }

    #[cfg(test)]
    pub(super) fn is_ignored(&self, id: u32) -> bool {
        self.ignored.contains_key(&id)
    }

    fn waiting(&self, id: u32, now: Instant) -> bool {
        self.windows
            .get(&id)
            .is_some_and(|failure| now < failure.retry_at)
    }

    fn failed(&mut self, id: u32, frame: crate::Rect, now: Instant) {
        let failures = self.windows.get(&id).map_or(0, |failure| failure.failures) + 1;
        let delay = RESCUE_RETRY_FIRST
            .saturating_mul(1 << (failures - 1).min(8))
            .min(RESCUE_RETRY_CAP);
        self.windows.insert(
            id,
            RescueFailure {
                frame,
                failures,
                retry_at: now + delay,
            },
        );
    }

    fn succeeded(&mut self, id: u32) {
        self.windows.remove(&id);
    }
}

impl<B: WorkspaceBackend> WorkspaceRuntime<B> {
    /// Reveal every window sitting exactly at a display's parking spot that Spaceman neither
    /// parked nor owns.
    pub(super) fn recover_stale_parked_windows(
        &mut self,
        outputs: &macos::DisplayOutputs,
        server_frames: &HashMap<u32, crate::Rect>,
    ) {
        let displays = outputs.frames().collect::<Vec<_>>();
        let screens = outputs.screens();
        let parked_ids: HashSet<u32> = self.parked.keys().map(|window| window.window_id).collect();
        let now = self.backend.now();
        self.unmovable_stale.refresh(&displays, server_frames);
        // Almost every window is on screen or parked by Spaceman: both are skipped before any
        // geometry or window-server lookup. Menu-bar strips (more than the parking strip, give
        // or take its one-pixel tolerance, shows on a screen), one-pixel and empty windows are
        // never at a parking spot.
        let mut stale: Vec<(u32, crate::Rect, crate::Rect)> = server_frames
            .iter()
            .filter(|(id, frame)| {
                !parked_ids.contains(id)
                    && !self.unmovable_stale.waiting(**id, now)
                    && !self.unmovable_stale.ignored.contains_key(*id)
                    && frame.width > 1
                    && frame.height > 1
                    && !displays.iter().any(|d| d.contains_center(**frame))
                    && !crate::geometry::shows_wider_than(**frame, &screens, 2)
            })
            .filter_map(|(&id, &frame)| {
                outputs.displays().iter().find_map(|display| {
                    let parked = crate::geometry::parked_frame_clear_of(
                        display.frame,
                        &displays,
                        &screens,
                        frame,
                    );
                    crate::geometry::parked_at(frame, parked, 1).then_some((
                        id,
                        frame,
                        display.frame,
                    ))
                })
            })
            .collect();
        if stale.is_empty() {
            return;
        }
        // A window Spaceman owns but did not park (an app holding it off screen itself) is never
        // rescued; leave it out before asking the window server, so it costs nothing per cycle.
        let owned: HashSet<u32> = WorkspaceId::all()
            .flat_map(|workspace| self.workspaces.windows(workspace))
            .map(|window| window.window_id)
            .collect();
        stale.retain(|(id, ..)| !owned.contains(id));
        if stale.is_empty() {
            return;
        }
        // Rescue in creation order, so the cascade matches the order recall assigns.
        stale.sort_by_key(|(id, ..)| *id);
        let ids = stale.iter().map(|(id, ..)| *id).collect();
        // Always-on-top windows have no owner here: observation ignores them, so recall and
        // tiling could never adopt one, and an app may keep a hidden one at the edge (#581).
        let owners = self.backend.window_owners(&ids);
        let mut cascade: HashMap<(i32, i32), i32> = HashMap::new();
        for (id, frame, output) in stale {
            let Some(app) = owners.get(&id) else {
                self.unmovable_stale.ignored.insert(id, frame);
                continue;
            };
            let window = crate::PhysicalWindowId {
                app: app.clone(),
                window_id: id,
            };
            if self.workspaces.workspace_of_any(&window).is_some()
                || self.workspaces.scratchpad().contains(&window)
            {
                continue;
            }
            match self.stale_window_ignored(&window, frame) {
                Some(false) => {}
                Some(true) => {
                    self.unmovable_stale.ignored.insert(id, frame);
                    continue;
                }
                None => {
                    log_event!(
                        "stale parked window {:?}: title unreadable for its rules; retrying",
                        window
                    );
                    self.unmovable_stale.failed(id, frame, now);
                    continue;
                }
            }
            let offset = cascade.entry((output.x, output.y)).or_insert(0);
            let target = crate::Rect {
                x: output.x + *offset,
                y: output.y + *offset,
                ..frame
            };
            *offset += RESCUE_CASCADE;
            log_event!(
                "recover stale parked window: {:?} from {:?} to {:?}",
                window,
                frame,
                target
            );
            let outcome = self.backend.set_position(&window, target);
            if matches!(outcome, macos::ApplyOutcome::Applied) {
                self.unmovable_stale.succeeded(id);
            } else {
                log_event!(
                    "stale parked recovery incomplete for {:?}: {:?}",
                    window,
                    outcome
                );
                self.unmovable_stale.failed(id, frame, now);
            }
        }
    }

    /// Whether an `ignore` rule covers a window no observation in hand lists (a stale parked
    /// window, #562, or one about to be parked, #663), judged by its app's bundle and its title
    /// like any observed window. A just-relaunched app's bundle comes from the window's owner.
    /// `None` when a title rule could decide but the title could not be read yet: the caller
    /// keeps its usual course rather than guess.
    pub(super) fn stale_window_ignored(
        &mut self,
        window: &crate::PhysicalWindowId,
        frame: crate::Rect,
    ) -> Option<bool> {
        let Some(probe) = self.backend.window_rule_probe(window) else {
            return Some(
                self.bundle_disposition(window, frame)
                    .is_some_and(crate::config::DispositionPatch::is_ignored),
            );
        };
        if !probe.resolved && self.config.has_title_rules(&probe.bundle_id) {
            return None;
        }
        let observed = crate::ObservedWindow {
            id: window.clone(),
            frame,
            resizable: true,
            title: probe.title,
            document: None,
        };
        Some(self.config.patch(&probe.bundle_id, &observed).is_ignored())
    }
}
