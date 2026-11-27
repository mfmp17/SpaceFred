mod fitting;
mod retain;
mod sizing;
#[cfg(test)]
mod tests;

use std::collections::HashSet;

use serde::{Deserialize, Serialize};

use crate::{Axis, Direction, Rect, SlotId, config::LayoutMode};
use retain::{extract, retain_root};
pub(crate) use sizing::Minimums;

/// The smallest cell a split gives a tile, before the inner gap.
pub(crate) const MIN_TILE_EXTENT: i32 = 50;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct LayoutTree {
    root: Node,
}

impl Default for LayoutTree {
    fn default() -> Self {
        Self::flat(Axis::Horizontal, [])
    }
}

const fn is_false(value: &bool) -> bool {
    !*value
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
enum Node {
    Leaf(SlotId),
    Split {
        axis: Axis,
        /// True when Dwindle picked this split's axis from the container's
        /// shape. Missing in state saved by earlier builds, which defaults to
        /// false so saved layouts keep their existing split directions.
        #[serde(default, skip_serializing_if = "is_false")]
        auto_axis: bool,
        children: Vec<Child>,
    },
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
struct Child {
    weight: u32,
    node: Node,
}

impl LayoutTree {
    pub(crate) fn flat(axis: Axis, slots: impl IntoIterator<Item = SlotId>) -> Self {
        Self {
            root: Node::Split {
                axis,
                auto_axis: false,
                children: slots
                    .into_iter()
                    .map(|slot| Child {
                        weight: 1,
                        node: Node::Leaf(slot),
                    })
                    .collect(),
            },
        }
    }

    pub(crate) fn weighted_flat(
        axis: Axis,
        slots: impl IntoIterator<Item = (SlotId, u32)>,
    ) -> Self {
        Self {
            root: Node::Split {
                axis,
                auto_axis: false,
                children: slots
                    .into_iter()
                    .map(|(slot, weight)| Child {
                        weight: weight.max(1),
                        node: Node::Leaf(slot),
                    })
                    .collect(),
            },
        }
    }

    pub(crate) const fn root_axis(&self) -> Axis {
        match &self.root {
            Node::Split { axis, .. } => *axis,
            Node::Leaf(_) => Axis::Horizontal,
        }
    }

    pub(crate) fn sync(&mut self, fallback_axis: Axis, slots: &[crate::Slot]) {
        self.sync_near(
            fallback_axis,
            slots,
            None,
            LayoutMode::Columns,
            Rect::default(),
        );
    }

    pub(crate) fn sync_near(
        &mut self,
        fallback_axis: Axis,
        slots: &[crate::Slot],
        anchor: Option<SlotId>,
        mode: LayoutMode,
        output: Rect,
    ) -> bool {
        self.sync_shown(fallback_axis, slots, anchor, mode, output, None)
    }

    /// Like `sync_near`, where `shown` is the layout on display when it differs
    /// from this requested one (a minimum-size fit): Dwindle splits the anchor
    /// tile along the longer side of the frame it actually has (#410), unless
    /// a tile also left in this change, which makes that fit stale (#599).
    pub(crate) fn sync_shown(
        &mut self,
        fallback_axis: Axis,
        slots: &[crate::Slot],
        mut anchor: Option<SlotId>,
        mode: LayoutMode,
        output: Rect,
        mut shown: Option<&Self>,
    ) -> bool {
        let retained = slots.iter().map(|slot| slot.id).collect::<HashSet<_>>();
        let existing = self.slots().into_iter().collect::<HashSet<_>>();
        if existing == retained && existing.len() == slots.len() {
            return false;
        }
        // A tile leaving in this same change reshapes its neighbors, so the
        // fit on display no longer shows the frame the anchor will have (#599).
        if !existing.is_subset(&retained) {
            shown = None;
        }
        let reorient = mode == LayoutMode::Dwindle && output.width > 0 && output.height > 0;
        let root = std::mem::replace(
            &mut self.root,
            Node::Split {
                axis: fallback_axis,
                auto_axis: false,
                children: Vec::new(),
            },
        );
        self.root = retain_root(root, &retained, fallback_axis, output, reorient);
        self.unwrap_single_split_root();
        for slot in slots.iter().filter(|slot| !existing.contains(&slot.id)) {
            match mode {
                LayoutMode::Dwindle => {
                    anchor = Some(self.insert_dwindle(slot.id, anchor, output, shown));
                    shown = None;
                }
                LayoutMode::Columns => {
                    if let Some(previous) = anchor
                        && insert_sibling(&mut self.root, previous, slot.id)
                    {
                        anchor = Some(slot.id);
                    } else {
                        self.append(fallback_axis, slot.id, 1);
                    }
                }
            }
        }
        true
    }

    pub(crate) fn slots(&self) -> Vec<SlotId> {
        let mut slots = Vec::new();
        collect_slots(&self.root, &mut slots);
        slots
    }

    pub(crate) fn validate(&self, slots: &[crate::Slot]) -> Result<(), String> {
        let expected: HashSet<_> = slots.iter().map(|slot| slot.id).collect();
        if expected.len() != slots.len() {
            return Err("layout slots contain duplicate IDs".into());
        }
        let mut leaves = HashSet::new();
        validate_node(&self.root, true, &mut leaves)?;
        if leaves != expected {
            return Err("layout leaves do not exactly match workspace slots".into());
        }
        Ok(())
    }

    pub(crate) fn weight(&self, slot: SlotId) -> Option<u32> {
        leaf_weight(&self.root, slot)
    }

    pub(crate) fn placements(&self, output: Rect) -> Vec<(SlotId, Rect)> {
        let mut placements = Vec::new();
        place(&self.root, output, &mut placements);
        placements
    }

    pub(crate) fn swap(&mut self, first: SlotId, second: SlotId) -> bool {
        if first == second || !self.root.contains(first) || !self.root.contains(second) {
            return false;
        }
        swap_slots(&mut self.root, first, second);
        true
    }

    pub(crate) fn create_implicit_split(&mut self, slot: SlotId, direction: Direction) -> bool {
        let Some((remaining, moved)) = self.take_slot(slot) else {
            return false;
        };
        self.root = split_pair(direction, moved, remaining);
        true
    }

    pub(crate) fn move_relative(
        &mut self,
        moved: SlotId,
        target: SlotId,
        direction: Direction,
    ) -> bool {
        if moved == target || !self.root.contains(target) {
            return false;
        }
        let Some((mut remaining, extracted)) = self.take_slot(moved) else {
            return false;
        };
        if !insert_relative(&mut remaining, target, extracted, direction) {
            return false;
        }
        self.root = remaining;
        true
    }

    pub(crate) fn toggle_root_axis(&mut self) {
        if let Node::Split {
            axis, auto_axis, ..
        } = &mut self.root
        {
            *axis = match *axis {
                Axis::Horizontal => Axis::Vertical,
                Axis::Vertical => Axis::Horizontal,
            };
            *auto_axis = false;
        }
    }

    fn take_slot(&self, slot: SlotId) -> Option<(Node, Node)> {
        match extract(self.root.clone(), slot) {
            (Some(remaining), Some(moved)) => Some((remaining, moved)),
            _ => None,
        }
    }

    /// Insert `slot` by splitting the focused (or last) leaf along that tile's longer side.
    fn insert_dwindle(
        &mut self,
        slot: SlotId,
        anchor: Option<SlotId>,
        output: Rect,
        shown: Option<&Self>,
    ) -> SlotId {
        let target = anchor
            .filter(|&id| self.root.contains(id))
            .or_else(|| self.slots().last().copied());
        let Some(target) = target else {
            self.root = Node::Split {
                axis: longer_axis(output),
                auto_axis: true,
                children: vec![Child {
                    weight: 1,
                    node: Node::Leaf(slot),
                }],
            };
            return slot;
        };
        // A user-arranged split, root (#507) or nested (#611), left holding
        // just this tile keeps its direction for the next tile; only
        // auto-chosen splits follow the shape.
        if join_lone_manual_split(&mut self.root, target, slot) {
            return slot;
        }
        let frame = shown
            .and_then(|shown| tile_frame(&shown.root, target, output))
            .or_else(|| tile_frame(&self.root, target, output))
            .unwrap_or(output);
        let axis = longer_axis(frame);
        split_leaf(&mut self.root, target, slot, axis);
        self.unwrap_single_split_root();
        slot
    }

    fn unwrap_single_split_root(&mut self) {
        while let Node::Split { children, .. } = &mut self.root
            && children.len() == 1
            && matches!(children[0].node, Node::Split { .. })
        {
            self.root = children.remove(0).node;
        }
    }

    fn append(&mut self, axis: Axis, slot: SlotId, weight: u32) {
        match &mut self.root {
            Node::Split { children, .. } => {
                let weight = if children.is_empty() {
                    weight.max(1)
                } else {
                    average_weight(children)
                };
                children.push(Child {
                    weight,
                    node: Node::Leaf(slot),
                });
            }
            root => {
                let previous = std::mem::replace(root, Node::Leaf(slot));
                *root = Node::Split {
                    axis,
                    auto_axis: false,
                    children: vec![
                        Child {
                            weight: 1,
                            node: previous,
                        },
                        Child {
                            weight,
                            node: Node::Leaf(slot),
                        },
                    ],
                };
            }
        }
    }
}

const fn longer_axis(rect: Rect) -> Axis {
    if rect.height > rect.width {
        Axis::Vertical
    } else {
        Axis::Horizontal
    }
}

fn tile_frame(node: &Node, target: SlotId, frame: Rect) -> Option<Rect> {
    match node {
        Node::Leaf(slot) => (*slot == target).then_some(frame),
        Node::Split { axis, children, .. } => {
            for (child, child_frame) in children
                .iter()
                .zip(partition_children(frame, *axis, children))
            {
                if let Some(found) = tile_frame(&child.node, target, child_frame) {
                    return Some(found);
                }
            }
            None
        }
    }
}

fn split_leaf(node: &mut Node, target: SlotId, inserted: SlotId, axis: Axis) -> bool {
    match node {
        Node::Leaf(existing) if *existing == target => {
            *node = Node::Split {
                axis,
                auto_axis: true,
                children: vec![
                    Child {
                        weight: 1,
                        node: Node::Leaf(target),
                    },
                    Child {
                        weight: 1,
                        node: Node::Leaf(inserted),
                    },
                ],
            };
            true
        }
        Node::Leaf(_) => false,
        Node::Split { children, .. } => children
            .iter_mut()
            .any(|child| split_leaf(&mut child.node, target, inserted, axis)),
    }
}

/// Add `inserted` beside `target` when `target` is the only tile left in a
/// user-arranged split, so the pair keeps that split's direction.
fn join_lone_manual_split(node: &mut Node, target: SlotId, inserted: SlotId) -> bool {
    let Node::Split {
        auto_axis,
        children,
        ..
    } = node
    else {
        return false;
    };
    if !*auto_axis && children.len() == 1 && children[0].node == Node::Leaf(target) {
        children.push(Child {
            weight: children[0].weight,
            node: Node::Leaf(inserted),
        });
        return true;
    }
    children
        .iter_mut()
        .any(|child| join_lone_manual_split(&mut child.node, target, inserted))
}

/// Insert in the same container without changing its share of the parent.
fn insert_sibling(node: &mut Node, anchor: SlotId, slot: SlotId) -> bool {
    let Node::Split { children, .. } = node else {
        return false;
    };
    if let Some(i) = children.iter().position(|c| c.node == Node::Leaf(anchor)) {
        children.insert(
            i + 1,
            Child {
                weight: average_weight(children),
                node: Node::Leaf(slot),
            },
        );
        return true;
    }
    children
        .iter_mut()
        .any(|child| insert_sibling(&mut child.node, anchor, slot))
}

fn validate_node(node: &Node, root: bool, leaves: &mut HashSet<SlotId>) -> Result<(), String> {
    match node {
        Node::Leaf(slot) => {
            if !leaves.insert(*slot) {
                return Err(format!("layout contains duplicate leaf {}", slot.0));
            }
        }
        Node::Split { children, .. } => {
            if !root && children.is_empty() {
                return Err("layout contains an empty nested split".into());
            }
            for child in children {
                if child.weight == 0 {
                    return Err("layout contains a zero-weight child".into());
                }
                validate_node(&child.node, false, leaves)?;
            }
        }
    }
    Ok(())
}

const fn direction_axis(direction: Direction) -> Axis {
    match direction {
        Direction::Left | Direction::Right => Axis::Horizontal,
        Direction::Down | Direction::Up => Axis::Vertical,
    }
}

fn split_pair(direction: Direction, moved: Node, anchor: Node) -> Node {
    let moved = Child {
        weight: 1,
        node: moved,
    };
    let anchor = Child {
        weight: 1,
        node: anchor,
    };
    Node::Split {
        axis: direction_axis(direction),
        auto_axis: false,
        children: if matches!(direction, Direction::Left | Direction::Up) {
            vec![moved, anchor]
        } else {
            vec![anchor, moved]
        },
    }
}

fn insert_relative(node: &mut Node, target: SlotId, moved: Node, direction: Direction) -> bool {
    let axis = direction_axis(direction);
    let before = matches!(direction, Direction::Left | Direction::Up);
    match node {
        Node::Leaf(existing) if *existing == target => {
            let target = std::mem::replace(node, Node::Leaf(target));
            *node = split_pair(direction, moved, target);
            true
        }
        Node::Leaf(_) => false,
        Node::Split {
            axis: parent_axis,
            auto_axis,
            children,
        } => {
            let Some(index) = children
                .iter()
                .position(|child| child.node.contains(target))
            else {
                return false;
            };
            if *parent_axis == axis
                && matches!(children[index].node, Node::Leaf(existing) if existing == target)
            {
                // Resize stores pixel-scale weights. A literal 1 here would
                // collapse the inserted tile beside those larger siblings.
                let weight = average_weight(children);
                children.insert(
                    index + usize::from(!before),
                    Child {
                        weight,
                        node: moved,
                    },
                );
                *auto_axis = false;
                true
            } else {
                insert_relative(&mut children[index].node, target, moved, direction)
            }
        }
    }
}

fn average_weight(children: &[Child]) -> u32 {
    let total = children
        .iter()
        .map(|child| u64::from(child.weight))
        .sum::<u64>();
    u32::try_from(total / children.len().max(1) as u64)
        .unwrap_or(u32::MAX)
        .max(1)
}

fn collect_slots(node: &Node, slots: &mut Vec<SlotId>) {
    match node {
        Node::Leaf(slot) => slots.push(*slot),
        Node::Split { children, .. } => {
            for child in children {
                collect_slots(&child.node, slots);
            }
        }
    }
}

fn leaf_weight(node: &Node, slot: SlotId) -> Option<u32> {
    let Node::Split { children, .. } = node else {
        return None;
    };
    for child in children {
        if matches!(child.node, Node::Leaf(existing) if existing == slot) {
            return Some(child.weight);
        }
        if let Some(weight) = leaf_weight(&child.node, slot) {
            return Some(weight);
        }
    }
    None
}

fn place(node: &Node, frame: Rect, placements: &mut Vec<(SlotId, Rect)>) {
    match node {
        Node::Leaf(slot) => placements.push((*slot, frame)),
        Node::Split { axis, children, .. } => {
            for (child, child_frame) in children
                .iter()
                .zip(partition_children(frame, *axis, children))
            {
                place(&child.node, child_frame, placements);
            }
        }
    }
}

fn partition_children(frame: Rect, axis: Axis, children: &[Child]) -> Vec<Rect> {
    let total = children
        .iter()
        .map(|child| u64::from(child.weight))
        .sum::<u64>();
    if total == 0 {
        return Vec::new();
    }
    let length = extent(frame, axis);
    let mut cursor = 0;
    let mut cumulative = 0_u64;
    children
        .iter()
        .enumerate()
        .map(|(index, child)| {
            cumulative += u64::from(child.weight);
            let end = if index + 1 == children.len() {
                length
            } else {
                (i64::from(length) * cumulative as i64 / total as i64) as i32
            };
            let result = match axis {
                Axis::Horizontal => Rect {
                    x: frame.x + cursor,
                    width: end - cursor,
                    ..frame
                },
                Axis::Vertical => Rect {
                    y: frame.y + cursor,
                    height: end - cursor,
                    ..frame
                },
            };
            cursor = end;
            result
        })
        .collect()
}

const fn extent(frame: Rect, axis: Axis) -> i32 {
    match axis {
        Axis::Horizontal => frame.width,
        Axis::Vertical => frame.height,
    }
}

const fn leading(frame: Rect, axis: Axis) -> i32 {
    match axis {
        Axis::Horizontal => frame.x,
        Axis::Vertical => frame.y,
    }
}

const fn trailing(frame: Rect, axis: Axis) -> i32 {
    leading(frame, axis) + extent(frame, axis)
}

impl Node {
    fn contains(&self, slot: SlotId) -> bool {
        match self {
            Self::Leaf(existing) => *existing == slot,
            Self::Split { children, .. } => children.iter().any(|child| child.node.contains(slot)),
        }
    }
}

fn swap_slots(node: &mut Node, first: SlotId, second: SlotId) {
    match node {
        Node::Leaf(slot) if *slot == first => *slot = second,
        Node::Leaf(slot) if *slot == second => *slot = first,
        Node::Leaf(_) => {}
        Node::Split { children, .. } => {
            for child in children {
                swap_slots(&mut child.node, first, second);
            }
        }
    }
}
