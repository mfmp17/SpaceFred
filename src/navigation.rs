use std::{fmt, str::FromStr};

use crate::{PhysicalWindowId, Rect, WorkspaceId};

#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Direction {
    Left,
    Down,
    Up,
    Right,
}

impl fmt::Display for Direction {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Left => "left",
            Self::Down => "down",
            Self::Up => "up",
            Self::Right => "right",
        })
    }
}

impl FromStr for Direction {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "left" => Ok(Self::Left),
            "down" => Ok(Self::Down),
            "up" => Ok(Self::Up),
            "right" => Ok(Self::Right),
            _ => Err(format!("invalid direction {value:?}")),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize)]
pub struct NavigationTile {
    pub workspace: WorkspaceId,
    pub window: PhysicalWindowId,
    pub frame: Rect,
    /// The tiling area of the display showing the tile's workspace. Picking a
    /// workspace across displays compares these, not where its tiles happen
    /// to sit, so a width cap cannot hide an adjacent display (#396).
    #[serde(skip)]
    pub display: Option<Rect>,
}

impl NavigationTile {
    /// The area that stands for this tile's workspace across displays.
    fn workspace_area(&self) -> Rect {
        self.display.unwrap_or(self.frame)
    }
}

/// Tiles this close past the current tile's edge still count as lying beyond
/// it, absorbing rounding in computed splits.
const EDGE_TOLERANCE: i64 = 8;

/// Tiles sharing no more than this much of a row or column only touch at a
/// corner (rounding in computed splits), so they don't border for recency;
/// a short real shared edge still does (#525).
const CORNER_TOLERANCE: i64 = 2;

/// The tile a directional key moves to from the focused tile.
///
/// Within the focused tile's workspace, neighbors whose perpendicular band
/// overlaps the current tile win, nearest edge first, the way i3 and AeroSpace
/// pick them: from either tile of a stacked column, left always reaches the
/// column beside it, however narrow or wide either is. When no tile lies ahead
/// on the same workspace, navigation picks the nearest display's workspace in
/// that direction first and then the closest tile on it, so a tall side display
/// never skips over a shorter middle display (#356).
#[must_use]
pub fn target<'a>(
    tiles: &'a [NavigationTile],
    focused: &PhysicalWindowId,
    direction: Direction,
) -> Option<&'a NavigationTile> {
    target_by_recency(tiles, focused, direction, |_| None)
}

/// Like [`target`], but when several tiles on the focused tile's workspace
/// border it on the side pressed (they share part of its row or column and
/// none has a nearer edge), the one `recency` ranks lowest wins: 0 is the
/// most recently focused, None never focused. A further tile never wins for
/// being more recent, and with no ranked bordering tile the choice is
/// [`target`]'s. Crossing to another workspace is unchanged.
#[must_use]
pub fn target_by_recency<'a>(
    tiles: &'a [NavigationTile],
    focused: &PhysicalWindowId,
    direction: Direction,
    recency: impl Fn(&PhysicalWindowId) -> Option<usize>,
) -> Option<&'a NavigationTile> {
    let current = tiles.iter().find(|tile| &tile.window == focused)?;
    let local = tiles
        .iter()
        .filter(|tile| tile.workspace == current.workspace && &tile.window != focused);
    if let Some(bordering) = bordering(local.clone(), current.frame, direction)
        .into_iter()
        .filter_map(|tile| recency(&tile.window).map(|rank| (rank, tile)))
        .min_by_key(|(rank, _)| *rank)
        .map(|(_, tile)| tile)
    {
        return Some(bordering);
    }
    if let Some(local) = neighbor(local, current.frame, direction) {
        return Some(local);
    }
    let current_bounds = workspace_bounds(tiles, current.workspace)?;
    cross_workspace_neighbor(
        tiles,
        current.frame,
        current_bounds,
        Some(current.workspace),
        direction,
    )
}

