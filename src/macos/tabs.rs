//! Native tab detach: the focused app's own "Move Tab to New Window" menu
//! command, found through Accessibility and pressed with `AXPress`. The menu
//! bar is only read; that one press is the only action this module performs.

use std::{collections::HashSet, sync::OnceLock};

use objc2::{rc::autoreleasepool, runtime::AnyObject};
use objc2_application_services::{AXError, AXUIElement};
use objc2_core_foundation::{CFArray, CFBoolean, CFRetained, CFString, Type};
use objc2_foundation::{NSDictionary, NSMutableDictionary, NSString};

use crate::PhysicalWindowId;

use super::ax::{application_element, attribute, cf_array_items};

/// What pressing "Move Tab to New Window" did.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TabDetachResult {
    /// The enabled command was pressed; a new window is on its way.
    Detached,
    /// The command is missing (the app has no native tabs) or disabled (the
    /// window holds a single tab). Nothing was pressed.
    SingleTabOrUnsupported,
    /// Accessibility failed before or while pressing.
    Error(String),
}

/// AppKit's action selector, which it exposes as the item's `AXIdentifier`
/// whatever the UI language.
const MOVE_TAB_SELECTOR: &str = "moveTabToNewWindow:";
/// English titles, including Chromium's plural for a multi-tab selection.
const ENGLISH_TITLES: [&str; 3] = [
    "Move Tab to New Window",
    "Move Tabs to New Window",
    "Move Tab(s) to New Window",
];
/// AppKit ships every translation of its menu commands here, keyed by English.
const MENU_COMMANDS: &str =
    "/System/Library/Frameworks/AppKit.framework/Resources/MenuCommands.loctable";
/// The command lives in one of the trailing menus (Tab, Window, Help); the
/// Apple, app, File and Edit menus are never read.
const TRAILING_MENUS: usize = 4;

/// Presses the "Move Tab to New Window" item of `window`'s application,
/// which acts on its key window: the focused `window`.
pub fn detach_tab(window: &PhysicalWindowId) -> TabDetachResult {
    autoreleasepool(|_| {
        let application = application_element(window.app.pid);
        let item = match move_tab_item(&application) {
            Ok(Some(item)) => item,
            Ok(None) => return TabDetachResult::SingleTabOrUnsupported,
            Err(error) => return TabDetachResult::Error(format!("AXMenuBar: {error:?}")),
        };
        match attribute::<CFBoolean>(&item, "AXEnabled").map(|enabled| enabled.value()) {
            Ok(false) => return TabDetachResult::SingleTabOrUnsupported,
            Ok(true) | Err(AXError::NoValue | AXError::AttributeUnsupported) => {}
            Err(error) => return TabDetachResult::Error(format!("AXEnabled: {error:?}")),
        }
        // SAFETY: `item` is a live `AXUIElement` and `"AXPress"` is a standard AX action.
        match unsafe { item.perform_action(&CFString::from_static_str("AXPress")) } {
            AXError::Success => TabDetachResult::Detached,
            error => TabDetachResult::Error(format!("AXPress: {error:?}")),
        }
    })
}

/// The first trailing-menu item whose identifier is AppKit's selector, or else
/// the first whose title is a known translation of the command.
fn move_tab_item(application: &AXUIElement) -> Result<Option<CFRetained<AXUIElement>>, AXError> {
    let bar = attribute::<AXUIElement>(application, "AXMenuBar")?;
    let bar_items = attribute::<CFArray>(&bar, "AXChildren")?;
    let bar_items = cf_array_items::<AXUIElement>(&bar_items).collect::<Vec<_>>();
    let mut by_title = None;
    for bar_item in bar_items.iter().rev().take(TRAILING_MENUS) {
        let Ok(menus) = attribute::<CFArray>(bar_item, "AXChildren") else {
            continue;
        };
        for menu in cf_array_items::<AXUIElement>(&menus) {
            let Ok(items) = attribute::<CFArray>(menu, "AXChildren") else {
                continue;
            };
            for item in cf_array_items::<AXUIElement>(&items) {
                let identifier = string_attribute(item, "AXIdentifier");
                let title = by_title
                    .is_none()
                    .then(|| string_attribute(item, "AXTitle"))
                    .flatten();
                match classify_item(identifier.as_deref(), title.as_deref(), localized_titles()) {
                    ItemMatch::Selector => return Ok(Some(item.retain())),
                    ItemMatch::Title if by_title.is_none() => by_title = Some(item.retain()),
                    ItemMatch::Title | ItemMatch::Other => {}
                }
            }
        }
    }
    Ok(by_title)
}

