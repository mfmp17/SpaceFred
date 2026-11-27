//! Keep a layout tree in step with its workspace's slots: drop the leaves of
//! slots that left, and (in Dwindle) re-orient auto-chosen splits that changed shape.
use std::collections::HashSet;

use super::{Child, Node, longer_axis, partition_children};
use crate::{Axis, Rect, SlotId};

pub(super) fn retain_root(
    root: Node,
    retained: &HashSet<SlotId>,
    fallback_axis: Axis,
    output: Rect,
    reorient: bool,
) -> Node {
    match root {
        Node::Split {
            axis,
            auto_axis,
            children,
        } => {
            let old_len = children.len();
            let mut surviving = retain_children(children, retained, output, axis, reorient);
            let shrank = surviving.len() < old_len;
            match surviving.len() {
                0 => Node::Split {
                    axis,
                    auto_axis,
                    children: Vec::new(),
                },
                1 if matches!(surviving[0].node, Node::Split { .. }) => {
                    let mut promoted = surviving.remove(0).node;
                    if reorient && shrank {
                        reorient_dwindle(&mut promoted, output);
                    }
                    promoted
                }
                1 => Node::Split {
                    axis: if reorient && auto_axis && shrank {
                        longer_axis(output)
                    } else {
                        axis
                    },
                    auto_axis,
                    children: surviving,
                },
                _ => {
                    if reorient && shrank {
                        reorient_children(&mut surviving, output, axis);
                    }
                    Node::Split {
                        axis,
                        auto_axis,
                        children: surviving,
                    }
                }
            }
        }
        other => retain_node(other, retained, output, reorient).unwrap_or(Node::Split {
            axis: fallback_axis,
            auto_axis: false,
            children: Vec::new(),
        }),
    }
}

pub(super) fn retain_node(
    node: Node,
    retained: &HashSet<SlotId>,
    frame: Rect,
    reorient: bool,
) -> Option<Node> {
    match node {
        Node::Leaf(slot) => retained.contains(&slot).then_some(Node::Leaf(slot)),
        Node::Split {
            axis,
            auto_axis,
            children,
        } => {
            let old_len = children.len();
            let mut surviving = retain_children(children, retained, frame, axis, reorient);
            let shrank = surviving.len() < old_len;
            match surviving.len() {
                0 => None,
                // A user-arranged split left holding one tile keeps its
                // direction for the next tile that joins it (#611).
                1 if reorient && !auto_axis && matches!(surviving[0].node, Node::Leaf(_)) => {
                    Some(Node::Split {
                        axis,
                        auto_axis,
                        children: surviving,
                    })
                }
                1 => {
                    let mut promoted = surviving.remove(0).node;
                    if reorient && shrank {
                        reorient_dwindle(&mut promoted, frame);
                    }
                    Some(promoted)
                }
                _ => {
                    if reorient && shrank {
                        reorient_children(&mut surviving, frame, axis);
                    }
                    Some(Node::Split {
                        axis,
                        auto_axis,
                        children: surviving,
                    })
                }
            }
        }
    }
}

pub(super) fn retain_children(
    children: Vec<Child>,
    retained: &HashSet<SlotId>,
    frame: Rect,
    axis: Axis,
    reorient: bool,
) -> Vec<Child> {
    let child_frames = if reorient {
        partition_children(frame, axis, &children)
    } else {
        Vec::new()
    };
    children
        .into_iter()
        .enumerate()
        .filter_map(|(index, child)| {
            let child_frame = child_frames.get(index).copied().unwrap_or(frame);
            retain_node(child.node, retained, child_frame, reorient).map(|node| Child {
                weight: child.weight,
                node,
            })
        })
        .collect()
}

pub(super) fn reorient_children(children: &mut [Child], frame: Rect, axis: Axis) {
    let child_frames = partition_children(frame, axis, children);
    for (child, child_frame) in children.iter_mut().zip(child_frames) {
        reorient_dwindle(&mut child.node, child_frame);
    }
}

/// Re-choose the axis of every auto-chosen split in `node`, outermost first,
/// from its new container `frame`. Splits the user arranged keep their axis,
/// while auto-chosen splits nested inside them still follow the shape they get.
pub(super) fn reorient_dwindle(node: &mut Node, frame: Rect) {
    let Node::Split {
        axis,
        auto_axis,
        children,
    } = node
    else {
        return;
    };
    if *auto_axis {
        *axis = longer_axis(frame);
    }
    reorient_children(children, frame, *axis);
}

pub(super) fn collapse(axis: Axis, auto_axis: bool, mut children: Vec<Child>) -> Option<Node> {
    match children.len() {
        0 => None,
        1 => Some(children.remove(0).node),
        _ => Some(Node::Split {
            axis,
            auto_axis,
            children,
        }),
    }
}

pub(super) fn extract(node: Node, slot: SlotId) -> (Option<Node>, Option<Node>) {
    match node {
        Node::Leaf(existing) if existing == slot => (None, Some(Node::Leaf(existing))),
        Node::Leaf(existing) => (Some(Node::Leaf(existing)), None),
        Node::Split {
            axis,
            auto_axis,
            mut children,
        } => {
            for index in 0..children.len() {
                let child = children.remove(index);
                let (remaining, moved) = extract(child.node, slot);
                if let Some(moved) = moved {
                    if let Some(remaining) = remaining {
                        children.insert(
                            index,
                            Child {
                                weight: child.weight,
                                node: remaining,
                            },
                        );
                    }
                    return (collapse(axis, auto_axis, children), Some(moved));
                }
                if let Some(remaining) = remaining {
                    children.insert(
                        index,
                        Child {
                            weight: child.weight,
                            node: remaining,
                        },
                    );
                }
            }
            (
                Some(Node::Split {
                    axis,
                    auto_axis,
                    children,
                }),
                None,
            )
        }
    }
}
