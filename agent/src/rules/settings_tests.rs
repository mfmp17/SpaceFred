//! Settings, Layout and Labs edits that remove a key keep the notes set off above it.

use spaceman::{
    config::{Config, LabFeature, LayoutMode},
    spacing::Spacing,
};

use super::super::Doc;

fn saved_after(source: &str, edit: impl FnOnce(&mut Doc)) -> String {
    let mut doc = Doc::parse(source);
    edit(&mut doc);
    let saved = doc.source();
    assert!(Config::parse(&saved).is_ok(), "{saved}");
    saved
}

#[test]
fn turning_a_lab_off_keeps_a_commented_out_lab() {
    let source = "version = 1\n\n[labs]\nactivity = true\n\n# Off until I try it:\n# focus_follows_mouse = true\n\nrecent_focus = true\nworkspace_steps = true\n";
    let saved = saved_after(source, |doc| doc.set_lab(LabFeature::RecentFocus, false));
    assert!(!saved.contains("recent_focus"), "{saved}");
    assert!(
        saved.contains(
            "# Off until I try it:\n# focus_follows_mouse = true\n\nworkspace_steps = true"
        ),
        "{saved}"
    );
}

#[test]
fn a_default_setting_keeps_a_heading_above_it() {
    let source = "version = 1\n\n[settings]\nhud = false\n\n# Tiling notes.\n\nlayout = \"columns\"\nmouse_follows_focus = \"window\"\n";
    let saved = saved_after(source, |doc| doc.set_layout(LayoutMode::Dwindle));
    assert!(!saved.contains("layout ="), "{saved}");
    assert!(saved.contains("# Tiling notes."), "{saved}");

    // The last key of a table, before another table.
    let source = "version = 1\n\n[settings]\nhud = false\n\n# Laptop notes.\n\npause_on_builtin_display = true\n\n[labs]\nactivity = true\n";
    let saved = saved_after(source, |doc| doc.set_pause_on_builtin_display(false));
    assert!(!saved.contains("pause_on_builtin_display"), "{saved}");
    let note = saved.find("# Laptop notes.").expect(&saved);
    assert!(note < saved.find("[labs]").unwrap(), "{saved}");
}

#[test]
fn emptying_a_table_keeps_the_note_above_its_only_key() {
    let source = "version = 1\n\n# Settings heading.\n\n[settings]\n\n# Laptop notes.\n\npause_on_builtin_display = true\n";
    let saved = saved_after(source, |doc| doc.set_pause_on_builtin_display(false));
    assert!(!saved.contains("[settings]"), "{saved}");
    let heading = saved.find("# Settings heading.").expect(&saved);
    assert!(
        heading < saved.find("# Laptop notes.").expect(&saved),
        "{saved}"
    );
}

#[test]
fn a_zero_gap_keeps_a_heading_above_it() {
    let source =
        "version = 1\n\n[settings.gaps]\ninner = 6\n\n# Room for the bar.\n\ntop = 32\nleft = 8\n";
    let saved = saved_after(source, |doc| {
        let spacing = Spacing {
            top: 0,
            ..doc.spacing()
        };
        doc.set_spacing(spacing);
    });
    assert!(!saved.contains("top ="), "{saved}");
    assert!(saved.contains("# Room for the bar.\n\nleft = 8"), "{saved}");
}

#[test]
fn a_note_directly_above_a_removed_key_goes_with_it() {
    let source =
        "version = 1\n\n[labs]\nactivity = true\n# Trying this week.\nrecent_focus = true\n";
    let saved = saved_after(source, |doc| doc.set_lab(LabFeature::RecentFocus, false));
    assert!(!saved.contains("# Trying this week."), "{saved}");
    assert!(saved.contains("activity = true"), "{saved}");
}
