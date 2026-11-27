//! Send Tab to Workspace (`move-tab`): presses the app's own "Move Tab to New
//! Window", then claims the window that press opens for the target workspace
//! the first time an observation sees it. A window with a single tab (or an
//! app without native tabs) moves whole, like `move`.

use super::commands::CommandOrigin;
use super::switching::RepeatSwitch;
use super::*;

/// How long a pressed detach waits for its new window before it is dropped.
pub(super) const TAB_DETACH_TIMEOUT: Duration = Duration::from_millis(800);
/// How soon to observe again while a detach waits for its window.
pub(super) const TAB_DETACH_RECHECK: Duration = Duration::from_millis(15);
/// Frames within this many points are the same place on screen.
const SAME_PLACE: i32 = 8;

/// A pressed "Move Tab to New Window" awaiting the window it opens.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct PendingTabDetach {
    pub(super) app: AppInstanceId,
    pub(super) source_window: crate::PhysicalWindowId,
    pub(super) source_workspace: WorkspaceId,
    pub(super) target_workspace: WorkspaceId,
    pub(super) follow: bool,
    pub(super) expires_at: Instant,
    /// Where `source_window` was before the press.
    source_frame: Option<crate::Rect>,
    /// Every window on screen before the press; none of them is the new one.
    /// `None` when the window server could not be read before the press.
    onscreen_before: Option<HashSet<u32>>,
    /// Where focus goes once a detach without `--follow` settles.
    settle_focus: SettleFocus,
    phase: DetachPhase,
}

/// Where a detach without `--follow` leaves focus.
#[derive(Clone, Debug, Eq, PartialEq)]
enum SettleFocus {
    /// The press went to the focused window: focus stays with its tabs.
    Remaining,
    /// Keying the source moved focus off this window (#597): it gets it back.
    HandBack(crate::PhysicalWindowId),
    /// Keying moved focus off nothing reported: leave it alone.
    Keep,
}

/// `move_tab` focused the window to press on, away from `left`.
#[derive(Clone, Debug, Eq, PartialEq)]
struct Keyed {
    left: Option<crate::PhysicalWindowId>,
    /// The window was parked on a hidden workspace and keyed under a
    /// departure guard, without a focus request.
    parked: bool,
}

/// A settled tab detach: `detached` went to its target and `remaining` took
/// over the source slot at `settled_at`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct TabDetachHandoff {
    detached: crate::PhysicalWindowId,
    remaining: crate::PhysicalWindowId,
    settled_at: Instant,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum DetachPhase {
    /// The press went through; no new window has been observed yet.
    Awaiting,
    /// This cycle routed `detached` to the target; `remaining` holds the
    /// source slot. Focus settles at the end of the cycle.
    Claimed {
        detached: crate::PhysicalWindowId,
        remaining: crate::PhysicalWindowId,
    },
}

