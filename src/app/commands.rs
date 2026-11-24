use super::*;

mod sizing;

/// How long Accessibility may keep reporting the previously focused window
/// after a focus command. App activation is asynchronous, so a second ⌥L
/// pressed in that window would otherwise step from the stale window and land
/// on the same target again.
pub(super) const FOCUS_SETTLE: Duration = Duration::from_millis(250);

pub(super) struct PendingFocus {
    pub(super) target: crate::PhysicalWindowId,
    /// Windows Accessibility may still report while `target` takes focus: the
    /// one focus left and earlier targets in a run of quick presses.
    pub(super) stale: Vec<Option<crate::PhysicalWindowId>>,
    pub(super) at: Instant,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(super) enum WarpContext {
    /// Direct method call outside `execute_action` (unit tests).
    #[default]
    Direct,
    /// Explicit keyboard or `ctl` focus-moving action.
    Explicit,
    /// Passive cycle, gesture, swipe, or non-focus action: never warp.
    Suppressed,
}

pub(super) enum WorkspaceFocusOutcome {
    Focused(crate::PhysicalWindowId),
    Empty,
    Failed,
}

const fn rect_center(rect: crate::Rect) -> (i32, i32) {
    (rect.x + rect.width / 2, rect.y + rect.height / 2)
}

impl<B: WorkspaceBackend> WorkspaceRuntime<B> {
    pub(super) fn with_warp_context<T>(
        &mut self,
        context: WarpContext,
        f: impl FnOnce(&mut Self) -> T,
    ) -> T {
        let previous = std::mem::replace(&mut self.warp_context, context);
        let result = f(self);
        self.warp_context = previous;
        result
    }

    pub(super) fn focus_window_frame(
        &mut self,
        window: &crate::PhysicalWindowId,
        workspace: WorkspaceId,
        output: crate::Rect,
    ) -> Option<crate::Rect> {
        let settings = self.layout_settings(output);
        let tile_frame = if self.workspaces.uses_fullscreen_layout(workspace)
            && self.workspaces.workspace_of(window) == Some(workspace)
        {
            Some(settings.single())
        } else {
            self.workspaces
                .engine(workspace)
                .placements(&settings)
                .into_iter()
                .find(|placement| placement.window.as_ref() == Some(window))
                .map(|placement| placement.frame)
        };
        tile_frame.or_else(|| {
            self.backend
                .window_server_frame(window)
                .or_else(|| self.backend.frame(window).ok())
        })
    }

    pub(super) fn follow_pointer_to(
        &mut self,
        target_display_id: &str,
        output: crate::Rect,
        focused_window: Option<crate::Rect>,
        outputs: &macos::DisplayOutputs,
    ) {
        let mode = self.config.mouse_follows_focus();
        if mode == crate::config::MouseFollowsFocus::Off
            || self.warp_context == WarpContext::Suppressed
            || self.backend.mouse_down()
        {
            return;
        }
        let target_rect = focused_window.unwrap_or(output);
        let target_display_id = focused_window
            .and_then(|frame| outputs.display_at(rect_center(frame)))
            .map_or(target_display_id, |display| display.id.as_str());
        let already_there = self
            .backend
            .pointer_location()
            .is_some_and(|(x, y)| match mode {
                crate::config::MouseFollowsFocus::Off => true,
                crate::config::MouseFollowsFocus::Display => outputs
                    .display_at((x, y))
                    .is_some_and(|display| display.id == target_display_id),
                crate::config::MouseFollowsFocus::Window => target_rect.contains(x, y),
            });
        if already_there {
            return;
        }
        let _: bool = self.backend.warp_pointer(rect_center(target_rect));
    }