/// The windows of `tiles` that border `focused`'s on the side pressed, the
/// way [`target_by_recency`] picks its candidates, judged by the part of the
/// shared edge the windows really cover: each tile, the focused one too, is
/// trimmed to its window's `accepted` frame across the direction only, so a
/// window macOS kept shorter (or narrower) than its tile (a size cap) does
/// not border across the gap it leaves (#648), while one capped along the
/// direction keeps its tile's place. A tile keeps its planned frame when its
/// accepted one is unknown or lies outside it.
#[must_use]
pub fn bordering_accepted(
    tiles: &[NavigationTile],
    focused: &PhysicalWindowId,
    direction: Direction,
    accepted: impl Fn(&PhysicalWindowId) -> Option<Rect>,
) -> Vec<PhysicalWindowId> {
    let clipped = |tile: &NavigationTile| NavigationTile {
        frame: accepted(&tile.window)
            .and_then(|frame| intersection(tile.frame, frame))
            .map_or(tile.frame, |covered| across(tile.frame, covered, direction)),
        ..tile.clone()
    };
    let Some(current) = tiles.iter().find(|tile| &tile.window == focused) else {
        return Vec::new();
    };
    let current = clipped(current);
    let local = tiles
        .iter()
        .filter(|tile| tile.workspace == current.workspace && &tile.window != focused)
        .map(clipped)
        .collect::<Vec<_>>();
    bordering(local.iter(), current.frame, direction)
        .into_iter()
        .map(|tile| tile.window.clone())
        .collect()
}

/// The area two frames share, if any.
fn intersection(a: Rect, b: Rect) -> Option<Rect> {
    let x = a.x.max(b.x);
    let y = a.y.max(b.y);
    let width = (a.x + a.width).min(b.x + b.width) - x;
    let height = (a.y + a.height).min(b.y + b.height) - y;
    (width > 0 && height > 0).then_some(Rect {
        x,
        y,
        width,
        height,
    })
}

/// `planned` with its span across `direction` taken from `covered` and its
/// span along it kept.
fn across(planned: Rect, covered: Rect, direction: Direction) -> Rect {
    match direction {
        Direction::Left | Direction::Right => Rect {
            y: covered.y,
            height: covered.height,
            ..planned
        },
        Direction::Up | Direction::Down => Rect {
            x: covered.x,
            width: covered.width,
            ..planned
        },
    }
}

/// The tile a directional key moves to when focus is on a window that isn't a
/// tile (a floating window, dialog or popup) at `from`: its neighbor in that
/// direction, else the nearest tile whose center lies that way, else the
/// nearest tile, so the key always brings focus back onto the tiles.
#[must_use]
pub fn rescue(
    tiles: &[NavigationTile],
    from: Rect,
    direction: Direction,
) -> Option<&NavigationTile> {
    rescue_from_display(tiles, from, None, direction)
}

/// Like [`rescue`], when the caller knows the visible frame of the display
/// `from` lives on (such as an empty workspace or a floating window near the
/// edge of a tall side display, #355/#356).
#[must_use]
pub fn rescue_from_display(
    tiles: &[NavigationTile],
    from: Rect,
    from_display: Option<Rect>,
    direction: Direction,
) -> Option<&NavigationTile> {
    let home_workspace = workspace_containing_center(tiles, from);
    if let Some(workspace) = home_workspace
        && let Some(local) = neighbor(
            tiles.iter().filter(|tile| tile.workspace == workspace),
            from,
            direction,
        )
    {
        return Some(local);
    }
    let origin_bounds = from_display
        .or_else(|| home_workspace.and_then(|workspace| workspace_bounds(tiles, workspace)))
        .unwrap_or(from);
    cross_workspace_neighbor(tiles, from, origin_bounds, home_workspace, direction)
        .or_else(|| {
            tiles
                .iter()
                .map(|tile| (Reach::new(from, tile.frame, direction), tile))
                .filter(|(reach, _)| reach.travel > 0)
                .min_by_key(|(reach, _)| (reach.band_gap(), reach.travel, reach.offset))
                .map(|(_, tile)| tile)
        })
        .or_else(|| {
            tiles
                .iter()
                .min_by_key(|tile| center_distance(from, tile.frame))
        })
}

