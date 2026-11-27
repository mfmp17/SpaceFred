use std::collections::HashMap;

use crate::{Axis, Rect, ResizeDirection, SlotId};

use super::{
    Child, LayoutTree, MIN_TILE_EXTENT, Node, extent, leading, partition_children, trailing,
};

/// The smallest cell each tile may be given by a size edit: a known minimum
/// window size (its cell, gap included) or else the floor every tile keeps.
#[derive(Clone, Copy)]
pub(crate) struct Minimums<'a> {
    pub(crate) known: Option<&'a HashMap<SlotId, (i32, i32)>>,
    pub(crate) floor: i32,
}

impl Minimums<'_> {
    #[cfg(test)]
    pub(crate) const FLOOR: Minimums<'static> = Minimums {
        known: None,
        floor: MIN_TILE_EXTENT,
    };

    fn leaf(self, slot: SlotId, axis: Axis) -> i32 {
        let known = self
            .known
            .and_then(|known| known.get(&slot))
            .map_or(0, |&(width, height)| match axis {
                Axis::Horizontal => width,
                Axis::Vertical => height,
            });
        known.max(self.floor)
    }
}

impl LayoutTree {
    #[cfg(test)]
    pub(crate) fn resize(&mut self, slot: SlotId, current: Rect, output: Rect) -> bool {
        self.resize_within(slot, current, output, Minimums::FLOOR)
    }

    #[cfg(test)]
    pub(crate) fn resize_by(
        &mut self,
        slot: SlotId,
        direction: ResizeDirection,
        amount: i32,
        output: Rect,
    ) -> bool {
        self.resize_by_within(slot, direction, amount, output, Minimums::FLOOR)
    }

    #[cfg(test)]
    pub(crate) fn resize_to(
        &mut self,
        slot: SlotId,
        axis: Axis,
        target: i32,
        output: Rect,
    ) -> bool {
        self.resize_to_within(slot, axis, target, output, Minimums::FLOOR)
    }

    #[cfg(test)]
    pub(crate) fn balance(&mut self, slot: SlotId, output: Rect) -> bool {
        self.balance_within(slot, output, Minimums::FLOOR)
    }

    /// Reads a tile the user resized to `current` as the split boundaries it moved.
    pub(crate) fn resize_within(
        &mut self,
        slot: SlotId,
        current: Rect,
        output: Rect,
        minimums: Minimums<'_>,
    ) -> bool {
        let Some(expected) = self
            .placements(output)
            .into_iter()
            .find_map(|(candidate, frame)| (candidate == slot).then_some(frame))
        else {
            return false;
        };
        // A corner drag can cross one boundary on each axis. The two edits
        // affect independent coordinates, even when one split contains the other.
        let mut changed = false;
        for axis in [Axis::Horizontal, Axis::Vertical] {
            if let Some(candidate) = resize_candidate(
                &self.root,
                slot,
                current,
                expected,
                output,
                Vec::new(),
                axis,
            ) {
                changed |= self.apply_resize(candidate, output, minimums);
            }
        }
        changed
    }

    fn apply_resize(
        &mut self,
        candidate: ResizeCandidate,
        output: Rect,
        minimums: Minimums<'_>,
    ) -> bool {
        if candidate.delta.abs() <= 2 {
            return false;
        }
        let Some((split_frame, axis, children)) = self.split_at_mut(output, &candidate.path) else {
            return false;
        };
        let frames = partition_children(split_frame, *axis, children);
        let before = candidate.boundary;
        let after = before + 1;
        let pair_start = leading(frames[before], *axis);
        let pair_end = trailing(frames[after], *axis);
        let before_minimum = minimum_extent(&children[before].node, *axis, minimums);
        let after_minimum = minimum_extent(&children[after].node, *axis, minimums);
        if pair_end - pair_start < before_minimum + after_minimum {
            return false;
        }
        let boundary = candidate
            .coordinate
            .clamp(pair_start + before_minimum, pair_end - after_minimum);
        set_pair_extents(
            children,
            &frames,
            *axis,
            (before, boundary - pair_start),
            (after, pair_end - boundary),
        );
        true
    }