    pub(super) fn follow_pointer_for_workspace(
        &mut self,
        workspace: WorkspaceId,
        output: crate::Rect,
        outcome: WorkspaceFocusOutcome,
        outputs: &macos::DisplayOutputs,
    ) {
        let target_display_id = outputs
            .resolve(self.workspaces.assignment(workspace))
            .id
            .as_str();
        match outcome {
            WorkspaceFocusOutcome::Focused(window) => {
                let frame = self.focus_window_frame(&window, workspace, output);
                self.follow_pointer_to(target_display_id, output, frame, outputs);
            }
            WorkspaceFocusOutcome::Empty => {
                self.follow_pointer_to(target_display_id, output, None, outputs);
            }
            WorkspaceFocusOutcome::Failed => {}
        }
    }
    pub(super) fn reorder_focused(
        &mut self,
        direction: Direction,
        outputs: macos::DisplayOutputs,
        cancelled: &AtomicBool,
    ) -> Result<CycleStatus, String> {
        let (window, workspace) = self.focused_active_workspace(&outputs, cancelled, true)?;
        if self.workspaces.uses_fullscreen_layout(workspace) {
            return Err("Exit Fullscreen Layout before reordering the split layout".into());
        }
        let windows = self.workspaces.engine(workspace).bound_windows();
        let axis = self.workspaces.engine(workspace).root_axis();
        let perpendicular = matches!(
            (axis, direction),
            (crate::Axis::Horizontal, Direction::Down | Direction::Up)
                | (crate::Axis::Vertical, Direction::Left | Direction::Right)
        );
        let tiles = self.navigation_tiles(&outputs);
        let local_tiles = tiles
            .iter()
            .filter(|tile| tile.workspace == workspace)
            .cloned()
            .collect::<Vec<_>>();
        if let Some(target) = crate::navigation::target(&local_tiles, &window, direction) {
            if !self
                .workspaces
                .engine_mut(workspace)
                .swap_windows(&window, &target.window)
            {
                return Ok(self.status(0, &outputs));
            }
            return self.relayout_persist(workspace, &outputs, cancelled);
        }
        if windows.len() >= 2
            && perpendicular
            && self
                .workspaces
                .engine_mut(workspace)
                .create_implicit_split(&window, direction)
        {
            return self.relayout_persist(workspace, &outputs, cancelled);
        }
        let mut candidates = self.empty_display_targets(&outputs, &tiles);
        candidates.extend(tiles);
        let Some(target) = crate::navigation::target(&candidates, &window, direction) else {
            return Ok(self.status(0, &outputs));
        };
        self.with_warp_context(WarpContext::Suppressed, |rt| {
            rt.move_window(window, target.workspace, outputs, cancelled)
        })
    }

    /// A display whose workspace has no tiles offers no window to move toward,
    /// so its single-tile frame stands in for one. The stand-in's window id
    /// is never a real window; only its workspace is used.
    fn empty_display_targets(
        &self,
        outputs: &macos::DisplayOutputs,
        tiles: &[NavigationTile],
    ) -> Vec<NavigationTile> {
        self.visible_workspaces(outputs)
            .into_iter()
            .filter(|resolved| {
                self.scope.manages_selector(&resolved.selector)
                    && !tiles
                        .iter()
                        .any(|tile| tile.workspace == resolved.workspace)
            })
            .map(|resolved| NavigationTile {
                workspace: resolved.workspace,
                window: crate::PhysicalWindowId {
                    app: crate::AppInstanceId {
                        pid: -1,
                        launched_at: 0,
                    },
                    window_id: u32::try_from(resolved.workspace.index()).unwrap_or(u32::MAX),
                },
                frame: self.layout_settings(resolved.output).single(),
            })
            .collect()
    }

