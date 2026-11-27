use std::collections::HashMap;

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
pub struct AppInstanceId {
    pub pid: i32,
    pub launched_at: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
pub struct PhysicalWindowId {
    pub app: AppInstanceId,
    pub window_id: u32,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
pub struct SlotId(pub u64);

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum BindingState {
    Bound(PhysicalWindowId),
    Unresolved {
        previous: PhysicalWindowId,
        candidates: Vec<PhysicalWindowId>,
        since: u64,
    },
}

/// A tile's identity and the window bound to it. Its size lives only in the
/// workspace's layout tree; the saved state projects it back out per slot.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Slot {
    pub id: SlotId,
    pub binding: BindingState,
}

impl Slot {
    #[must_use]
    pub fn bound_window(&self) -> Option<&PhysicalWindowId> {
        match &self.binding {
            BindingState::Bound(window) => Some(window),
            BindingState::Unresolved { .. } => None,
        }
    }

    #[must_use]
    pub fn belongs_to(&self, app: &AppInstanceId) -> bool {
        &self.identity_window().app == app
    }

    #[must_use]
    pub(crate) fn identity_window(&self) -> &PhysicalWindowId {
        match &self.binding {
            BindingState::Bound(window) => window,
            BindingState::Unresolved { previous, .. } => previous,
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct AppContinuity {
    pub previous_focused: Option<PhysicalWindowId>,
}

/// A workspace's tiles in slot order. Its split axis and weights live only in
/// the engine's layout tree.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct Workspace {
    pub slots: Vec<Slot>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Model {
    pub workspace: Workspace,
    #[serde(with = "crate::serde_map")]
    pub continuity: HashMap<AppInstanceId, AppContinuity>,
    pub next_slot_id: u64,
}

impl Default for Model {
    fn default() -> Self {
        Self {
            workspace: Workspace::default(),
            continuity: HashMap::new(),
            next_slot_id: 1,
        }
    }
}

impl Model {
    #[must_use]
    pub(crate) fn slot(&self, id: SlotId) -> Option<&Slot> {
        self.workspace.slots.iter().find(|slot| slot.id == id)
    }

    pub fn validate(&self) -> Result<(), String> {
        let mut slot_ids = std::collections::HashSet::new();
        let mut windows = std::collections::HashSet::new();
        for slot in &self.workspace.slots {
            if !slot_ids.insert(slot.id) {
                return Err(format!("duplicate slot ID {}", slot.id.0));
            }
            let window = slot.identity_window();
            if !windows.insert(window) {
                return Err(format!(
                    "window {} is owned more than once",
                    window.window_id
                ));
            }
            match &slot.binding {
                BindingState::Bound(_) => {}
                BindingState::Unresolved {
                    previous,
                    candidates,
                    ..
                } => {
                    if candidates
                        .iter()
                        .any(|candidate| candidate.app != previous.app)
                    {
                        return Err(format!(
                            "slot {} has cross-application candidates",
                            slot.id.0
                        ));
                    }
                }
            }
        }
        if self
            .workspace
            .slots
            .iter()
            .any(|slot| slot.id.0 >= self.next_slot_id)
        {
            return Err("next_slot_id does not exceed every existing slot ID".into());
        }
        Ok(())
    }
}
