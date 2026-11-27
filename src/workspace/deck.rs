//! How a workspace shows its tiles on its display.

/// How a workspace arranges its tiles on screen. The split tree is the same in
/// every mode; a mode only decides how its tiles are projected onto the display,
/// so Strip and Accordion modes slot in as further variants.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) enum DeckMode {
    /// Tiles share the display along the split tree.
    #[default]
    Split,
    /// Fullscreen Layout: every tile fills the display and the focused one is
    /// in front.
    Monocle,
}

impl DeckMode {
    pub(crate) const fn from_fullscreen(enabled: bool) -> Self {
        if enabled { Self::Monocle } else { Self::Split }
    }

    pub(crate) const fn is_monocle(self) -> bool {
        matches!(self, Self::Monocle)
    }

    #[must_use]
    pub(crate) const fn toggled(self) -> Self {
        match self {
            Self::Split => Self::Monocle,
            Self::Monocle => Self::Split,
        }
    }

    /// Project the split tree's placements onto the display: each tile's frame
    /// in this mode, given the single-tile frame `whole`.
    pub(crate) fn place(self, placements: &mut [crate::Placement], whole: crate::Rect) {
        match self {
            Self::Split => {}
            Self::Monocle => {
                for placement in placements {
                    placement.frame = whole;
                }
            }
        }
    }

    /// Edits of the split (reorder, resize, balance) only make sense while the
    /// split is what the display shows.
    pub(crate) fn require_split(self, verb: &str) -> Result<(), String> {
        match self {
            Self::Split => Ok(()),
            Self::Monocle => Err(format!(
                "Exit Fullscreen Layout before {verb} the split layout"
            )),
        }
    }
}
