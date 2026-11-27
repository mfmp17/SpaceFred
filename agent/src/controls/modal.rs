//! The one modal shell. A modal is a small page: its cards sit at the page's card edge, its
//! title on the text rail and its footer on the control rail.

use rune::{El, SpringPair, Transition, col, el, row, text};

use super::{METRICS, MODAL, ModalWidth, Paint, WINDOW};
use crate::Ui;

/// A modal's panel: `title` on the text rail, `body` at the card edge, and `footer` ending on
/// the control rail. No description paragraph and no close button: the footer's default button
/// closes it, as do Escape and a press outside.
pub fn modal_panel(
    p: Paint,
    key: &str,
    title: &str,
    width: ModalWidth,
    body: Vec<El>,
    footer: Vec<El>,
) -> El {
    let c = &p.k.c;
    col()
        .key(key)
        .role("dialog")
        .label(title)
        .w(width.px())
        .pt(MODAL.top)
        .px(METRICS.card_edge())
        .pb(MODAL.bottom)
        .gap(MODAL.gap)
        .radius(p.k.radius.xl)
        .fill(c.bg_base)
        .border(p.hair, c.border_weak)
        .child(
            text(title)
                .key("title")
                .mx(METRICS.row_px)
                .font(p.title())
                .color(p.text()),
        )
        .children(body)
        .child(
            row()
                .key("footer")
                .mx(METRICS.row_px)
                .gap(8.0)
                .center_y()
                .justify_end()
                .children(footer),
        )
}

/// Mounts `panel` as a modal sheet: a scrim over the window, the panel centred over the content
/// panel and `MODAL.margin` clear of its edges, on the kit's overlay motion. Keys stay inside;
/// Escape or a press outside runs `close`.
pub fn modal_sheet(p: Paint, key: &str, mut panel: El, close: fn(&mut Ui), layer: &mut Vec<El>) {
    for shadow in &p.k.elevation.overlay {
        panel = panel.box_shadow(*shadow);
    }
    let springs = SpringPair::new(p.k.motion.overlay, p.k.motion.overlay);
    layer.push(
        el().key(format!("{key}-scrim"))
            .pin(Some(0.0), Some(0.0), Some(0.0), Some(0.0))
            .fill(p.k.c.scrim)
            .on_press(close)
            .transition(Transition::opacity().springs(springs))
            .a11y_hidden(),
    );
    layer.push(
        col()
            .key(format!("{key}-layer"))
            .pin(
                Some(METRICS.sidebar_w),
                Some(WINDOW.panel_inset),
                Some(WINDOW.panel_inset),
                Some(WINDOW.panel_inset),
            )
            .padding(MODAL.margin)
            .center()
            .transition(Transition::scale(0.96).springs(springs))
            .child(panel.modal(true, close)),
    );
}
