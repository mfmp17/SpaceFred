use super::*;

/// Size difference, in points, below which a constrained readback is noise.
const MINIMUM_TOLERANCE: i32 = 2;
/// Refitting can shrink a sibling below a minimum nobody knew yet, which the
/// next write reveals. Each round learns at least one window's minimum, so a
/// row settles within one round per tile; a bound keeps it finite (#413).
const MIN_REFITS: usize = 3;

/// Maximum minimum-learning rounds for a workspace with `tiles` bound windows:
/// each round learns at least one window's minimum, so a row settles within one
/// round per tile (#413, #492).
pub(super) fn learning_rounds(tiles: usize) -> usize {
    MIN_REFITS.max(tiles + 1)
}

impl<B: WorkspaceBackend> WorkspaceRuntime<B> {
    /// Refit and re-apply while writes keep revealing refused sizes. Returns
    /// the status of the last re-apply, or `None` when nothing was refitted.
    pub(super) fn refit_minimum_sizes(
        &mut self,
        workspace: WorkspaceId,
        output: crate::Rect,
        results: Vec<macos::ApplyResult>,
        cancelled: &AtomicBool,
    ) -> Option<LayoutStatus> {
        let mut results = results;
        let mut status = None;
        let tiles = self.workspaces.engine(workspace).bound_windows().len();
        let was_exact = self.exact_spacing_workspaces.contains(&workspace);
        let mut deferred = false;
        for _ in 0..learning_rounds(tiles) {
            if !self.fit_minimum_sizes(workspace, output, &results) {
                break;
            }
            // A refit moves seams by a point or two, so a tile an earlier
            // round already wrote can sit inside the write tolerance of its
            // new target while its neighbor moves: write the refit exactly so
            // seams meet (#629).
            self.exact_spacing_workspaces.insert(workspace);
            let (applied, next) = self.apply_layout(workspace, output, None, cancelled);
            deferred = next
                .iter()
                .any(|result| matches!(result.outcome, macos::ApplyOutcome::Deferred));
            status = Some(applied);
            results = next;
        }
        if !was_exact && !deferred {
            self.exact_spacing_workspaces.remove(&workspace);
        }
        status
    }

    /// Grow the tiles of windows that refused to shrink to their target, so
    /// they stop overlapping their neighbors. A refused size is the window's
    /// minimum; siblings give up the space in proportion to what they can
    /// spare above their own known minimums. Returns whether the layout changed.
    fn fit_minimum_sizes(
        &mut self,
        workspace: WorkspaceId,
        output: crate::Rect,
        results: &[macos::ApplyResult],
    ) -> bool {
        if self.workspaces.deck(workspace).is_monocle() {
            return false;
        }
        let mut refused = false;
        for result in results {
            let (macos::ApplyOutcome::Constrained, Some(actual)) = (result.outcome, result.actual)
            else {
                continue;
            };
            let target = result.target;
            let wider =
                actual.width > target.width + MINIMUM_TOLERANCE && actual.width <= output.width;
            let taller =
                actual.height > target.height + MINIMUM_TOLERANCE && actual.height <= output.height;
            if !wider && !taller {
                continue;
            }
            let minimum = self
                .minimum_sizes
                .entry(result.window.clone())
                .or_insert((0, 0));
            if wider {
                minimum.0 = minimum.0.max(actual.width);
            }
            if taller {
                minimum.1 = minimum.1.max(actual.height);
            }
            refused = true;
        }
        if !refused {
            return false;
        }
        let settings = self.layout_settings(output);
        let engine = self.workspaces.engine_mut(workspace);
        if !engine.fit_minimums(&settings, &self.minimum_sizes) {
            log_event!(
                "minimum size: workspace {} cannot fit its windows' minimum sizes",
                workspace
            );
            return false;
        }
        for placement in engine.placements(&settings) {
            engine.record_placement_target(placement.slot, placement.frame);
        }
        log_event!(
            "minimum size: refit workspace {} around windows that refused their tile size",
            workspace
        );
        true
    }
}
