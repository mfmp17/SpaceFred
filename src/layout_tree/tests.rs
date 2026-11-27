use std::collections::HashMap;

use crate::{Axis, Direction, Rect, ResizeDirection, SlotId};

use super::{Child, LayoutTree, MIN_TILE_EXTENT, Node};

mod dwindle_close_open;
mod dwindle_nested_manual_axis;

#[test]
fn edge_insertion_after_resize_uses_the_destination_weight_scale() {
    let output = Rect {
        width: 1000,
        height: 800,
        ..Rect::default()
    };
    for direction in [Direction::Left, Direction::Right] {
        let mut tree = LayoutTree::flat(Axis::Horizontal, [SlotId(1), SlotId(2), SlotId(3)]);
        assert!(tree.resize_by(SlotId(1), ResizeDirection::Grow, 50, output));
        assert!(tree.move_relative(SlotId(3), SlotId(2), direction));
        let placements = tree.placements(output);
        assert!(
            placements
                .iter()
                .all(|(_, frame)| frame.width >= MIN_TILE_EXTENT),
            "{placements:?}"
        );
        let restored: LayoutTree =
            serde_json::from_slice(&serde_json::to_vec(&tree).unwrap()).unwrap();
        assert_eq!(restored.placements(output), placements);
    }
}

#[test]
fn implicit_split_preserves_the_remaining_layout() {
    let mut tree = LayoutTree::flat(Axis::Horizontal, [SlotId(1), SlotId(2), SlotId(3)]);

    assert!(tree.create_implicit_split(SlotId(2), Direction::Up));

    assert_eq!(tree.slots(), vec![SlotId(2), SlotId(1), SlotId(3)]);
    assert_eq!(
        tree.placements(Rect {
            x: 0,
            y: 0,
            width: 1200,
            height: 800,
        }),
        vec![
            (
                SlotId(2),
                Rect {
                    x: 0,
                    y: 0,
                    width: 1200,
                    height: 400
                }
            ),
            (
                SlotId(1),
                Rect {
                    x: 0,
                    y: 400,
                    width: 600,
                    height: 400
                }
            ),
            (
                SlotId(3),
                Rect {
                    x: 600,
                    y: 400,
                    width: 600,
                    height: 400
                }
            ),
        ]
    );
}

#[test]
fn persisted_layout_validation_rejects_duplicate_leaves_and_zero_weights() {
    let slots = [crate::Slot {
        id: SlotId(1),
        binding: crate::BindingState::Bound(crate::PhysicalWindowId {
            app: crate::AppInstanceId {
                pid: 1,
                launched_at: 1,
            },
            window_id: 1,
        }),
    }];
    let duplicate: LayoutTree = serde_json::from_value(serde_json::json!({
        "root": {"Split": {"axis": "horizontal", "children": [
            {"weight": 1, "node": {"Leaf": 1}},
            {"weight": 1, "node": {"Leaf": 1}}
        ]}}
    }))
    .unwrap();
    let zero_weight: LayoutTree = serde_json::from_value(serde_json::json!({
        "root": {"Split": {"axis": "horizontal", "children": [
            {"weight": 0, "node": {"Leaf": 1}}
        ]}}
    }))
    .unwrap();
    let nested_empty: LayoutTree = serde_json::from_value(serde_json::json!({
        "root": {"Split": {"axis": "horizontal", "children": [
            {"weight": 1, "node": {"Leaf": 1}},
            {"weight": 1, "node": {"Split": {
                "axis": "vertical",
                "children": []
            }}}
        ]}}
    }))
    .unwrap();

    assert!(duplicate.validate(&slots).is_err());
    assert!(zero_weight.validate(&slots).is_err());
    assert_eq!(
        nested_empty.validate(&slots).unwrap_err(),
        "layout contains an empty nested split"
    );
    assert!(LayoutTree::default().validate(&[]).is_ok());
}

