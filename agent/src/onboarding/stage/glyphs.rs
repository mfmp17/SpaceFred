//! The small line glyphs the miniature desktop draws: its menu bar's status items and the
//! windows' toolbar buttons, in the manner of SF Symbols at a few points across. Each is a
//! `Shape` in its own box; strokes take an opaque colour, so no joint shows twice.

use rune::{Color, El, LineCap, Path, Shape, el};

const LINE: f32 = 0.7;

/// The points of an arc around `c`, from `from` to `to` degrees, clockwise from three o'clock.
fn arc(c: [f32; 2], r: f32, from: f32, to: f32) -> Vec<[f32; 2]> {
    let steps = (((to - from).abs() / 15.0).ceil() as usize).max(2);
    (0..=steps)
        .map(|i| {
            let a = (from + (to - from) * i as f32 / steps as f32).to_radians();
            [c[0] + r * a.cos(), c[1] + r * a.sin()]
        })
        .collect()
}

/// A rounded rectangle's outline, clockwise from its top-left corner.
fn rounded(x: f32, y: f32, w: f32, h: f32, r: f32) -> Vec<[f32; 2]> {
    let r = r.min(w / 2.0).min(h / 2.0);
    let mut points = arc([x + r, y + r], r, 180.0, 270.0);
    points.extend(arc([x + w - r, y + r], r, 270.0, 360.0));
    points.extend(arc([x + w - r, y + h - r], r, 0.0, 90.0));
    points.extend(arc([x + r, y + h - r], r, 90.0, 180.0));
    points
}

fn outline(shape: Shape, points: Vec<[f32; 2]>, color: Color) -> Shape {
    shape.stroke_with(points, true, LINE, LineCap::Round, color)
}

fn glyph(key: &str, w: f32, h: f32, shape: Shape) -> El {
    el().key(key).size(w, h).shape(shape)
}

/// Safari's sidebar button: a window with its sidebar ruled off.
pub fn sidebar(key: &str, color: Color) -> El {
    let shape = outline(Shape::new(), rounded(0.5, 0.8, 8.0, 6.4, 1.6), color).line(
        [3.3, 0.8],
        [3.3, 7.2],
        LINE,
        LineCap::Butt,
        color,
    );
    glyph(key, 9.0, 8.0, shape)
}

/// Back, or forward when `forward`.
pub fn chevron(key: &str, forward: bool, color: Color) -> El {
    let (a, b) = if forward { (1.0, 3.6) } else { (3.6, 1.0) };
    glyph(
        key,
        4.6,
        8.0,
        Shape::new().stroke(vec![[a, 1.4], [b, 4.0], [a, 6.6]], 0.8, color),
    )
}

pub fn share(key: &str, color: Color) -> El {
    let shape = Shape::new()
        .stroke(
            vec![
                [2.6, 3.2],
                [1.2, 3.2],
                [1.2, 8.2],
                [6.8, 8.2],
                [6.8, 3.2],
                [5.4, 3.2],
            ],
            LINE,
            color,
        )
        .line([4.0, 5.6], [4.0, 0.6], LINE, LineCap::Round, color)
        .stroke(vec![[2.5, 2.0], [4.0, 0.6], [5.5, 2.0]], LINE, color);
    glyph(key, 8.0, 9.0, shape)
}

pub fn plus(key: &str, color: Color) -> El {
    let shape = Shape::new()
        .line([3.5, 0.8], [3.5, 6.2], LINE, LineCap::Round, color)
        .line([0.8, 3.5], [6.2, 3.5], LINE, LineCap::Round, color);
    glyph(key, 7.0, 7.0, shape)
}

/// Safari's tab overview: two pages, one over the other.
pub fn tabs(key: &str, color: Color) -> El {
    let shape = outline(Shape::new(), rounded(0.6, 2.2, 5.6, 5.0, 1.2), color).stroke(
        vec![[2.4, 0.9], [6.4, 0.9], [7.6, 2.0], [7.6, 5.6]],
        LINE,
        color,
    );
    glyph(key, 8.5, 8.0, shape)
}

pub fn refresh(key: &str, color: Color) -> El {
    let shape = Shape::new()
        .stroke(arc([3.0, 3.2], 2.1, -40.0, 250.0), 0.6, color)
        .fill(vec![[3.9, 0.2], [5.4, 1.5], [3.7, 2.4]], color);
    glyph(key, 6.0, 6.0, shape)
}

