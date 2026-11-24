//! Workspace rows in the status menu: a section per display, a check on each display's visible
//! workspace, the icons of the apps on it, and its shortcut (Control+number, or Option+letter).
//! Choosing a row switches to it.

use objc2::{AnyThread, MainThreadMarker, rc::Retained, runtime::AnyObject, sel};
use objc2_app_kit::{
    NSAttributedStringAttachmentConveniences, NSControlStateValueOn, NSEventModifierFlags, NSFont,
    NSFontAttributeName, NSImage, NSMenuItem, NSRunningApplication, NSTextAttachment,
};
use objc2_foundation::{
    NSAttributedString, NSDictionary, NSMutableAttributedString, NSPoint, NSRect, NSSize, NSString,
};

use super::Target;
use crate::WorkspaceId;

/// Workspaces shown for one display, in the order the menu lists them.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct DisplayWorkspaces {
    pub(crate) display: String,
    pub(crate) workspaces: Vec<MenuWorkspace>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct MenuWorkspace {
    pub(crate) workspace: WorkspaceId,
    pub(crate) current: bool,
    /// Process IDs of the apps with windows on the workspace, in tile order, without repeats.
    pub(crate) apps: Vec<i32>,
}

const ICON_SIZE: f64 = 16.0;
const MAX_ICONS: usize = 6;

/// Menu items for `displays`, ending in a separator; empty when there is nothing to list.
pub(super) fn items(
    displays: &[DisplayWorkspaces],
    target: &Target,
    marker: MainThreadMarker,
) -> Vec<Retained<NSMenuItem>> {
    let mut items = Vec::new();
    let headed = displays.len() > 1;
    for display in displays {
        if headed {
            items.push(NSMenuItem::sectionHeaderWithTitle(
                &NSString::from_str(&display.display),
                marker,
            ));
        }
        items.extend(
            display
                .workspaces
                .iter()
                .map(|workspace| item(workspace, target, marker)),
        );
    }
    if !items.is_empty() {
        items.push(NSMenuItem::separatorItem(marker));
    }
    items
}

fn item(
    workspace: &MenuWorkspace,
    target: &Target,
    marker: MainThreadMarker,
) -> Retained<NSMenuItem> {
    let label = workspace.workspace.to_string();
    let item = super::action_item(
        &label,
        target,
        sel!(switchWorkspace:),
        &key_equivalent(workspace.workspace),
        marker,
    );
    // Numbered spaces use Control; named workspaces stay on Option.
    let modifiers = if workspace.workspace.number().is_some() {
        NSEventModifierFlags::Control
    } else {
        NSEventModifierFlags::Option
    };
    item.setKeyEquivalentModifierMask(modifiers);
    item.setTag(isize::try_from(workspace.workspace.index()).unwrap_or_default());
    if workspace.current {
        item.setState(NSControlStateValueOn);
    }
    let apps = workspace
        .apps
        .iter()
        .filter_map(|pid| NSRunningApplication::runningApplicationWithProcessIdentifier(*pid))
        .collect::<Vec<_>>();
    let icons = apps
        .iter()
        .filter_map(|app| app.icon())
        .take(MAX_ICONS)
        .collect::<Vec<_>>();
    if !icons.is_empty() {
        item.setAttributedTitle(Some(&title_with_icons(&label, &icons)));
    }
    let names = apps
        .iter()
        .filter_map(|app| app.localizedName().map(|name| name.to_string()))
        .collect::<Vec<_>>();
    if !names.is_empty() {
        item.setToolTip(Some(&NSString::from_str(&names.join(", "))));
    }
    item
}

/// The key that switches to `workspace`: Control plus this key for a number, Option plus
/// this key for a named workspace.
pub(crate) fn key_equivalent(workspace: WorkspaceId) -> String {
    match (workspace.number(), workspace.letter()) {
        (Some(number), _) => (number % 10).to_string(),
        (None, Some(letter)) => letter.to_ascii_lowercase().to_string(),
        (None, None) => String::new(),
    }
}

/// `label` in the menu font followed by `icons`, centred on its cap height.
fn title_with_icons(label: &str, icons: &[Retained<NSImage>]) -> Retained<NSAttributedString> {
    let font = NSFont::menuFontOfSize(0.0);
    // SAFETY: `NSFontAttributeName` is an immutable AppKit constant.
    let key = unsafe { NSFontAttributeName };
    let attributes = NSDictionary::<NSString, AnyObject>::from_slices(&[key], &[font.as_ref()]);
    // SAFETY: the dictionary maps `NSFontAttributeName` to an `NSFont`, as AppKit expects.
    let title = unsafe {
        NSMutableAttributedString::initWithString_attributes(
            NSMutableAttributedString::alloc(),
            &NSString::from_str(&format!("{label}   ")),
            Some(&attributes),
        )
    };
    let y = (font.capHeight() - ICON_SIZE) / 2.0;
    for (index, icon) in icons.iter().enumerate() {
        if index > 0 {
            // SAFETY: as above.
            let gap = unsafe {
                NSAttributedString::initWithString_attributes(
                    NSAttributedString::alloc(),
                    &NSString::from_str("\u{2009}"),
                    Some(&attributes),
                )
            };
            title.appendAttributedString(&gap);
        }
        let attachment = NSTextAttachment::new();
        attachment.setImage(Some(icon));
        attachment.setBounds(NSRect::new(
            NSPoint::new(0.0, y),
            NSSize::new(ICON_SIZE, ICON_SIZE),
        ));
        // SAFETY: as above.
        let icon = unsafe {
            NSAttributedString::attributedStringWithAttachment_attributes(&attachment, &attributes)
        };
        title.appendAttributedString(&icon);
    }
    title.into_super()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_equivalents_match_the_global_shortcuts() {
        assert_eq!(key_equivalent(WorkspaceId::ONE), "1");
        assert_eq!(key_equivalent(WorkspaceId::new(10).unwrap()), "0");
        assert_eq!(key_equivalent(WorkspaceId::named('D').unwrap()), "d");
    }
}
