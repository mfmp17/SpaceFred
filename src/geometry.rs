use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Axis {
    #[default]
    Horizontal,
    Vertical,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

impl Rect {
    #[must_use]
    pub fn approximately_equals(self, other: Self, tolerance: i32) -> bool {
        self.position_approximately_equals(other, tolerance)
            && (self.width - other.width).abs() <= tolerance
            && (self.height - other.height).abs() <= tolerance
    }

    /// Position-only tolerance comparison; sizes may legitimately differ when
    /// an application constrains its own dimensions.
    #[must_use]
    pub fn position_approximately_equals(self, other: Self, tolerance: i32) -> bool {
        (self.x - other.x).abs() <= tolerance && (self.y - other.y).abs() <= tolerance
    }

    #[must_use]
    pub const fn contains(self, x: i32, y: i32) -> bool {
        x >= self.x && x < self.x + self.width && y >= self.y && y < self.y + self.height
    }

    #[must_use]
    pub const fn contains_center(self, other: Self) -> bool {
        self.contains(other.x + other.width / 2, other.y + other.height / 2)
    }
}

/// Width and height of the intersection of two rectangles; not positive when disjoint.
fn overlap(a: Rect, b: Rect) -> (i64, i64) {
    let width = (i64::from(a.x) + i64::from(a.width)).min(i64::from(b.x) + i64::from(b.width))
        - i64::from(a.x.max(b.x));
    let height = (i64::from(a.y) + i64::from(a.height)).min(i64::from(b.y) + i64::from(b.height))
        - i64::from(a.y.max(b.y));
    (width, height)
}

/// Whether more of `frame` than the one-pixel parking strip shows on any display.
/// Center containment is not enough: a window wider than its tile overflows the
/// display edge with its center outside while most of it stays visible.
#[must_use]
pub fn shows_on(frame: Rect, displays: &[Rect]) -> bool {
    shows_wider_than(frame, displays, 1)
}

/// Whether a column of `frame` wider than `strip` pixels shows on any display.
#[must_use]
pub fn shows_wider_than(frame: Rect, displays: &[Rect], strip: i64) -> bool {
    displays.iter().any(|display| {
        let (width, height) = overlap(frame, *display);
        width > strip && height > 0
    })
}

/// `frame` itself while its center sits on a connected display, otherwise moved
/// (size kept) inside `fallback`: the display it was recorded on has been
/// disconnected or rearranged.
#[must_use]
pub fn onto_displays(frame: Rect, displays: &[Rect], fallback: Rect) -> Rect {
    if displays
        .iter()
        .any(|display| display.contains_center(frame))
    {
        return frame;
    }
    Rect {
        x: frame.x.clamp(
            fallback.x,
            fallback.x + (fallback.width - frame.width).max(0),
        ),
        y: frame.y.clamp(
            fallback.y,
            fallback.y + (fallback.height - frame.height).max(0),
        ),
        ..frame
    }
}

/// Whether a window at `actual` sits where parking it at `target` leaves it.
/// macOS keeps a window's title bar on screen, so it may hold the window above
/// the requested bottom corner; the horizontal position is what hides it.
#[must_use]
pub fn parked_at(actual: Rect, target: Rect, tolerance: i32) -> bool {
    (actual.x - target.x).abs() <= tolerance && actual.y <= target.y + tolerance
}

/// [`parked_frame_clear_of`] when the displays' full bounds are their visible frames.
#[must_use]
pub fn parked_frame(output: Rect, displays: &[Rect], window: Rect) -> Rect {
    parked_frame_clear_of(output, displays, displays, window)
}

/// Where to park `window` beyond an edge of one of `displays` (visible frames),
/// nearest `output`, clear of every display's full bounds in `screens`
/// (menu bar and Dock included).
#[must_use]
pub fn parked_frame_clear_of(
    output: Rect,
    displays: &[Rect],
    screens: &[Rect],
    window: Rect,
) -> Rect {
    // Keep one pixel of a top corner at a display's bottom corner so macOS
    // does not clamp the window back on-screen; a strip along the whole edge
    // would show a sliver of every hidden window. macOS may hold a titled
    // window up, as far as bottom-aligned, to keep its title bar on screen, so
    // every position in between must stay off the other displays. Score whole
    // rectangles, not corners: the nearest output may have a neighbor on both
    // sides. When a display below blocks both corners, the bottom-aligned edge
    // strip still beats another display's corner, which macOS clamps back
    // onto the display the window is on.
    std::iter::once(&output)
        .chain(displays)
        .flat_map(|display| {
            [(true, false), (false, false), (true, true), (false, true)].map(|(right, strip)| {
                let inset = window.width.clamp(0, 2) / 2;
                let held = Rect {
                    x: if right {
                        display.x + display.width - inset
                    } else {
                        display.x - window.width + inset
                    },
                    y: display.y + (display.height - window.height).max(0),
                    ..window
                };
                let frame = if strip {
                    held
                } else {
                    Rect {
                        y: display.y + display.height - window.height.clamp(0, 2) / 2,
                        ..held
                    }
                };
                let distance = (i64::from(display.x) - i64::from(output.x)).abs()
                    + (i64::from(display.y) - i64::from(output.y)).abs();
                (frame, held, (distance, strip))
            })
        })
        .filter(|(frame, held, _)| {
            let swept = Rect {
                height: frame.y + frame.height - held.y,
                ..*held
            };
            !shows_on(swept, screens)
                && !screens
                    .iter()
                    .any(|screen| screen.contains_center(*frame) || screen.contains_center(*held))
        })
        .min_by_key(|(_, _, rank)| *rank)
        .map_or(
            Rect {
                x: output.x + output.width,
                y: output.y,
                ..window
            },
            |(frame, _, _)| frame,
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_window_overflowing_the_display_edge_still_shows() {
        let display = Rect {
            width: 1000,
            height: 800,
            ..Rect::default()
        };
        let overflowing = Rect {
            x: 700,
            y: 20,
            width: 700,
            height: 300,
        };
        let parked = Rect {
            x: 999,
            ..overflowing
        };

        assert!(!display.contains_center(overflowing));
        assert!(shows_on(overflowing, &[display]));
        assert!(!shows_on(parked, &[display]));
    }

    #[test]
    fn frames_on_a_disconnected_display_move_onto_the_fallback() {
        let main = Rect {
            width: 1000,
            height: 800,
            ..Rect::default()
        };
        let on_main = Rect {
            x: 50,
            y: 20,
            width: 400,
            height: 300,
        };
        let on_unplugged = Rect { x: 1050, ..on_main };

        assert_eq!(onto_displays(on_main, &[main], main), on_main);
        assert_eq!(onto_displays(on_unplugged, &[main], main).x, 600);
        assert_eq!(onto_displays(on_unplugged, &[main], main).y, 20);

        // A 2-px sliver overlaps `main` (`shows_on` is true), but its center
        // sits off every display: `onto_displays` must clamp it squarely onto
        // `fallback` instead of confirming a 99%-off-screen restore.
        let two_px_sliver = Rect { x: 998, ..on_main };
        assert!(shows_on(two_px_sliver, &[main]));
        assert!(!main.contains_center(two_px_sliver));
        let clamped = onto_displays(two_px_sliver, &[main], main);
        assert_eq!(clamped.x, 600);
        assert_eq!(clamped.y, 20);
        assert!(main.contains_center(clamped));
    }

    #[test]
    fn parks_beyond_outer_edges_without_requesting_a_clamped_position() {
        let left = Rect {
            width: 1000,
            height: 800,
            ..Rect::default()
        };
        let right = Rect {
            x: 1000,
            width: 1000,
            height: 800,
            ..Rect::default()
        };
        let window = Rect {
            width: 400,
            height: 300,
            ..Rect::default()
        };

        assert_eq!(parked_frame(left, &[left, right], window).x, -399);
        assert_eq!(parked_frame(right, &[left, right], window).x, 1999);
        assert_eq!(parked_frame(left, &[left, right], window).y, 799);
    }

    #[test]
    fn parking_leaves_a_single_corner_pixel_on_screen() {
        let display = Rect {
            y: 40,
            width: 1920,
            height: 1000,
            ..Rect::default()
        };
        for (width, height) in [(400, 300), (1920, 1000), (3000, 1500)] {
            let window = Rect {
                width,
                height,
                ..Rect::default()
            };
            let parked = parked_frame(display, &[display], window);

            assert_eq!(overlap(parked, display), (1, 1), "{parked:?}");
        }
    }

    #[test]
    fn a_window_held_up_by_its_title_bar_stays_off_a_shorter_neighbor() {
        let rect = |x, y, width, height| Rect {
            x,
            y,
            width,
            height,
        };
        // A shorter neighbor, and a neighbor whose Dock shortens its visible frame.
        for [left, right] in [
            [rect(0, 0, 1920, 1080), rect(1920, 0, 1920, 1050)],
            [rect(0, 25, 1920, 1055), rect(1920, 25, 1920, 985)],
        ] {
            let window = rect(0, 0, 600, 400);
            let parked = parked_frame(left, &[left, right], window);

            assert_eq!(parked.x, -599);
            for held in [0, 65, 399] {
                let held = Rect {
                    y: parked.y - held,
                    ..parked
                };
                assert!(!shows_on(held, &[left, right]), "{held:?}");
            }
        }
    }

    #[test]
    fn a_window_held_up_stays_off_a_neighbors_menu_bar() {
        let rect = |x, y, width, height| Rect {
            x,
            y,
            width,
            height,
        };
        let screens = [
            rect(0, 0, 1920, 1080),
            rect(0, 1080, 1920, 1080),
            rect(1920, 1443, 1920, 1080),
        ];
        let displays = [
            rect(0, 25, 1920, 1055),
            rect(0, 1105, 1920, 1055),
            rect(1920, 1480, 1920, 1043),
        ];
        let window = rect(0, 0, 600, 400);
        let parked = parked_frame_clear_of(displays[0], &displays, &screens, window);

        for held in [0, 31, 65, 399] {
            let held = Rect {
                y: parked.y - held,
                ..parked
            };
            assert!(!shows_on(held, &screens), "{held:?}");
        }
    }

    #[test]
    fn a_side_dock_moves_parking_to_the_other_corner() {
        let screen = Rect {
            width: 1920,
            height: 1080,
            ..Rect::default()
        };
        // The Dock on the right ends above the bottom: a window parked under it
        // would show a Dock-wide block below its end.
        let display = Rect {
            y: 25,
            width: 1850,
            height: 1055,
            ..Rect::default()
        };
        let window = Rect {
            width: 600,
            height: 400,
            ..Rect::default()
        };

        let parked = parked_frame_clear_of(display, &[display], &[screen], window);

        assert_eq!(parked.x, -599);
        assert!(!shows_on(parked, &[screen]));
    }

    #[test]
    fn a_display_below_falls_back_to_the_edge_strip_not_its_corner() {
        // Both bottom corners of the narrower top display run into the wider
        // display below; parking there would be a cross-display park.
        let top = Rect {
            x: 200,
            width: 1000,
            height: 800,
            ..Rect::default()
        };
        let bottom = Rect {
            x: 0,
            y: 800,
            width: 1600,
            height: 900,
        };
        let window = Rect {
            width: 400,
            height: 300,
            ..Rect::default()
        };

        let parked = parked_frame(top, &[top, bottom], window);

        assert_eq!((parked.x, parked.y), (1199, 500));
        assert!(!shows_on(parked, &[top, bottom]));
        assert_eq!(parked_frame(bottom, &[top, bottom], window).y, 1699);
    }

    #[test]
    fn a_window_held_above_its_parking_corner_is_still_parked() {
        let target = Rect {
            x: 1919,
            y: 1079,
            width: 600,
            height: 400,
        };

        assert!(parked_at(target, target, 1));
        assert!(parked_at(Rect { y: 1014, ..target }, target, 1));
        assert!(!parked_at(Rect { y: 1081, ..target }, target, 1));
        assert!(!parked_at(Rect { x: 1900, ..target }, target, 1));
    }

    #[test]
    fn parking_avoids_neighbors_in_middle_and_staggered_display_topologies() {
        for offsets in [[0, 0, 0], [-400, 0, 300], [0, 900, 0]] {
            let displays = offsets
                .into_iter()
                .enumerate()
                .map(|(index, y)| Rect {
                    x: (index as i32 - 1) * 1000,
                    y,
                    width: 1000,
                    height: 800,
                })
                .collect::<Vec<_>>();
            for output in &displays {
                for width in [100, 400, 1200, 3000] {
                    for height in [100, 800, 1500] {
                        let window = Rect {
                            width,
                            height,
                            ..Rect::default()
                        };
                        let parked = parked_frame(*output, &displays, window);
                        for display in &displays {
                            let overlap_width = (parked.x + parked.width)
                                .min(display.x + display.width)
                                - parked.x.max(display.x);
                            let overlap_height = (parked.y + parked.height)
                                .min(display.y + display.height)
                                - parked.y.max(display.y);
                            assert!(
                                overlap_width <= 1 || overlap_height <= 0,
                                "parked {parked:?} overlaps {display:?}"
                            );
                            assert!(!display.contains_center(parked));
                        }
                        // macOS may hold it up to keep its title bar on screen.
                        for held in [65, (height - 1).min(799)] {
                            let held = Rect {
                                y: parked.y - held,
                                ..parked
                            };
                            assert!(!shows_on(held, &displays), "held {held:?} shows");
                        }
                    }
                }
            }
        }
    }
}
