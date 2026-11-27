use super::super::input::PointerBounds;
use super::super::*;
use super::gesture::{GestureOverlay, OverlayKind, ShownDrop};

/// Depth inside the tiled area (in addition to any outer gap) of the display's
/// left and right full-height column drop bands, and of the top and bottom
/// full-width row bands on portrait displays (`[labs] portrait_edge_rows`).
pub(in crate::app) const DISPLAY_EDGE_DROP_BAND: i32 = 64;

#[derive(Clone, Debug)]
pub(in crate::app) enum DropTarget {
    /// Dock into a full-height half-width column on the display's left or right
    /// edge, or a full-width half-height row on a portrait display's top or bottom.
    DisplayEdge(Direction),
    /// Swap with or nest beside another tile on the same workspace.
    LocalTile {
        target: crate::PhysicalWindowId,
        direction: Option<Direction>,
    },
    /// Move to another display's active workspace.
    OtherWorkspace(WorkspaceId),
    /// The drag started in Fullscreen Layout; toggling Split mid-drag must not
    /// turn its release into an accidental drop.
    IgnoredFullscreen,
    /// Return to the pre-drag layout without editing it.
    None,
}

impl<B: WorkspaceBackend> WorkspaceRuntime<B> {
    #[cfg(test)]
    pub(in crate::app) fn drop_window(
        &mut self,
        window: crate::PhysicalWindowId,
        point: (i32, i32),
        preview: bool,
        outputs: macos::DisplayOutputs,
        cancelled: &AtomicBool,
    ) -> Result<CycleStatus, String> {
        self.drop_window_tracked(window, point, None, None, preview, outputs, cancelled)
    }

