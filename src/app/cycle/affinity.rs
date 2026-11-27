use super::*;
use crate::config::DispositionPatch;
use crate::journal::PlacementSource;
use crate::workspace::{FloatCause, Residency};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum FrozenWindowAffinity {
    Owned(WorkspaceId),
    PendingReplacement(WorkspaceId),
    AmbiguousReplacement,
    Floating,
    ExplicitRoute(WorkspaceId),
    /// The window a `move-tab` detach opened, sent to its target ahead of any
    /// route rule, or the tab an app's own detach moved out of its group (#756).
    DetachedTab(WorkspaceId),
    /// The app's existing workspace, or a remembered placement a relaunched
    /// application's window claimed.
    Inferred(WorkspaceId),
    CurrentDisplay,
    /// Never tiled, owned, or parked (ignore rule or parking exemption).
    Ignored,
}

/// A float sent to `workspace` from wherever it opened (`observed`).
pub(super) struct FloatHome {
    pub(super) window: crate::PhysicalWindowId,
    pub(super) workspace: WorkspaceId,
    pub(super) observed: crate::Rect,
    /// Where a recalled placement had it.
    pub(super) remembered: Option<crate::Rect>,
}

pub(super) struct FrozenAffinityPolicy {
    pub(super) windows: HashMap<crate::PhysicalWindowId, FrozenWindowAffinity>,
    /// Floats this observation gave a workspace (a route, a recall, a detached tab); routing
    /// brings each onto its workspace's display before reconciliation.
    pub(super) float_homes: Vec<FloatHome>,
    pub(super) visible_workspaces: HashSet<WorkspaceId>,
    /// A visible Fullscreen Layout workspace that gained a tiled window during
    /// affinity healing or pre-reconcile routing.
    pub(super) fullscreen_hud: Option<WorkspaceId>,
    /// Apps with an incoming native tab held mid-handoff: every display skips them this
    /// cycle, so neither the tile it is replacing nor its focus history changes (#706).
    pub(super) held_apps: HashSet<crate::AppInstanceId>,
    /// When a held handoff gives up and routes the window as new.
    pub(super) handoff_recheck: Option<Instant>,
}