fn cross_workspace_neighbor(
    tiles: &[NavigationTile],
    from_tile: Rect,
    from_bounds: Rect,
    exclude_workspace: Option<WorkspaceId>,
    direction: Direction,
) -> Option<&NavigationTile> {
    let mut workspaces: Vec<(WorkspaceId, Rect)> = Vec::new();
    for tile in tiles {
        if Some(tile.workspace) == exclude_workspace {
            continue;
        }
        if let Some((_, bounds)) = workspaces
            .iter_mut()
            .find(|(workspace, _)| *workspace == tile.workspace)
        {
            *bounds = union_rect(*bounds, tile.workspace_area());
        } else {
            workspaces.push((tile.workspace, tile.workspace_area()));
        }
    }
    let target_workspace = pick_best_rect(workspaces.into_iter(), from_bounds, direction)?;
    let target_tiles: Vec<&NavigationTile> = tiles
        .iter()
        .filter(|tile| tile.workspace == target_workspace)
        .collect();
    neighbor(target_tiles.iter().copied(), from_tile, direction).or_else(|| {
        target_tiles
            .into_iter()
            .min_by_key(|tile| center_distance(from_tile, tile.frame))
    })
}

fn workspace_bounds(tiles: &[NavigationTile], workspace: WorkspaceId) -> Option<Rect> {
    tiles
        .iter()
        .filter(|tile| tile.workspace == workspace)
        .map(NavigationTile::workspace_area)
        .reduce(union_rect)
}

fn workspace_containing_center(tiles: &[NavigationTile], frame: Rect) -> Option<WorkspaceId> {
    let mut workspaces: Vec<(WorkspaceId, Rect)> = Vec::new();
    for tile in tiles {
        if let Some((_, bounds)) = workspaces
            .iter_mut()
            .find(|(workspace, _)| *workspace == tile.workspace)
        {
            *bounds = union_rect(*bounds, tile.workspace_area());
        } else {
            workspaces.push((tile.workspace, tile.workspace_area()));
        }
    }
    workspaces
        .into_iter()
        .find(|(_, bounds)| bounds.contains_center(frame))
        .map(|(workspace, _)| workspace)
}

fn union_rect(left: Rect, right: Rect) -> Rect {
    let x = left.x.min(right.x);
    let y = left.y.min(right.y);
    Rect {
        x,
        y,
        width: (left.x + left.width).max(right.x + right.width) - x,
        height: (left.y + left.height).max(right.y + right.height) - y,
    }
}

fn neighbor<'a>(
    candidates: impl Iterator<Item = &'a NavigationTile>,
    from: Rect,
    direction: Direction,
) -> Option<&'a NavigationTile> {
    pick_best_rect(candidates.map(|tile| (tile, tile.frame)), from, direction)
}

/// The tiles sharing part of `from`'s band with the nearest edge: the first
/// rank [`pick_best_rect`] ties on before its center offset decides, plus
/// those sharing only a short stretch of it.
fn bordering<'a>(
    candidates: impl Iterator<Item = &'a NavigationTile>,
    from: Rect,
    direction: Direction,
) -> Vec<&'a NavigationTile> {
    let ahead = candidates
        .map(|tile| (Reach::new(from, tile.frame, direction), tile))
        .filter(|(reach, _)| {
            reach.gap >= -EDGE_TOLERANCE && reach.travel > 0 && reach.overlap > CORNER_TOLERANCE
        })
        .collect::<Vec<_>>();
    let Some(nearest) = ahead.iter().map(|(reach, _)| reach.gap.max(0)).min() else {
        return Vec::new();
    };
    ahead
        .into_iter()
        .filter(|(reach, _)| reach.gap.max(0) == nearest)
        .map(|(_, tile)| tile)
        .collect()
}