#[test]
fn edge_insertion_creates_a_split_at_the_target() {
    let mut tree = LayoutTree::flat(Axis::Horizontal, [SlotId(1), SlotId(2), SlotId(3)]);

    assert!(tree.move_relative(SlotId(1), SlotId(2), Direction::Up));

    assert_eq!(
        tree.placements(Rect {
            x: 0,
            y: 0,
            width: 1000,
            height: 800
        }),
        vec![
            (
                SlotId(1),
                Rect {
                    x: 0,
                    y: 0,
                    width: 500,
                    height: 400
                }
            ),
            (
                SlotId(2),
                Rect {
                    x: 0,
                    y: 400,
                    width: 500,
                    height: 400
                }
            ),
            (
                SlotId(3),
                Rect {
                    x: 500,
                    y: 0,
                    width: 500,
                    height: 800
                }
            ),
        ]
    );
}

#[test]
fn resize_updates_the_nearest_nested_boundary() {
    let mut tree = LayoutTree::flat(Axis::Horizontal, [SlotId(1), SlotId(2), SlotId(3)]);
    assert!(tree.move_relative(SlotId(1), SlotId(2), Direction::Up));

    assert!(tree.resize(
        SlotId(1),
        Rect {
            x: 0,
            y: 0,
            width: 500,
            height: 500
        },
        Rect {
            x: 0,
            y: 0,
            width: 1000,
            height: 800
        },
    ));

    let placements = tree.placements(Rect {
        x: 0,
        y: 0,
        width: 1000,
        height: 800,
    });
    assert_eq!(placements[0].1.height, 500);
    assert_eq!(placements[1].1.y, 500);
    assert_eq!(
        placements[2].1,
        Rect {
            x: 500,
            y: 0,
            width: 500,
            height: 800
        }
    );
}

#[test]
fn resize_ignores_splits_too_small_for_both_minimums() {
    let mut tree = LayoutTree::flat(Axis::Horizontal, [SlotId(1), SlotId(2)]);

    assert!(!tree.resize(
        SlotId(1),
        Rect {
            width: 60,
            height: 100,
            ..Rect::default()
        },
        Rect {
            width: 80,
            height: 100,
            ..Rect::default()
        },
    ));
}

fn output() -> Rect {
    Rect {
        width: 1000,
        height: 800,
        ..Rect::default()
    }
}

#[test]
fn fitting_minimums_reaches_into_nested_splits_and_leaves_impossible_splits_alone() {
    let mut layout = LayoutTree {
        root: Node::Split {
            axis: Axis::Horizontal,
            auto_axis: false,
            children: vec![
                Child {
                    weight: 1,
                    node: Node::Leaf(SlotId(1)),
                },
                Child {
                    weight: 1,
                    node: Node::Split {
                        axis: Axis::Vertical,
                        auto_axis: false,
                        children: vec![
                            Child {
                                weight: 1,
                                node: Node::Leaf(SlotId(2)),
                            },
                            Child {
                                weight: 1,
                                node: Node::Leaf(SlotId(3)),
                            },
                        ],
                    },
                },
            ],
        },
    };
    let minimums = HashMap::from([(SlotId(3), (700, 500))]);

    assert!(layout.fit_minimums(output(), &minimums));
    let frames = layout.placements(output());
    assert_eq!(frames[0].1.width, 300);
    assert_eq!(frames[2].1.width, 700);
    assert_eq!(frames[2].1.height, 500);
    assert_eq!(frames[1].1.height, 300);
    assert!(!layout.fit_minimums(output(), &minimums));

    let before = layout.clone();
    let impossible = HashMap::from([(SlotId(1), (600, 0)), (SlotId(3), (700, 0))]);
    assert!(!layout.fit_minimums(output(), &impossible));
    assert_eq!(layout, before);
}

