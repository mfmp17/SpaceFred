//! Derive a display layout from the user's split tree and known window sizes.
//! Never write these temporary groupings to workspace state.
use std::collections::HashMap;

use super::{Child, LayoutTree, Node, extent, sizing::required_extent};
use crate::{Axis, Rect, SlotId};

impl LayoutTree {
    pub(crate) fn fitted(
        &self,
        output: Rect,
        minimums: &HashMap<SlotId, (i32, i32)>,
    ) -> Option<Self> {
        if minimums.is_empty() {
            return None;
        }
        let mut candidate = self.clone();
        group_if_needed(&mut candidate.root, output, minimums);
        candidate.fit_minimums(output, minimums);
        (candidate != *self).then_some(candidate)
    }

    /// Whether this fit groups tiles that `requested` keeps apart, so it
    /// differs in more than weights and an edit to it must never be saved.
    pub(crate) fn regroups(&self, requested: &Self) -> bool {
        !same_shape(&self.root, &requested.root)
    }
}

fn same_shape(left: &Node, right: &Node) -> bool {
    match (left, right) {
        (Node::Leaf(left), Node::Leaf(right)) => left == right,
        (
            Node::Split {
                axis: left_axis,
                children: left,
                ..
            },
            Node::Split {
                axis: right_axis,
                children: right,
                ..
            },
        ) => {
            left_axis == right_axis
                && left.len() == right.len()
                && left
                    .iter()
                    .zip(right)
                    .all(|(left, right)| same_shape(&left.node, &right.node))
        }
        _ => false,
    }
}

/// Grouping neighbors on the other axis saves the smaller of their two
/// required extents, and costs the sum of their cross-axis minimums. Prefer
/// the trailing pair to keep the leading tile in place.
fn group_if_needed(node: &mut Node, output: Rect, minimums: &HashMap<SlotId, (i32, i32)>) {
    let Node::Split { children, axis, .. } = node else {
        return;
    };
    let cross = match axis {
        Axis::Horizontal => Axis::Vertical,
        Axis::Vertical => Axis::Horizontal,
    };
    let sizes: Vec<_> = children
        .iter()
        .map(|child| {
            (
                i64::from(required_extent(&child.node, *axis, minimums)),
                i64::from(required_extent(&child.node, cross, minimums)),
            )
        })
        .collect();
    let shortage = sizes.iter().map(|s| s.0).sum::<i64>() - i64::from(extent(output, *axis));
    let available = i64::from(extent(output, cross));
    if shortage <= 0 || sizes.iter().any(|s| s.1 > available) {
        return;
    }
    let Some(index) = sizes.windows(2).rposition(|pair| {
        pair[0].0.min(pair[1].0) >= shortage && pair[0].1 + pair[1].1 <= available
    }) else {
        return;
    };
    let mut right = children.remove(index + 1);
    let mut left = children.remove(index);
    let weight = left.weight.saturating_add(right.weight);
    left.weight = 1;
    right.weight = 1;
    let stack = Node::Split {
        axis: cross,
        auto_axis: false,
        children: vec![left, right],
    };
    if children.is_empty() {
        *node = stack;
    } else {
        children.insert(
            index,
            Child {
                weight,
                node: stack,
            },
        );
    }
}
