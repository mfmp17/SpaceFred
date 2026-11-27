use super::*;

#[derive(Clone, Debug, serde::Serialize)]
pub(in crate::app) struct RuntimeDebug {
    frontmost: Option<AppInstanceId>,
    focused: Option<crate::PhysicalWindowId>,
    active_workspaces: Vec<WorkspaceId>,
    displays: Vec<DebugDisplay>,
    tiles: Vec<NavigationTile>,
    floating: Vec<DebugFloatingWindow>,
    /// Managed windows left visible across workspace switches because they
    /// repeatedly reject parking positions.
    parking_exempt: Vec<crate::PhysicalWindowId>,
    targets: DebugTargets,
}

#[derive(Clone, Debug, serde::Serialize)]
struct DebugFloatingWindow {
    window: crate::PhysicalWindowId,
    workspace: WorkspaceId,
    /// Present exactly when the window is parked off-screen.
    parked_original_frame: Option<crate::Rect>,
}

#[derive(Clone, Debug, serde::Serialize)]
struct DebugDisplay {
    id: String,
    name: String,
    frame: crate::Rect,
    workspace: WorkspaceId,
    axis: crate::Axis,
    fullscreen_layout: bool,
    layout: Vec<crate::Rect>,
}

#[derive(Clone, Debug, serde::Serialize)]
struct DebugTargets {
    left: Option<crate::PhysicalWindowId>,
    down: Option<crate::PhysicalWindowId>,
    up: Option<crate::PhysicalWindowId>,
    right: Option<crate::PhysicalWindowId>,
}

impl<B: WorkspaceBackend> WorkspaceRuntime<B> {
    /// The result of a restore, carrying the debug state it leaves behind on
    /// the last known displays.
    pub(in crate::app) fn restored(&mut self, failed: usize) -> WorkerResult {
        let debug = self
            .displays
            .known_outputs()
            .cloned()
            .map(|outputs| Box::new(self.debug_state(&outputs)));
        WorkerResult::Restored { failed, debug }
    }

    pub(in crate::app) fn debug_state(&mut self, outputs: &macos::DisplayOutputs) -> RuntimeDebug {
        let visible = self.visible_workspaces(outputs);
        let tiles = self.navigation_tiles_for(&visible);
        let focused = self.backend.focused_window();
        // The same resolver the focus command runs, from the same effective
        // focus, so these never drift from what the keys do (#382).
        let origin = self
            .effective_focus(focused.clone())
            .filter(|_| !self.empty_choice_stands(focused.as_ref()));
        let mut target = |direction| self.focus_target(origin.as_ref(), direction, outputs);
        let targets = DebugTargets {
            left: target(Direction::Left),
            down: target(Direction::Down),
            up: target(Direction::Up),
            right: target(Direction::Right),
        };
        let floating = self
            .workspaces
            .floating_entries()
            .map(|(window, workspace)| DebugFloatingWindow {
                parked_original_frame: self.parked.get(window).copied(),
                window: window.clone(),
                workspace,
            })
            .collect();
        RuntimeDebug {
            frontmost: self.backend.frontmost_application(),
            focused,
            active_workspaces: visible.iter().map(|resolved| resolved.workspace).collect(),
            displays: visible
                .into_iter()
                .filter_map(|resolved| {
                    let display = outputs
                        .displays()
                        .iter()
                        .find(|display| display.id == resolved.display_id)?;
                    Some(DebugDisplay {
                        id: display.id.clone(),
                        name: display.name.clone(),
                        frame: display.frame,
                        workspace: resolved.workspace,
                        axis: self.workspaces.engine(resolved.workspace).root_axis(),
                        fullscreen_layout: self.workspaces.deck(resolved.workspace).is_monocle(),
                        layout: self.preview_layout(resolved.workspace, HUD_PREVIEW_RECT),
                    })
                })
                .collect(),
            tiles,
            floating,
            parking_exempt: {
                let mut windows: Vec<_> = self.parked.exempt_windows().cloned().collect();
                windows.sort_by_key(|window| (window.app.pid, window.window_id));
                windows
            },
            targets,
        }
    }
}