fn pick_best_rect<T: Copy>(
    candidates: impl Iterator<Item = (T, Rect)>,
    from: Rect,
    direction: Direction,
) -> Option<T> {
    let ahead = candidates
        .map(|(item, rect)| (Reach::new(from, rect, direction), item))
        .filter(|(reach, _)| reach.gap >= -EDGE_TOLERANCE && reach.travel > 0)
        .collect::<Vec<_>>();
    ahead
        .iter()
        .filter(|(reach, _)| reach.overlap > EDGE_TOLERANCE)
        .min_by_key(|(reach, _)| (reach.gap.max(0), reach.offset, reach.travel))
        .or_else(|| {
            ahead
                .iter()
                .min_by_key(|(reach, _)| (reach.band_gap(), reach.gap.max(0), reach.offset))
        })
        .map(|(_, item)| *item)
}

/// Where a candidate lies relative to the current frame, measured as if
/// travelling right/down along the requested direction. Centers are doubled
/// to stay in integers.
struct Reach {
    /// From the current frame's far edge to the candidate's near edge.
    gap: i64,
    /// From the current center to the candidate center along the direction.
    travel: i64,
    /// How much the two frames share across the direction; negative is a gap.
    overlap: i64,
    /// Distance between the two centers across the direction.
    offset: i64,
}

impl Reach {
    fn new(from: Rect, to: Rect, direction: Direction) -> Self {
        let (from_along, from_across) = spans(from, direction);
        let (to_along, to_across) = spans(to, direction);
        Self {
            gap: to_along.0 - from_along.1,
            travel: (to_along.0 + to_along.1) - (from_along.0 + from_along.1),
            overlap: to_across.1.min(from_across.1) - to_across.0.max(from_across.0),
            offset: ((to_across.0 + to_across.1) - (from_across.0 + from_across.1)).abs(),
        }
    }

    fn band_gap(&self) -> i64 {
        (-self.overlap).max(0)
    }
}

/// A frame's span along the direction (mirrored for left and up, so travel is
/// always toward larger values) and across it.
fn spans(rect: Rect, direction: Direction) -> ((i64, i64), (i64, i64)) {
    let x = (i64::from(rect.x), i64::from(rect.x) + i64::from(rect.width));
    let y = (
        i64::from(rect.y),
        i64::from(rect.y) + i64::from(rect.height),
    );
    match direction {
        Direction::Right => (x, y),
        Direction::Left => ((-x.1, -x.0), y),
        Direction::Down => (y, x),
        Direction::Up => ((-y.1, -y.0), x),
    }
}

fn center_distance(a: Rect, b: Rect) -> i64 {
    let dx = (i64::from(a.x) * 2 + i64::from(a.width)) - (i64::from(b.x) * 2 + i64::from(b.width));
    let dy =
        (i64::from(a.y) * 2 + i64::from(a.height)) - (i64::from(b.y) * 2 + i64::from(b.height));
    dx * dx + dy * dy
}

#[cfg(test)]
mod tests {
    use crate::AppInstanceId;

    use super::*;

    fn tile(id: u32, workspace: u8, x: i32, y: i32) -> NavigationTile {
        sized(id, workspace, x, y, 500, 500)
    }

    fn sized(id: u32, workspace: u8, x: i32, y: i32, width: i32, height: i32) -> NavigationTile {
        NavigationTile {
            workspace: WorkspaceId::new(workspace).unwrap(),
            window: PhysicalWindowId {
                app: AppInstanceId {
                    pid: i32::try_from(id).unwrap(),
                    launched_at: 1,
                },
                window_id: id,
            },
            frame: Rect {
                x,
                y,
                width,
                height,
            },
            display: None,
        }
    }

    fn id(tiles: &[NavigationTile], from: u32, direction: Direction) -> Option<u32> {
        let focused = &tiles
            .iter()
            .find(|tile| tile.window.window_id == from)
            .unwrap()
            .window;
        target(tiles, focused, direction).map(|tile| tile.window.window_id)
    }