pub fn lock(key: &str, color: Color) -> El {
    let shape = Shape::new()
        .stroke(arc([2.0, 2.4], 1.15, 180.0, 360.0), 0.55, color)
        .line([0.85, 2.4], [0.85, 2.9], 0.55, LineCap::Butt, color)
        .line([3.15, 2.4], [3.15, 2.9], 0.55, LineCap::Butt, color)
        .fill(rounded(0.2, 2.7, 3.6, 2.7, 0.6), color);
    glyph(key, 4.0, 5.5, shape)
}

/// A new message or note: a page with a pencil across its corner.
pub fn compose(key: &str, color: Color) -> El {
    let shape = Shape::new()
        .stroke(
            vec![
                [4.0, 1.2],
                [1.8, 1.2],
                [1.0, 2.0],
                [1.0, 7.2],
                [1.8, 8.0],
                [7.0, 8.0],
                [7.8, 7.2],
                [7.8, 5.0],
            ],
            LINE,
            color,
        )
        .line([4.0, 5.0], [8.2, 0.8], 0.9, LineCap::Round, color);
    glyph(key, 9.0, 9.0, shape)
}

/// Notes' checklist button: a ticked circle beside a line, twice.
pub fn checklist(key: &str, color: Color) -> El {
    let mut shape = Shape::new();
    for y in [2.0, 6.0] {
        shape = shape
            .stroke_with(
                arc([1.8, y], 1.3, 0.0, 360.0),
                true,
                0.6,
                LineCap::Round,
                color,
            )
            .line([4.4, y], [8.4, y], LINE, LineCap::Round, color);
    }
    glyph(key, 9.0, 8.0, shape)
}

pub fn table(key: &str, color: Color) -> El {
    let shape = outline(Shape::new(), rounded(0.5, 0.8, 8.0, 6.6, 1.2), color)
        .line([0.5, 3.0], [8.5, 3.0], 0.6, LineCap::Butt, color)
        .line([3.2, 3.0], [3.2, 7.4], 0.6, LineCap::Butt, color)
        .line([5.8, 3.0], [5.8, 7.4], 0.6, LineCap::Butt, color);
    glyph(key, 9.0, 8.0, shape)
}

pub fn archive(key: &str, color: Color) -> El {
    let shape = outline(Shape::new(), rounded(0.5, 0.8, 8.0, 2.2, 0.6), color)
        .stroke(
            vec![[1.2, 3.0], [1.2, 7.6], [7.8, 7.6], [7.8, 3.0]],
            LINE,
            color,
        )
        .line([3.4, 4.8], [5.6, 4.8], LINE, LineCap::Round, color);
    glyph(key, 9.0, 8.5, shape)
}

pub fn trash(key: &str, color: Color) -> El {
    let shape = Shape::new()
        .line([0.6, 1.9], [7.4, 1.9], LINE, LineCap::Round, color)
        .stroke(
            vec![[2.8, 1.9], [2.8, 0.7], [5.2, 0.7], [5.2, 1.9]],
            LINE,
            color,
        )
        .stroke(
            vec![[1.5, 1.9], [2.0, 8.2], [6.0, 8.2], [6.5, 1.9]],
            LINE,
            color,
        )
        .line([3.3, 3.4], [3.4, 6.8], 0.55, LineCap::Round, color)
        .line([4.7, 3.4], [4.6, 6.8], 0.55, LineCap::Round, color);
    glyph(key, 8.0, 9.0, shape)
}

pub fn reply(key: &str, color: Color) -> El {
    let mut curve = vec![[1.2, 3.6], [5.0, 3.6]];
    curve.extend(arc([5.0, 5.9], 2.3, 270.0, 360.0).into_iter().skip(1));
    curve.push([7.3, 7.6]);
    let shape = Shape::new().stroke(curve, LINE, color).stroke(
        vec![[3.3, 1.4], [1.1, 3.6], [3.3, 5.8]],
        LINE,
        color,
    );
    glyph(key, 9.0, 8.5, shape)
}

pub fn search(key: &str, color: Color) -> El {
    let shape = Shape::new()
        .stroke_with(
            arc([2.6, 2.6], 1.9, 0.0, 360.0),
            true,
            0.65,
            LineCap::Round,
            color,
        )
        .line([4.0, 4.0], [5.6, 5.6], 0.75, LineCap::Round, color);
    glyph(key, 6.0, 6.0, shape)
}

pub fn wifi(key: &str, color: Color) -> El {
    let c = [4.5, 6.6];
    let shape = Shape::new()
        .stroke(arc(c, 5.4, 225.0, 315.0), 0.9, color)
        .stroke(arc(c, 3.6, 225.0, 315.0), 0.9, color)
        .fill(
            {
                let mut wedge = vec![c];
                wedge.extend(arc(c, 2.0, 225.0, 315.0));
                wedge
            },
            color,
        );
    glyph(key, 9.0, 7.0, shape)
}