    pub(super) fn drop_window(
        &mut self,
        window: crate::PhysicalWindowId,
        point: (i32, i32),
        preview: bool,
        outputs: macos::DisplayOutputs,
        cancelled: &AtomicBool,
    ) -> Result<CycleStatus, String> {
        let Some(source) = self.workspaces.workspace_of(&window) else {
            return self.cycle(outputs, cancelled);
        };
        if !self.is_visible(source, &outputs) || self.parked.contains_key(&window) {
            return Ok(self.status(0, &outputs));
        }
        if self.workspaces.uses_fullscreen_layout(source) {
            return Ok(self.status(0, &outputs));
        }
        self.constrained[source.index()].remove(&window);
        self.applied[source.index()].remove(&window);
        let target = self
            .navigation_tiles(&outputs)
            .into_iter()
            .find(|tile| tile.window != window && tile.frame.contains(point.0, point.1));
        if let Some(target) = target {
            if target.workspace != source {
                if preview {
                    return Ok(self.status(0, &outputs));
                }
                return self.with_warp_context(WarpContext::Suppressed, |rt| {
                    rt.move_window(window, target.workspace, outputs, cancelled)
                });
            }
            let direction = drop_direction(point, target.frame);
            let changed = match direction {
                Some(direction) => self.workspaces.engine_mut(source).move_window_relative(
                    &window,
                    &target.window,
                    direction,
                ),
                None => self
                    .workspaces
                    .engine_mut(source)
                    .swap_windows(&window, &target.window),
            };
            if changed {
                let layout = self.layout_workspace_during_drag(
                    source,
                    output_for(&self.workspaces, source, &outputs),
                    preview.then_some(&window),
                    cancelled,
                );
                if !preview {
                    self.persist_workspace_state_if_changed()?;
                }
                return Ok(self.status(layout.failed, &outputs));
            }
        }
        if let Some(target) = self
            .visible_workspaces(&outputs)
            .into_iter()
            .find(|resolved| resolved.output.contains(point.0, point.1))
            .map(|resolved| resolved.workspace)
            && target != source
        {
            if preview {
                return Ok(self.status(0, &outputs));
            }
            return self.with_warp_context(WarpContext::Suppressed, |rt| {
                rt.move_window(window, target, outputs, cancelled)
            });
        }
        let layout = self.layout_workspace(
            source,
            output_for(&self.workspaces, source, &outputs),
            cancelled,
        );
        if !preview {
            self.persist_workspace_state_if_changed()?;
        }
        Ok(self.status(layout.failed, &outputs))
    }

    pub(super) fn resize_window(
        &mut self,
        window: crate::PhysicalWindowId,
        frame: crate::Rect,
        preview: bool,
        outputs: macos::DisplayOutputs,
        cancelled: &AtomicBool,
    ) -> Result<CycleStatus, String> {
        let Some(workspace) = self.workspaces.workspace_of(&window) else {
            return self.cycle(outputs, cancelled);
        };
        if !self.is_visible(workspace, &outputs) || self.parked.contains_key(&window) {
            return Ok(self.status(0, &outputs));
        }
        if self.workspaces.uses_fullscreen_layout(workspace) {
            return Ok(self.status(0, &outputs));
        }
        self.constrained[workspace.index()].remove(&window);
        self.applied[workspace.index()].remove(&window);
        let output = output_for(&self.workspaces, workspace, &outputs);
        let settings = self.layout_settings(output);
        let failed = if self
            .workspaces
            .engine_mut(workspace)
            .resize_window(&window, frame, &settings)
        {
            self.layout_workspace_during_drag(workspace, output, Some(&window), cancelled)
                .failed
        } else {
            0
        };
        if !preview {
            self.persist_workspace_state_if_changed()?;
        }
        Ok(self.status(failed, &outputs))
    }