    /// A narrow master column beside three stacked tiles on a portrait
    /// display, 8 px gaps.
    fn master_and_stack() -> Vec<NavigationTile> {
        vec![
            sized(1, 1, 0, 0, 600, 2560),
            sized(2, 1, 608, 0, 832, 848),
            sized(3, 1, 608, 856, 832, 848),
            sized(4, 1, 608, 1712, 832, 848),
        ]
    }

    #[test]
    fn left_from_any_tile_of_a_stack_reaches_the_master_column() {
        let tiles = master_and_stack();
        for stacked in [2, 3, 4] {
            assert_eq!(
                id(&tiles, stacked, Direction::Left),
                Some(1),
                "from {stacked}"
            );
            assert_eq!(
                id(&tiles, stacked, Direction::Right),
                None,
                "from {stacked}"
            );
        }
        assert_eq!(id(&tiles, 2, Direction::Down), Some(3));
        assert_eq!(id(&tiles, 4, Direction::Up), Some(3));
        assert_eq!(id(&tiles, 2, Direction::Up), None);
        assert_eq!(id(&tiles, 4, Direction::Down), None);
        // The master reaches the stacked tile level with its center.
        assert_eq!(id(&tiles, 1, Direction::Right), Some(3));
        assert_eq!(id(&tiles, 1, Direction::Left), None);
    }

    #[test]
    fn down_from_any_tile_of_a_row_reaches_the_wide_tile_below() {
        let tiles = vec![
            sized(1, 1, 0, 0, 848, 600),
            sized(2, 1, 856, 0, 848, 600),
            sized(3, 1, 1712, 0, 848, 600),
            sized(4, 1, 0, 608, 2560, 832),
        ];
        for top in [1, 2, 3] {
            assert_eq!(id(&tiles, top, Direction::Down), Some(4), "from {top}");
            assert_eq!(id(&tiles, top, Direction::Up), None, "from {top}");
        }
        assert_eq!(id(&tiles, 1, Direction::Right), Some(2));
        assert_eq!(id(&tiles, 3, Direction::Left), Some(2));
        assert_eq!(id(&tiles, 4, Direction::Up), Some(2));
        assert_eq!(id(&tiles, 4, Direction::Down), None);
        assert_eq!(id(&tiles, 4, Direction::Left), None);
        assert_eq!(id(&tiles, 4, Direction::Right), None);
    }

    #[test]
    fn resized_panes_keep_their_adjacent_neighbors() {
        // A 20% left column split top/bottom beside an 80% column.
        let tiles = vec![
            sized(1, 1, 0, 0, 512, 300),
            sized(2, 1, 0, 308, 512, 1132),
            sized(3, 1, 520, 0, 2040, 1440),
        ];
        assert_eq!(id(&tiles, 1, Direction::Right), Some(3));
        assert_eq!(id(&tiles, 2, Direction::Right), Some(3));
        assert_eq!(id(&tiles, 3, Direction::Left), Some(2));
        assert_eq!(id(&tiles, 1, Direction::Down), Some(2));
        assert_eq!(id(&tiles, 2, Direction::Up), Some(1));
    }

    #[test]
    fn grid_moves_one_cell_at_a_time_and_stops_at_the_edges() {
        let tiles = vec![
            sized(1, 1, 0, 0, 1000, 700),
            sized(2, 1, 1008, 0, 1000, 700),
            sized(3, 1, 0, 708, 1000, 700),
            sized(4, 1, 1008, 708, 1000, 700),
        ];
        assert_eq!(id(&tiles, 1, Direction::Right), Some(2));
        assert_eq!(id(&tiles, 1, Direction::Down), Some(3));
        assert_eq!(id(&tiles, 4, Direction::Left), Some(3));
        assert_eq!(id(&tiles, 4, Direction::Up), Some(2));
        assert_eq!(id(&tiles, 1, Direction::Left), None);
        assert_eq!(id(&tiles, 1, Direction::Up), None);
        assert_eq!(id(&tiles, 4, Direction::Right), None);
        assert_eq!(id(&tiles, 4, Direction::Down), None);
    }

