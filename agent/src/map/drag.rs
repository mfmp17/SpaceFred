//! Dragging an app on the Rules map: from the press, through the slop that tells a drag from a
//! click, to the drop that rewrites its rule.

use super::{Layout, SLOP, TILE_H};
use crate::{
    Ui,
    rules::{Doc, Place, Rule},
};

/// An app being dragged: its rule, where it started and how far the pointer has gone.
#[derive(Clone, Debug, PartialEq)]
pub struct Drag {
    pub rule: usize,
    pub(super) snapshot: Rule,
    /// The dragged item's top-left in the map, and its size.
    pub(super) origin: (f32, f32),
    pub(super) size: (f32, f32),
    /// Where in the item the press landed.
    pub(super) grab: (f32, f32),
    pub(super) delta: (f32, f32),
    pub(super) moved: bool,
    pub(super) target: Option<Place>,
}

impl Drag {
    /// Slack mid-drag over workspace 4, for stills.
    pub(super) fn fixture(doc: &Doc, width: f32) -> Option<Drag> {
        let layout = Layout::new(doc, width);
        let rule = doc
            .rules()
            .into_iter()
            .find(|rule| rule.bundle_id.as_deref() == Some("com.tinyspeck.slackmacgap"))?;
        let (origin, size) = layout.spot(&rule)?;
        let grab = (size.0 / 2.0, size.1 / 2.0);
        let to = layout.grid.tile_origin(3);
        let tile_w = layout.grid.tile_w();
        let delta = (
            to.0 + tile_w / 2.0 - origin.0 - grab.0,
            to.1 + TILE_H / 2.0 - origin.1 - grab.1,
        );
        Some(Drag {
            rule: rule.index,
            snapshot: rule,
            origin,
            size,
            grab,
            delta,
            moved: true,
            target: layout
                .grid
                .target_at(to.0 + tile_w / 2.0, to.1 + TILE_H / 2.0),
        })
    }
}

pub(super) fn drag(
    ui: &mut Ui,
    d: &rune::Drag,
    rule: &Rule,
    origin: (f32, f32),
    size: (f32, f32),
    target_at: impl Fn(f32, f32) -> Option<Place>,
) {
    drag_step(
        ui,
        d.phase,
        (d.start_x, d.start_y),
        (d.dx, d.dy),
        d.cancelled(),
        rule,
        origin,
        size,
        target_at,
    );
}

#[allow(clippy::too_many_arguments)]
pub(super) fn drag_step(
    ui: &mut Ui,
    phase: rune::Phase,
    start: (f32, f32),
    delta: (f32, f32),
    cancelled: bool,
    rule: &Rule,
    origin: (f32, f32),
    size: (f32, f32),
    target_at: impl Fn(f32, f32) -> Option<Place>,
) {
    match phase {
        rune::Phase::Down => {
            ui.menu = None;
            ui.drag = Some(Drag {
                rule: rule.index,
                snapshot: rule.clone(),
                origin,
                size,
                grab: start,
                delta: (0.0, 0.0),
                moved: false,
                target: None,
            });
        }
        rune::Phase::Move => {
            let Some(drag) = ui.drag.as_mut().filter(|drag| drag.rule == rule.index) else {
                return;
            };
            drag.delta = delta;
            drag.moved |= delta.0.abs().max(delta.1.abs()) >= SLOP;
            drag.target = target_at(
                origin.0 + drag.grab.0 + delta.0,
                origin.1 + drag.grab.1 + delta.1,
            );
        }
        rune::Phase::Up => {
            let Some(drag) = ui.drag.take().filter(|drag| drag.rule == rule.index) else {
                return;
            };
            if cancelled || !drag.moved {
                ui.flush_pending_reload();
                return;
            }
            let Some(target) = drag.target else {
                ui.flush_pending_reload();
                return;
            };
            let current = drag.snapshot.place;
            let unchanged = current == target
                || (target == Place::Float && matches!(current, Place::Ignore | Place::Pin));
            if unchanged {
                ui.flush_pending_reload();
            } else {
                ui.try_edit(|doc| doc.place_matching(&drag.snapshot, target));
            }
        }
    }
}