    pub(super) fn focus_relative(
        &mut self,
        direction: Direction,
        outputs: macos::DisplayOutputs,
        cancelled: &AtomicBool,
    ) -> Result<CycleStatus, String> {
        let origin = self.current_focus();
        let managed = match &origin {
            Some(window) => self
                .active_workspace_of(window, &outputs, cancelled, true)
                .ok(),
            None => None,
        };
        let target = match (&origin, managed) {
            (Some(focused), Some(workspace)) => self
                .stacked_target(workspace, focused, direction)
                .or_else(|| {
                    self.spatial_target(focused, direction, &outputs)
                        .map(|(_, target)| target)
                }),
            // Focus is on a floating window, dialog or popup, or nowhere: the
            // key brings it back onto the tiles instead of doing nothing.
            _ => self.rescue_target(origin.as_ref(), direction, &outputs),
        };
        let Some(target) = target else {
            return Ok(self.status(0, &outputs));
        };
        self.focus_tile(origin, &target, &outputs)?;
        Ok(self.status(0, &outputs))
    }

    fn rescue_target(
        &mut self,
        origin: Option<&crate::PhysicalWindowId>,
        direction: Direction,
        outputs: &macos::DisplayOutputs,
    ) -> Option<crate::PhysicalWindowId> {
        let tiles = self.navigation_tiles(outputs);
        let frame = origin.and_then(|window| {
            self.backend
                .window_server_frame(window)
                .or_else(|| self.backend.frame(window).ok())
        });
        if let Some(frame) = frame {
            return crate::navigation::rescue(&tiles, frame, direction)
                .map(|tile| tile.window.clone());
        }
        self.visible_workspaces(outputs)
            .iter()
            .find_map(|resolved| {
                self.workspaces
                    .preferred_focus(resolved.workspace)
                    .filter(|window| tiles.iter().any(|tile| &tile.window == window))
                    .or_else(|| {
                        tiles
                            .iter()
                            .find(|tile| tile.workspace == resolved.workspace)
                            .map(|tile| tile.window.clone())
                    })
            })
    }

    /// Resolves `reported` against a focus command still in flight, trusting the
    /// pending target over a known stale Accessibility report until it settles.
    pub(super) fn resolve_focus(
        &mut self,
        reported: Option<crate::PhysicalWindowId>,
    ) -> Option<crate::PhysicalWindowId> {
        let Some(pending) = self.pending_focus.take() else {
            return reported;
        };
        if pending.at.elapsed() < FOCUS_SETTLE && pending.stale.contains(&reported) {
            let target = pending.target.clone();
            self.pending_focus = Some(pending);
            return Some(target);
        }
        reported
    }

    /// The focused window, trusting a focus command still in flight over the
    /// stale window Accessibility reports until the target app activates.
    pub(super) fn current_focus(&mut self) -> Option<crate::PhysicalWindowId> {
        let reported = self.backend.focused_window();
        self.resolve_focus(reported)
    }

    /// Drains an unexpired `pending_focus` into its stale-report list (prior
    /// stale windows plus its superseded target) so a replacement focus request
    /// still ignores earlier targets in the same burst (#307).
    pub(super) fn take_pending_focus_stale(&mut self) -> Vec<Option<crate::PhysicalWindowId>> {
        let Some(pending) = self.pending_focus.take() else {
            return Vec::new();
        };
        if pending.at.elapsed() >= FOCUS_SETTLE {
            return Vec::new();
        }
        let mut stale = pending.stale;
        let previous = Some(pending.target);
        if !stale.contains(&previous) {
            stale.push(previous);
        }
        stale
    }

    pub(super) fn focus_tile(
        &mut self,
        origin: Option<crate::PhysicalWindowId>,
        target: &crate::PhysicalWindowId,
        outputs: &macos::DisplayOutputs,
    ) -> Result<(), String> {
        let mut stale = self
            .pending_focus
            .take()
            .map(|pending| pending.stale)
            .unwrap_or_default();
        if !self.backend.focus_window(target) {
            return Err("Target window could not be focused".into());
        }
        stale.push(origin);
        self.pending_focus = Some(PendingFocus {
            target: target.clone(),
            stale,
            at: Instant::now(),
        });
        if let Some(workspace) = self.workspaces.workspace_of(target) {
            self.remember_fullscreen_focus(workspace, target)?;
        }
        if let Some(workspace) = self.workspaces.workspace_of_any(target) {
            let display = outputs.resolve(self.workspaces.assignment(workspace));
            let output = display.frame;
            let frame = self.focus_window_frame(target, workspace, output);
            self.follow_pointer_to(&display.id, output, frame, outputs);
        }
        Ok(())
    }