impl<B: WorkspaceBackend> WorkspaceRuntime<B> {
    pub(super) fn freeze_window_affinities(
        &mut self,
        observation: &crate::NativeObservation,
        visible: &[ResolvedWorkspace],
        server_frames: &HashMap<u32, crate::Rect>,
        parked_before: &HashSet<crate::PhysicalWindowId>,
    ) -> Result<FrozenAffinityPolicy, String> {
        let has_floating = self.workspaces.floating_entries().next().is_some();
        if has_floating {
            let floating_apps = self
                .workspaces
                .floating_entries()
                .map(|(window, _)| window.app.clone())
                .collect::<HashSet<_>>();
            for app in floating_apps {
                if !self.backend.is_process_running(&app) {
                    for window in self.workspaces.floating_windows_for_app(&app) {
                        log_event!("forget floating {:?}: app exited", window);
                        self.workspaces.forget_floating(&window);
                    }
                }
            }
        }
        self.forget_exited_apps_on_hidden_workspaces(visible);

        self.close_recalls();
        // Before any new instance below is considered: a relaunch's new
        // process can appear in the same observation that reports the old
        // one terminated.
        self.note_absent_departures(observation);
        let mut affinities = HashMap::new();
        let mut float_homes = Vec::new();
        let mut fullscreen_hud = None;
        let mut held_apps = HashSet::new();
        let mut handoff_recheck = None;
        // Read once, and only if a Complete snapshot omits an owned float or a hidden tile.
        let mut onscreen_frames = None;
        let hidden_tabs = super::hidden_tab::HiddenTabEvidence {
            visible,
            server_frames,
            parked_before,
        };
        for application in &observation.applications {
            let crate::AppObservation::Complete {
                app,
                focused,
                windows,
            } = application
            else {
                continue;
            };
            if has_floating {
                let floating_windows = self.workspaces.floating_windows_for_app(&app.id);
                for window in floating_windows {
                    if windows.iter().any(|observed| observed.id == window)
                        || self.parked.contains_key(&window)
                    {
                        self.omitted_windows.remove(&window);
                    } else if server_frames.contains_key(&window.window_id)
                        && self.backend.confirms_set_aside(&window)
                    {
                        // Minimized (⌘M) or its app hidden (⌘H): set aside, not closed. It
                        // keeps its float, so restoring it brings it back floating (#699).
                        self.omitted_windows.remove(&window);
                    } else if self.hand_float_to_incoming_tab(
                        &app.bundle_id,
                        &window,
                        windows,
                        server_frames,
                    ) {
                        // The tab group now shows another tab, which keeps the float (#747).
                        self.omitted_windows.remove(&window);
                    } else if self.omitted_float_is_on_another_desktop(
                        &window,
                        server_frames,
                        &mut onscreen_frames,
                    ) {
                        // Left on the desktop a Space slide left; its omission entry
                        // remembers that until the window is observed again (#768).
                    } else if !self.omitted_float_is_settling(&window, &mut onscreen_frames) {
                        log_event!("forget floating {:?}: not observed and not parked", window);
                        self.workspaces.forget_floating(&window);
                        self.omitted_windows.remove(&window);
                    }
                }
            }
            self.prepare_recall(app, windows);
            let detached_tab = self
                .claim_detached_tab(app, windows)
                .or_else(|| self.claim_app_detached_tab(app, focused.as_ref(), windows, visible));
            let mut app_owns_window = self.workspaces.app_owns_windows(&app.id);
            let mut app_owns_tiled = self.workspaces.app_holds_route(&app.id);
            for window in windows {
                if let Some(ws) = self.heal_reappeared_window(window, visible, server_frames) {
                    fullscreen_hud = Some(ws);
                }
                if self.parked.contains_key(&window.id)
                    && matches!(
                        self.workspaces.residency(&window.id),
                        Some(Residency::Scratchpad { shown_on: None, .. })
                    )
                {
                    // Still hidden in the scratchpad (re-parked or settling); only an
                    // honored reappearance clears its parked record and lets it adopt.
                    affinities.insert(window.id.clone(), FrozenWindowAffinity::Ignored);
                    continue;
                }
                // A window that already holds a tile keeps it while it temporarily looks
                // non-resizable (an attached window-modal sheet), and a tile rule tiles it
                // whatever it looks like: user rule ⊕ built-in default ⊕ window traits.
                let tiled = self.workspaces.workspace_of(&window.id).is_some();
                let rules = self.config.patch(&app.bundle_id, window);
                let patch = rules.over(DispositionPatch::traits(window.resizable, tiled));
                if patch.is_ignored() {
                    if let Some(owner) = self.workspaces.workspace_of_any(&window.id) {
                        log_event!("release {:?} from workspace {}: ignored", window.id, owner);
                        self.forget_window(owner, &window.id);
                    }
                    affinities.insert(window.id.clone(), FrozenWindowAffinity::Ignored);
                    continue;
                }
                // An agent restart restores every float without saying why it floated, so its
                // first observation hands it to a matching float rule (removing the rule then
                // tiles it again, #160) or keeps it floating by hand. A float set by hand this
                // session stays manual whatever its rules say (#493). A window no workspace owns
                // yet floats by hand when the setup tour or a recalled placement floated it.
                self.workspaces
                    .settle_restored_float(&window.id, rules.floats());
                let recalled = self.memory.recalled(&window.id);
                let by_hand = self.workspaces.is_manual_float(&window.id)
                    || (self.workspaces.residency(&window.id).is_none()
                        && !rules.floats()
                        && (self.arranged_float(&window.id)
                            || recalled.is_some_and(|placement| placement.floating)));
                let floating = patch.floats() || by_hand;
                let cause = if by_hand {
                    FloatCause::Manual
                } else {
                    FloatCause::Automatic
                };
                if floating {
                    if let Some(owner) = self.workspaces.workspace_of(&window.id) {
                        self.forget_window(owner, &window.id);
                        self.workspaces
                            .float_former_tile(window.id.clone(), owner, cause);
                    }
                } else if self.workspaces.floating_workspace_of(&window.id).is_some() {
                    self.workspaces.forget_floating(&window.id);
                }
                let mut float_home = |runtime: &mut Self, target, frame| {
                    runtime.workspaces.float(window.id.clone(), target, cause);
                    float_homes.push(FloatHome {
                        window: window.id.clone(),
                        workspace: target,
                        observed: window.frame,
                        remembered: frame,
                    });
                    FrozenWindowAffinity::Floating
                };
                let affinity = if let Some(owner) = self.workspaces.workspace_of(&window.id) {
                    FrozenWindowAffinity::Owned(owner)
                } else if let Some((_, target)) = detached_tab
                    .as_ref()
                    .filter(|(detached, _)| *detached == window.id)
                {
                    app_owns_window = true;
                    if floating {
                        float_home(self, *target, None)
                    } else {
                        app_owns_tiled = true;
                        FrozenWindowAffinity::DetachedTab(*target)
                    }
                } else if let Some(deadline) =
                    self.hold_float_tab_handoff(&app.bundle_id, window, windows)
                {
                    // Opened over its app's float that is still listed: a native tab
                    // mid-handoff, held until the outgoing tab hands its float over (#747).
                    held_apps.insert(app.id.clone());
                    handoff_recheck =
                        Some(handoff_recheck.map_or(deadline, |at: Instant| at.min(deadline)));
                    FrozenWindowAffinity::Ignored
                } else if !floating
                    && let Some(owner) = self.claim_moved_tab_tile(
                        &app.bundle_id,
                        window,
                        windows,
                        server_frames,
                        &mut onscreen_frames,
                    )
                {
                    // The tab its group shows in place of a tile just moved away (#759).
                    app_owns_window = true;
                    app_owns_tiled = true;
                    FrozenWindowAffinity::Owned(owner)
                } else if !floating
                    && let Some(owner) = self.claim_hidden_tab_tile(
                        &app.bundle_id,
                        window,
                        windows,
                        &hidden_tabs,
                        &mut onscreen_frames,
                    )
                {
                    // The selected tab of a group whose tile is on a hidden workspace (#753).
                    app_owns_window = true;
                    app_owns_tiled = true;
                    FrozenWindowAffinity::Owned(owner)
                } else if !floating
                    && let Some(affinity) = {
                        let mut reserved = self.workspaces.replacement_workspaces(&window.id);
                        reserved.next().map(|workspace| {
                            if reserved.next().is_some() {
                                FrozenWindowAffinity::AmbiguousReplacement
                            } else {
                                FrozenWindowAffinity::PendingReplacement(workspace)
                            }
                        })
                    }
                {
                    affinity
                } else if floating && self.workspaces.floating_workspace_of(&window.id).is_some() {
                    FrozenWindowAffinity::Floating
                } else if floating && let Some(placement) = recalled {
                    app_owns_window = true;
                    self.note_placement(&window.id, PlacementSource::Recall, placement.workspace);
                    float_home(self, placement.workspace, Some(placement.frame))
                } else if floating {
                    // Routed only from a managed display holding its center, like a tiled
                    // route: a window centered between displays stays unmanaged (#730).
                    let on_managed_display = visible.iter().any(|resolved| {
                        self.scope.manages_selector(&resolved.selector)
                            && resolved.screen.contains_center(window.frame)
                    });
                    let routed = patch.home.filter(|target| {
                        on_managed_display
                            && (!app_owns_window
                                || self.config.routes_every_window(&app.bundle_id, window))
                            && self
                                .scope
                                .manages_selector(self.workspaces.assignment(*target))
                    });
                    if let Some(target) = routed {
                        app_owns_window = true;
                        log_event!("route floating {:?} -> workspace {}", window.id, target);
                        self.note_placement(&window.id, PlacementSource::Rule, target);
                        float_home(self, target, None)
                    } else if let Some(resolved) = visible
                        .iter()
                        .find(|resolved| resolved.screen.contains_center(window.frame))
                    {
                        app_owns_window = true;
                        log_event!(
                            "adopt floating {:?} -> workspace {}",
                            window.id,
                            resolved.workspace
                        );
                        self.workspaces
                            .float(window.id.clone(), resolved.workspace, cause);
                        self.note_placement(
                            &window.id,
                            PlacementSource::Active,
                            resolved.workspace,
                        );
                        FrozenWindowAffinity::Floating
                    } else {
                        FrozenWindowAffinity::CurrentDisplay
                    }
                } else if let Some(target) = self.arranged_target(&window.id) {
                    // The setup tour's plan placed this open window: a route for it alone.
                    app_owns_window = true;
                    app_owns_tiled = true;
                    FrozenWindowAffinity::ExplicitRoute(target)
                } else if let Some(target) = patch.home {
                    if !self
                        .scope
                        .manages_selector(self.workspaces.assignment(target))
                    {
                        return Err(format!(
                            "window rule routes to unmanaged workspace {}",
                            target
                        ));
                    }
                    let routes_all = self.config.routes_every_window(&app.bundle_id, window);
                    if app_owns_tiled
                        && routes_all
                        && self.replaces_native_tab(
                            &app.bundle_id,
                            window,
                            focused.as_ref(),
                            windows,
                            visible,
                        )
                    {
                        FrozenWindowAffinity::CurrentDisplay
                    } else if app_owns_tiled
                        && routes_all
                        && let Some(deadline) = self.hold_tab_handoff(window, windows, visible)
                    {
                        // Mid native-tab handoff: neither routed nor reconciled yet (#706).
                        held_apps.insert(app.id.clone());
                        handoff_recheck =
                            Some(handoff_recheck.map_or(deadline, |at: Instant| at.min(deadline)));
                        FrozenWindowAffinity::Ignored
                    } else if !app_owns_tiled || routes_all {
                        app_owns_window = true;
                        app_owns_tiled = true;
                        FrozenWindowAffinity::ExplicitRoute(target)
                    } else {
                        FrozenWindowAffinity::CurrentDisplay
                    }
                } else {
                    app_owns_window = true;
                    app_owns_tiled = true;
                    recalled.map_or(FrozenWindowAffinity::CurrentDisplay, |placement| {
                        FrozenWindowAffinity::Inferred(placement.workspace)
                    })
                };
                self.workspaces.set_rule_pinned(&window.id, patch.pins());
                affinities.insert(window.id.clone(), affinity);
            }
        }
        self.tab_handoffs.retain(|window, _| {
            affinities.get(window) == Some(&FrozenWindowAffinity::Ignored)
                && held_apps.contains(&window.app)
        });
        Ok(FrozenAffinityPolicy {
            held_apps,
            handoff_recheck,
            windows: affinities,
            float_homes,
            visible_workspaces: visible.iter().map(|resolved| resolved.workspace).collect(),
            fullscreen_hud,
        })
    }