#[test]
fn keyboard_resize_grows_and_shrinks_a_flat_tile_by_fifty_pixels() {
    let mut tree = LayoutTree::flat(Axis::Horizontal, [SlotId(1), SlotId(2)]);

    assert!(tree.resize_by(SlotId(1), ResizeDirection::Grow, 50, output()));
    assert_eq!(tree.placements(output())[0].1.width, 550);
    assert!(tree.resize_by(SlotId(1), ResizeDirection::Shrink, 50, output()));
    assert_eq!(tree.placements(output())[0].1.width, 500);
}

#[test]
fn keyboard_resize_uses_the_immediate_nested_parent() {
    let mut tree = LayoutTree::flat(Axis::Horizontal, [SlotId(1), SlotId(2), SlotId(3)]);
    assert!(tree.move_relative(SlotId(1), SlotId(2), Direction::Up));

    assert!(tree.resize_by(SlotId(1), ResizeDirection::Grow, 50, output()));

    let placements = tree.placements(output());
    assert_eq!(placements[0].1.height, 450);
    assert_eq!(placements[1].1.height, 350);
    assert_eq!(placements[2].1.width, 500);
}

#[test]
fn keyboard_resize_stops_at_recursive_tile_minimums() {
    let mut tree = LayoutTree::flat(Axis::Horizontal, [SlotId(1), SlotId(2), SlotId(3)]);
    assert!(tree.create_implicit_split(SlotId(2), Direction::Right));

    for _ in 0..20 {
        tree.resize_by(SlotId(2), ResizeDirection::Grow, 50, output());
    }

    let placements = tree.placements(output());
    assert_eq!(placements[0].1.width, 50);
    assert_eq!(placements[1].1.width, 50);
    assert_eq!(placements[2].1.width, 900);
    assert!(!tree.resize_by(SlotId(2), ResizeDirection::Grow, 50, output()));
}

#[test]
fn balance_equalizes_the_focused_tiles_immediate_parent() {
    let mut tree = LayoutTree::flat(Axis::Horizontal, [SlotId(1), SlotId(2), SlotId(3)]);
    assert!(tree.move_relative(SlotId(1), SlotId(2), Direction::Up));
    assert!(tree.resize_by(SlotId(1), ResizeDirection::Grow, 50, output()));

    assert!(tree.balance(SlotId(1), output()));

    let placements = tree.placements(output());
    assert_eq!(placements[0].1.height, 400);
    assert_eq!(placements[1].1.height, 400);
    assert_eq!(placements[2].1.width, 500);
}

#[test]
fn resize_to_sets_the_extent_and_keeps_the_other_tiles_proportions() {
    let mut tree = LayoutTree::flat(Axis::Horizontal, [SlotId(1), SlotId(2), SlotId(3)]);
    assert!(tree.resize_by(SlotId(2), ResizeDirection::Grow, 100, output()));
    // 333 | 433 | 234

    assert!(tree.resize_to(SlotId(1), Axis::Horizontal, 600, output()));

    let widths = tree
        .placements(output())
        .iter()
        .map(|(_, frame)| frame.width)
        .collect::<Vec<_>>();
    assert_eq!(widths[0], 600);
    assert_eq!(widths.iter().sum::<i32>(), 1000);
    // The other two give up 267 in proportion to what each could spare, 383 and 184.
    assert_eq!(widths[1..], [252, 148]);

    // Asking again for the same size changes nothing.
    assert!(!tree.resize_to(SlotId(1), Axis::Horizontal, 600, output()));

    // Shrinking gives the space back in proportion to the others' widths.
    assert!(tree.resize_to(SlotId(1), Axis::Horizontal, 200, output()));
    let widths = tree
        .placements(output())
        .iter()
        .map(|(_, frame)| frame.width)
        .collect::<Vec<_>>();
    assert_eq!(widths[0], 200);
    assert_eq!(widths.iter().sum::<i32>(), 1000);
}