    /// Fullscreen Layout tiles share one frame, so spatial navigation cannot
    /// tell them apart. Left/up and right/down instead step backward and
    /// forward through the stable tile order, without wrapping, so the ends of
    /// the stack still lead to the adjacent display.
    fn stacked_target(
        &self,
        workspace: WorkspaceId,
        focused: &crate::PhysicalWindowId,
        direction: Direction,
    ) -> Option<crate::PhysicalWindowId> {
        if !self.workspaces.uses_fullscreen_layout(workspace) {
            return None;
        }
        let windows = self.workspaces.engine(workspace).bound_windows();
        let index = windows.iter().position(|window| window == focused)?;
        let target = match direction {
            Direction::Left | Direction::Up => index.checked_sub(1)?,
            Direction::Right | Direction::Down => index + 1,
        };
        windows.get(target).cloned()
    }

    pub(super) fn cycle_focus(
        &mut self,
        outputs: macos::DisplayOutputs,
        cancelled: &AtomicBool,
    ) -> Result<CycleStatus, String> {
        self.cycle_focus_by(outputs, cancelled, false)
    }

    pub(super) fn cycle_focus_previous(
        &mut self,
        outputs: macos::DisplayOutputs,
        cancelled: &AtomicBool,
    ) -> Result<CycleStatus, String> {
        self.cycle_focus_by(outputs, cancelled, true)
    }

    /// Steps through the workspace's tiles in stable order, then its floats,
    /// wrapping either way, so a workspace of floats (or one tile plus floats)
    /// can still be cycled. Focus outside that ring enters at the first window
    /// going forward and the last going backward.
    fn cycle_focus_by(
        &mut self,
        outputs: macos::DisplayOutputs,
        cancelled: &AtomicBool,
        backward: bool,
    ) -> Result<CycleStatus, String> {
        let (focused, workspace) = self.focused_owner_workspace(&outputs, cancelled)?;
        let windows = self.workspaces.windows(workspace);
        let len = windows.len();
        let target = match windows.iter().position(|window| window == &focused) {
            Some(_) if len < 2 => return Ok(self.status(0, &outputs)),
            Some(index) if backward => windows[(index + len - 1) % len].clone(),
            Some(index) => windows[(index + 1) % len].clone(),
            None => match if backward {
                windows.last()
            } else {
                windows.first()
            } {
                Some(target) => target.clone(),
                None => return Ok(self.status(0, &outputs)),
            },
        };
        self.focus_tile(Some(focused), &target, &outputs)?;
        Ok(self.status(0, &outputs))
    }

    fn focused_active_workspace(
        &mut self,
        outputs: &macos::DisplayOutputs,
        cancelled: &AtomicBool,
        refresh_if_unmanaged: bool,
    ) -> Result<(crate::PhysicalWindowId, WorkspaceId), String> {
        let window = self
            .current_focus()
            .ok_or("Focused window is unavailable")?;
        let workspace =
            self.active_workspace_of(&window, outputs, cancelled, refresh_if_unmanaged)?;
        Ok((window, workspace))
    }

    /// The visible workspace that owns the focused window, as a tile or a
    /// float, for commands that act on the whole workspace.
    fn focused_owner_workspace(
        &mut self,
        outputs: &macos::DisplayOutputs,
        cancelled: &AtomicBool,
    ) -> Result<(crate::PhysicalWindowId, WorkspaceId), String> {
        let window = self
            .current_focus()
            .ok_or("Focused window is unavailable")?;
        if self.workspaces.workspace_of_any(&window).is_none() {
            self.with_warp_context(WarpContext::Suppressed, |rt| {
                rt.cycle(outputs.clone(), cancelled)
            })?;
        }
        let workspace = self
            .workspaces
            .workspace_of_any(&window)
            .ok_or("Focused window is not managed by SpaceFred")?;
        if !self.is_visible(workspace, outputs) {
            return Err("Focused window is not on an active workspace".into());
        }
        Ok((window, workspace))
    }