    /// Whether `window` is the incoming native tab of one of its app's tiles on the visible
    /// workspace it opened over, so reconciling that workspace binds it to the tile's slot.
    /// A rule that routes every new window leaves it there instead of sending it to the
    /// rule's workspace (#509).
    fn replaces_native_tab(
        &self,
        bundle_id: &str,
        window: &crate::ObservedWindow,
        focused: Option<&crate::PhysicalWindowId>,
        windows: &[crate::ObservedWindow],
        visible: &[ResolvedWorkspace],
    ) -> bool {
        if focused != Some(&window.id) {
            return false;
        }
        let Some(resolved) = visible
            .iter()
            .find(|resolved| resolved.screen.contains_center(window.frame))
        else {
            return false;
        };
        // The app's windows that workspace's reconciliation would see as new. It never sees
        // an ignored window, so an ignored one left open on this display must not make the
        // incoming tab look ambiguous (#598).
        let unbound: Vec<_> = windows
            .iter()
            .filter(|other| {
                resolved.screen.contains_center(other.frame)
                    && self.workspaces.workspace_of_any(&other.id).is_none()
                    && !self.config.patch(bundle_id, other).is_ignored()
            })
            .map(|other| other.id.clone())
            .collect();
        self.workspaces
            .engine(resolved.workspace)
            .replaces_native_tab(
                &self.layout_settings(resolved.output),
                windows,
                &unbound,
                focused,
            )
    }

