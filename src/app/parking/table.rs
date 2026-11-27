//! Every parked window's recovery record and parking phase, plus the parking policy of
//! windows that keep un-parking themselves.
//!
//! One record per parked window holds everything parking knows about it, so a window cannot
//! carry a settle timestamp, a re-park count, a pending summon or a display re-park without a
//! recovery frame. Time is passed in; nothing here reads a clock.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use crate::{PhysicalWindowId, Rect};

/// How long a park or restore write may still be applying inside the app. Until then an
/// on-screen observation of the window is not trusted as a genuine reappearance.
pub(in crate::app) const PARK_SETTLE_GRACE: Duration = Duration::from_secs(2);
/// Two confirmed self-un-parks within this window exempt the window from parking: its
/// application insists on managing it, and fighting back loses.
const PARKING_EXEMPT_WINDOW: Duration = Duration::from_secs(300);

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::app) struct ParkRecord {
    /// Where the window returns to. The only part persisted to `parked-windows.json`.
    pub(in crate::app) original: Rect,
    /// The display `original` was recorded on. In memory only: a record loaded from disk has
    /// none and restores by its frame alone.
    pub(in crate::app) original_display: Option<String>,
    /// The display arrangement the window was last parked against.
    topology: u64,
    phase: ParkPhase,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ParkPhase {
    /// Recorded and off-screen (or not yet moved). `reparks` counts re-parks the app undid
    /// in a row.
    Resting { reparks: u8 },
    /// A park write applied at `issued_at` may still be landing. `from` is where the window
    /// was on screen when a first park was issued (in memory only), which a stale readback
    /// still reports; none for a re-park or a park whose starting frame is not known.
    Settling {
        issued_at: Instant,
        reparks: u8,
        from: Option<Rect>,
    },
    /// A restore write applied at `last_write_at` awaits window-server confirmation. A
    /// scratchpad summon restores to `destination` rather than the recovery frame.
    Restoring {
        last_write_at: Instant,
        destination: Option<Rect>,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum OnScreenPolicy {
    /// The window un-parked itself at this time.
    Healed(Instant),
    /// It keeps un-parking itself; it stays visible across switches for the session.
    Exempt,
}

#[derive(Debug, Default)]
pub(in crate::app) struct ParkTable {
    records: HashMap<PhysicalWindowId, ParkRecord>,
    policies: HashMap<PhysicalWindowId, OnScreenPolicy>,
    /// Bumped on every display change; a record parked against an older one awaits re-park.
    topology: u64,
}

impl ParkRecord {
    fn new(original: Rect, original_display: Option<String>, topology: u64) -> Self {
        Self {
            original,
            original_display,
            topology,
            phase: ParkPhase::Resting { reparks: 0 },
        }
    }

    /// The record as recovery data alone, with no write in flight.
    #[must_use]
    pub(in crate::app) fn resting(self) -> Self {
        Self {
            phase: ParkPhase::Resting { reparks: 0 },
            ..self
        }
    }

    fn reparks(&self) -> u8 {
        match self.phase {
            ParkPhase::Resting { reparks } | ParkPhase::Settling { reparks, .. } => reparks,
            ParkPhase::Restoring { .. } => 0,
        }
    }

    fn last_write(&self) -> Option<Instant> {
        match self.phase {
            ParkPhase::Resting { .. } => None,
            ParkPhase::Settling { issued_at, .. } => Some(issued_at),
            ParkPhase::Restoring { last_write_at, .. } => Some(last_write_at),
        }
    }
}

impl From<HashMap<PhysicalWindowId, Rect>> for ParkTable {
    fn from(frames: HashMap<PhysicalWindowId, Rect>) -> Self {
        Self {
            records: frames
                .into_iter()
                .map(|(window, original)| (window, ParkRecord::new(original, None, 0)))
                .collect(),
            ..Self::default()
        }
    }
}

impl std::ops::Index<&PhysicalWindowId> for ParkTable {
    type Output = Rect;

    fn index(&self, window: &PhysicalWindowId) -> &Rect {
        &self.records[window].original
    }
}

impl ParkTable {
    #[must_use]
    pub(in crate::app) fn is_empty(&self) -> bool {
        self.records.is_empty()
    }

    #[must_use]
    pub(in crate::app) fn len(&self) -> usize {
        self.records.len()
    }

    #[must_use]
    pub(in crate::app) fn contains_key(&self, window: &PhysicalWindowId) -> bool {
        self.records.contains_key(window)
    }

    /// The window's recovery frame.
    #[must_use]
    pub(in crate::app) fn get(&self, window: &PhysicalWindowId) -> Option<&Rect> {
        self.records.get(window).map(|record| &record.original)
    }

    #[must_use]
    pub(in crate::app) fn record(&self, window: &PhysicalWindowId) -> Option<&ParkRecord> {
        self.records.get(window)
    }

    pub(in crate::app) fn keys(&self) -> impl Iterator<Item = &PhysicalWindowId> {
        self.records.keys()
    }

    pub(in crate::app) fn iter(&self) -> impl Iterator<Item = (&PhysicalWindowId, &Rect)> {
        self.records
            .iter()
            .map(|(window, record)| (window, &record.original))
    }

    /// The recovery frames, as `parked-windows.json` stores them.
    #[must_use]
    pub(in crate::app) fn recovery_frames(&self) -> HashMap<PhysicalWindowId, Rect> {
        self.iter()
            .map(|(window, frame)| (window.clone(), *frame))
            .collect()
    }

    /// Record `original` as the window's recovery frame, keeping the phase of an existing
    /// record. Returns the record it replaced.
    pub(in crate::app) fn insert(
        &mut self,
        window: PhysicalWindowId,
        original: Rect,
        original_display: Option<String>,
    ) -> Option<ParkRecord> {
        let record = match self.records.get(&window) {
            Some(previous) => ParkRecord {
                original,
                original_display,
                ..previous.clone()
            },
            None => ParkRecord::new(original, original_display, self.topology),
        };
        self.records.insert(window, record)
    }

    /// Put back a record exactly as it was, after a failed persist.
    pub(in crate::app) fn put(&mut self, window: PhysicalWindowId, record: ParkRecord) {
        self.records.insert(window, record);
    }

    pub(in crate::app) fn remove(&mut self, window: &PhysicalWindowId) -> Option<ParkRecord> {
        self.records.remove(window)
    }

    /// A park write landed: the window settles against the current displays.
    pub(in crate::app) fn park_applied(&mut self, window: &PhysicalWindowId, now: Instant) {
        self.park_issued(window, now);
        self.display_settled(window);
    }

    /// A park write landed for a window that was on screen at `from`.
    pub(in crate::app) fn park_applied_from(
        &mut self,
        window: &PhysicalWindowId,
        now: Instant,
        from: Rect,
    ) {
        self.settle(window, now, 0, Some(from));
        self.display_settled(window);
    }

    /// A park was issued but may not have landed (mid Space slide); the window settles,
    /// still due any re-park for the current displays.
    pub(in crate::app) fn park_issued(&mut self, window: &PhysicalWindowId, now: Instant) {
        self.settle(window, now, 0, None);
    }

    /// A re-park of a window the app moved back on screen landed.
    pub(in crate::app) fn repark_applied(&mut self, window: &PhysicalWindowId, now: Instant) {
        self.settle(window, now, 1, None);
        self.display_settled(window);
    }

    fn settle(
        &mut self,
        window: &PhysicalWindowId,
        issued_at: Instant,
        more_reparks: u8,
        from: Option<Rect>,
    ) {
        if let Some(record) = self.records.get_mut(window) {
            record.phase = ParkPhase::Settling {
                issued_at,
                reparks: record.reparks().saturating_add(more_reparks),
                from,
            };
        }
    }

    /// A restore write landed; it is confirmed (and the record removed) once the window server
    /// reports the window there. A pending summon keeps its destination.
    pub(in crate::app) fn restore_written(&mut self, window: &PhysicalWindowId, now: Instant) {
        let destination = self.summon_destination(window);
        self.restoring(window, now, destination);
    }

    /// A scratchpad summon wrote `destination`; it stays pending until confirmed there.
    pub(in crate::app) fn summon_written(
        &mut self,
        window: &PhysicalWindowId,
        destination: Rect,
        now: Instant,
    ) {
        self.restoring(window, now, Some(destination));
    }

    fn restoring(
        &mut self,
        window: &PhysicalWindowId,
        last_write_at: Instant,
        destination: Option<Rect>,
    ) {
        if let Some(record) = self.records.get_mut(window) {
            record.phase = ParkPhase::Restoring {
                last_write_at,
                destination,
            };
        }
    }

    /// Move a pending summon's destination (its workspace moved to another display).
    pub(in crate::app) fn move_summon(&mut self, window: &PhysicalWindowId, destination: Rect) {
        if let Some(ParkRecord {
            phase:
                ParkPhase::Restoring {
                    destination: Some(pending),
                    ..
                },
            ..
        }) = self.records.get_mut(window)
        {
            *pending = destination;
        }
    }

    /// The window left the scratchpad: a summon still landing is an ordinary restore now.
    pub(in crate::app) fn cancel_summon(&mut self, window: &PhysicalWindowId) {
        if let Some(ParkRecord {
            phase: ParkPhase::Restoring { destination, .. },
            ..
        }) = self.records.get_mut(window)
        {
            *destination = None;
        }
    }

    /// The centered frame a scratchpad summon is still waiting to reach.
    #[must_use]
    pub(in crate::app) fn summon_destination(&self, window: &PhysicalWindowId) -> Option<Rect> {
        match self.records.get(window)?.phase {
            ParkPhase::Restoring { destination, .. } => destination,
            _ => None,
        }
    }

    /// When the latest park or restore write for the window landed.
    #[must_use]
    pub(in crate::app) fn last_write(&self, window: &PhysicalWindowId) -> Option<Instant> {
        self.records.get(window).and_then(ParkRecord::last_write)
    }

    /// Whether the latest write may still be landing inside the app.
    #[must_use]
    pub(in crate::app) fn settling(&self, window: &PhysicalWindowId, now: Instant) -> bool {
        self.last_write(window)
            .is_some_and(|at| now.saturating_duration_since(at) < PARK_SETTLE_GRACE)
    }

    /// Where the window was on screen when its first park was issued, while that park is
    /// still settling. None after a re-park the app undid, or when the frame is unknown.
    #[must_use]
    pub(in crate::app) fn settling_from(
        &self,
        window: &PhysicalWindowId,
        now: Instant,
    ) -> Option<Rect> {
        if !self.settling(window, now) {
            return None;
        }
        match self.records.get(window)?.phase {
            ParkPhase::Settling {
                reparks: 0,
                from: Some(from),
                ..
            } => Some(from),
            _ => None,
        }
    }

    /// The window is off-screen; once it has stayed there past the settle grace, the app has
    /// stopped undoing its parks.
    pub(in crate::app) fn stayed_parked(&mut self, window: &PhysicalWindowId, now: Instant) {
        if self.settling(window, now) {
            return;
        }
        if let Some(record) = self.records.get_mut(window)
            && matches!(
                record.phase,
                ParkPhase::Settling { .. } | ParkPhase::Resting { .. }
            )
        {
            record.phase = ParkPhase::Resting { reparks: 0 };
        }
    }

    /// Re-parks the app undid in a row.
    #[must_use]
    pub(in crate::app) fn reparks(&self, window: &PhysicalWindowId) -> u8 {
        self.records.get(window).map_or(0, ParkRecord::reparks)
    }

    /// The displays changed: every parked window is due a re-park against them, and earlier
    /// undone re-parks no longer count. Returns whether one was already due.
    pub(in crate::app) fn displays_changed(&mut self) -> bool {
        let already_due = self.awaiting_display().next().is_some();
        self.topology += 1;
        for record in self.records.values_mut() {
            if let ParkPhase::Resting { reparks } | ParkPhase::Settling { reparks, .. } =
                &mut record.phase
            {
                *reparks = 0;
            }
        }
        already_due
    }

    /// Windows parked against displays that have since changed.
    pub(in crate::app) fn awaiting_display(&self) -> impl Iterator<Item = &PhysicalWindowId> {
        self.records
            .iter()
            .filter(|(_, record)| record.topology != self.topology)
            .map(|(window, _)| window)
    }

    #[must_use]
    pub(in crate::app) fn awaits_display(&self, window: &PhysicalWindowId) -> bool {
        self.records
            .get(window)
            .is_some_and(|record| record.topology != self.topology)
    }

    /// The window needs no re-park for the current displays.
    pub(in crate::app) fn display_settled(&mut self, window: &PhysicalWindowId) {
        let topology = self.topology;
        if let Some(record) = self.records.get_mut(window) {
            record.topology = topology;
        }
    }

    /// The window un-parked itself and was re-homed. Returns true when it did so twice within
    /// [`PARKING_EXEMPT_WINDOW`], which exempts it from parking for the session.
    pub(in crate::app) fn note_heal(&mut self, window: &PhysicalWindowId, now: Instant) -> bool {
        self.policies.retain(|_, policy| match policy {
            OnScreenPolicy::Healed(at) => {
                now.saturating_duration_since(*at) < PARKING_EXEMPT_WINDOW
            }
            OnScreenPolicy::Exempt => true,
        });
        let repeated = self.policies.contains_key(window);
        let policy = if repeated {
            OnScreenPolicy::Exempt
        } else {
            OnScreenPolicy::Healed(now)
        };
        self.policies.insert(window.clone(), policy);
        repeated
    }

    #[must_use]
    pub(in crate::app) fn is_exempt(&self, window: &PhysicalWindowId) -> bool {
        self.policies.get(window) == Some(&OnScreenPolicy::Exempt)
    }

    pub(in crate::app) fn exempt_windows(&self) -> impl Iterator<Item = &PhysicalWindowId> {
        self.policies
            .iter()
            .filter(|(_, policy)| **policy == OnScreenPolicy::Exempt)
            .map(|(window, _)| window)
    }

    /// The window left management; a later heal starts its history afresh. An exemption
    /// lasts the session.
    pub(in crate::app) fn forget_heals(&mut self, window: &PhysicalWindowId) {
        if let Some(OnScreenPolicy::Healed(_)) = self.policies.get(window) {
            self.policies.remove(window);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::AppInstanceId;

    fn window(id: u32) -> PhysicalWindowId {
        PhysicalWindowId {
            app: AppInstanceId {
                pid: 1,
                launched_at: 1,
            },
            window_id: id,
        }
    }

    fn frame() -> Rect {
        Rect {
            x: 10,
            y: 20,
            width: 300,
            height: 200,
        }
    }

    #[test]
    fn a_settle_grace_covers_park_and_restore_writes_only_until_it_expires() {
        let start = Instant::now();
        let mut table = ParkTable::default();
        let w = window(1);
        table.insert(w.clone(), frame(), None);
        assert!(!table.settling(&w, start));
        table.park_applied(&w, start);
        assert!(table.settling(&w, start + Duration::from_secs(1)));
        assert!(!table.settling(&w, start + PARK_SETTLE_GRACE));
        table.restore_written(&w, start + PARK_SETTLE_GRACE);
        assert!(table.settling(&w, start + PARK_SETTLE_GRACE));
    }

    #[test]
    fn undone_reparks_count_until_the_window_stays_parked_or_displays_change() {
        let start = Instant::now();
        let mut table = ParkTable::default();
        let w = window(1);
        table.insert(w.clone(), frame(), None);
        table.repark_applied(&w, start);
        table.repark_applied(&w, start);
        table.stayed_parked(&w, start + Duration::from_secs(1));
        assert_eq!(table.reparks(&w), 2);
        table.stayed_parked(&w, start + PARK_SETTLE_GRACE);
        assert_eq!(table.reparks(&w), 0);
        table.repark_applied(&w, start);
        assert!(!table.displays_changed());
        assert_eq!(table.reparks(&w), 0);
    }

    #[test]
    fn a_display_change_leaves_each_record_due_until_it_is_parked_again() {
        let mut table = ParkTable::default();
        let (a, b) = (window(1), window(2));
        table.insert(a.clone(), frame(), None);
        table.insert(b.clone(), frame(), None);
        table.displays_changed();
        assert!(table.awaits_display(&a) && table.awaits_display(&b));
        table.park_applied(&a, Instant::now());
        table.display_settled(&b);
        assert_eq!(table.awaiting_display().count(), 0);
        table.insert(window(3), frame(), None);
        assert!(!table.awaits_display(&window(3)));
    }

    #[test]
    fn a_summon_destination_lives_and_dies_with_the_record() {
        let now = Instant::now();
        let mut table = ParkTable::default();
        let w = window(1);
        let centered = Rect { x: 500, ..frame() };
        table.summon_written(&w, centered, now);
        assert_eq!(table.summon_destination(&w), None, "no record, no summon");
        table.insert(w.clone(), frame(), None);
        table.summon_written(&w, centered, now);
        table.restore_written(&w, now);
        assert_eq!(table.summon_destination(&w), Some(centered));
        table.park_applied(&w, now);
        assert_eq!(table.summon_destination(&w), None);
        table.summon_written(&w, centered, now);
        table.remove(&w);
        assert_eq!(table.summon_destination(&w), None);
    }

    #[test]
    fn a_second_heal_within_the_window_exempts_and_a_late_one_does_not() {
        let start = Instant::now();
        let mut table = ParkTable::default();
        let (w, late) = (window(1), window(2));
        assert!(!table.note_heal(&w, start));
        assert!(table.note_heal(&w, start + Duration::from_secs(10)));
        assert!(table.is_exempt(&w));
        assert!(!table.note_heal(&late, start));
        assert!(!table.note_heal(&late, start + PARKING_EXEMPT_WINDOW));
        table.forget_heals(&w);
        assert!(table.is_exempt(&w));
    }
}