#[test]
fn resize_to_uses_the_nearest_split_on_its_axis() {
    // [1 / 2] | 3
    let mut tree = LayoutTree::flat(Axis::Horizontal, [SlotId(1), SlotId(2), SlotId(3)]);
    assert!(tree.move_relative(SlotId(1), SlotId(2), Direction::Up));

    // Width is set at the root columns, so both stacked tiles get it.
    assert!(tree.resize_to(SlotId(1), Axis::Horizontal, 700, output()));
    let frames = tree.placements(output());
    assert_eq!(frames[0].1.width, 700);
    assert_eq!(frames[1].1.width, 700);
    assert_eq!(frames[2].1.width, 300);

    // Height is set in the nested rows.
    assert!(tree.resize_to(SlotId(2), Axis::Vertical, 300, output()));
    let frames = tree.placements(output());
    assert_eq!(frames[0].1.height, 500);
    assert_eq!(frames[1].1.height, 300);
    assert_eq!(frames[2].1.height, 800);

    // Tile 3 spans the full height: no split sets it.
    assert!(!tree.resize_to(SlotId(3), Axis::Vertical, 300, output()));
}

#[test]
fn resize_to_stops_at_recursive_tile_minimums() {
    // 1 | [2 / 3] | 4
    let mut tree = LayoutTree::flat(
        Axis::Horizontal,
        [SlotId(1), SlotId(2), SlotId(3), SlotId(4)],
    );
    assert!(tree.move_relative(SlotId(3), SlotId(2), Direction::Down));

    assert!(tree.resize_to(SlotId(1), Axis::Horizontal, 5000, output()));
    let widths = tree
        .placements(output())
        .into_iter()
        .map(|(slot, frame)| (slot, frame.width))
        .collect::<HashMap<_, _>>();
    assert_eq!(widths[&SlotId(1)], 1000 - 2 * MIN_TILE_EXTENT);
    assert_eq!(widths[&SlotId(2)], MIN_TILE_EXTENT);
    assert_eq!(widths[&SlotId(4)], MIN_TILE_EXTENT);

    assert!(tree.resize_to(SlotId(1), Axis::Horizontal, 1, output()));
    assert_eq!(tree.placements(output())[0].1.width, MIN_TILE_EXTENT);
}

#[test]
fn resize_to_leaves_a_lone_tile_alone() {
    let mut tree = LayoutTree::flat(Axis::Horizontal, [SlotId(1)]);
    assert!(!tree.resize_to(SlotId(1), Axis::Horizontal, 400, output()));
    assert!(!tree.resize_to(SlotId(1), Axis::Vertical, 400, output()));
}

#[test]
fn newly_discovered_slot_uses_existing_weight_scale() {
    let mut tree = LayoutTree::flat(Axis::Horizontal, [SlotId(1)]);
    if let Node::Split { children, .. } = &mut tree.root {
        children[0].weight = 1480;
    }
    let slots = [
        crate::Slot {
            id: SlotId(1),
            binding: crate::BindingState::Unresolved {
                previous: test_window(1),
                candidates: Vec::new(),
                since: 0,
            },
        },
        crate::Slot {
            id: SlotId(2),
            binding: crate::BindingState::Unresolved {
                previous: test_window(2),
                candidates: Vec::new(),
                since: 0,
            },
        },
    ];

    tree.sync(Axis::Horizontal, &slots);

    assert_eq!(
        tree.placements(Rect {
            x: 0,
            y: 0,
            width: 1000,
            height: 800,
        }),
        vec![
            (
                SlotId(1),
                Rect {
                    x: 0,
                    y: 0,
                    width: 500,
                    height: 800,
                },
            ),
            (
                SlotId(2),
                Rect {
                    x: 500,
                    y: 0,
                    width: 500,
                    height: 800,
                },
            ),
        ]
    );
}

fn test_window(window_id: u32) -> crate::PhysicalWindowId {
    crate::PhysicalWindowId {
        app: crate::AppInstanceId {
            pid: 1,
            launched_at: 1,
        },
        window_id,
    }
}