#[derive(Debug, Eq, PartialEq)]
pub(super) enum ItemMatch {
    Selector,
    Title,
    Other,
}

pub(super) fn classify_item(
    identifier: Option<&str>,
    title: Option<&str>,
    localized: &HashSet<String>,
) -> ItemMatch {
    if identifier == Some(MOVE_TAB_SELECTOR) {
        return ItemMatch::Selector;
    }
    let Some(title) = title.map(str::trim).filter(|title| !title.is_empty()) else {
        return ItemMatch::Other;
    };
    if ENGLISH_TITLES
        .iter()
        .any(|english| english.eq_ignore_ascii_case(title))
        || localized.contains(title)
    {
        ItemMatch::Title
    } else {
        ItemMatch::Other
    }
}

fn string_attribute(element: &AXUIElement, name: &'static str) -> Option<String> {
    attribute::<CFString>(element, name)
        .ok()
        .map(|value| value.to_string())
}

/// Every translation AppKit ships of "Move Tab to New Window", read once.
fn localized_titles() -> &'static HashSet<String> {
    static TITLES: OnceLock<HashSet<String>> = OnceLock::new();
    TITLES.get_or_init(|| autoreleasepool(|_| load_localized_titles().unwrap_or_default()))
}

fn load_localized_titles() -> Option<HashSet<String>> {
    let path = NSString::from_str(MENU_COMMANDS);
    // SAFETY: The loctable is a property-list dictionary; both generic
    // parameters are `AnyObject`, so every key and value is downcast checked
    // below before use.
    let table: objc2::rc::Retained<NSMutableDictionary<AnyObject, AnyObject>> =
        unsafe { NSMutableDictionary::dictionaryWithContentsOfFile(&path) }?;
    let key = NSString::from_str(ENGLISH_TITLES[0]);
    let titles = table
        .allValues()
        .iter()
        .filter_map(|locale| {
            let locale = locale.downcast_ref::<NSDictionary>()?;
            let title = locale.objectForKey(&key)?;
            Some(title.downcast_ref::<NSString>()?.to_string())
        })
        .collect();
    Some(titles)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_selector_identifier_matches_in_any_language() {
        let none = HashSet::new();
        assert_eq!(
            classify_item(
                Some("moveTabToNewWindow:"),
                Some("Tab in ein neues Fenster bewegen"),
                &none
            ),
            ItemMatch::Selector
        );
        assert_eq!(
            classify_item(Some("moveTabToNewWindow:"), None, &none),
            ItemMatch::Selector
        );
    }

    #[test]
    fn titles_match_english_chromium_plurals_and_appkit_translations() {
        let localized = HashSet::from(["Tab in ein neues Fenster bewegen".to_owned()]);
        for title in [
            "Move Tab to New Window",
            "Move Tabs to New Window",
            " move tab to new window ",
            "Tab in ein neues Fenster bewegen",
        ] {
            assert_eq!(
                classify_item(None, Some(title), &localized),
                ItemMatch::Title,
                "{title}"
            );
        }
        for title in [
            "Merge All Windows",
            "New Window",
            "",
            "Move Window to Left Side of Screen",
        ] {
            assert_eq!(
                classify_item(Some("_NS:42"), Some(title), &localized),
                ItemMatch::Other
            );
        }
        assert_eq!(classify_item(None, None, &localized), ItemMatch::Other);
    }

    #[test]
    fn appkit_ships_translations_of_the_command() {
        let titles = localized_titles();
        assert!(titles.contains("Move Tab to New Window"), "{titles:?}");
        assert!(titles.len() > 10, "{titles:?}");
    }
}
