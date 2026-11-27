//! Where each card sits: centred on its window, kept inside the display.

use spaceman::Rect;

/// The closest a card comes to its display's edges, in points.
pub const EDGE_MARGIN: f32 = 12.0;

/// The card's top-left corner in display-local points: centred on `window` (global points),
/// then clamped so a narrow window, or one partly off the display, keeps its card whole on
/// `screen` (the display's full bounds, global points). A card wider than the display sits at
/// its left margin.
#[must_use]
pub fn card_origin(window: Rect, screen: Rect, card: (f32, f32)) -> (f32, f32) {
    let axis = |start: i32, length: i32, screen_start: i32, screen_length: i32, size: f32| {
        let centre = (start - screen_start) as f32 + length as f32 / 2.0;
        let far = (screen_length as f32 - size - EDGE_MARGIN).max(EDGE_MARGIN);
        (centre - size / 2.0).clamp(EDGE_MARGIN, far).round()
    };
    (
        axis(window.x, window.width, screen.x, screen.width, card.0),
        axis(window.y, window.height, screen.y, screen.height, card.1),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const SCREEN: Rect = Rect {
        x: 1000,
        y: 0,
        width: 1600,
        height: 1000,
    };

    fn rect(x: i32, y: i32, width: i32, height: i32) -> Rect {
        Rect {
            x,
            y,
            width,
            height,
        }
    }

    #[test]
    fn a_card_centres_on_its_window_in_display_local_points() {
        // A window at (1400, 200) 800×600 on a display whose origin is (1000, 0).
        let origin = card_origin(rect(1400, 200, 800, 600), SCREEN, (200.0, 100.0));
        assert_eq!(origin, (700.0, 450.0));
    }

    #[test]
    fn a_window_at_an_edge_keeps_its_card_whole_inside_the_display() {
        let card = (240.0, 110.0);
        // Hard against the top-left corner, narrower than its card.
        assert_eq!(
            card_origin(rect(1000, 0, 80, 40), SCREEN, card),
            (EDGE_MARGIN, EDGE_MARGIN)
        );
        // Mostly off the right and bottom edges.
        let (x, y) = card_origin(rect(2500, 950, 400, 300), SCREEN, card);
        assert_eq!(x, 1600.0 - 240.0 - EDGE_MARGIN);
        assert_eq!(y, 1000.0 - 110.0 - EDGE_MARGIN);
    }

    #[test]
    fn a_card_wider_than_the_display_sits_at_its_margin() {
        let tiny = rect(0, 0, 200, 150);
        assert_eq!(
            card_origin(rect(0, 0, 200, 150), tiny, (400.0, 300.0)),
            (EDGE_MARGIN, EDGE_MARGIN)
        );
    }
}