    #[allow(clippy::too_many_arguments)]
    pub(in crate::app) fn drop_window_tracked(
        &mut self,
        window: crate::PhysicalWindowId,
        point: (i32, i32),
        bounds: Option<PointerBounds>,
        pressed_at: Option<Instant>,
        preview: bool,
        outputs: macos::DisplayOutputs,
        cancelled: &AtomicBool,
    ) -> Result<CycleStatus, String> {
        let Some(source) = self.workspaces.workspace_of(&window) else {
            self.gesture = None;
            if !preview
                && let Some(moved) =
                    self.drop_floating_window(window.clone(), outputs.clone(), cancelled)
            {
                return moved;
            }
            return self.cycle(outputs, cancelled);
        };
        if !self.is_visible(source, &outputs) || self.parked.contains_key(&window) {
            self.gesture = None;
            return Ok(self.status(0, &outputs));
        }
        let source_output = output_for(&self.workspaces, source, &outputs);
        let marked_fullscreen = matches!(
            self.gesture.as_ref(),
            Some(GestureOverlay {
                window: dragged,
                kind: OverlayKind::Drag {
                    shown: Some((_, DropTarget::IgnoredFullscreen)),
                    ..
                },
                ..
            }) if *dragged == window
        );
        // Fullscreen Layout left after the button went down, possibly before
        // the worker saw any preview of this drag (#619).
        let started_in_fullscreen = marked_fullscreen
            || pressed_at
                .zip(self.fullscreen_left_at.get(&source))
                .is_some_and(|(pressed, left)| pressed <= *left);
        if self.workspaces.deck(source).is_monocle() || started_in_fullscreen {
            // Fullscreen Layout ignores pointer drags: remember that the drag
            // started in Fullscreen so toggling back to Split mid-drag never
            // commits an accidental drop, and on release restore the tile
            // wherever it was let go (#392).
            if preview {
                if !marked_fullscreen {
                    self.gesture = Some(GestureOverlay {
                        window,
                        workspace: source,
                        kind: OverlayKind::Drag {
                            layout: self.workspaces.engine(source).layout().clone(),
                            pointer_bounds: bounds,
                            shown: Some((point, DropTarget::IgnoredFullscreen)),
                        },
                    });
                }
                return Ok(self.status(0, &outputs));
            }
            self.gesture = None;
            self.forget_readback(source, &window);
            // A tile let go a point or two from its frame is still inside the
            // write tolerance, so restore it exactly (#613).
            self.exact_spacing_workspaces.insert(source);
            let layout = self.layout_workspace(source, source_output, cancelled);
            return Ok(self.status(layout.failed, &outputs));
        }
        self.forget_readback(source, &window);
        if let Some(left) = self.moved_mid_drag(&window, source, point, bounds) {
            return self.settle_moved_mid_drag(left, source, preview, &outputs, cancelled);
        }

        let (pre_drag_layout, effective_bounds, shown) = self.drag_overlay(&window, source, bounds);
        let source_settings = self.layout_settings(source_output);
        // Released where the last preview was shown, with the pointer nowhere
        // new since, the drop is what that preview showed, even if settings
        // changed in between (#390). A destination workspace that stopped
        // being shown on the display under the pointer while the button was
        // held no longer stands, whether it was hidden (#550) or moved to
        // another display (#605).
        let shown = shown.filter(|(at, target)| {
            *at == point
                && match target {
                    DropTarget::OtherWorkspace(workspace) => {
                        self.shown_at(point, &outputs) == Some(*workspace)
                    }
                    _ => true,
                }
        });
        let target = match shown {
            Some((_, target)) => target,
            None => self.resolve_drop_target(
                &window,
                source,
                point,
                source_output,
                source_settings.usable(),
                effective_bounds,
                &outputs,
            ),
        };
        if preview {
            self.gesture = Some(GestureOverlay {
                window: window.clone(),
                workspace: source,
                kind: OverlayKind::Drag {
                    layout: pre_drag_layout.clone(),
                    pointer_bounds: effective_bounds,
                    shown: Some((point, target.clone())),
                },
            });
        }

        match target {
            DropTarget::OtherWorkspace(target) => {
                if preview {
                    let failed =
                        self.preview_saved_layout(source, &source_settings, &window, cancelled);
                    return Ok(self.status(failed, &outputs));
                }
                self.move_window(window, target, outputs, cancelled)
            }
            DropTarget::DisplayEdge(direction) => {
                let mut candidate = pre_drag_layout.clone();
                let changed = self
                    .workspaces
                    .engine(source)
                    .slot_for_window(&window)
                    .is_some_and(|slot| candidate.create_implicit_split(slot, direction));
                self.finish_local_drop(
                    source,
                    &window,
                    changed.then_some(candidate),
                    preview,
                    source_output,
                    &source_settings,
                    &outputs,
                    cancelled,
                )
            }
            DropTarget::LocalTile { target, direction } => {
                let mut candidate = pre_drag_layout.clone();
                let engine = self.workspaces.engine(source);
                let changed = match (
                    engine.slot_for_window(&window),
                    engine.slot_for_window(&target),
                ) {
                    (Some(moved), Some(target_slot)) => match direction {
                        Some(direction) => candidate.move_relative(moved, target_slot, direction),
                        None => candidate.swap(moved, target_slot),
                    },
                    _ => false,
                };
                self.finish_local_drop(
                    source,
                    &window,
                    changed.then_some(candidate),
                    preview,
                    source_output,
                    &source_settings,
                    &outputs,
                    cancelled,
                )
            }
            DropTarget::None | DropTarget::IgnoredFullscreen => self.finish_local_drop(
                source,
                &window,
                None,
                preview,
                source_output,
                &source_settings,
                &outputs,
                cancelled,
            ),
        }
    }

    /// The workspace the dragged window was previewed on, when a command moved
    /// it to `source` while the button was held and the pointer has gone
    /// nowhere new since that preview. Its old target belongs to the workspace
    /// it left, so it must not be classified afresh into a move back (#528).
    fn moved_mid_drag(
        &self,
        window: &crate::PhysicalWindowId,
        source: WorkspaceId,
        point: (i32, i32),
        bounds: Option<PointerBounds>,
    ) -> Option<WorkspaceId> {
        let GestureOverlay {
            window: dragged,
            workspace,
            kind:
                OverlayKind::Drag {
                    pointer_bounds,
                    shown: Some((at, _)),
                    ..
                },
        } = self.gesture.as_ref()?
        else {
            return None;
        };
        let mut merged = *pointer_bounds;
        if let Some(bounds) = bounds {
            match &mut merged {
                Some(existing) => existing.merge(bounds),
                None => merged = Some(bounds),
            }
        }
        (dragged == window && *workspace != source && *at == point && merged == *pointer_bounds)
            .then_some(*workspace)
    }

