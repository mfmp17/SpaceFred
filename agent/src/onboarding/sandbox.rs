//! The Done step's practice desktop: two pretend workspaces that the real shortcuts, or a click
//! on their keycaps, play. Pure state; nothing here touches a real window.

/// A shortcut the practice desktop answers. Focus and flip use Option; workspace
/// switches use Control, matching the built-in shortcuts.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Key {
    Left,
    Down,
    Up,
    Right,
    Flip,
    Workspace(u8),
}

impl Key {
    /// The practice key for an ANSI virtual key code, as the key tap reports it.
    pub fn from_key_code(code: u32) -> Option<Self> {
        Some(match code {
            4 => Self::Left,
            38 => Self::Down,
            40 => Self::Up,
            37 => Self::Right,
            44 => Self::Flip,
            18 => Self::Workspace(1),
            19 => Self::Workspace(2),
            _ => return None,
        })
    }

    /// The legend on its keycap.
    pub fn legend(self) -> &'static str {
        match self {
            Self::Left => "H",
            Self::Down => "J",
            Self::Up => "K",
            Self::Right => "L",
            Self::Flip => "/",
            Self::Workspace(1) => "1",
            Self::Workspace(_) => "2",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Lesson {
    Focus,
    Flip,
    Switch,
}

impl Lesson {
    pub const ALL: [Self; 3] = [Self::Focus, Self::Flip, Self::Switch];

    pub fn title(self) -> &'static str {
        match self {
            Self::Focus => "Move focus",
            Self::Flip => "Flip the split",
            Self::Switch => "Switch workspace",
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            Self::Focus => "focus",
            Self::Flip => "flip",
            Self::Switch => "switch",
        }
    }

    pub fn index(self) -> usize {
        match self {
            Self::Focus => 0,
            Self::Flip => 1,
            Self::Switch => 2,
        }
    }
}

/// How a workspace's first tile shares the screen with the rest.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Split {
    /// First tile on the left, the rest stacked on the right.
    Side,
    /// First tile on top, the rest side by side below.
    Stacked,
}

/// A frame in the unit square: `[x, y, width, height]`.
pub type Unit = [f32; 4];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Desk {
    tiles: usize,
    split: Split,
    focus: usize,
}

impl Desk {
    fn frames(self) -> Vec<Unit> {
        if self.tiles <= 1 {
            return vec![[0.0, 0.0, 1.0, 1.0]];
        }
        let rest = (self.tiles - 1) as f32;
        (0..self.tiles)
            .map(|index| {
                let n = index.saturating_sub(1) as f32;
                match (self.split, index) {
                    (Split::Side, 0) => [0.0, 0.0, 0.5, 1.0],
                    (Split::Side, _) => [0.5, n / rest, 0.5, 1.0 / rest],
                    (Split::Stacked, 0) => [0.0, 0.0, 1.0, 0.5],
                    (Split::Stacked, _) => [n / rest, 0.5, 1.0 / rest, 0.5],
                }
            })
            .collect()
    }

