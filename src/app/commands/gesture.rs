//! A pointer gesture in progress. Drag and resize previews are evaluated over
//! the workspace's saved layout and written to the sibling tiles only; the
//! engine's layout changes once, on mouse-up. A gesture that ends any other way
//! (pause, a lost mouse-up sample, another command) puts the saved layout back.

use super::super::input::PointerBounds;
use super::super::*;
use super::drag::DropTarget;

/// The pointer location a drag preview was shown at, and the target it showed.
pub(in crate::app) type ShownDrop = ((i32, i32), DropTarget);

#[derive(Clone, Debug)]
pub(in crate::app) struct GestureOverlay {
    pub(in crate::app) window: crate::PhysicalWindowId,
    pub(in crate::app) workspace: WorkspaceId,
    pub(in crate::app) kind: OverlayKind,
}

#[derive(Clone, Debug)]
pub(in crate::app) enum OverlayKind {
    /// A title-bar drag: the layout when it began, where the pointer has been,
    /// and the drop target its last preview showed at which point.
    Drag {
        layout: crate::layout_tree::LayoutTree,
        pointer_bounds: Option<PointerBounds>,
        shown: Option<ShownDrop>,
    },
    /// A border drag, previewed by resizing a copy of the engine.
    Resize,
}

impl<B: WorkspaceBackend> WorkspaceRuntime<B> {
    /// End a gesture that will get no mouse-up commit: write the workspace's
    /// saved layout back over its previewed siblings. With the button still
    /// held (pause mid-drag) the dragged window is left to the user.
    pub(in crate::app) fn abandon_gesture(
        &mut self,
        outputs: Option<&macos::DisplayOutputs>,
        cancelled: &AtomicBool,
    ) {
        let Some(overlay) = self.gesture.take() else {
            return;
        };
        let Some(outputs) = outputs.filter(|outputs| self.is_visible(overlay.workspace, outputs))
        else {
            return;
        };
        let output = output_for(&self.workspaces, overlay.workspace, outputs);
        if self.backend.mouse_down() {
            let settings = self.layout_settings(output);
            let mut placements = self
                .workspaces
                .engine(overlay.workspace)
                .placements(&settings);
            self.workspaces
                .deck(overlay.workspace)
                .place(&mut placements, settings.single());
            let _ = self.backend.apply_managed_during_drag(
                &placements,
                &settings,
                &overlay.window,
                cancelled,
            );
            return;
        }
        log_event!(
            "gesture: abandoned {:?} on workspace {}; restoring its layout",
            overlay.window,
            overlay.workspace
        );
        self.forget_readback(overlay.workspace, &overlay.window);
        let _ = self.layout_workspace(overlay.workspace, output, cancelled);
    }

    /// The window's own frame is the user's now, not a refused write.
    pub(in crate::app) fn forget_readback(
        &mut self,
        workspace: WorkspaceId,
        window: &crate::PhysicalWindowId,
    ) {
        self.constrained[workspace.index()].remove(window);
        self.applied[workspace.index()].remove(window);
    }

    /// Write `placements` to every tile but the one under the pointer, without
    /// recording them as targets: a preview must not read back as a bounce.
    pub(in crate::app) fn apply_gesture_preview(
        &mut self,
        mut placements: Vec<crate::Placement>,
        workspace: WorkspaceId,
        settings: &Settings,
        dragged: &crate::PhysicalWindowId,
        cancelled: &AtomicBool,
    ) -> usize {
        self.workspaces
            .deck(workspace)
            .place(&mut placements, settings.single());
        let results =
            self.backend
                .apply_managed_during_drag(&placements, settings, dragged, cancelled);
        failed_results(&results)
    }

    pub(in crate::app) fn resize_window(
        &mut self,
        window: crate::PhysicalWindowId,
        frame: crate::Rect,
        preview: bool,
        outputs: macos::DisplayOutputs,
        cancelled: &AtomicBool,
    ) -> Result<CycleStatus, String> {
        let Some(workspace) = self.workspaces.workspace_of(&window) else {
            self.gesture = None;
            return self.cycle(outputs, cancelled);
        };
        if !self.is_visible(workspace, &outputs) || self.parked.contains_key(&window) {
            self.gesture = None;
            return Ok(self.status(0, &outputs));
        }
        let output = output_for(&self.workspaces, workspace, &outputs);
        let settings = self.layout_settings(output);
        if self.workspaces.deck(workspace).is_monocle() {
            // Fullscreen Layout has no split to resize: the tile keeps its
            // display-sized frame once the button is released.
            self.gesture = None;
            if preview {
                return Ok(self.status(0, &outputs));
            }
            self.forget_readback(workspace, &window);
            self.exact_spacing_workspaces.insert(workspace);
            let layout = self.layout_workspace(workspace, output, cancelled);
            return Ok(self.status(layout.failed, &outputs));
        }
        if preview {
            self.gesture = Some(GestureOverlay {
                window: window.clone(),
                workspace,
                kind: OverlayKind::Resize,
            });
            // Every preview reads the frame against the saved layout, so
            // siblings follow the edge without the split moving under it.
            let mut engine = self.workspaces.engine(workspace).clone();
            let _: bool = engine.resize_window(&window, frame, &settings, &self.minimum_sizes);
            engine.fit_minimums(&settings, &self.minimum_sizes);
            let placements = engine.placements(&settings);
            let failed =
                self.apply_gesture_preview(placements, workspace, &settings, &window, cancelled);
            return Ok(self.status(failed, &outputs));
        }
        // Mouse-up: one edit against the saved layout, however the previews went (#387).
        self.gesture = None;
        self.forget_readback(workspace, &window);
        // The user sized this window by hand, so it may sit a point or two off
        // its tile; the next retile writes the workspace exactly (#506).
        self.exact_spacing_workspaces.insert(workspace);
        let changed = self.workspaces.engine_mut(workspace).resize_window(
            &window,
            frame,
            &settings,
            &self.minimum_sizes,
        );
        let failed = if changed {
            self.layout_workspace_during_drag(workspace, output, Some(&window), cancelled)
                .failed
        } else {
            0
        };
        self.persist_workspace_state_if_changed()?;
        Ok(self.status(failed, &outputs))
    }
}
