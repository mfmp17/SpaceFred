//! Window disposition as a composable patch.
//!
//! Three sources say how a window is handled: the first matching user `[[rules]]` entry, the
//! built-in defaults ([`crate::defaults`]), and the window's own traits (a fixed-size panel
//! floats). Each is a [`DispositionPatch`] that may set how the window is managed and which
//! workspace it opens on, independently. [`DispositionPatch::over`] merges them left-biased,
//! field by field, so `rule.over(default).over(traits)` keeps the rule's choice wherever it
//! made one and falls through to the default and then the traits elsewhere: a `route` rule
//! for a utility that floats by default floats it on the routed workspace.

use crate::WorkspaceId;

use super::{Config, WindowDisposition};

/// How a window is managed, when a source decides it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Management {
    /// Tiled even when it looks like a dialog or non-resizable panel.
    Tile,
    /// Native size, owned by one workspace: parks and restores with it.
    Float,
    /// Floating, and shown on every workspace of its display.
    Pin,
    /// Never tiled, owned, moved, or parked.
    Ignore,
}

/// What one source says about a window. Merging with [`Self::over`] is associative and
/// [`Self::NONE`] is its identity, so any number of sources fold in precedence order.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct DispositionPatch {
    pub management: Option<Management>,
    /// The workspace a newly seen window opens on.
    pub home: Option<WorkspaceId>,
}

impl DispositionPatch {
    pub const NONE: Self = Self {
        management: None,
        home: None,
    };

    #[must_use]
    pub const fn managed(management: Management) -> Self {
        Self {
            management: Some(management),
            home: None,
        }
    }

    /// `self` wherever it decides, else `fallback`.
    #[must_use]
    pub const fn over(self, fallback: Self) -> Self {
        Self {
            management: match self.management {
                Some(management) => Some(management),
                None => fallback.management,
            },
            home: match self.home {
                Some(home) => Some(home),
                None => fallback.home,
            },
        }
    }

    /// What a window's own shape contributes: a fixed-size window that holds no tile floats. A
    /// tiled window keeps its tile while it briefly looks fixed-size (an attached sheet).
    #[must_use]
    pub const fn traits(resizable: bool, tiled: bool) -> Self {
        if resizable || tiled {
            Self::NONE
        } else {
            Self::managed(Management::Float)
        }
    }

    #[must_use]
    pub fn is_ignored(self) -> bool {
        self.management == Some(Management::Ignore)
    }

    /// Floats, pinned or not.
    #[must_use]
    pub fn floats(self) -> bool {
        matches!(self.management, Some(Management::Float | Management::Pin))
    }

    #[must_use]
    pub fn pins(self) -> bool {
        self.management == Some(Management::Pin)
    }
}

impl Config {
    /// Whether any rule ignores windows. Built-in defaults never do.
    #[must_use]
    pub fn has_ignore_rules(&self) -> bool {
        self.rules
            .iter()
            .any(|rule| rule.action == WindowDisposition::Ignored)
    }

    /// Whether some rule that can match `bundle_id`'s windows looks at titles, so a window's
    /// disposition is unknown until its title is read.
    #[must_use]
    pub fn has_title_rules(&self, bundle_id: &str) -> bool {
        self.rules.iter().any(|rule| {
            rule.matcher.matches_titles()
                && rule
                    .matcher
                    .bundle_id
                    .as_deref()
                    .is_none_or(|value| value == bundle_id)
        })
    }
}

impl From<WindowDisposition> for DispositionPatch {
    fn from(disposition: WindowDisposition) -> Self {
        match disposition {
            WindowDisposition::Managed => Self::NONE,
            WindowDisposition::Floating => Self::managed(Management::Float),
            WindowDisposition::Pinned => Self::managed(Management::Pin),
            WindowDisposition::Tiled => Self::managed(Management::Tile),
            WindowDisposition::Ignored => Self::managed(Management::Ignore),
            WindowDisposition::Route(workspace) => Self {
                management: None,
                home: Some(workspace),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn patches() -> Vec<DispositionPatch> {
        let mut patches = vec![DispositionPatch::NONE];
        for management in [
            None,
            Some(Management::Tile),
            Some(Management::Float),
            Some(Management::Pin),
            Some(Management::Ignore),
        ] {
            for home in [
                None,
                Some(WorkspaceId::TWO),
                Some(WorkspaceId::new(7).unwrap()),
            ] {
                patches.push(DispositionPatch { management, home });
            }
        }
        patches
    }

    #[test]
    fn over_is_an_associative_merge_with_an_identity() {
        let patches = patches();
        for &a in &patches {
            assert_eq!(a.over(DispositionPatch::NONE), a);
            assert_eq!(DispositionPatch::NONE.over(a), a);
            for &b in &patches {
                for &c in &patches {
                    assert_eq!(a.over(b).over(c), a.over(b.over(c)));
                }
            }
        }
    }

    #[test]
    fn a_route_rule_keeps_a_default_float_and_a_tile_rule_overrides_it() {
        let default = DispositionPatch::managed(Management::Float);
        let route = DispositionPatch::from(WindowDisposition::Route(WorkspaceId::TWO));
        assert_eq!(
            route.over(default),
            DispositionPatch {
                management: Some(Management::Float),
                home: Some(WorkspaceId::TWO),
            }
        );
        let tile = DispositionPatch::from(WindowDisposition::Tiled);
        let fixed_size = DispositionPatch::traits(false, false);
        assert!(!tile.over(default).over(fixed_size).floats());
        assert!(DispositionPatch::NONE.over(fixed_size).floats());
        assert!(!DispositionPatch::traits(false, true).floats());
    }
}