    pub(crate) fn resize_by_within(
        &mut self,
        slot: SlotId,
        direction: ResizeDirection,
        amount: i32,
        output: Rect,
        minimums: Minimums<'_>,
    ) -> bool {
        let Some((path, index)) = immediate_parent(&self.root, slot, Vec::new()) else {
            return false;
        };
        let Some((split_frame, axis, children)) = self.split_at_mut(output, &path) else {
            return false;
        };
        let neighbor = if index + 1 < children.len() {
            index + 1
        } else if index > 0 {
            index - 1
        } else {
            return false;
        };
        let frames = partition_children(split_frame, *axis, children);
        let pair_extent = extent(frames[index], *axis) + extent(frames[neighbor], *axis);
        let focused_minimum = minimum_extent(&children[index].node, *axis, minimums);
        let neighbor_minimum = minimum_extent(&children[neighbor].node, *axis, minimums);
        if pair_extent < focused_minimum + neighbor_minimum {
            return false;
        }
        let delta = match direction {
            ResizeDirection::Shrink => -amount,
            ResizeDirection::Grow => amount,
        };
        let focused_extent = (extent(frames[index], *axis) + delta)
            .clamp(focused_minimum, pair_extent - neighbor_minimum);
        if focused_extent == extent(frames[index], *axis) {
            return false;
        }
        set_pair_extents(
            children,
            &frames,
            *axis,
            (index, focused_extent),
            (neighbor, pair_extent - focused_extent),
        );
        true
    }

    /// Set the focused tile's extent along `axis` at the nearest split on that axis. The
    /// split's other children give or take the difference in proportion to their size (or to
    /// what each can spare above its nested minimum), so their relative sizes hold.
    pub(crate) fn resize_to_within(
        &mut self,
        slot: SlotId,
        axis: Axis,
        target: i32,
        output: Rect,
        minimums: Minimums<'_>,
    ) -> bool {
        let Some((path, index)) = nearest_split_on(&self.root, slot, axis, Vec::new()) else {
            return false;
        };
        let Some((split_frame, _, children)) = self.split_at_mut(output, &path) else {
            return false;
        };
        let extents = partition_children(split_frame, axis, children)
            .iter()
            .map(|frame| extent(*frame, axis))
            .collect::<Vec<_>>();
        let minimums = children
            .iter()
            .map(|child| minimum_extent(&child.node, axis, minimums))
            .collect::<Vec<_>>();
        let Some(sized) = extents_with(&extents, &minimums, index, target) else {
            return false;
        };
        if sized == extents {
            return false;
        }
        for (child, extent) in children.iter_mut().zip(sized) {
            child.weight = pixel_weight(extent);
        }
        true
    }

    pub(crate) fn balance_within(
        &mut self,
        slot: SlotId,
        output: Rect,
        minimums: Minimums<'_>,
    ) -> bool {
        let Some((path, _)) = immediate_parent(&self.root, slot, Vec::new()) else {
            return false;
        };
        let Some((split_frame, axis, children)) = self.split_at_mut(output, &path) else {
            return false;
        };
        let total = extent(split_frame, *axis);
        let minimums = children
            .iter()
            .map(|child| minimum_extent(&child.node, *axis, minimums))
            .collect::<Vec<_>>();
        if minimums.iter().sum::<i32>() > total {
            return false;
        }
        let mut low = 0;
        let mut high = total;
        while low < high {
            let middle = low + (high - low + 1) / 2;
            if minimums
                .iter()
                .map(|minimum| (*minimum).max(middle))
                .sum::<i32>()
                <= total
            {
                low = middle;
            } else {
                high = middle - 1;
            }
        }
        let mut extents = minimums
            .into_iter()
            .map(|minimum| minimum.max(low))
            .collect::<Vec<_>>();
        let mut remainder = total - extents.iter().sum::<i32>();
        for extent in extents.iter_mut().filter(|extent| **extent == low) {
            if remainder == 0 {
                break;
            }
            *extent += 1;
            remainder -= 1;
        }
        let changed = partition_children(split_frame, *axis, children)
            .iter()
            .zip(&extents)
            .any(|(frame, target)| extent(*frame, *axis) != *target);
        if !changed {
            return false;
        }
        for (child, extent) in children.iter_mut().zip(extents) {
            child.weight = pixel_weight(extent);
        }
        true
    }