    #[test]
    fn crosses_to_the_nearest_tile_on_a_staggered_display() {
        // A display split in two, one to its lower right sharing no band, and
        // one below it.
        let tiles = vec![
            sized(1, 1, 0, 0, 1000, 600),
            sized(2, 1, 1008, 0, 1000, 600),
            sized(3, 6, 2600, 700, 1400, 900),
            sized(4, 7, 600, 1800, 800, 400),
        ];
        assert_eq!(id(&tiles, 2, Direction::Right), Some(3));
        assert_eq!(id(&tiles, 3, Direction::Left), Some(2));
        assert_eq!(id(&tiles, 1, Direction::Down), Some(4));
        assert_eq!(id(&tiles, 2, Direction::Down), Some(4));
        assert_eq!(id(&tiles, 1, Direction::Left), None);
        assert_eq!(id(&tiles, 3, Direction::Right), None);
    }

    #[test]
    fn rescues_focus_from_a_floating_window_onto_the_tiles() {
        let tiles = vec![
            sized(1, 1, 0, 0, 1000, 800),
            sized(2, 1, 1008, 0, 1000, 800),
        ];
        let rescue_id =
            |from, direction| rescue(&tiles, from, direction).map(|tile| tile.window.window_id);
        // A dialog floating over the right tile.
        let dialog = Rect {
            x: 1200,
            y: 200,
            width: 400,
            height: 300,
        };
        assert_eq!(rescue_id(dialog, Direction::Left), Some(1));
        // A panel straddling both tiles still lands on the side pressed.
        let panel = Rect {
            x: 700,
            y: 300,
            width: 600,
            height: 200,
        };
        assert_eq!(rescue_id(panel, Direction::Left), Some(1));
        assert_eq!(rescue_id(panel, Direction::Right), Some(2));
        // Nothing lies that way: the nearest tile still takes focus.
        assert_eq!(rescue_id(dialog, Direction::Right), Some(2));
        assert_eq!(rescue(&[], dialog, Direction::Left), None);
    }

    #[test]
    fn chooses_geometric_neighbors_in_all_directions() {
        let center = tile(1, 1, 500, 500);
        let tiles = vec![
            center.clone(),
            tile(2, 1, 0, 500),
            tile(3, 1, 500, 1000),
            tile(4, 1, 500, 0),
            tile(5, 1, 1000, 500),
        ];

        assert_eq!(
            target(&tiles, &center.window, Direction::Left)
                .unwrap()
                .window
                .window_id,
            2
        );
        assert_eq!(
            target(&tiles, &center.window, Direction::Down)
                .unwrap()
                .window
                .window_id,
            3
        );
        assert_eq!(
            target(&tiles, &center.window, Direction::Up)
                .unwrap()
                .window
                .window_id,
            4
        );
        assert_eq!(
            target(&tiles, &center.window, Direction::Right)
                .unwrap()
                .window
                .window_id,
            5
        );
    }

    #[test]
    fn ignores_small_vertical_offset_between_side_by_side_displays() {
        let left = tile(1, 1, 0, 30);
        let right = tile(2, 6, 2560, 0);
        let tiles = vec![left.clone(), right.clone()];

        assert_eq!(target(&tiles, &left.window, Direction::Right), Some(&right));
        assert_eq!(target(&tiles, &left.window, Direction::Up), None);
        assert_eq!(target(&tiles, &right.window, Direction::Left), Some(&left));
        assert_eq!(target(&tiles, &right.window, Direction::Down), None);
    }