pub fn battery(key: &str, color: Color) -> El {
    let shape = outline(
        Shape::new(),
        rounded(0.4, 0.4, 10.4, 5.2, 1.6),
        color.alpha(color.a * 0.55),
    )
    .fill(rounded(1.5, 1.5, 6.6, 3.0, 0.8), color)
    .fill(
        rounded(11.3, 2.0, 0.9, 2.0, 0.4),
        color.alpha(color.a * 0.55),
    );
    glyph(key, 12.5, 6.0, shape)
}

/// Control Center: two switches, one on and one off.
pub fn control_center(key: &str, color: Color) -> El {
    let shape = outline(Shape::new(), rounded(0.5, 0.4, 7.0, 2.7, 1.35), color)
        .fill(arc([5.9, 1.75], 0.85, 0.0, 360.0), color)
        .stroke_with(
            rounded(0.5, 3.9, 7.0, 2.7, 1.35),
            true,
            LINE,
            LineCap::Round,
            color,
        )
        .fill(arc([2.1, 5.25], 0.85, 0.0, 360.0), color);
    glyph(key, 8.0, 7.0, shape)
}

/// A Notes folder: an outline with its tab.
pub fn folder(key: &str, color: Color) -> El {
    let shape = outline(
        Shape::new(),
        vec![
            [0.6, 1.0],
            [3.0, 1.0],
            [3.8, 1.9],
            [7.6, 1.9],
            [7.6, 6.4],
            [0.6, 6.4],
        ],
        color,
    )
    .line([0.6, 2.8], [7.6, 2.8], 0.55, LineCap::Butt, color);
    glyph(key, 8.2, 7.0, shape)
}

/// Hex's mark: a hexagon in a hexagon, its core lit.
pub fn hex_mark(key: &str, size: f32, line: Color, core: Color) -> El {
    let hexagon = |r: f32| {
        (0..6)
            .map(|i| {
                let a = (60.0 * i as f32 - 90.0_f32).to_radians();
                [size / 2.0 + r * a.cos(), size / 2.0 + r * a.sin()]
            })
            .collect::<Vec<_>>()
    };
    let shape = Shape::new()
        .stroke_with(
            hexagon(size * 0.46),
            true,
            size * 0.07,
            LineCap::Round,
            line,
        )
        .stroke_with(
            hexagon(size * 0.28),
            true,
            size * 0.07,
            LineCap::Round,
            line,
        )
        .fill(hexagon(size * 0.13), core);
    glyph(key, size, size, shape)
}

/// The Apple logo, `h` points tall.
pub fn apple(key: &str, h: f32, color: Color) -> El {
    let k = h / 24.0;
    let p = |x: f32, y: f32| [x * k, y * k];
    let body = Path::new()
        .move_to(p(12.152, 6.896))
        .cubic_to(p(11.204, 6.896), p(9.737, 5.818), p(8.192, 5.856))
        .cubic_to(p(6.152, 5.883), p(4.282, 7.039), p(3.231, 8.87))
        .cubic_to(p(1.114, 12.545), p(2.685, 17.973), p(4.75, 20.96))
        .cubic_to(p(5.763, 22.414), p(6.958, 24.05), p(8.542, 23.999))
        .cubic_to(p(10.062, 23.934), p(10.632, 23.012), p(12.477, 23.012))
        .cubic_to(p(14.308, 23.012), p(14.827, 23.999), p(16.437, 23.96))
        .cubic_to(p(18.074, 23.934), p(19.113, 22.48), p(20.113, 21.012))
        .cubic_to(p(21.269, 19.324), p(21.749, 17.687), p(21.775, 17.597))
        .cubic_to(p(21.736, 17.584), p(18.593, 16.376), p(18.555, 12.74))
        .cubic_to(p(18.529, 9.7), p(21.035, 8.246), p(21.152, 8.181))
        .cubic_to(p(19.723, 6.091), p(17.529, 5.857), p(16.762, 5.805))
        .cubic_to(p(14.762, 5.649), p(13.087, 6.895), p(12.152, 6.895))
        .close();
    let leaf = Path::new()
        .move_to(p(15.53, 3.83))
        .cubic_to(p(16.373, 2.818), p(16.93, 1.403), p(16.775, 0.0))
        .cubic_to(p(15.568, 0.052), p(14.113, 0.805), p(13.243, 1.818))
        .cubic_to(p(12.463, 2.714), p(11.789, 4.156), p(11.97, 5.532))
        .cubic_to(p(13.308, 5.636), p(14.685, 4.844), p(15.53, 3.83))
        .close();
    let shape = Shape::new().fill_path(&body, color).fill_path(&leaf, color);
    glyph(key, 22.0 * k, h, shape)
}