    fn active_workspace_of(
        &mut self,
        window: &crate::PhysicalWindowId,
        outputs: &macos::DisplayOutputs,
        cancelled: &AtomicBool,
        refresh_if_unmanaged: bool,
    ) -> Result<WorkspaceId, String> {
        let workspace = match self.workspaces.workspace_of(window) {
            Some(workspace) => workspace,
            None if refresh_if_unmanaged => {
                self.with_warp_context(WarpContext::Suppressed, |rt| {
                    rt.cycle(outputs.clone(), cancelled)
                })?;
                self.workspaces
                    .workspace_of(window)
                    .ok_or("Focused window is not managed by SpaceFred")?
            }
            None => return Err("Focused window is not managed by SpaceFred".into()),
        };
        if !self.is_visible(workspace, outputs) {
            return Err("Focused window is not on an active workspace".into());
        }
        Ok(workspace)
    }

    fn remember_fullscreen_focus(
        &mut self,
        workspace: WorkspaceId,
        window: &crate::PhysicalWindowId,
    ) -> Result<(), String> {
        if !self.workspaces.uses_fullscreen_layout(workspace) {
            return Ok(());
        }
        self.pending_fullscreen_focus_restore.remove(&workspace);
        self.workspaces.remember_focus(workspace, window);
        self.persist_workspace_state_if_changed()
    }

    pub(super) fn toggle_layout(
        &mut self,
        outputs: macos::DisplayOutputs,
        cancelled: &AtomicBool,
    ) -> Result<CycleStatus, String> {
        let (_, workspace) = self.focused_owner_workspace(&outputs, cancelled)?;
        if self.workspaces.uses_fullscreen_layout(workspace) {
            self.workspaces.set_fullscreen_layout(workspace, false);
            self.pending_fullscreen_focus_restore.remove(&workspace);
        }
        self.workspaces.engine_mut(workspace).toggle_axis();
        let axis = self.workspaces.engine(workspace).root_axis();
        let mut status = self.relayout_persist(workspace, &outputs, cancelled)?;
        status.hud = Some(self.hud_for(workspace, UiHudKind::Layout { axis }, None, &outputs));
        Ok(status)
    }

    pub(super) fn toggle_fullscreen_layout(
        &mut self,
        outputs: macos::DisplayOutputs,
        cancelled: &AtomicBool,
    ) -> Result<CycleStatus, String> {
        let (window, workspace) = self.focused_owner_workspace(&outputs, cancelled)?;
        self.workspaces.remember_focus(workspace, &window);
        let enabled = !self.workspaces.uses_fullscreen_layout(workspace);
        self.workspaces.set_fullscreen_layout(workspace, enabled);
        self.pending_fullscreen_focus_restore.remove(&workspace);
        let mut status = self.relayout_persist(workspace, &outputs, cancelled)?;
        status.hud =
            Some(self.hud_for(workspace, UiHudKind::Fullscreen { enabled }, None, &outputs));
        Ok(status)
    }

    /// Float the focused window above its workspace's tiles, or tile it again.
    ///
    /// Floating is workspace ownership, not a rule: the window keeps its exact
    /// frame, follows its workspace across switches, parks and restores with
    /// it, and reconciliation leaves it alone until the user toggles it back.
    pub(super) fn toggle_float(
        &mut self,
        outputs: macos::DisplayOutputs,
        cancelled: &AtomicBool,
    ) -> Result<CycleStatus, String> {
        let window = self
            .current_focus()
            .ok_or("Focused window is unavailable")?;
        if self.workspaces.workspace_of_any(&window).is_none() {
            // A window observed for the first time may not be bound yet.
            self.cycle(outputs.clone(), cancelled)?;
        }
        let workspace = self
            .workspaces
            .workspace_of_any(&window)
            .ok_or("Focused window is not managed by SpaceFred")?;
        if !self.is_visible(workspace, &outputs) {
            return Err("Focused window is not on an active workspace".into());
        }
        if self.workspaces.workspace_of(&window).is_some() {
            self.float_window(window, workspace, outputs, cancelled)
        } else {
            self.tile_window(window, workspace, outputs, cancelled)
        }
    }