    /// Give every tile at least its window's known minimum `(width, height)`,
    /// taking the space from siblings in proportion to what each can spare.
    /// Splits whose minimums cannot all fit are left unchanged.
    pub(crate) fn fit_minimums(
        &mut self,
        output: Rect,
        minimums: &HashMap<SlotId, (i32, i32)>,
    ) -> bool {
        fit_node(&mut self.root, output, minimums)
    }

    fn split_at_mut(
        &mut self,
        output: Rect,
        path: &[usize],
    ) -> Option<(Rect, &mut Axis, &mut Vec<Child>)> {
        let frame = output_at(&self.root, output, path);
        let Node::Split { axis, children, .. } = node_at_mut(&mut self.root, path)? else {
            return None;
        };
        Some((frame, axis, children))
    }
}

#[derive(Clone)]
struct ResizeCandidate {
    path: Vec<usize>,
    boundary: usize,
    coordinate: i32,
    delta: i32,
}

fn resize_candidate(
    node: &Node,
    slot: SlotId,
    current: Rect,
    expected: Rect,
    frame: Rect,
    path: Vec<usize>,
    drag_axis: Axis,
) -> Option<ResizeCandidate> {
    let Node::Split { axis, children, .. } = node else {
        return None;
    };
    let frames = partition_children(frame, *axis, children);
    let index = children
        .iter()
        .position(|child| child.node.contains(slot))?;
    let child_frame = frames[index];
    let mut candidates = Vec::new();
    if *axis == drag_axis && index > 0 && leading(expected, *axis) == leading(child_frame, *axis) {
        let coordinate = leading(current, *axis);
        candidates.push(ResizeCandidate {
            path: path.clone(),
            boundary: index - 1,
            coordinate,
            delta: coordinate - leading(child_frame, *axis),
        });
    }
    if *axis == drag_axis
        && index + 1 < children.len()
        && trailing(expected, *axis) == trailing(child_frame, *axis)
    {
        let coordinate = trailing(current, *axis);
        candidates.push(ResizeCandidate {
            path: path.clone(),
            boundary: index,
            coordinate,
            delta: coordinate - trailing(child_frame, *axis),
        });
    }
    let mut child_path = path;
    child_path.push(index);
    if let Some(nested) = resize_candidate(
        &children[index].node,
        slot,
        current,
        expected,
        child_frame,
        child_path,
        drag_axis,
    ) {
        candidates.push(nested);
    }
    candidates
        .into_iter()
        .max_by_key(|candidate| candidate.delta.abs())
}

/// `extents` with child `index` set to `target`, clamped so every child keeps its minimum.
/// The others grow in proportion to their extents, or shrink in proportion to their slack.
/// `None` when the minimums cannot all fit.
fn extents_with(extents: &[i32], minimums: &[i32], index: usize, target: i32) -> Option<Vec<i32>> {
    let total = extents.iter().sum::<i32>();
    let others_minimum = minimums.iter().sum::<i32>() - minimums[index];
    if minimums[index] + others_minimum > total {
        return None;
    }
    let focused = target.clamp(minimums[index], total - others_minimum);
    let others = (0..extents.len()).filter(|&other| other != index);
    // Positive: the others grow by this much; negative: they shrink.
    let change = (total - focused) - (total - extents[index]);
    let (weights, caps): (Vec<i64>, Vec<i64>) = others
        .clone()
        .map(|other| {
            if change > 0 {
                (i64::from(extents[other].max(1)), i64::MAX)
            } else {
                let slack = i64::from(extents[other] - minimums[other]).max(0);
                (slack, slack)
            }
        })
        .unzip();
    let shares = spread(i64::from(change.abs()), &weights, &caps);
    let mut sized = extents.to_vec();
    sized[index] = focused;
    for (other, share) in others.zip(shares) {
        let share = i32::try_from(share).unwrap_or(i32::MAX);
        sized[other] += if change > 0 { share } else { -share };
    }
    Some(sized)
}

