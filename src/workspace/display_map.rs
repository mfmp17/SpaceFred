//! Which workspace each display shows.
//!
//! A lane is a display selector's active workspace. Two selectors can name one
//! physical display (secondary and that display's UUID), and the display a
//! selector names changes when one is unplugged, so lanes alone cannot say what
//! a display shows. Each physical display therefore records, by its display ID,
//! the lane it brought forward last and when. That record outlives an unplug,
//! so a reconnected display shows what it showed (#404), it is saved with the
//! workspace state so a restart does too (#386), and showing a workspace always
//! writes its lane and the display's record together, so they cannot disagree.

use std::collections::{BTreeMap, HashMap};

use super::{DisplaySelector, WorkspaceId};

#[derive(Clone, Debug, Default)]
pub(crate) struct DisplayMap {
    lanes: HashMap<DisplaySelector, WorkspaceId>,
    previous: HashMap<DisplaySelector, WorkspaceId>,
    /// Per physical display ID: the lane it shows, and when it was brought forward.
    fronts: BTreeMap<String, Front>,
    clock: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct Front {
    lane: DisplaySelector,
    at: u64,
}

impl DisplayMap {
    pub(crate) fn new(lanes: impl IntoIterator<Item = (DisplaySelector, WorkspaceId)>) -> Self {
        Self {
            lanes: lanes.into_iter().collect(),
            ..Self::default()
        }
    }

    pub(crate) fn lane(&self, lane: &DisplaySelector) -> Option<WorkspaceId> {
        self.lanes.get(lane).copied()
    }

    pub(crate) fn lanes(&self) -> impl Iterator<Item = (&DisplaySelector, WorkspaceId)> {
        self.lanes
            .iter()
            .map(|(lane, workspace)| (lane, *workspace))
    }

    /// Makes workspace the lane's active one, returning the one it replaced.
    pub(crate) fn set_lane(
        &mut self,
        lane: DisplaySelector,
        workspace: WorkspaceId,
    ) -> Option<WorkspaceId> {
        self.lanes.insert(lane, workspace)
    }

    pub(crate) fn remove_lane(&mut self, lane: &DisplaySelector) {
        self.lanes.remove(lane);
    }

    pub(crate) fn previous(&self, lane: &DisplaySelector) -> Option<WorkspaceId> {
        self.previous.get(lane).copied()
    }

    pub(crate) fn previous_lanes(&self) -> impl Iterator<Item = (&DisplaySelector, WorkspaceId)> {
        self.previous
            .iter()
            .map(|(lane, workspace)| (lane, *workspace))
    }

    pub(crate) fn set_previous(&mut self, lane: DisplaySelector, workspace: WorkspaceId) {
        self.previous.insert(lane, workspace);
    }

    pub(crate) fn remove_previous(&mut self, lane: &DisplaySelector) {
        self.previous.remove(lane);
    }

    /// The display brought the lane forward just now.
    pub(crate) fn bring_forward(&mut self, display: &str, lane: DisplaySelector) {
        self.clock += 1;
        self.fronts.insert(
            display.to_owned(),
            Front {
                lane,
                at: self.clock,
            },
        );
    }

    /// Records what a display shows when it has no record yet (first seen).
    pub(crate) fn seed_front(&mut self, display: &str, lane: &DisplaySelector) {
        if !self.fronts.contains_key(display) {
            self.bring_forward(display, lane.clone());
        }
    }

    /// The display no longer shows any lane of its own.
    pub(crate) fn forget_front(&mut self, display: &str) {
        self.fronts.remove(display);
    }

    /// Every display's record: its ID, its lane and when it came forward.
    pub(crate) fn fronts(&self) -> impl Iterator<Item = (&str, &DisplaySelector, u64)> {
        self.fronts
            .iter()
            .map(|(display, front)| (display.as_str(), &front.lane, front.at))
    }

    /// A lane renamed in place (its workspace now assigned by another selector
    /// naming the same display): displays showing it keep showing it.
    pub(crate) fn rename_front_lanes(&mut self, from: &DisplaySelector, to: &DisplaySelector) {
        for front in self.fronts.values_mut() {
            if &front.lane == from {
                front.lane = to.clone();
            }
        }
    }

    /// The records oldest first: the persisted order is the recency.
    pub(crate) fn fronts_in_order(&self) -> Vec<(String, DisplaySelector)> {
        let mut fronts: Vec<_> = self.fronts.iter().collect();
        fronts.sort_by_key(|(_, front)| front.at);
        fronts
            .into_iter()
            .map(|(display, front)| (display.clone(), front.lane.clone()))
            .collect()
    }

    /// Restores records saved oldest first.
    pub(crate) fn restore_fronts(&mut self, fronts: Vec<(String, DisplaySelector)>) {
        for (display, lane) in fronts {
            if self.lanes.contains_key(&lane) {
                self.bring_forward(&display, lane);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn records_keep_their_recency_through_a_save() {
        let mut map = DisplayMap::new([
            (DisplaySelector::Main, WorkspaceId::ONE),
            (DisplaySelector::Secondary, WorkspaceId::SIX),
        ]);
        map.bring_forward("b", DisplaySelector::Secondary);
        map.bring_forward("a", DisplaySelector::Main);
        map.seed_front("a", &DisplaySelector::Secondary);
        let saved = map.fronts_in_order();
        assert_eq!(
            saved,
            [
                ("b".to_owned(), DisplaySelector::Secondary),
                ("a".to_owned(), DisplaySelector::Main)
            ]
        );
        let mut restored = DisplayMap::new(map.lanes().map(|(lane, ws)| (lane.clone(), ws)));
        restored.restore_fronts(saved.clone());
        assert_eq!(restored.fronts_in_order(), saved);
    }

    #[test]
    fn a_record_for_a_lane_that_no_longer_exists_is_dropped_on_restore() {
        let mut map = DisplayMap::new([(DisplaySelector::Main, WorkspaceId::ONE)]);
        map.restore_fronts(vec![("b".to_owned(), DisplaySelector::Secondary)]);
        assert_eq!(map.fronts().count(), 0);
    }
}