    /// Keep the command's move: a preview waits, and the release lays out the
    /// workspace the window left and the one it joined.
    fn settle_moved_mid_drag(
        &mut self,
        left: WorkspaceId,
        source: WorkspaceId,
        preview: bool,
        outputs: &macos::DisplayOutputs,
        cancelled: &AtomicBool,
    ) -> Result<CycleStatus, String> {
        if preview {
            return Ok(self.status(0, outputs));
        }
        self.gesture = None;
        let mut failed = 0;
        if self.is_visible(left, outputs) {
            let output = output_for(&self.workspaces, left, outputs);
            failed += self.layout_workspace(left, output, cancelled).failed;
        }
        let output = output_for(&self.workspaces, source, outputs);
        failed += self.layout_workspace(source, output, cancelled).failed;
        self.persist_workspace_state_if_changed()?;
        Ok(self.status(failed, outputs))
    }

    /// The drag's layout at its start, the pointer's extent so far, and the
    /// target its last preview showed. A drag of another window starts over,
    /// and so does one whose workspace layout an earlier command already edited
    /// (a layout toggle while the button was held): the drop applies to that
    /// edited layout and is classified afresh instead of replaying a preview
    /// shown against the old one (#524).
    fn drag_overlay(
        &mut self,
        window: &crate::PhysicalWindowId,
        source: WorkspaceId,
        bounds: Option<PointerBounds>,
    ) -> (
        crate::layout_tree::LayoutTree,
        Option<PointerBounds>,
        Option<ShownDrop>,
    ) {
        match self.gesture.take() {
            Some(GestureOverlay {
                window: dragged,
                workspace,
                kind:
                    OverlayKind::Drag {
                        layout,
                        mut pointer_bounds,
                        shown,
                    },
            }) if dragged == *window && workspace == source => {
                let shown_with = pointer_bounds;
                if let Some(bounds) = bounds {
                    match &mut pointer_bounds {
                        Some(existing) => existing.merge(bounds),
                        None => pointer_bounds = Some(bounds),
                    }
                }
                // A pointer that has been somewhere new since that preview (out of
                // an edge band and back while the worker was busy) may have armed a
                // different target, so the shown one no longer stands (#495).
                let shown = shown.filter(|_| pointer_bounds == shown_with);
                let current = self.workspaces.engine(source).layout();
                if *current != layout {
                    return (current.clone(), pointer_bounds, None);
                }
                (layout, pointer_bounds, shown)
            }
            _ => (
                self.workspaces.engine(source).layout().clone(),
                bounds,
                None,
            ),
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn resolve_drop_target(
        &self,
        window: &crate::PhysicalWindowId,
        source: WorkspaceId,
        point: (i32, i32),
        source_output: crate::Rect,
        usable: crate::Rect,
        bounds: Option<PointerBounds>,
        outputs: &macos::DisplayOutputs,
    ) -> DropTarget {
        let rows = self.config.labs().portrait_edge_rows;
        // With row bands, a side band also covers the menu bar and Dock strips at its
        // ends, so a pointer that only crossed into them has not left the side (#612).
        let strips =
            (rows && usable.height > usable.width).then(|| display_screen(source_output, outputs));
        if let Some(direction) = display_edge_drop(point, source_output, usable, bounds, strips) {
            return DropTarget::DisplayEdge(direction);
        }
        if rows
            && let Some(direction) = display_row_drop(point, source_output, usable, bounds, outputs)
        {
            return DropTarget::DisplayEdge(direction);
        }
        if let Some(tile) = self
            .navigation_tiles(outputs)
            .into_iter()
            .find(|tile| tile.window != *window && tile.frame.contains(point.0, point.1))
        {
            return if tile.workspace != source {
                DropTarget::OtherWorkspace(tile.workspace)
            } else {
                DropTarget::LocalTile {
                    direction: drop_direction(point, tile.frame),
                    target: tile.window,
                }
            };
        }
        if let Some(target) = self.shown_at(point, outputs)
            && target != source
        {
            return DropTarget::OtherWorkspace(target);
        }
        DropTarget::None
    }

    /// The workspace shown on the display under `point`, if any.
    pub(in crate::app) fn shown_at(
        &self,
        point: (i32, i32),
        outputs: &macos::DisplayOutputs,
    ) -> Option<WorkspaceId> {
        let display = outputs.display_at(point)?;
        self.visible_workspaces(outputs)
            .into_iter()
            .find(|resolved| resolved.display_id == display.id)
            .map(|resolved| resolved.workspace)
    }

    #[allow(clippy::too_many_arguments)]
    fn finish_local_drop(
        &mut self,
        source: WorkspaceId,
        window: &crate::PhysicalWindowId,
        candidate: Option<crate::layout_tree::LayoutTree>,
        preview: bool,
        source_output: crate::Rect,
        source_settings: &Settings,
        outputs: &macos::DisplayOutputs,
        cancelled: &AtomicBool,
    ) -> Result<CycleStatus, String> {
        if preview {
            let failed = match candidate {
                Some(candidate) => {
                    let placements = self.workspaces.engine(source).placements_for_tree(
                        &candidate,
                        source_settings,
                        &self.minimum_sizes,
                    );
                    self.apply_gesture_preview(
                        placements,
                        source,
                        source_settings,
                        window,
                        cancelled,
                    )
                }
                None => self.preview_saved_layout(source, source_settings, window, cancelled),
            };
            return Ok(self.status(failed, outputs));
        }
        if let Some(candidate) = candidate {
            self.workspaces
                .engine_mut(source)
                .commit_layout_tree(candidate);
        }
        self.note_tab_selected_mid_drag(source, window);
        let layout = self.layout_workspace(source, source_output, cancelled);
        self.persist_workspace_state_if_changed()?;
        Ok(self.status(layout.failed, outputs))
    }

    /// A native tab selected while the button was held: the incoming tab is
    /// where the drag left the group, and no cycle ran to see the outgoing one
    /// there. Remember that frame as the dragged tile's last observed one, so
    /// the next cycle hands the incoming tab the tile and its committed size
    /// instead of a new equal split (#750).
    fn note_tab_selected_mid_drag(
        &mut self,
        workspace: WorkspaceId,
        dragged: &crate::PhysicalWindowId,
    ) {
        let Some(incoming) = self.backend.focused_window().filter(|focused| {
            focused.app == dragged.app
                && focused != dragged
                && self.workspaces.workspace_of_any(focused).is_none()
        }) else {
            return;
        };
        let Some(frame) = self.backend.window_server_frame(&incoming) else {
            return;
        };
        let engine = self.workspaces.engine_mut(workspace);
        if let Some(slot) = engine.slot_for_window(dragged) {
            engine.record_observed_frame(slot, frame);
        }
    }

    /// Show the siblings where the saved layout puts them, a minimum-size fit
    /// and any resize of it included (#388).
    fn preview_saved_layout(
        &mut self,
        workspace: WorkspaceId,
        settings: &Settings,
        dragged: &crate::PhysicalWindowId,
        cancelled: &AtomicBool,
    ) -> usize {
        let placements = self.workspaces.engine(workspace).placements(settings);
        self.apply_gesture_preview(placements, workspace, settings, dragged, cancelled)
    }
}

/// Left and right full-height edge bands on `output`. Each band runs the full
/// height of `output`, includes the outer gap on that edge, and lies strictly
/// inside `output`.
pub(in crate::app) fn display_edge_bands(
    output: crate::Rect,
    usable: crate::Rect,
) -> (crate::Rect, crate::Rect) {
    let inset = DISPLAY_EDGE_DROP_BAND.min(usable.width / 6).max(0);
    let left_end = (usable.x + inset).clamp(output.x, output.x + output.width);
    let right_start = (usable.x + usable.width - inset).clamp(output.x, output.x + output.width);
    let left = crate::Rect {
        x: output.x,
        y: output.y,
        width: left_end - output.x,
        height: output.height,
    };
    let right = crate::Rect {
        x: right_start,
        y: output.y,
        width: output.x + output.width - right_start,
        height: output.height,
    };
    (left, right)
}

/// The full bounds of the display whose tiled area is `output`, or `output` itself.
fn display_screen(output: crate::Rect, outputs: &macos::DisplayOutputs) -> crate::Rect {
    outputs
        .displays()
        .iter()
        .zip(outputs.screens())
        .find(|(display, _)| display.frame == output)
        .map_or(output, |(_, screen)| screen)
}

/// `strips`, when given, is the display's full bounds: a side band then arms only once
/// the pointer has left it together with the menu bar and Dock strips at its ends.
fn display_edge_drop(
    point: (i32, i32),
    output: crate::Rect,
    usable: crate::Rect,
    bounds: Option<PointerBounds>,
    strips: Option<crate::Rect>,
) -> Option<Direction> {
    if !output.contains(point.0, point.1) {
        return None;
    }
    let (left, right) = display_edge_bands(output, usable);
    let arming = |side: crate::Rect| {
        strips.map_or(side, |screen| {
            let y = screen.y.min(side.y);
            let end = (screen.y + screen.height).max(side.y + side.height);
            crate::Rect {
                y,
                height: end - y,
                ..side
            }
        })
    };
    for (side, direction) in [(left, Direction::Left), (right, Direction::Right)] {
        if side.contains(point.0, point.1) {
            let armed = bounds.is_none_or(|bounds| !bounds.entirely_inside(arming(side)));
            return armed.then_some(direction);
        }
    }
    None
}

/// Top and bottom full-width edge bands on a portrait `output`, `None` when the
/// tiled area is not taller than it is wide. Each band reaches `DISPLAY_EDGE_DROP_BAND`
/// into the tiled area, includes the outer gap on that edge, and extends over the
/// menu bar or Dock strip between `output` and the edge of `screen` (the display's
/// full bounds). A corner belongs to the side band, armed or not, in the strips too.
pub(in crate::app) fn display_row_bands(
    output: crate::Rect,
    usable: crate::Rect,
    screen: crate::Rect,
) -> Option<(crate::Rect, crate::Rect)> {
    if usable.height <= usable.width {
        return None;
    }
    let inset = DISPLAY_EDGE_DROP_BAND.min(usable.height / 6).max(0);
    let output_bottom = output.y + output.height;
    let top_end = (usable.y + inset).clamp(output.y, output_bottom);
    let bottom_start = (usable.y + usable.height - inset).clamp(output.y, output_bottom);
    let top_start = screen.y.min(output.y);
    let bottom_end = (screen.y + screen.height).max(output_bottom);
    let left = screen.x.min(output.x);
    let width = (screen.x + screen.width).max(output.x + output.width) - left;
    let top = crate::Rect {
        x: left,
        y: top_start,
        width,
        height: top_end - top_start,
    };
    let bottom = crate::Rect {
        x: left,
        y: bottom_start,
        width,
        height: bottom_end - bottom_start,
    };
    Some((top, bottom))
}

fn display_row_drop(
    point: (i32, i32),
    output: crate::Rect,
    usable: crate::Rect,
    bounds: Option<PointerBounds>,
    outputs: &macos::DisplayOutputs,
) -> Option<Direction> {
    // A corner belongs to the side band even before that band is armed.
    let (left, right) = display_edge_bands(output, usable);
    if left.contains(point.0, point.1) || right.contains(point.0, point.1) {
        return None;
    }
    let screen = display_screen(output, outputs);
    let (top, bottom) = display_row_bands(output, usable, screen)?;
    let (band, row) = [(top, Direction::Up), (bottom, Direction::Down)]
        .into_iter()
        .find(|(band, _)| band.contains(point.0, point.1))?;
    // Past `output`, a row band's ends over the menu bar or Dock strip are corners
    // too, and belong to the side band (#504). A side Dock strip outside the row
    // bands stays no band at all.
    let corner = |from: i32, to: i32| crate::Rect {
        x: from,
        y: band.y,
        width: to - from,
        height: band.height,
    };
    // A corner arms with its side band: a pointer that has only been in the two
    // has not left the side.
    let with_side = |corner: crate::Rect, side: crate::Rect| {
        let y = corner.y.min(side.y);
        let end = (corner.y + corner.height).max(side.y + side.height);
        crate::Rect {
            height: end - y,
            y,
            ..corner
        }
    };
    let (arming, direction) = [
        (corner(band.x, left.x + left.width), left, Direction::Left),
        (
            corner(right.x, band.x + band.width),
            right,
            Direction::Right,
        ),
    ]
    .into_iter()
    .find(|(end, _, _)| end.contains(point.0, point.1))
    .map_or((band, row), |(end, side, direction)| {
        (with_side(end, side), direction)
    });
    let armed = bounds.is_none_or(|bounds| !bounds.entirely_inside(arming));
    armed.then_some(direction)
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
