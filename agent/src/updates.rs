//! The update controls: the sidebar's foot under the version, and General's
//! Updates section with the release channel.

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

/// The version this build was released as (the Release workflow's stamp), else Cargo's.
fn build_version() -> &'static str {
    spaceman::build_version()
}

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

/// What the updater is doing, in a sentence.
fn describe(status: &UiUpdateStatus, install_error: Option<&str>) -> String {
    match status {
        UiUpdateStatus::Idle => "Checks for updates automatically.".into(),
        UiUpdateStatus::Checking => "Checking for updates…".into(),
        UiUpdateStatus::UpToDate => "SpaceFred is up to date.".into(),
        UiUpdateStatus::Downloading => "Downloading an update…".into(),
        UiUpdateStatus::Ready { version } => {
            format!("SpaceFred {version} is ready. Windows are restored before it restarts.")
        }
        UiUpdateStatus::Switch { channel, version } => format!(
            "Switching to {} installs SpaceFred {version}.",
            channel_label(channel)
        ),
        UiUpdateStatus::Download { version } => format!(
            "SpaceFred {version} is out. This copy can't replace itself, so download it, \
             or move SpaceFred to Applications to update in place."
        ),
        UiUpdateStatus::Installing => "Installing and restarting…".into(),
        UiUpdateStatus::Failed => install_error
            .map(str::to_owned)
            .unwrap_or_else(|| "Couldn't check for updates. It tries again soon.".into()),
        UiUpdateStatus::SourceBuild => {
            "Built from source. SpaceFred does not install Anomaly's Spaceman releases.".into()
        }
        UiUpdateStatus::Unavailable => "Updates are off for this copy of SpaceFred.".into(),
    }
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
        "Install & Restart",
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
            .mt(6.0)
            .pl(6.0)
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
        .px(6.0)
        .center_y()
        .radius(p.k.radius.sm)
        .child(
            text(label)
                .key("label")
                .font(p.k.ty.small)
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

/// General's Updates section: the running version and its update, and the channel.
pub fn section(ui: &Ui, p: Paint) -> El {
    let update = status(ui);
    let color = match update {
        UiUpdateStatus::Ready { .. }
        | UiUpdateStatus::Switch { .. }
        | UiUpdateStatus::Download { .. } => p.accent(),
        UiUpdateStatus::Failed => p.k.c.warning.fg,
        _ => p.muted(),
    };
    let control = if update.ready_version().is_some() {
        install_button(p, "install", ButtonKind::Primary)
    } else if let UiUpdateStatus::Switch { channel, .. } = &update {
        controls::button(
            p,
            "switch",
            &format!("Switch to {}", channel_label(channel)),
            ButtonKind::Primary,
            |ui: &mut Ui, _: &mut Tx| ui.confirm_update_switch(),
        )
    } else if matches!(update, UiUpdateStatus::Download { .. }) {
        controls::button(
            p,
            "download",
            "Download",
            ButtonKind::Primary,
            |ui: &mut Ui, _: &mut Tx| ui.download_update(),
        )
    } else if update.can_check() {
        controls::button(
            p,
            "check",
            "Check Now",
            ButtonKind::Secondary,
            |ui: &mut Ui, _: &mut Tx| ui.check_for_updates(),
        )
    } else {
        el().key("idle")
    };
    let version = controls::setting_row(
        p,
        "version",
        &format!("SpaceFred {}", build_version()),
        Some((describe(&update, install_failure(ui)), color)),
        control,
    );
    let current = ui.doc.update_channel();
    let channel = controls::setting_row(
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
    );
    controls::section(p, "updates", "Updates", vec![version, channel])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failed_install_shows_install_reason_instead_of_check_failure() {
        let reason = format!("{UPDATE_FAILED_PREFIX}could not replace app bundle");
        assert_eq!(describe(&UiUpdateStatus::Failed, Some(&reason)), reason);
        assert_eq!(
            sidebar_label(&UiUpdateStatus::Failed, Some(&reason)),
            "Update install failed"
        );
        assert_eq!(
            describe(&UiUpdateStatus::Failed, None),
            "Couldn't check for updates. It tries again soon."
        );
        assert_eq!(
            sidebar_label(&UiUpdateStatus::Failed, None),
            "Update check failed"
        );
    }
}