    /// The nearest tile from the focused one in `key`'s direction, if any.
    fn neighbour(self, key: Key) -> Option<usize> {
        let (dx, dy) = match key {
            Key::Left => (-1.0, 0.0),
            Key::Right => (1.0, 0.0),
            Key::Up => (0.0, -1.0),
            Key::Down => (0.0, 1.0),
            Key::Flip | Key::Workspace(_) => return None,
        };
        let frames = self.frames();
        let from = frames[self.focus];
        let center = |f: Unit| (f[0] + f[2] / 2.0, f[1] + f[3] / 2.0);
        let (fx, fy) = center(from);
        frames
            .iter()
            .enumerate()
            .filter(|(index, _)| *index != self.focus)
            .filter_map(|(index, frame)| {
                let (cx, cy) = center(*frame);
                let along = (cx - fx) * dx + (cy - fy) * dy;
                let across = ((cx - fx) * dy).abs() + ((cy - fy) * dx).abs();
                // Only tiles that lie that way and overlap the focused tile across it.
                let overlaps = if dx != 0.0 {
                    frame[1] < from[1] + from[3] && from[1] < frame[1] + frame[3]
                } else {
                    frame[0] < from[0] + from[2] && from[0] < frame[0] + frame[2]
                };
                (along > 0.0 && overlaps).then_some((index, along, across))
            })
            .min_by(|a, b| a.1.total_cmp(&b.1).then(a.2.total_cmp(&b.2)))
            .map(|(index, _, _)| index)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Sandbox {
    /// The visible workspace, 1 or 2.
    pub workspace: u8,
    desks: [Desk; 2],
    done: [bool; 3],
    visited_two: bool,
    /// Every press so far, so each one replays its keycap's dip.
    pub presses: u32,
    pub last: Option<Key>,
    /// Workspace switches so far, so each one replays the HUD's flash.
    pub switches: u32,
}

impl Default for Sandbox {
    fn default() -> Self {
        Self {
            workspace: 1,
            desks: [
                Desk {
                    tiles: 2,
                    split: Split::Side,
                    focus: 0,
                },
                Desk {
                    tiles: 3,
                    split: Split::Side,
                    focus: 0,
                },
            ],
            done: [false; 3],
            visited_two: false,
            presses: 0,
            last: None,
            switches: 0,
        }
    }
}

impl Sandbox {
    /// Plays `key`. Every press changes what the view reads, if only the keycap's dip.
    pub fn press(&mut self, key: Key) {
        self.presses = self.presses.wrapping_add(1);
        self.last = Some(key);
        let desk = &mut self.desks[usize::from(self.workspace - 1)];
        match key {
            Key::Left | Key::Down | Key::Up | Key::Right => {
                if let Some(next) = desk.neighbour(key) {
                    desk.focus = next;
                    self.done[Lesson::Focus.index()] = true;
                }
            }
            Key::Flip => {
                desk.split = match desk.split {
                    Split::Side => Split::Stacked,
                    Split::Stacked => Split::Side,
                };
                self.done[Lesson::Flip.index()] = true;
            }
            Key::Workspace(target) => {
                if target != self.workspace && (1..=2).contains(&target) {
                    self.workspace = target;
                    self.switches = self.switches.wrapping_add(1);
                    if target == 2 {
                        self.visited_two = true;
                    } else if self.visited_two {
                        self.done[Lesson::Switch.index()] = true;
                    }
                }
            }
        }
    }

    pub fn done(&self, lesson: Lesson) -> bool {
        self.done[lesson.index()]
    }

    /// The first lesson still to do.
    pub fn current(&self) -> Option<Lesson> {
        Lesson::ALL.into_iter().find(|lesson| !self.done(*lesson))
    }

    pub fn complete(&self) -> bool {
        self.current().is_none()
    }

    pub fn split(&self, workspace: u8) -> Split {
        self.desk(workspace).split
    }

    pub fn focus(&self, workspace: u8) -> usize {
        self.desk(workspace).focus
    }

    pub fn frames(&self, workspace: u8) -> Vec<Unit> {
        self.desk(workspace).frames()
    }

    /// The focus keys that move something on the visible workspace.
    pub fn focus_keys(&self) -> [Key; 2] {
        let desk = self.desk(self.workspace);
        if desk.split == Split::Stacked && (desk.tiles == 2 || desk.focus == 0) {
            [Key::Up, Key::Down]
        } else {
            [Key::Left, Key::Right]
        }
    }

    pub fn keys(&self, lesson: Lesson) -> Vec<Key> {
        match lesson {
            Lesson::Focus => self.focus_keys().to_vec(),
            Lesson::Flip => vec![Key::Flip],
            Lesson::Switch => vec![Key::Workspace(2), Key::Workspace(1)],
        }
    }

    fn desk(&self, workspace: u8) -> Desk {
        self.desks[usize::from(workspace.clamp(1, 2) - 1)]
    }
}