#[test]
fn corner_drag_moves_both_nested_boundaries() {
    let output = Rect {
        width: 1200,
        height: 800,
        ..Rect::default()
    };
    let mut layout = LayoutTree::flat(Axis::Horizontal, [SlotId(1), SlotId(2), SlotId(3)]);
    assert!(layout.move_relative(SlotId(1), SlotId(2), Direction::Up));
    let dragged = Rect {
        width: 700,
        height: 460,
        ..Rect::default()
    };
    assert!(layout.resize(SlotId(1), dragged, output));
    let tiles = layout.placements(output);
    assert_eq!(tiles[0].1, dragged);
    assert_eq!(tiles[1].1.y, 460);
    assert_eq!(tiles[2].1.x, 700);
}

#[test]
fn stacking_is_rejected_when_the_other_dimension_cannot_fit() {
    let tree = LayoutTree::flat(Axis::Horizontal, [SlotId(1), SlotId(2)]);
    let output = Rect {
        width: 1000,
        height: 800,
        ..Rect::default()
    };
    let minimums = HashMap::from([(SlotId(1), (600, 450)), (SlotId(2), (600, 450))]);
    assert!(tree.fitted(output, &minimums).is_none());
}

fn bound_slot(id: u64) -> crate::Slot {
    crate::Slot {
        id: SlotId(id),
        binding: crate::BindingState::Bound(test_window(id as u32)),
    }
}

#[test]
fn dwindle_landscape_splits_one_through_four_windows() {
    let output = Rect {
        x: 0,
        y: 0,
        width: 1280,
        height: 800,
    };
    let mut tree = LayoutTree::default();

    // 1 window -> fills display.
    let mut slots = vec![bound_slot(1)];
    assert!(tree.sync_near(
        Axis::Horizontal,
        &slots,
        None,
        crate::config::LayoutMode::Dwindle,
        output
    ));
    assert_eq!(tree.placements(output), vec![(SlotId(1), output)]);

    // 2 windows -> 50% | 50% left/right.
    slots.push(bound_slot(2));
    assert!(tree.sync_near(
        Axis::Horizontal,
        &slots,
        Some(SlotId(1)),
        crate::config::LayoutMode::Dwindle,
        output
    ));
    assert_eq!(
        tree.placements(output),
        vec![
            (
                SlotId(1),
                Rect {
                    x: 0,
                    y: 0,
                    width: 640,
                    height: 800
                }
            ),
            (
                SlotId(2),
                Rect {
                    x: 640,
                    y: 0,
                    width: 640,
                    height: 800
                }
            ),
        ]
    );

    // 3 windows -> [ 1 | (2 / 3) ] (tile 2 is 640x800, taller than wide).
    slots.push(bound_slot(3));
    assert!(tree.sync_near(
        Axis::Horizontal,
        &slots,
        Some(SlotId(2)),
        crate::config::LayoutMode::Dwindle,
        output
    ));
    assert_eq!(
        tree.placements(output),
        vec![
            (
                SlotId(1),
                Rect {
                    x: 0,
                    y: 0,
                    width: 640,
                    height: 800
                }
            ),
            (
                SlotId(2),
                Rect {
                    x: 640,
                    y: 0,
                    width: 640,
                    height: 400
                }
            ),
            (
                SlotId(3),
                Rect {
                    x: 640,
                    y: 400,
                    width: 640,
                    height: 400
                }
            ),
        ]
    );

    // 4 windows -> [ 1 | (2 / (3 | 4)) ] (tile 3 is 640x400, wider than tall).
    slots.push(bound_slot(4));
    assert!(tree.sync_near(
        Axis::Horizontal,
        &slots,
        Some(SlotId(3)),
        crate::config::LayoutMode::Dwindle,
        output
    ));
    assert_eq!(
        tree.placements(output),
        vec![
            (
                SlotId(1),
                Rect {
                    x: 0,
                    y: 0,
                    width: 640,
                    height: 800
                }
            ),
            (
                SlotId(2),
                Rect {
                    x: 640,
                    y: 0,
                    width: 640,
                    height: 400
                }
            ),
            (
                SlotId(3),
                Rect {
                    x: 640,
                    y: 400,
                    width: 320,
                    height: 400
                }
            ),
            (
                SlotId(4),
                Rect {
                    x: 960,
                    y: 400,
                    width: 320,
                    height: 400
                }
            ),
        ]
    );
}

