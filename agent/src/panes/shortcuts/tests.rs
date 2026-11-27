//! The three-finger swipe's row: the fixed tray while `[labs] swipe_switch` is off, a switch
//! for `settings.trackpad_swipe` only with it on.

use spaceman::config::{Config, LabFeature};

use super::SwipeRow;
use crate::{controls::Paint, rules::Doc};

#[test]
fn the_swipe_row_stays_fixed_until_its_lab_is_on() {
    let plain = Doc::parse("version = 1\n");
    assert_eq!(SwipeRow::of(&plain), SwipeRow::Fixed);
    // The setting alone changes nothing while the Lab is off.
    let off = Doc::parse("version = 1\n[settings]\ntrackpad_swipe = false\n");
    assert_eq!(SwipeRow::of(&off), SwipeRow::Fixed);

    let mut doc = Doc::parse("version = 1\n[labs]\nswipe_switch = true\n");
    assert_eq!(SwipeRow::of(&doc), SwipeRow::Switch { on: true });
    doc.set_trackpad_swipe(false);
    assert_eq!(SwipeRow::of(&doc), SwipeRow::Switch { on: false });
    assert!(!Config::parse(&doc.source()).unwrap().trackpad_swipe());

    // Turning the Lab back off brings the fixed row back, and swipes switch again.
    doc.set_lab(LabFeature::SwipeSwitch, false);
    assert_eq!(SwipeRow::of(&doc), SwipeRow::Fixed);
    assert!(Config::parse(&doc.source()).unwrap().trackpad_swipe());
}

#[test]
fn the_shortcuts_pane_draws_with_the_swipe_lab_off_and_on() {
    let p = Paint::new(true, 2.0);
    let mut ui = crate::playground::blank_fixture();
    let _ = super::pane(&ui, p);
    ui.doc.set_lab(LabFeature::SwipeSwitch, true);
    let _ = super::pane(&ui, p);
}

fn shortcuts(source: &str) -> spaceman::shortcuts::Shortcuts {
    Config::parse(source).unwrap().shortcuts().clone()
}

#[test]
fn a_rejected_table_marks_the_rows_only_while_the_previous_shortcuts_run() {
    let p = Paint::new(true, 2.0);
    let mut ui = crate::playground::blank_fixture();
    let rejected = "Shortcuts not applied: no shortcut registered".to_owned();
    // Rolled back: the agent publishes the table that still runs.
    ui.agent = Some(spaceman::app::UiAgentSnapshot {
        automatic: true,
        last_error: Some(rejected.clone()),
        running_shortcuts: Some(shortcuts("version = 1\n")),
        ..Default::default()
    });
    assert!(super::running_shortcuts(&ui).is_some());
    let _ = super::pane(&ui, p);
    // The saved table turns the built-ins off, so the running workspace keys get their own rows.
    ui.doc = Doc::parse(concat!(
        "version = 1\n[shortcuts]\ndefaults = false\n",
        "[shortcuts.bindings]\n\"ctrl+alt+d\" = \"switch next\"\n",
    ));
    let _ = super::pane(&ui, p);
    // The rollback failed too, or tiling was paused: the reason stays, nothing runs.
    for automatic in [false, true] {
        ui.agent = Some(spaceman::app::UiAgentSnapshot {
            automatic,
            last_error: Some(rejected.clone()),
            running_shortcuts: None,
            ..Default::default()
        });
        assert!(super::running_shortcuts(&ui).is_none());
        let _ = super::pane(&ui, p);
    }
}

#[test]
fn a_row_names_the_chord_that_still_runs() {
    let running =
        shortcuts("version = 1\n[shortcuts.bindings]\n\"ctrl+alt+y\" = \"focus right\"\n");
    let saved = shortcuts(concat!(
        "version = 1\n[shortcuts]\ndefaults = false\n",
        "[shortcuts.bindings]\n\"ctrl+alt+y\" = \"switch next\"\n",
    ));
    let focus_right = super::catalog::find("focus-right").unwrap();
    let note = super::running_note(&running, &saved, &[], focus_right).unwrap();
    assert!(note.starts_with("Still runs"), "{note}");
    assert_eq!(super::running_note(&saved, &saved, &[], focus_right), None);
    let float = super::catalog::find("float").unwrap();
    assert!(super::running_note(&running, &saved, &[], float).is_some());
}