impl<B: WorkspaceBackend> WorkspaceRuntime<B> {
    pub(super) fn move_tab(
        &mut self,
        target: WorkspaceId,
        follow: bool,
        outputs: macos::DisplayOutputs,
        cancelled: &AtomicBool,
    ) -> Result<CycleStatus, String> {
        self.ensure_managed_workspace(target)?;
        // Capture who had focus when the command arrived, before waiting for an
        // earlier detach whose settlement may switch workspaces or focus the
        // first detached tab.
        let mut window = self.current_focus();
        let handoff = self.finish_pending_tab_detach(&outputs, cancelled)?;
        if let Some((detached, remaining)) = handoff
            && window.as_ref() == Some(&detached)
        {
            // An AppKit tab group moved `source_window` out and handed the
            // remaining tab group's slot to `remaining`.
            window = Some(remaining);
        }
        let window = window
            .or_else(|| self.current_focus())
            .ok_or("Focused window is unavailable")?;
        let source = if let Some(source) = self.workspaces.workspace_of_any(&window) {
            source
        } else {
            self.cycle(outputs.clone(), cancelled)?;
            self.workspaces
                .workspace_of_any(&window)
                .ok_or("Window is not managed by SpaceFred")?
        };
        let keyed = self.key_window_for_detach(&window, source, &outputs)?;
        let onscreen_before = self
            .backend
            .onscreen_window_frames()
            .map(|frames| frames.into_keys().collect());
        let source_frame = self.backend.frame(&window).ok();
        match self.backend.detach_tab(&window) {
            macos::TabDetachResult::SingleTabOrUnsupported => {
                log_event!("move tab: {window:?} has no tab to detach; moving the window");
                // Keyed only for the press: hand focus back and move the
                // window the way `move` moves one that is not focused.
                let keyed = keyed.map(|keyed| self.hand_back_keyed(keyed, &window));
                if keyed.is_none() && self.current_focus().as_ref() == Some(&window) {
                    if follow {
                        self.move_focused_follow(target, outputs, cancelled)
                    } else {
                        self.move_focused(target, outputs, cancelled)
                    }
                } else if follow {
                    let was_visible = self.is_visible(target, &outputs);
                    let moved = self.move_window_explicit(
                        window.clone(),
                        target,
                        outputs.clone(),
                        cancelled,
                    )?;
                    if was_visible {
                        return Ok(moved);
                    }
                    self.workspaces.remember_focus(target, &window);
                    let mut switched = self.switch_with(
                        target,
                        outputs,
                        cancelled,
                        RepeatSwitch::KeepFocus,
                        CommandOrigin::Explicit,
                    )?;
                    switched.failed += moved.failed;
                    Ok(switched)
                } else {
                    self.move_window_explicit(window, target, outputs, cancelled)
                }
            }
            macos::TabDetachResult::Error(error) => {
                if let Some(keyed) = keyed {
                    self.hand_back_keyed(keyed, &window);
                }
                Err(format!("Move Tab to New Window failed: {error}"))
            }
            macos::TabDetachResult::Detached => {
                log_event!("move tab: detached a tab of {window:?} for workspace {target}");
                let settle_focus = match keyed {
                    None => SettleFocus::Remaining,
                    Some(Keyed {
                        left: Some(left), ..
                    }) => SettleFocus::HandBack(left),
                    Some(Keyed { left: None, .. }) => SettleFocus::Keep,
                };
                self.tab_detach = Some(PendingTabDetach {
                    app: window.app.clone(),
                    source_window: window,
                    source_workspace: source,
                    target_workspace: target,
                    follow,
                    expires_at: self.backend.now() + TAB_DETACH_TIMEOUT,
                    source_frame,
                    onscreen_before,
                    settle_focus,
                    phase: DetachPhase::Awaiting,
                });
                self.cycle(outputs, cancelled)
            }
        }
    }

    /// The app's menu command acts on its key window. After an earlier
    /// `--follow` that is the tab it sent away, not the tab group this command
    /// means (#597), so key `window` first and return the focus it took from
    /// another window. A window on a shown workspace takes focus as usual; one
    /// parked on a hidden workspace is keyed without letting focus follow chase
    /// it. Fails if `window` cannot be focused, rather than pressing on another.
    fn key_window_for_detach(
        &mut self,
        window: &crate::PhysicalWindowId,
        source: WorkspaceId,
        outputs: &macos::DisplayOutputs,
    ) -> Result<Option<Keyed>, String> {
        let reported = self.backend.focused_window();
        if reported.as_ref() == Some(window) {
            return Ok(None);
        }
        // What the user has focused: a focus command still in flight counts
        // over Accessibility's lagging report of the window it left.
        let effective = self.effective_focus(reported.clone());
        if effective.as_ref() == Some(window) {
            // Already the focused window in all but the report: key it again
            // for the press, but it stays focused like any other move-tab.
            if !self.request_focus(window, [reported]) {
                log_event!("move tab: could not key {window:?} again; pressing anyway");
            }
            return Ok(None);
        }
        if self.is_visible(source, outputs) {
            if self.request_focus(window, [reported, effective.clone()]) {
                return Ok(Some(Keyed {
                    left: effective,
                    parked: false,
                }));
            }
        } else if self.backend.focus_window(window) {
            self.guard_departure(Some(window.clone()), Some(window.app.clone()));
            return Ok(Some(Keyed {
                left: effective,
                parked: true,
            }));
        }
        Err(format!("Could not focus {window:?} to detach its tab"))
    }

    /// Undoes `key_window_for_detach` when the press detached nothing.
    fn hand_back_keyed(&mut self, keyed: Keyed, window: &crate::PhysicalWindowId) {
        if keyed.parked {
            self.clear_suppressed_focus();
        }
        if let Some(left) = keyed.left {
            let _: bool = self.request_focus(&left, [Some(window.clone())]);
        }
    }

