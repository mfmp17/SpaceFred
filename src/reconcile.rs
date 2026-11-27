use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};

use crate::{
    AppInstanceId, AppObservation, BindingState, Model, NativeObservation, PhysicalWindowId, Rect,
    Slot, SlotId, layout_tree::LayoutTree,
};

const GHOSTTY_REPLACEMENT_SETTLE_CYCLES: u64 = 3;

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct ReconciliationEvidence {
    #[serde(with = "crate::serde_map")]
    pub previous_placements: HashMap<SlotId, Rect>,
    /// Each slot's window frame as last observed. An app that clamps its
    /// window (a maximum size) never reaches the placement above, but its
    /// incoming native tab takes the frame the outgoing one had.
    #[serde(
        default,
        with = "crate::serde_map",
        skip_serializing_if = "HashMap::is_empty"
    )]
    pub previous_frames: HashMap<SlotId, Rect>,
}

impl ReconciliationEvidence {
    /// Remember an accepted cycle's targets and the frames it observed for
    /// the slots still bound afterwards.
    pub fn record(&mut self, observation: &NativeObservation, result: &Reconciliation) {
        self.previous_placements = result
            .placements
            .iter()
            .map(|placement| (placement.slot, placement.frame))
            .collect();
        let slots = &result.model.workspace.slots;
        self.previous_frames
            .retain(|id, _| slots.iter().any(|slot| slot.id == *id));
        for app in &observation.applications {
            let AppObservation::Complete { windows, .. } = app else {
                continue;
            };
            for window in windows {
                if let Some(slot) = slots
                    .iter()
                    .find(|slot| slot.bound_window() == Some(&window.id))
                {
                    self.previous_frames.insert(slot.id, window.frame);
                }
            }
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Settings {
    pub output: Rect,
    #[serde(default = "default_frame_tolerance")]
    pub frame_tolerance: i32,
    /// Gaps, outer padding and the lone-window width cap applied to every tile.
    #[serde(default, skip_serializing_if = "crate::spacing::Spacing::is_none")]
    pub spacing: crate::spacing::Spacing,
    /// How newly opened tiled windows split the workspace.
    #[serde(default, skip_serializing_if = "is_default_layout")]
    pub layout: crate::config::LayoutMode,
}

pub const DEFAULT_FRAME_TOLERANCE: i32 = 2;

fn is_default_layout(layout: &crate::config::LayoutMode) -> bool {
    *layout == crate::config::LayoutMode::default()
}

impl Settings {
    #[must_use]
    pub const fn for_output(output: Rect) -> Self {
        Self {
            output,
            frame_tolerance: DEFAULT_FRAME_TOLERANCE,
            spacing: crate::spacing::Spacing::NONE,
            layout: crate::config::LayoutMode::Dwindle,
        }
    }

    #[must_use]
    pub const fn spaced(self, spacing: crate::spacing::Spacing) -> Self {
        Self { spacing, ..self }
    }

    #[must_use]
    pub const fn with_layout(self, layout: crate::config::LayoutMode) -> Self {
        Self { layout, ..self }
    }

    /// The rect the layout partitions: the output less its outer padding.
    #[must_use]
    pub fn usable(&self) -> Rect {
        self.spacing.usable(self.output)
    }

    /// The rect the layout tree partitions: [`crate::spacing::Spacing::canvas`].
    #[must_use]
    pub fn canvas(&self) -> Rect {
        self.spacing.canvas(self.usable())
    }

    /// A lone tile's window frame: Fullscreen Layout's frame, and a single tiled window's.
    #[must_use]
    pub fn single(&self) -> Rect {
        self.spacing.single(self.usable())
    }
}

const fn default_frame_tolerance() -> i32 {
    DEFAULT_FRAME_TOLERANCE
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Placement {
    pub slot: SlotId,
    pub window: Option<PhysicalWindowId>,
    pub frame: Rect,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Decision {
    WindowInserted {
        slot: SlotId,
        window: PhysicalWindowId,
    },
    WindowRemoved {
        slot: SlotId,
        window: PhysicalWindowId,
    },
    NativeTabReplaced {
        slot: SlotId,
        previous: PhysicalWindowId,
        current: PhysicalWindowId,
    },
    NativeTabAmbiguous {
        app: AppInstanceId,
    },
    ObservationUnavailable {
        app: AppInstanceId,
        error: String,
    },
    ApplicationTerminated {
        app: AppInstanceId,
    },
    MouseDownDeferred,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Reconciliation {
    pub model: Model,
    pub placements: Vec<Placement>,
    pub decisions: Vec<Decision>,
}

#[must_use]
pub fn reconcile(
    model: &Model,
    observation: &NativeObservation,
    evidence: &ReconciliationEvidence,
    settings: &Settings,
) -> Reconciliation {
    if observation.mouse_down {
        let mut model = model.clone();
        reset_settling(&mut model, observation.cycle, None);
        return Reconciliation {
            placements: flat_placements(settings, &model.workspace.slots),
            model,
            decisions: vec![Decision::MouseDownDeferred],
        };
    }

    let mut next = model.clone();
    let mut decisions = Vec::new();

    for app_observation in &observation.applications {
        match app_observation {
            AppObservation::Unavailable { app, error } => {
                reset_settling(&mut next, observation.cycle, Some(app));
                decisions.push(Decision::ObservationUnavailable {
                    app: app.clone(),
                    error: error.clone(),
                });
            }
            AppObservation::Terminated { app } => {
                remove_app(&mut next, app, &mut decisions);
                next.continuity.remove(app);
                decisions.push(Decision::ApplicationTerminated { app: app.clone() });
            }
            AppObservation::Complete {
                app,
                focused,
                windows,
            } => {
                let context = AppReconcileContext {
                    focused: focused.as_ref(),
                    windows,
                    cycle: observation.cycle,
                    evidence,
                    settings,
                };
                if !reconcile_unresolved(&mut next, &app.id, &context, &mut decisions) {
                    let current: HashSet<_> =
                        windows.iter().map(|window| window.id.clone()).collect();
                    let existing: HashSet<_> = next
                        .workspace
                        .slots
                        .iter()
                        .filter(|slot| slot.belongs_to(&app.id))
                        .filter_map(|slot| slot.bound_window().cloned())
                        .collect();
                    let stale: Vec<_> = existing.difference(&current).cloned().collect();
                    let new: Vec<_> = current.difference(&existing).cloned().collect();

                    // Any app's native tab (Finder, Safari, Terminal...) can
                    // replace its window this way; only Ghostty also settles
                    // ambiguous evidence before retiring the slot.
                    let replacement = replacement_candidate(&next, &stale, &new, &context);

                    if replacement {
                        let previous = &stale[0];
                        let current = &new[0];
                        if let Some(slot) = next
                            .workspace
                            .slots
                            .iter_mut()
                            .find(|slot| slot.bound_window() == Some(previous))
                        {
                            slot.binding = BindingState::Bound(current.clone());
                            decisions.push(Decision::NativeTabReplaced {
                                slot: slot.id,
                                previous: previous.clone(),
                                current: current.clone(),
                            });
                        }
                    } else if app.bundle_id == "com.mitchellh.ghostty"
                        && !stale.is_empty()
                        && !new.is_empty()
                    {
                        for slot in &mut next.workspace.slots {
                            let Some(previous) = slot
                                .bound_window()
                                .filter(|window| stale.contains(window))
                                .cloned()
                            else {
                                continue;
                            };
                            slot.binding = BindingState::Unresolved {
                                previous,
                                candidates: new.clone(),
                                since: observation.cycle,
                            };
                        }
                        decisions.push(Decision::NativeTabAmbiguous {
                            app: app.id.clone(),
                        });
                    } else {
                        remove_windows(&mut next, &stale, &mut decisions);
                        insert_windows(&mut next, &new, &mut decisions);
                    }
                }

                // A snapshot with no focused window (focus mid native-tab switch, or on a
                // tab not listed yet) proves nothing about focus; keep the last one known so
                // the incoming tab still replaces the outgoing one (#718).
                if let Some(focused) = focused {
                    next.continuity
                        .entry(app.id.clone())
                        .or_default()
                        .previous_focused = Some(focused.clone());
                }
            }
        }
    }

    let placements = flat_placements(settings, &next.workspace.slots);
    Reconciliation {
        model: next,
        placements,
        decisions,
    }
}

struct AppReconcileContext<'a> {
    focused: Option<&'a PhysicalWindowId>,
    windows: &'a [crate::ObservedWindow],
    cycle: u64,
    evidence: &'a ReconciliationEvidence,
    settings: &'a Settings,
}

fn reconcile_unresolved(
    model: &mut Model,
    app: &AppInstanceId,
    context: &AppReconcileContext<'_>,
    decisions: &mut Vec<Decision>,
) -> bool {
    let unresolved: Vec<_> = model
        .workspace
        .slots
        .iter()
        .enumerate()
        .filter_map(|(index, slot)| {
            matches!(slot.binding, BindingState::Unresolved { .. })
                .then_some(index)
                .filter(|_| slot.belongs_to(app))
        })
        .collect();
    if unresolved.is_empty() {
        return false;
    }

    let bound: HashSet<_> = model
        .workspace
        .slots
        .iter()
        .filter_map(|slot| slot.bound_window().cloned())
        .collect();
    let candidates: Vec<_> = context
        .windows
        .iter()
        .filter(|window| !bound.contains(&window.id))
        .map(|window| window.id.clone())
        .collect();

    if unresolved.len() == 1 && candidates.len() == 1 && context.focused == candidates.first() {
        let index = unresolved[0];
        let slot = &model.workspace.slots[index];
        if candidate_matches_target(slot.id, &candidates[0], context) {
            let slot_id = slot.id;
            let previous = match &slot.binding {
                BindingState::Unresolved { previous, .. } => previous.clone(),
                BindingState::Bound(_) => unreachable!(),
            };
            model.workspace.slots[index].binding = BindingState::Bound(candidates[0].clone());
            decisions.push(Decision::NativeTabReplaced {
                slot: slot_id,
                previous,
                current: candidates[0].clone(),
            });
            return true;
        }
    }

    let expired = unresolved.iter().all(|index| {
        let BindingState::Unresolved { since, .. } = &model.workspace.slots[*index].binding else {
            unreachable!()
        };
        context.cycle.saturating_sub(*since) >= GHOSTTY_REPLACEMENT_SETTLE_CYCLES
    });
    if expired {
        model.workspace.slots.retain(|slot| match &slot.binding {
            BindingState::Unresolved { previous, .. } if slot.belongs_to(app) => {
                decisions.push(Decision::WindowRemoved {
                    slot: slot.id,
                    window: previous.clone(),
                });
                false
            }
            _ => true,
        });
        insert_windows(model, &candidates, decisions);
        return true;
    }

    for index in unresolved {
        if let BindingState::Unresolved {
            candidates: existing,
            ..
        } = &mut model.workspace.slots[index].binding
        {
            *existing = candidates.clone();
        }
    }
    decisions.push(Decision::NativeTabAmbiguous { app: app.clone() });
    true
}

fn reset_settling(model: &mut Model, cycle: u64, app: Option<&AppInstanceId>) {
    for slot in model
        .workspace
        .slots
        .iter_mut()
        .filter(|slot| app.is_none_or(|app| slot.belongs_to(app)))
    {
        if let BindingState::Unresolved { since, .. } = &mut slot.binding {
            *since = cycle;
        }
    }
}

/// Whether reconciling an app's `windows` against `model` would hand the app's one
/// missing tile to its `focused` window as an unambiguous native-tab replacement.
/// `unbound` is the app's windows no slot holds yet, as reconciliation would see them.
/// Routing asks first, so a rule that routes every new window doesn't take the
/// incoming tab away from its slot (#509).
#[must_use]
pub fn replaces_native_tab(
    model: &Model,
    evidence: &ReconciliationEvidence,
    settings: &Settings,
    windows: &[crate::ObservedWindow],
    unbound: &[PhysicalWindowId],
    focused: Option<&PhysicalWindowId>,
) -> bool {
    let Some(app) = focused.map(|window| &window.app) else {
        return false;
    };
    let slots = move || {
        model
            .workspace
            .slots
            .iter()
            .filter(move |slot| slot.belongs_to(app))
    };
    if slots().any(|slot| matches!(slot.binding, BindingState::Unresolved { .. })) {
        return false;
    }
    let stale: Vec<_> = slots()
        .filter_map(Slot::bound_window)
        .filter(|bound| !windows.iter().any(|window| &window.id == *bound))
        .cloned()
        .collect();
    let context = AppReconcileContext {
        focused,
        windows,
        cycle: 0,
        evidence,
        settings,
    };
    replacement_candidate(model, &stale, unbound, &context)
}

/// Whether `candidate`, a window of its app that no slot holds yet, sits where one of the
/// app's tiles is (that tile's target or last observed frame), while the tile's window is
/// gone from `windows` or still listed at that same frame: a native-tab switch caught
/// mid-handoff, before the outgoing tab leaves the list or focus reaches the incoming one,
/// so [`replaces_native_tab`] can't tell yet. Routing holds it a moment instead of sending
/// it to a rule's workspace (#706).
#[must_use]
pub fn native_tab_handoff_pending(
    model: &Model,
    evidence: &ReconciliationEvidence,
    settings: &Settings,
    candidate: &crate::ObservedWindow,
    windows: &[crate::ObservedWindow],
) -> bool {
    let app = &candidate.id.app;
    let tolerance = settings.frame_tolerance;
    let mut slots = model
        .workspace
        .slots
        .iter()
        .filter(|slot| slot.belongs_to(app));
    if slots
        .clone()
        .any(|slot| matches!(slot.binding, BindingState::Unresolved { .. }))
    {
        return false;
    }
    slots.any(|slot| {
        let Some(bound) = slot.bound_window() else {
            return false;
        };
        let at_tile = [
            evidence.previous_placements.get(&slot.id),
            evidence.previous_frames.get(&slot.id),
        ]
        .into_iter()
        .flatten()
        .any(|known| candidate.frame.approximately_equals(*known, tolerance));
        at_tile
            && windows
                .iter()
                .find(|window| &window.id == bound)
                .is_none_or(|window| {
                    window
                        .frame
                        .approximately_equals(candidate.frame, tolerance)
                })
    })
}

fn replacement_candidate(
    model: &Model,
    stale: &[PhysicalWindowId],
    new: &[PhysicalWindowId],
    context: &AppReconcileContext<'_>,
) -> bool {
    if stale.len() != 1 || new.len() != 1 || context.focused != new.first() {
        return false;
    }
    if model
        .continuity
        .get(&stale[0].app)
        .and_then(|value| value.previous_focused.as_ref())
        != stale.first()
    {
        return false;
    }
    let Some(slot) = model
        .workspace
        .slots
        .iter()
        .find(|slot| slot.bound_window() == stale.first())
    else {
        return false;
    };
    candidate_matches_target(slot.id, &new[0], context)
}

/// The candidate sits where the slot's window was asked to go, or, when the
/// app clamped that request, exactly where the outgoing window was last seen.
fn candidate_matches_target(
    slot: SlotId,
    candidate: &PhysicalWindowId,
    context: &AppReconcileContext<'_>,
) -> bool {
    let Some(frame) = context
        .windows
        .iter()
        .find(|window| &window.id == candidate)
        .map(|window| window.frame)
    else {
        return false;
    };
    let evidence = context.evidence;
    [
        evidence.previous_placements.get(&slot),
        evidence.previous_frames.get(&slot),
    ]
    .into_iter()
    .flatten()
    .any(|known| frame.approximately_equals(*known, context.settings.frame_tolerance))
}

fn remove_app(model: &mut Model, app: &AppInstanceId, decisions: &mut Vec<Decision>) {
    model.workspace.slots.retain(|slot| {
        if !slot.belongs_to(app) {
            return true;
        }
        if let Some(window) = slot.bound_window().cloned() {
            decisions.push(Decision::WindowRemoved {
                slot: slot.id,
                window,
            });
        }
        false
    });
}

fn remove_windows(model: &mut Model, windows: &[PhysicalWindowId], decisions: &mut Vec<Decision>) {
    model.workspace.slots.retain(|slot| {
        let Some(window) = slot
            .bound_window()
            .filter(|window| windows.contains(window))
            .cloned()
        else {
            return true;
        };
        decisions.push(Decision::WindowRemoved {
            slot: slot.id,
            window,
        });
        false
    });
}

fn insert_windows(model: &mut Model, windows: &[PhysicalWindowId], decisions: &mut Vec<Decision>) {
    let mut windows = windows.to_vec();
    windows.sort_by_key(|window| window.window_id);
    for window in windows {
        let slot_id = SlotId(model.next_slot_id);
        model.next_slot_id += 1;
        model.workspace.slots.push(Slot {
            id: slot_id,
            binding: BindingState::Bound(window.clone()),
        });
        decisions.push(Decision::WindowInserted {
            slot: slot_id,
            window,
        });
    }
}

/// An even row of the slots: what a model alone, with no layout tree, places.
/// The engine replaces these with its own layout's placements.
fn flat_placements(settings: &Settings, slots: &[Slot]) -> Vec<Placement> {
    let cells = LayoutTree::flat(crate::Axis::Horizontal, slots.iter().map(|slot| slot.id))
        .placements(settings.canvas());
    slots
        .iter()
        .zip(settings.spacing.place(settings.usable(), cells))
        .map(|(slot, (slot_id, frame))| Placement {
            slot: slot_id,
            window: slot.bound_window().cloned(),
            frame,
        })
        .collect()
}

#[cfg(test)]
mod tests;