    /// Hidden workspaces are not reconciled, so an app that quit while its
    /// windows were parked would otherwise keep their tiles until the
    /// workspace was shown again.
    fn forget_exited_apps_on_hidden_workspaces(&mut self, visible: &[ResolvedWorkspace]) {
        let mut running = HashMap::new();
        for workspace in WorkspaceId::all() {
            if visible
                .iter()
                .any(|resolved| resolved.workspace == workspace)
            {
                continue;
            }
            let apps = self
                .workspaces
                .app_instances(workspace)
                .cloned()
                .collect::<HashSet<_>>();
            for app in apps {
                let alive = *running
                    .entry(app.clone())
                    .or_insert_with(|| self.backend.is_process_running(&app));
                if alive {
                    continue;
                }
                let windows = self
                    .workspaces
                    .engine(workspace)
                    .model()
                    .workspace
                    .slots
                    .iter()
                    .filter(|slot| slot.belongs_to(&app))
                    .map(|slot| slot.identity_window().clone())
                    .collect::<Vec<_>>();
                // A failed incarnation lookup is not proof of exit (#735). A window ID
                // belongs to the process that created it and dies with it, so the window
                // server itself (read directly, without asking `NSRunningApplication`)
                // still listing one of these windows under the app's pid means the app
                // is alive.
                if self.window_server_lists_app_window(&windows) {
                    log_event!(
                        "workspace {workspace}: keeping {app:?}: incarnation lookup failed but \
                         the window server still lists its windows"
                    );
                    running.insert(app, true);
                    continue;
                }
                self.note_departure(&app);
                let canvas = self.known_workspace_canvas(workspace);
                self.workspaces
                    .forget_app_with(workspace, &app, self.config.layout(), canvas);
                for window in &windows {
                    self.forget_window(workspace, window);
                }
                self.known_apps[workspace.index()].remove(&app);
                log_event!(
                    "workspace {workspace}: forgot {app:?}: app exited while hidden ({} tile(s))",
                    windows.len()
                );
                let parked = windows
                    .into_iter()
                    .filter(|window| self.parked.contains_key(window))
                    .collect();
                self.restore_windows(parked);
            }
        }
    }

    /// Whether the window server lists any of `windows` under its app's pid.
    fn window_server_lists_app_window(&mut self, windows: &[crate::PhysicalWindowId]) -> bool {
        windows
            .iter()
            .any(|window| self.backend.window_server_frame(window).is_some())
    }
}