    /// A newer `move-tab` lets an earlier detach still waiting for its window
    /// finish first: it observes until that window is claimed and sent to its
    /// own target, or the detach expires, so the earlier tab keeps its
    /// destination (#570). Returns `(detached, remaining)` if the pending
    /// detach settled while waiting, or settled in the moments before this
    /// command ran, while it was already queued (#597).
    fn finish_pending_tab_detach(
        &mut self,
        outputs: &macos::DisplayOutputs,
        cancelled: &AtomicBool,
    ) -> Result<Option<(crate::PhysicalWindowId, crate::PhysicalWindowId)>, String> {
        let mut was_cancelled = false;
        while let Some(pending) = self.tab_detach.as_ref() {
            let expires_at = pending.expires_at;
            if cancelled.load(Ordering::Acquire) {
                was_cancelled = true;
                break;
            }
            self.cycle(outputs.clone(), cancelled)?;
            if self.tab_detach.is_none() || self.backend.now() >= expires_at {
                break;
            }
            self.backend.pause(TAB_DETACH_RECHECK);
        }
        if let Some(dropped) = self.tab_detach.take() {
            let reason = if was_cancelled || cancelled.load(Ordering::Acquire) {
                "cancelled"
            } else {
                "a newer move tab began"
            };
            log_event!(
                "move tab: dropped the detach of {:?}: {reason}",
                dropped.source_window
            );
        }
        let now = self.backend.now();
        Ok(self
            .last_tab_detach_handoff
            .take()
            .filter(|handoff| now < handoff.settled_at + TAB_DETACH_TIMEOUT)
            .map(|handoff| (handoff.detached, handoff.remaining)))
    }

    /// Before `app`'s window affinities freeze: its first window that was not on
    /// screen before the press and unowned came from the detach.
    /// Returns the window to route to the target and the target.
    ///
    /// Chromium opens the tab in that new window. An AppKit tab group instead
    /// moves the tab's own window (`source_window`) out and shows its next
    /// tab's window in place, so that one takes over the source slot — unless
    /// an Ignore rule matches it: then the slot closes and it stays unmanaged.
    pub(super) fn claim_detached_tab(
        &mut self,
        app: &crate::ObservedApplication,
        windows: &[crate::ObservedWindow],
    ) -> Option<(crate::PhysicalWindowId, WorkspaceId)> {
        let pending = self.tab_detach.as_ref()?;
        if pending.app != app.id
            || pending.phase != DetachPhase::Awaiting
            || self.backend.now() >= pending.expires_at
        {
            return None;
        }
        let onscreen_before = pending.onscreen_before.as_ref()?;
        let fresh: Vec<&crate::ObservedWindow> = windows
            .iter()
            .filter(|window| {
                window.id != pending.source_window
                    && !onscreen_before.contains(&window.id.window_id)
                    && self.workspaces.residency(&window.id).is_none()
                    && !self.parked.contains_key(&window.id)
            })
            .collect();
        // An ignored window is never sent anywhere, but it may be the tab an
        // AppKit tab group shows in place of the one that moved out (#670).
        let (new, new_ignored) = match fresh
            .iter()
            .find(|window| !self.config.patch(&app.bundle_id, window).is_ignored())
        {
            Some(window) => (*window, false),
            None => (*fresh.first()?, true),
        };
        let source = pending.source_window.clone();
        let target = pending.target_workspace;
        let source_now = windows
            .iter()
            .find(|window| window.id == source)
            .map(|window| window.frame);
        let tab_group = match (pending.source_frame, source_now) {
            (Some(before), Some(now)) => {
                match (
                    new.frame.approximately_equals(before, SAME_PLACE),
                    now.approximately_equals(before, SAME_PLACE),
                ) {
                    (true, false) => true,
                    (false, true) => false,
                    // Both or neither in place: AppKit keeps the moved tab's
                    // window key, Chromium focuses its new window.
                    _ => self.backend.focused_window().as_ref() == Some(&source),
                }
            }
            _ => false,
        };
        let tab_group = tab_group
            && if new_ignored {
                // Its tile or float closes: an ignored window takes no slot.
                self.release_slot(&source)
            } else {
                self.hand_slot_to(&source, &new.id)
            };
        if new_ignored && !tab_group {
            // The tab itself opened in a window an Ignore rule matches.
            return None;
        }
        let (detached, remaining) = if tab_group {
            (source, new.id.clone())
        } else {
            (new.id.clone(), source)
        };
        log_event!(
            "move tab: claimed {detached:?} for workspace {target} ({}); {remaining:?} stays",
            if tab_group { "tab group" } else { "new window" }
        );
        if let Some(pending) = self.tab_detach.as_mut() {
            pending.phase = DetachPhase::Claimed {
                detached: detached.clone(),
                remaining,
            };
        }
        Some((detached, target))
    }

    /// Gives `from`'s tile or float to `to`, leaving `from` unowned.
    pub(super) fn hand_slot_to(
        &mut self,
        from: &crate::PhysicalWindowId,
        to: &crate::PhysicalWindowId,
    ) -> bool {
        if let Some(owner) = self.workspaces.workspace_of(from) {
            if !self.workspaces.engine_mut(owner).rebind_window(from, to) {
                return false;
            }
            self.forget_window_state(owner, from);
            return true;
        }
        self.workspaces.hand_float_to(from, to)
    }

