//! #611: a user-arranged split nested inside another keeps its direction
//! when it is left with one tile and another joins it in Dwindle.
use super::super::LayoutTree;
use super::bound_slot;
use crate::config::LayoutMode;
use crate::{Axis, Direction, Rect, ResizeDirection, SlotId};

const OUTPUT: Rect = Rect {
    x: 0,
    y: 0,
    width: 2560,
    height: 1410,
};

/// A narrowed to 500 beside a user-arranged B-over-C stack, so the stack's
/// region (2060x1410) is wider than tall and its longer side is horizontal;
/// then C closes in Dwindle and B fills that region.
fn stack_after_close() -> LayoutTree {
    let mut tree = LayoutTree::weighted_flat(
        Axis::Horizontal,
        [(SlotId(1), 500), (SlotId(2), 2060), (SlotId(3), 2060)],
    );
    assert!(tree.move_relative(SlotId(3), SlotId(2), Direction::Down));
    let left = [bound_slot(1), bound_slot(2)];
    assert!(tree.sync_near(Axis::Horizontal, &left, None, LayoutMode::Dwindle, OUTPUT));
    assert_eq!(
        tree.placements(OUTPUT),
        vec![(SlotId(1), column(0, 500)), (SlotId(2), column(500, 2060))]
    );
    tree
}

fn column(x: i32, width: i32) -> Rect {
    Rect {
        x,
        y: 0,
        width,
        height: 1410,
    }
}

fn opened(tree: &mut LayoutTree, anchor: SlotId, mode: LayoutMode) {
    let slots = [bound_slot(1), bound_slot(2), bound_slot(4)];
    assert!(tree.sync_near(Axis::Horizontal, &slots, Some(anchor), mode, OUTPUT));
}

#[test]
fn dwindle_keeps_a_nested_manual_axis_when_one_tile_is_left_and_another_joins() {
    let mut tree = stack_after_close();
    opened(&mut tree, SlotId(2), LayoutMode::Dwindle);
    let row = |y| Rect {
        x: 500,
        y,
        width: 2060,
        height: 705,
    };
    // Without keeping the manual axis, B and D become 1030-wide columns.
    assert_eq!(
        tree.placements(OUTPUT),
        vec![
            (SlotId(1), column(0, 500)),
            (SlotId(2), row(0)),
            (SlotId(4), row(705))
        ]
    );
}

#[test]
fn the_lone_tile_of_a_kept_split_still_resizes_and_balances_against_its_neighbor() {
    let mut tree = stack_after_close();
    assert!(tree.resize_by(SlotId(2), ResizeDirection::Grow, 100, OUTPUT));
    assert_eq!(
        tree.placements(OUTPUT),
        vec![(SlotId(1), column(0, 400)), (SlotId(2), column(400, 2160))]
    );

    assert!(tree.balance(SlotId(2), OUTPUT));
    assert_eq!(
        tree.placements(OUTPUT),
        vec![
            (SlotId(1), column(0, 1280)),
            (SlotId(2), column(1280, 1280))
        ]
    );

    // A drag of B's left edge moves the boundary with A.
    assert!(tree.resize(SlotId(2), column(600, 1960), OUTPUT));
    assert_eq!(
        tree.placements(OUTPUT),
        vec![(SlotId(1), column(0, 600)), (SlotId(2), column(600, 1960))]
    );
}

#[test]
fn the_lone_tile_still_resizes_after_the_next_tile_opens_beside_another_tile() {
    let mut tree = stack_after_close();
    opened(&mut tree, SlotId(1), LayoutMode::Dwindle);
    let placements = tree.placements(OUTPUT);
    assert_eq!(placements[2], (SlotId(2), column(500, 2060)));

    assert!(tree.resize_by(SlotId(2), ResizeDirection::Shrink, 100, OUTPUT));
    let placements = tree.placements(OUTPUT);
    assert_eq!(placements[2], (SlotId(2), column(600, 1960)));
    assert!(
        placements[..2]
            .iter()
            .all(|(_, frame)| frame.x == 0 && frame.width == 600)
    );
}

#[test]
fn columns_still_joins_the_outer_split_after_switching_back_from_dwindle() {
    let mut tree = stack_after_close();
    opened(&mut tree, SlotId(2), LayoutMode::Columns);
    assert!(
        tree.placements(OUTPUT)
            .iter()
            .all(|(_, frame)| frame.y == 0 && frame.height == 1410)
    );
    assert_eq!(tree.root_axis(), Axis::Horizontal);
}
