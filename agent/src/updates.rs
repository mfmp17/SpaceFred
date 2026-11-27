//! The update controls. The sidebar's foot shows the version and what the updater is doing, on
//! every pane; General's Updates section holds the release channel, and a row for an update
//! only while there is something to act on.

use rune::{El, Tx, el, row, text};
use rune_kit::SegmentedControl;
use spaceman::{
    app::{UPDATE_FAILED_PREFIX, UiUpdateStatus},
    config::UpdateChannel,
};

use crate::{
    Ui,
    controls::{self, ButtonKind, Paint},
};

const CHANNELS: [(&str, UpdateChannel); 2] = [
    ("Stable", UpdateChannel::Stable),
    ("Dev", UpdateChannel::Dev),
];

/// A channel's name as the picker shows it.
fn channel_label(channel: &str) -> &str {
    CHANNELS
        .iter()
        .find(|(_, value)| value.as_str() == channel)
        .map_or(channel, |(label, _)| label)
}

fn status(ui: &Ui) -> UiUpdateStatus {
    ui.agent
        .as_ref()
        .map_or(UiUpdateStatus::Unavailable, |agent| agent.update.clone())
}

fn install_failure(ui: &Ui) -> Option<&str> {
    ui.agent
        .as_ref()
        .and_then(|agent| agent.last_error.as_deref())
        .filter(|error| error.starts_with(UPDATE_FAILED_PREFIX))
}

/// An update that wants something from the user: what it is, and what happens next.
#[derive(Debug, PartialEq, Eq)]
struct Notice {
    title: String,
    detail: String,
}

/// The update to act on, if there is one. Checking, downloading and "up to date" need nothing
/// from the user, and the sidebar's foot already says them. A source build says why it never
/// installs an Anomaly release.
fn notice(status: &UiUpdateStatus, install_error: Option<&str>) -> Option<Notice> {
    let (title, detail) = match status {
        UiUpdateStatus::Ready { version } => (
            format!("SpaceFred {version} is ready"),
            "Windows are restored before it restarts".to_owned(),
        ),
        UiUpdateStatus::Switch { channel, version } => (
            format!("SpaceFred {version} is on {}", channel_label(channel)),
            "Switching installs it".to_owned(),
        ),
        UiUpdateStatus::Download { version } => (
            format!("SpaceFred {version} is out"),
            "This copy can't replace itself. Download it, or move SpaceFred to Applications \
             to update in place."
                .to_owned(),
        ),
        UiUpdateStatus::SourceBuild => (
            "Built from source".to_owned(),
            "SpaceFred does not install Anomaly's Spaceman releases.".to_owned(),
        ),
        UiUpdateStatus::Failed => match install_error {
            Some(error) => ("The update didn't install".to_owned(), error.to_owned()),
            None => (
                "Couldn't check for updates".to_owned(),
                "It tries again soon".to_owned(),
            ),
        },
        UiUpdateStatus::Idle
        | UiUpdateStatus::Checking
        | UiUpdateStatus::UpToDate
        | UiUpdateStatus::Downloading
        | UiUpdateStatus::Installing
        | UiUpdateStatus::Unavailable => return None,
    };
    Some(Notice { title, detail })
}

fn sidebar_label(update: &UiUpdateStatus, install_error: Option<&str>) -> &'static str {
    match update {
        UiUpdateStatus::Checking => "Checking for updates…",
        UiUpdateStatus::UpToDate => "SpaceFred is up to date",
        UiUpdateStatus::Downloading => "Downloading update…",
        UiUpdateStatus::Switch { .. } => "Channel switch ready",
        UiUpdateStatus::Download { .. } => "Update available",
        UiUpdateStatus::Installing => "Restarting…",
        UiUpdateStatus::Failed if install_error.is_some() => "Update install failed",
        UiUpdateStatus::Failed => "Update check failed",
        UiUpdateStatus::SourceBuild => "Built from source",
        UiUpdateStatus::Unavailable => "Updates unavailable",
        UiUpdateStatus::Idle | UiUpdateStatus::Ready { .. } => "Check for updates",
    }
}

fn install_button(p: Paint, key: &str, kind: ButtonKind) -> El {
    controls::button(
        p,
        key,
        "Install & restart",
        kind,
        |ui: &mut Ui, _: &mut Tx| {
            ui.install_update();
        },
    )
}

/// Under the version: the ready update, or a manual check and what the updater is doing.
pub fn sidebar(ui: &Ui, p: Paint) -> El {
    let c = &p.k.c;
    let update = status(ui);
    if update.ready_version().is_some() {
        return row()
            .key("update-ready")
            .mt(8.0)
            .pl(8.0)
            .gap(8.0)
            .center_y()
            .child(controls::status_dot(p.accent()))
            .child(install_button(p, "install-update", ButtonKind::Secondary));
    }
    let label = sidebar_label(&update, install_failure(ui));
    let mut foot = row()
        .key("check-for-updates")
        .h(20.0)
        .mt(2.0)
        .px(8.0)
        .center_y()
        .radius(p.k.radius.sm)
        .child(
            text(label)
                .key("label")
                .font(p.meta())
                .color(c.text_muted)
                .truncate(),
        );
    if update.can_check() {
        foot = foot
            .role("button")
            .label("Check for updates")
            .hover(c.hover)
            .press(c.pressed)
            .focusable(true)
            .focus_ring(2.0, -2.0, c.border_focus)
            .on_press(|ui: &mut Ui| ui.check_for_updates());
    }
    foot
}

