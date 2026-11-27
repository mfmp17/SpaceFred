//! #599: a tile closing and another opening in the same change must not
//! split the survivor by the fit shown before the close.
use std::collections::HashMap;

use super::super::LayoutTree;
use super::bound_slot;
use crate::{Axis, Rect, SlotId};

#[test]
fn dwindle_ignores_a_stale_fit_when_a_tile_closes_as_another_opens() {
    let output = Rect {
        x: 0,
        y: 30,
        width: 2560,
        height: 1410,
    };
    let dwindle = crate::config::LayoutMode::Dwindle;
    let mut tree = LayoutTree::default();
    let pair = [bound_slot(1), bound_slot(2)];
    assert!(tree.sync_near(Axis::Horizontal, &pair, None, dwindle, output));
    // A's minimum leaves B a narrow, portrait-shaped tile on display.
    let shown = tree
        .fitted(output, &HashMap::from([(SlotId(1), (1800, 0))]))
        .expect("A's minimum narrows B");
    let narrow = shown
        .placements(output)
        .into_iter()
        .find_map(|(slot, frame)| (slot == SlotId(2)).then_some(frame))
        .expect("B is shown");
    assert!(narrow.width < narrow.height, "{narrow:?}");

    // A closes and C opens in one change: B has the whole landscape canvas,
    // so C joins it side by side (#599).
    let next = [bound_slot(2), bound_slot(3)];
    assert!(tree.sync_shown(
        Axis::Horizontal,
        &next,
        Some(SlotId(2)),
        dwindle,
        output,
        Some(&shown),
    ));
    let half = |x| Rect {
        x,
        y: 30,
        width: 1280,
        height: 1410,
    };
    assert_eq!(
        tree.placements(output),
        vec![(SlotId(2), half(0)), (SlotId(3), half(1280))]
    );
}