    #[test]
    fn tall_side_display_never_skips_middle_display_and_empty_display_rescues_in_direction() {
        // Portrait left and right displays extending above and below a landscape center (#356).
        let tiles = vec![
            sized(60, 6, -1440, -583, 1440, 506),
            sized(61, 6, -1440, -77, 1440, 506),
            sized(62, 6, -1440, 429, 1440, 506),
            sized(63, 6, -1440, 935, 1440, 506),
            sized(64, 6, -1440, 1441, 1440, 506),
            sized(10, 1, 0, 30, 1280, 1410),
            sized(11, 1, 1280, 30, 1280, 1410),
            sized(90, 9, 2560, -539, 1440, 2530),
        ];
        // Top and bottom rows on the left display reach the nearest tile on center, never right.
        assert_eq!(id(&tiles, 60, Direction::Right), Some(10));
        assert_eq!(id(&tiles, 64, Direction::Right), Some(10));
        // From center, moving right reaches right and moving left reaches left.
        assert_eq!(id(&tiles, 11, Direction::Right), Some(90));
        assert_eq!(id(&tiles, 10, Direction::Left), Some(62));
        // From right, moving left reaches center's right column, never skipping to left.
        assert_eq!(id(&tiles, 90, Direction::Left), Some(11));

        // Rescuing from an empty center display picks the display in the pressed direction (#355).
        let outer = vec![
            sized(60, 6, -1440, -583, 1440, 2530),
            sized(90, 9, 2560, -539, 1440, 2530),
        ];
        let center = Rect {
            x: 0,
            y: 30,
            width: 2560,
            height: 1410,
        };
        assert_eq!(
            rescue_from_display(&outer, center, Some(center), Direction::Right)
                .map(|tile| tile.window.window_id),
            Some(90)
        );
        assert_eq!(
            rescue_from_display(&outer, center, Some(center), Direction::Left)
                .map(|tile| tile.window.window_id),
            Some(60)
        );
    }

    fn by_recency(
        tiles: &[NavigationTile],
        from: u32,
        direction: Direction,
        order: &[u32],
    ) -> Option<u32> {
        let focused = &tiles
            .iter()
            .find(|tile| tile.window.window_id == from)
            .unwrap()
            .window;
        target_by_recency(tiles, focused, direction, |window| {
            order.iter().position(|id| *id == window.window_id)
        })
        .map(|tile| tile.window.window_id)
    }

    #[test]
    fn the_most_recent_bordering_tile_wins() {
        // `1 | (2 / 3)` on a 1280 by 800 display, the pair 200 and 600 tall.
        let tiles = vec![
            sized(1, 1, 0, 0, 640, 800),
            sized(2, 1, 640, 0, 640, 200),
            sized(3, 1, 640, 200, 640, 600),
        ];
        assert_eq!(id(&tiles, 1, Direction::Right), Some(3));
        assert_eq!(by_recency(&tiles, 1, Direction::Right, &[1, 2, 3]), Some(2));
        assert_eq!(by_recency(&tiles, 1, Direction::Right, &[1, 3, 2]), Some(3));
        // A tile focused since starting outranks one that never was.
        assert_eq!(by_recency(&tiles, 1, Direction::Right, &[1, 2]), Some(2));
        // With none ranked, the geometric choice stands.
        assert_eq!(by_recency(&tiles, 1, Direction::Right, &[1]), Some(3));
        // `1 / (2 | 3)` on a portrait display.
        let portrait = vec![
            sized(1, 1, 0, 0, 1080, 960),
            sized(2, 1, 0, 960, 540, 960),
            sized(3, 1, 540, 960, 540, 960),
        ];
        assert_eq!(
            by_recency(&portrait, 1, Direction::Down, &[1, 3, 2]),
            Some(3)
        );
        assert_eq!(
            by_recency(&portrait, 1, Direction::Down, &[1, 2, 3]),
            Some(2)
        );
    }

    #[test]
    fn recency_never_reaches_past_a_nearer_tile() {
        // `1 | (2 / (3 | 4))`: 4 lies further right than 2 and 3.
        let tiles = vec![
            sized(1, 1, 0, 0, 640, 800),
            sized(2, 1, 640, 0, 640, 400),
            sized(3, 1, 640, 400, 320, 400),
            sized(4, 1, 960, 400, 320, 400),
        ];
        assert_eq!(
            by_recency(&tiles, 1, Direction::Right, &[1, 4, 3, 2]),
            Some(3)
        );
        assert_eq!(
            by_recency(&tiles, 1, Direction::Right, &[1, 4, 2, 3]),
            Some(2)
        );
        // Only 4 was focused: the geometric choice between 2 and 3 stands.
        assert_eq!(
            by_recency(&tiles, 1, Direction::Right, &[4]),
            id(&tiles, 1, Direction::Right)
        );
        // Another workspace's tiles are never ranked.
        let mut displays = tiles.clone();
        displays.push(sized(5, 6, 1280, 0, 640, 800));
        assert_eq!(by_recency(&displays, 2, Direction::Right, &[5, 4]), Some(5));
    }