/// `amount` split over `weights` in proportion, in whole points, no share above its cap.
fn spread(amount: i64, weights: &[i64], caps: &[i64]) -> Vec<i64> {
    let total = weights.iter().sum::<i64>();
    let mut shares = weights
        .iter()
        .zip(caps)
        .map(|(weight, cap)| {
            if total == 0 {
                0
            } else {
                (amount * weight / total).min(*cap)
            }
        })
        .collect::<Vec<_>>();
    let mut left = amount - shares.iter().sum::<i64>();
    while left > 0 {
        let before = left;
        for (share, cap) in shares.iter_mut().zip(caps) {
            if left > 0 && *share < *cap {
                *share += 1;
                left -= 1;
            }
        }
        if left == before {
            break;
        }
    }
    shares
}

/// The deepest split on `axis` with more than one child that contains `slot`, and the index of
/// the child holding it: the split whose boundaries set the tile's extent along `axis`.
fn nearest_split_on(
    node: &Node,
    slot: SlotId,
    axis: Axis,
    path: Vec<usize>,
) -> Option<(Vec<usize>, usize)> {
    let Node::Split {
        axis: split_axis,
        children,
        ..
    } = node
    else {
        return None;
    };
    let index = children
        .iter()
        .position(|child| child.node.contains(slot))?;
    let mut child_path = path.clone();
    child_path.push(index);
    nearest_split_on(&children[index].node, slot, axis, child_path)
        .or_else(|| (*split_axis == axis && children.len() > 1).then_some((path, index)))
}

fn pixel_weight(extent: i32) -> u32 {
    u32::try_from(extent).unwrap_or(1).max(1)
}

fn set_pair_extents(
    children: &mut [Child],
    frames: &[Rect],
    axis: Axis,
    first: (usize, i32),
    second: (usize, i32),
) {
    for (child, frame) in children.iter_mut().zip(frames) {
        child.weight = pixel_weight(extent(*frame, axis));
    }
    children[first.0].weight = pixel_weight(first.1);
    children[second.0].weight = pixel_weight(second.1);
}

fn output_at(root: &Node, output: Rect, path: &[usize]) -> Rect {
    let mut node = root;
    let mut frame = output;
    for &index in path {
        let Node::Split { axis, children, .. } = node else {
            break;
        };
        frame = partition_children(frame, *axis, children)[index];
        node = &children[index].node;
    }
    frame
}

fn node_at_mut<'a>(node: &'a mut Node, path: &[usize]) -> Option<&'a mut Node> {
    let mut node = node;
    for &index in path {
        let Node::Split { children, .. } = node else {
            return None;
        };
        node = &mut children.get_mut(index)?.node;
    }
    Some(node)
}

/// The split a keyboard resize or balance of `slot` acts on, and the index of the child that
/// holds it: the split directly holding the tile, or, when that is a one-child split kept for
/// its direction (#611), the nearest ancestor where the tile has a sibling.
fn immediate_parent(node: &Node, slot: SlotId, path: Vec<usize>) -> Option<(Vec<usize>, usize)> {
    sizing_parent(node, slot, path).map(|(path, index, _)| (path, index))
}

fn sizing_parent(
    node: &Node,
    slot: SlotId,
    path: Vec<usize>,
) -> Option<(Vec<usize>, usize, usize)> {
    let Node::Split { children, .. } = node else {
        return None;
    };
    let index = children
        .iter()
        .position(|child| child.node.contains(slot))?;
    if matches!(children[index].node, Node::Leaf(existing) if existing == slot) {
        return Some((path, index, children.len()));
    }
    let mut child_path = path.clone();
    child_path.push(index);
    let nested = sizing_parent(&children[index].node, slot, child_path)?;
    if nested.2 == 1 && children.len() > 1 {
        Some((path, index, children.len()))
    } else {
        Some(nested)
    }
}

