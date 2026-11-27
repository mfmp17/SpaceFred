//! The miniature desktop's menu bar: the Apple menu, the focused app's name and its own menus,
//! then the status items and the clock. It is nearly a quarter past two.

use rune::{Color, El, TextStyle, Transition, el, row, text, tok::motion::pair::SETTLE};

use super::{
    MENU_H, SCREEN_W,
    demo::{APPS, Demo},
    glyphs,
};
use crate::apps;

const MENU: TextStyle = TextStyle::new(5.8, 450.0).lh(7.0);
const NAME: TextStyle = TextStyle::new(5.8, 680.0).lh(7.0);

/// Each app's menus after its name, as far as the bar has room for them.
fn menus(bundle: &str) -> &'static [&'static str] {
    match bundle {
        "com.apple.Safari" => &[
            "File",
            "Edit",
            "View",
            "History",
            "Bookmarks",
            "Window",
            "Help",
        ],
        "com.apple.Terminal" => &["Shell", "Edit", "View", "Window", "Help"],
        "com.apple.Notes" => &["File", "Edit", "Format", "View", "Window", "Help"],
        _ => &[
            "File", "Edit", "View", "Mailbox", "Message", "Format", "Window", "Help",
        ],
    }
}

pub(super) fn menu_bar(demo: Demo) -> El {
    let ws = demo.workspace();
    let bundle = APPS[ws][demo.desk(ws).focus];
    let white = Color::WHITE.alpha(0.92);
    let glyph_ink = Color::WHITE.alpha(0.9);
    let word = |key: &str, label: &str, style: TextStyle| {
        text(label.to_owned()).key(key).font(style).color(white)
    };
    // The app's name and menus cross-fade as focus moves to another app.
    let mut app = row()
        .key(format!("menus-{bundle}"))
        .gap(8.5)
        .center_y()
        .transition(Transition::opacity().springs(SETTLE))
        .child(word("name", &apps::fallback_name(bundle), NAME));
    for (index, menu) in menus(bundle).iter().enumerate() {
        app = app.child(word(&format!("menu{index}"), menu, MENU));
    }
    row()
        .key("menu-bar")
        .absolute(0.0, 0.0)
        .w(SCREEN_W)
        .h(MENU_H)
        .pl(10.0)
        .pr(9.0)
        .gap(9.0)
        .center_y()
        .fill(Color::BLACK.alpha(0.2))
        .backdrop_blur(8.0)
        .a11y_hidden()
        .child(glyphs::apple("apple", 7.0, white).mt(-0.6))
        .child(el().key("app-slot").child(app))
        .child(el().key("space").grow(1.0))
        .child(glyphs::battery("battery", glyph_ink))
        .child(glyphs::wifi("wifi", glyph_ink))
        .child(glyphs::search("spotlight", glyph_ink))
        .child(glyphs::control_center("control-center", glyph_ink))
        .child(word("clock", "Tue 2:11 PM", MENU))
}