/// General's Updates section: the update to act on, while there is one, then the channel. The
/// running version and a manual check live in the sidebar's foot, on every pane.
pub fn section(ui: &Ui, p: Paint) -> El {
    let update = status(ui);
    let mut rows = Vec::new();
    if let Some(notice) = notice(&update, install_failure(ui)) {
        let color = if update == UiUpdateStatus::Failed {
            p.warning()
        } else {
            p.muted()
        };
        rows.push(controls::setting_row(
            p,
            "update",
            &notice.title,
            Some((notice.detail, color)),
            action(p, &update),
        ));
    }
    let current = ui.doc.update_channel();
    rows.push(controls::setting_row(
        p,
        "channel",
        "Update channel",
        Some((
            "Stable gets releases. Dev gets a build of every change merged to dev.".into(),
            p.muted(),
        )),
        SegmentedControl::new(
            &CHANNELS.map(|(label, _)| label.to_owned()),
            CHANNELS.iter().position(|(_, channel)| *channel == current),
        )
        .fit()
        .on_pick(|ui: &mut Ui, index, _: &mut Tx| {
            if let Some((_, channel)) = CHANNELS.get(index) {
                ui.set_update_channel(*channel);
            }
        })
        .build(p.k)
        .key("channel-picker"),
    ));
    controls::section(p, "updates", "Updates", rows)
}

/// The one button an update's row carries.
fn action(p: Paint, update: &UiUpdateStatus) -> El {
    match update {
        UiUpdateStatus::Ready { .. } => install_button(p, "install", ButtonKind::Primary),
        UiUpdateStatus::Switch { channel, .. } => controls::button(
            p,
            "switch",
            &format!("Switch to {}", channel_label(channel)),
            ButtonKind::Primary,
            |ui: &mut Ui, _: &mut Tx| ui.confirm_update_switch(),
        ),
        UiUpdateStatus::Download { .. } => controls::button(
            p,
            "download",
            "Download",
            ButtonKind::Primary,
            |ui: &mut Ui, _: &mut Tx| ui.download_update(),
        ),
        _ if update.can_check() => controls::button(
            p,
            "check",
            "Check again",
            ButtonKind::Secondary,
            |ui: &mut Ui, _: &mut Tx| ui.check_for_updates(),
        ),
        _ => el().key("idle"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn title(status: &UiUpdateStatus) -> Option<String> {
        notice(status, None).map(|notice| notice.title)
    }

    #[test]
    fn failed_install_shows_install_reason_instead_of_check_failure() {
        let reason = format!("{UPDATE_FAILED_PREFIX}could not replace app bundle");
        assert_eq!(
            notice(&UiUpdateStatus::Failed, Some(&reason)).map(|notice| notice.detail),
            Some(reason.clone())
        );
        assert_eq!(
            sidebar_label(&UiUpdateStatus::Failed, Some(&reason)),
            "Update install failed"
        );
        assert_eq!(
            title(&UiUpdateStatus::Failed).as_deref(),
            Some("Couldn't check for updates")
        );
        assert_eq!(
            sidebar_label(&UiUpdateStatus::Failed, None),
            "Update check failed"
        );
    }

    #[test]
    fn general_shows_an_update_row_only_when_there_is_something_to_act_on() {
        // The sidebar's foot already shows the version and what the updater is doing, so an
        // idle General repeats neither.
        for quiet in [
            UiUpdateStatus::Idle,
            UiUpdateStatus::Checking,
            UiUpdateStatus::UpToDate,
            UiUpdateStatus::Downloading,
            UiUpdateStatus::Installing,
            UiUpdateStatus::Unavailable,
        ] {
            assert_eq!(notice(&quiet, None), None, "{quiet:?}");
        }
        let version = || "0.3.4".to_owned();
        assert_eq!(
            title(&UiUpdateStatus::Ready { version: version() }).as_deref(),
            Some("SpaceFred 0.3.4 is ready")
        );
        assert_eq!(
            title(&UiUpdateStatus::Switch {
                channel: "dev".into(),
                version: version(),
            })
            .as_deref(),
            Some("SpaceFred 0.3.4 is on Dev")
        );
        assert_eq!(
            title(&UiUpdateStatus::Download { version: version() }).as_deref(),
            Some("SpaceFred 0.3.4 is out")
        );
        assert_eq!(
            notice(&UiUpdateStatus::SourceBuild, None).map(|notice| notice.detail),
            Some("SpaceFred does not install Anomaly's Spaceman releases.".to_owned())
        );
    }
}