    #[test]
    fn a_short_shared_edge_still_borders() {
        // Two columns of two rows on a 1440 by 2530 portrait display; the
        // right column's split sits just below the left one's (#525).
        let tiles = |upper: i32| {
            vec![
                sized(1, 1, 2560, -539, 720, 700),
                sized(2, 1, 2560, 161, 720, 1830),
                sized(3, 1, 3280, -539, 720, upper),
                sized(4, 1, 3280, -539 + upper, 720, 2530 - upper),
            ]
        };
        for upper in [706, 712] {
            let tiles = tiles(upper);
            assert_eq!(id(&tiles, 2, Direction::Right), Some(4));
            assert_eq!(by_recency(&tiles, 2, Direction::Right, &[2, 3, 4]), Some(3));
        }
        // Splits that only meet at a corner, off by rounding, don't border.
        let corner = tiles(701);
        assert_eq!(
            by_recency(&corner, 2, Direction::Right, &[2, 3, 4]),
            Some(4)
        );
        let apart = tiles(690);
        assert_eq!(by_recency(&apart, 2, Direction::Right, &[2, 3, 4]), Some(4));
    }

    #[test]
    fn a_window_capped_short_of_its_tile_does_not_border_across_the_gap() {
        // As above, but the upper right window accepted 600 of its 766
        // points, ending 100 points above the lower left tile (#648).
        let tiles = vec![
            sized(1, 1, 2560, -539, 720, 700),
            sized(2, 1, 2560, 161, 720, 1830),
            sized(3, 1, 3280, -539, 720, 766),
            sized(4, 1, 3280, 227, 720, 1764),
        ];
        let window = |id: u32| {
            tiles
                .iter()
                .find(|tile| tile.window.window_id == id)
                .unwrap()
                .window
                .clone()
        };
        let capped = |window: &PhysicalWindowId| {
            (window.window_id == 3).then_some(Rect {
                x: 3280,
                y: -539,
                width: 720,
                height: 600,
            })
        };
        assert_eq!(
            bordering_accepted(&tiles, &window(2), Direction::Right, capped),
            vec![window(4)]
        );
        // Uncapped, both right tiles border it.
        assert_eq!(
            bordering_accepted(&tiles, &window(2), Direction::Right, |_| None),
            vec![window(3), window(4)]
        );
    }

    #[test]
    fn a_window_capped_along_the_direction_still_borders() {
        // A left column beside a full-height right tile; the upper left
        // window accepted 500 of its 700 points of width (#648).
        let tiles = vec![
            sized(1, 1, 0, 0, 700, 500),
            sized(2, 1, 0, 500, 700, 500),
            sized(3, 1, 700, 0, 700, 1000),
        ];
        let window = |id: u32| {
            tiles
                .iter()
                .find(|tile| tile.window.window_id == id)
                .unwrap()
                .window
                .clone()
        };
        let narrow = |window: &PhysicalWindowId| {
            (window.window_id == 1).then_some(Rect {
                x: 0,
                y: 0,
                width: 500,
                height: 500,
            })
        };
        assert_eq!(
            bordering_accepted(&tiles, &window(3), Direction::Left, narrow),
            vec![window(1), window(2)]
        );
    }

    #[test]
    fn direction_strings_round_trip() {
        for direction in [
            Direction::Left,
            Direction::Down,
            Direction::Up,
            Direction::Right,
        ] {
            assert_eq!(direction.to_string().parse(), Ok(direction));
        }
        assert_eq!(
            "diagonal".parse::<Direction>(),
            Err("invalid direction \"diagonal\"".into())
        );
    }
}