    /// Removes `window`'s tile or float, leaving it unowned.
    fn release_slot(&mut self, window: &crate::PhysicalWindowId) -> bool {
        let Some(owner) = self
            .workspaces
            .workspace_of(window)
            .or_else(|| self.workspaces.floating_workspace_of(window))
        else {
            return false;
        };
        self.forget_window(owner, window);
        true
    }

    /// Per-window evidence about `window` on `workspace`, which no longer owns it.
    fn forget_window_state(&mut self, workspace: WorkspaceId, window: &crate::PhysicalWindowId) {
        self.constrained[workspace.index()].remove(window);
        self.applied[workspace.index()].remove(window);
        self.apply_attempts[workspace.index()].remove(window);
        self.minimum_sizes.remove(window);
    }

    /// At the end of a cycle: hands focus where `move-tab` promised once the
    /// detached window is claimed, keeps waiting (with a prompt re-observation)
    /// until the detach expires, and then drops it.
    pub(super) fn settle_tab_detach(
        &mut self,
        mut status: CycleStatus,
        outputs: &macos::DisplayOutputs,
        cancelled: &AtomicBool,
    ) -> Result<CycleStatus, String> {
        let Some(pending) = self.tab_detach.take() else {
            return Ok(status);
        };
        let (detached, remaining) = match &pending.phase {
            DetachPhase::Claimed {
                detached,
                remaining,
            } => {
                self.last_tab_detach_handoff = Some(TabDetachHandoff {
                    detached: detached.clone(),
                    remaining: remaining.clone(),
                    settled_at: self.backend.now(),
                });
                (detached.clone(), remaining.clone())
            }
            DetachPhase::Awaiting if self.backend.now() >= pending.expires_at => {
                log_event!(
                    "move tab: no window appeared for the tab detached from {:?}; dropped",
                    pending.source_window
                );
                return Ok(status);
            }
            DetachPhase::Awaiting => {
                let recheck = self.backend.now() + TAB_DETACH_RECHECK;
                status.close_recheck =
                    Some(status.close_recheck.map_or(recheck, |at| at.min(recheck)));
                self.tab_detach = Some(pending);
                return Ok(status);
            }
        };
        let target = pending.target_workspace;
        if self.workspaces.workspace_of_any(&detached) != Some(target) {
            log_event!("move tab: {detached:?} did not reach workspace {target}");
            return Ok(status);
        }
        // Routing already put it on the target's display, parked there if the target is hidden.
        let target_visible = self.is_visible(target, outputs);
        if pending.follow && !target_visible {
            self.workspaces.remember_focus(target, &detached);
            let failed = status.failed;
            status = self.switch_with(
                target,
                outputs.clone(),
                cancelled,
                RepeatSwitch::KeepFocus,
                CommandOrigin::Explicit,
            )?;
            status.failed += failed;
        } else {
            let focus = match (pending.follow, &pending.settle_focus) {
                (true, _) => Some(&detached),
                (false, SettleFocus::Remaining) => Some(&remaining),
                (false, SettleFocus::HandBack(left)) => Some(left),
                (false, SettleFocus::Keep) => None,
            };
            // Handing focus back leaves the window keyed for the press behind
            // too; it may still be reported while the request is in flight.
            let keyed = (!pending.follow
                && matches!(pending.settle_focus, SettleFocus::HandBack(_)))
            .then(|| pending.source_window.clone());
            let reported = self.backend.focused_window();
            match focus {
                Some(focus) if self.request_focus(focus, [reported, keyed]) => {
                    if pending.follow {
                        self.record_focused_workspace(target);
                        // The pointer follows the tab the command sent and
                        // followed, onto its display (#417).
                        let display = outputs.resolve(self.workspaces.assignment(target));
                        let (display_id, output) = (display.id.clone(), display.frame);
                        let frame = self.focus_window_frame(focus, target, output);
                        self.follow_pointer_to(
                            CommandOrigin::Explicit,
                            &display_id,
                            output,
                            frame,
                            outputs,
                        );
                    }
                }
                Some(focus) => log_event!("move tab: could not focus {focus:?}"),
                None => {}
            }
            if !target_visible {
                // Focus follow must not chase the parked tab back to its workspace.
                self.guard_departure(Some(detached.clone()), Some(detached.app.clone()));
            }
        }
        self.persist_workspace_state_if_changed()?;
        status.hud = Some(self.hud_for(target, UiHudKind::Moved, None, outputs));
        Ok(status)
    }
}