#[test]
fn dwindle_portrait_splits_vertically_first_then_alternates() {
    let output = Rect {
        x: 0,
        y: 0,
        width: 1080,
        height: 1920,
    };
    let mut tree = LayoutTree::default();
    let slots = vec![bound_slot(1), bound_slot(2), bound_slot(3), bound_slot(4)];

    // 2 windows on portrait -> 50% / 50% top/bottom (#208).
    assert!(tree.sync_near(
        Axis::Horizontal,
        &slots[..2],
        None,
        crate::config::LayoutMode::Dwindle,
        output
    ));
    assert_eq!(tree.root_axis(), Axis::Vertical);
    assert_eq!(
        tree.placements(output),
        vec![
            (
                SlotId(1),
                Rect {
                    x: 0,
                    y: 0,
                    width: 1080,
                    height: 960
                }
            ),
            (
                SlotId(2),
                Rect {
                    x: 0,
                    y: 960,
                    width: 1080,
                    height: 960
                }
            ),
        ]
    );

    // 3rd window splits bottom 1080x960 horizontally -> [ 1 / (2 | 3) ].
    assert!(tree.sync_near(
        Axis::Horizontal,
        &slots[..3],
        None,
        crate::config::LayoutMode::Dwindle,
        output
    ));
    assert_eq!(
        tree.placements(output),
        vec![
            (
                SlotId(1),
                Rect {
                    x: 0,
                    y: 0,
                    width: 1080,
                    height: 960
                }
            ),
            (
                SlotId(2),
                Rect {
                    x: 0,
                    y: 960,
                    width: 540,
                    height: 960
                }
            ),
            (
                SlotId(3),
                Rect {
                    x: 540,
                    y: 960,
                    width: 540,
                    height: 960
                }
            ),
        ]
    );

    // 4th window splits 540x960 vertically -> [ 1 / (2 | (3 / 4)) ].
    assert!(tree.sync_near(
        Axis::Horizontal,
        &slots,
        None,
        crate::config::LayoutMode::Dwindle,
        output
    ));
    assert_eq!(
        tree.placements(output)[2..],
        [
            (
                SlotId(3),
                Rect {
                    x: 540,
                    y: 960,
                    width: 540,
                    height: 480
                }
            ),
            (
                SlotId(4),
                Rect {
                    x: 540,
                    y: 1440,
                    width: 540,
                    height: 480
                }
            ),
        ]
    );
}