fn minimum_extent(node: &Node, axis: Axis, minimums: Minimums<'_>) -> i32 {
    match node {
        Node::Leaf(slot) => minimums.leaf(*slot, axis),
        Node::Split {
            axis: split_axis,
            children,
            ..
        } if *split_axis == axis => children
            .iter()
            .map(|child| minimum_extent(&child.node, axis, minimums))
            .sum(),
        Node::Split { children, .. } => children
            .iter()
            .map(|child| minimum_extent(&child.node, axis, minimums))
            .max()
            .unwrap_or(0),
    }
}

/// Shortfalls this small are readback noise, not a refused size.
const MINIMUM_TOLERANCE: i32 = 2;

fn fit_node(node: &mut Node, frame: Rect, minimums: &HashMap<SlotId, (i32, i32)>) -> bool {
    let Node::Split { axis, children, .. } = node else {
        return false;
    };
    let extents = partition_children(frame, *axis, children)
        .iter()
        .map(|child| extent(*child, *axis))
        .collect::<Vec<_>>();
    let required = children
        .iter()
        .map(|child| required_extent(&child.node, *axis, minimums))
        .collect::<Vec<_>>();
    let mut changed = false;
    if let Some(fitted) = redistribute(&extents, &required) {
        for (child, extent) in children.iter_mut().zip(fitted) {
            child.weight = pixel_weight(extent);
        }
        changed = true;
    }
    let frames = partition_children(frame, *axis, children);
    for (child, child_frame) in children.iter_mut().zip(frames) {
        changed |= fit_node(&mut child.node, child_frame, minimums);
    }
    changed
}

pub(super) fn required_extent(
    node: &Node,
    axis: Axis,
    minimums: &HashMap<SlotId, (i32, i32)>,
) -> i32 {
    match node {
        Node::Leaf(slot) => {
            let known = minimums.get(slot).map_or(0, |&(width, height)| match axis {
                Axis::Horizontal => width,
                Axis::Vertical => height,
            });
            known.max(MIN_TILE_EXTENT)
        }
        Node::Split {
            axis: split_axis,
            children,
            ..
        } if *split_axis == axis => children
            .iter()
            .map(|child| required_extent(&child.node, axis, minimums))
            .sum(),
        Node::Split { children, .. } => children
            .iter()
            .map(|child| required_extent(&child.node, axis, minimums))
            .max()
            .unwrap_or(0),
    }
}

/// Raise each extent short of its requirement and take the difference from
/// the others in proportion to their slack. `None` when nothing is short or
/// the requirements do not fit.
fn redistribute(extents: &[i32], required: &[i32]) -> Option<Vec<i32>> {
    if !extents
        .iter()
        .zip(required)
        .any(|(extent, required)| extent + MINIMUM_TOLERANCE < *required)
        || required.iter().sum::<i32>() > extents.iter().sum::<i32>()
    {
        return None;
    }
    let mut fitted = extents
        .iter()
        .zip(required)
        .map(|(extent, required)| (*extent).max(*required))
        .collect::<Vec<_>>();
    let mut deficit = fitted.iter().sum::<i32>() - extents.iter().sum::<i32>();
    let slack = fitted
        .iter()
        .zip(required)
        .map(|(extent, required)| extent - required)
        .collect::<Vec<_>>();
    let total_slack = i64::from(slack.iter().sum::<i32>());
    let mut taken = 0;
    for (extent, slack) in fitted.iter_mut().zip(&slack) {
        let share = (i64::from(deficit) * i64::from(*slack) / total_slack) as i32;
        *extent -= share;
        taken += share;
    }
    deficit -= taken;
    for (extent, required) in fitted.iter_mut().zip(required) {
        if deficit == 0 {
            break;
        }
        if *extent > *required {
            *extent -= 1;
            deficit -= 1;
        }
    }
    Some(fitted)
}