    fn float_window(
        &mut self,
        window: crate::PhysicalWindowId,
        workspace: WorkspaceId,
        outputs: macos::DisplayOutputs,
        cancelled: &AtomicBool,
    ) -> Result<CycleStatus, String> {
        // Detach from the tiles first so the remaining windows re-fill its
        // space on the very next layout. Focus and frame stay where they are.
        self.forget_window(workspace, &window);
        self.workspaces.own_floating(window.clone(), workspace);
        self.manual_floating.insert(window);
        self.relayout_persist(workspace, &outputs, cancelled)
    }

    fn tile_window(
        &mut self,
        window: crate::PhysicalWindowId,
        workspace: WorkspaceId,
        outputs: macos::DisplayOutputs,
        cancelled: &AtomicBool,
    ) -> Result<CycleStatus, String> {
        let was_manual = self.manual_floating.remove(&window);
        self.workspaces.forget_floating(&window);
        // Reconciliation adopts a window it observes but does not own, which is
        // exactly how a relaunched or moved window joins the tiles.
        let result = self.cycle(outputs, cancelled);
        if let Some(owner) = self.workspaces.workspace_of(&window) {
            if self.parked.contains_key(&window)
                && let Some(tile_frame) = self.applied[owner.index()]
                    .get(&window)
                    .map(|applied| applied.target)
                    .or_else(|| self.backend.frame(&window).ok())
            {
                let _: bool = self.record_recovery(&window, tile_frame);
            }
            return result;
        }
        self.workspaces.own_floating(window.clone(), workspace);
        if was_manual {
            self.manual_floating.insert(window);
        }
        match result {
            Err(error) => Err(error),
            Ok(_) => Err(concat!(
                "Focused window stays floating: a window rule or its ",
                "application keeps it out of the tiling"
            )
            .into()),
        }
    }

    fn relayout_persist(
        &mut self,
        workspace: WorkspaceId,
        outputs: &macos::DisplayOutputs,
        cancelled: &AtomicBool,
    ) -> Result<CycleStatus, String> {
        let layout = self.layout_workspace(
            workspace,
            output_for(&self.workspaces, workspace, outputs),
            cancelled,
        );
        self.persist_workspace_state_if_changed()?;
        Ok(self.status(layout.failed, outputs))
    }
}

/// Floating windows restored from the last run keep floating until toggled,
/// so an agent restart does not silently re-tile them. One a float rule
/// matches is handed back to the rule on its first observation.
pub(super) fn restored_floating(workspaces: &Workspaces) -> HashSet<crate::PhysicalWindowId> {
    workspaces
        .floating_entries()
        .map(|(window, _)| window.clone())
        .collect()
}

fn drop_direction(point: (i32, i32), frame: crate::Rect) -> Option<Direction> {
    let distances = [
        (point.0 - frame.x, Direction::Left, frame.width),
        (
            frame.x + frame.width - point.0,
            Direction::Right,
            frame.width,
        ),
        (point.1 - frame.y, Direction::Up, frame.height),
        (
            frame.y + frame.height - point.1,
            Direction::Down,
            frame.height,
        ),
    ];
    distances
        .into_iter()
        .filter(|(distance, _, extent)| *distance <= *extent / 4)
        .min_by_key(|(distance, _, _)| *distance)
        .map(|(_, direction, _)| direction)
}