#[test]
fn dwindle_splits_focused_anchor_and_collapses_on_close() {
    let output = Rect {
        x: 0,
        y: 0,
        width: 1280,
        height: 800,
    };
    let mut tree = LayoutTree::default();
    let slots = [bound_slot(1), bound_slot(2), bound_slot(3)];
    tree.sync_near(
        Axis::Horizontal,
        &slots[..2],
        None,
        crate::config::LayoutMode::Dwindle,
        output,
    );
    // Focus left tile (1): 640x800 -> splits vertically into (1 / 3) on the left.
    tree.sync_near(
        Axis::Horizontal,
        &slots,
        Some(SlotId(1)),
        crate::config::LayoutMode::Dwindle,
        output,
    );
    assert_eq!(
        tree.placements(output),
        vec![
            (
                SlotId(1),
                Rect {
                    x: 0,
                    y: 0,
                    width: 640,
                    height: 400
                }
            ),
            (
                SlotId(3),
                Rect {
                    x: 0,
                    y: 400,
                    width: 640,
                    height: 400
                }
            ),
            (
                SlotId(2),
                Rect {
                    x: 640,
                    y: 0,
                    width: 640,
                    height: 800
                }
            ),
        ]
    );

    // Closing tile 2 promotes the surviving auto-chosen split (1 / 3) into the
    // landscape root and re-orients it to two columns (1 | 3); toggle_root_axis
    // then flips it to rows.
    tree.sync_near(
        Axis::Horizontal,
        &[bound_slot(1), bound_slot(3)],
        None,
        crate::config::LayoutMode::Dwindle,
        output,
    );
    assert_eq!(tree.root_axis(), Axis::Horizontal);
    assert_eq!(
        tree.placements(output),
        vec![
            (
                SlotId(1),
                Rect {
                    x: 0,
                    y: 0,
                    width: 640,
                    height: 800
                }
            ),
            (
                SlotId(3),
                Rect {
                    x: 640,
                    y: 0,
                    width: 640,
                    height: 800
                }
            ),
        ]
    );
    tree.toggle_root_axis();
    assert_eq!(tree.root_axis(), Axis::Vertical);
    assert_eq!(tree.placements(output)[0].1.height, 400);
    assert_eq!(tree.placements(output)[1].1.height, 400);
}

#[test]
fn columns_mode_preserves_flat_sibling_insertion() {
    let output = Rect {
        x: 0,
        y: 0,
        width: 1200,
        height: 800,
    };
    let mut tree = LayoutTree::default();
    let slots = [bound_slot(1), bound_slot(2), bound_slot(3)];
    tree.sync_near(
        Axis::Horizontal,
        &slots,
        None,
        crate::config::LayoutMode::Columns,
        output,
    );
    assert_eq!(
        tree.placements(output),
        vec![
            (
                SlotId(1),
                Rect {
                    x: 0,
                    y: 0,
                    width: 400,
                    height: 800
                }
            ),
            (
                SlotId(2),
                Rect {
                    x: 400,
                    y: 0,
                    width: 400,
                    height: 800
                }
            ),
            (
                SlotId(3),
                Rect {
                    x: 800,
                    y: 0,
                    width: 400,
                    height: 800
                }
            ),
        ]
    );
}

#[test]
fn dwindle_keeps_a_flipped_root_axis_when_one_tile_is_left_and_another_joins() {
    let output = Rect {
        x: 0,
        y: 0,
        width: 1080,
        height: 1920,
    };
    let dwindle = crate::config::LayoutMode::Dwindle;
    let (a, b, c) = (bound_slot(1), bound_slot(2), bound_slot(3));
    let mut tree = LayoutTree::default();
    assert!(tree.sync_near(Axis::Horizontal, &[a.clone(), b], None, dwindle, output));
    assert_eq!(tree.root_axis(), Axis::Vertical);
    tree.toggle_root_axis();

    // B leaves: the flipped root keeps its direction with one tile (#507).
    assert!(tree.sync_near(
        Axis::Horizontal,
        std::slice::from_ref(&a),
        None,
        dwindle,
        output
    ));
    assert_eq!(tree.root_axis(), Axis::Horizontal);

    // C joins: A and C finish side by side, not top and bottom.
    assert!(tree.sync_near(Axis::Horizontal, &[a, c], Some(SlotId(1)), dwindle, output));
    let half = |x| Rect {
        x,
        y: 0,
        width: 540,
        height: 1920,
    };
    assert_eq!(
        tree.placements(output),
        vec![(SlotId(1), half(0)), (SlotId(3), half(540))]
    );

    // A fresh portrait workspace still starts with rows.
    let mut fresh = LayoutTree::default();
    assert!(fresh.sync_near(Axis::Horizontal, &[bound_slot(1)], None, dwindle, output));
    let pair = [bound_slot(1), bound_slot(2)];
    assert!(fresh.sync_near(Axis::Horizontal, &pair, None, dwindle, output));
    assert_eq!(fresh.root_axis(), Axis::Vertical);
}
