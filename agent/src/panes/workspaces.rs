//! Workspaces pane: display assignments for numbered and referenced named workspaces.

mod displays;

pub(crate) use displays::{
    ConnectedDisplay, THIRD_FIXTURE_ID, fixture_pair, fixture_quad, fixture_three,
};

use rune::{El, Tx, col, row, text};
use rune_kit::SegmentedControl;
use spaceman::WorkspaceId;

use crate::{
    Pane, Ui,
    controls::{self, Paint},
    rules::Doc,
    view::scroll_body,
};

use displays::{Choice, MAIN, SECONDARY};

/// How often the pane reads the connected displays while it is on screen, in seconds.
const DISPLAY_REFRESH_SECONDS: f64 = 1.0;

/// The label of the extra segment a workspace gets when its saved display is unplugged.
const UNPLUGGED: &str = "Not connected";

/// Built-in default display for `workspace` when `config.toml` sets no explicit `monitor`.
const fn default_monitor(workspace: WorkspaceId) -> &'static str {
    match workspace.number() {
        Some(n) if n >= 6 => SECONDARY,
        _ => MAIN,
    }
}

/// Which display `workspace` currently targets: explicit override from `config.toml`, or its
/// built-in default (1–5 and named workspaces on Main, 6–10 on Secondary).
fn effective_monitor(doc: &Doc, workspace: WorkspaceId) -> String {
    doc.monitor(workspace)
        .unwrap_or_else(|| default_monitor(workspace).to_owned())
}

/// Reads the connected displays when Settings › Workspaces is on screen, about once a second, so
/// plugging a display in shows its segment without reopening Settings.
pub(crate) fn poll_displays(ui: &mut Ui, clock: f64) -> bool {
    if ui.fixture || ui.pane != Pane::Workspaces {
        return false;
    }
    if ui
        .displays_at
        .is_some_and(|read| clock - read < DISPLAY_REFRESH_SECONDS)
    {
        return false;
    }
    ui.displays_at = Some(clock);
    let Some(connected) = read_displays() else {
        return false;
    };
    if connected == ui.displays {
        return false;
    }
    ui.displays = connected;
    true
}

#[cfg(target_os = "macos")]
fn read_displays() -> Option<Vec<displays::ConnectedDisplay>> {
    spaceman::macos::outputs()
        .ok()
        .map(|outputs| displays::connected(&outputs))
}

#[cfg(not(target_os = "macos"))]
fn read_displays() -> Option<Vec<displays::ConnectedDisplay>> {
    None
}

/// One card: a display and the workspaces assigned to it.
struct Section {
    key: String,
    title: String,
    caption: String,
    workspaces: Vec<WorkspaceId>,
}

/// The pane's cards in display order, then the displays a saved selector names but that are not
/// plugged in, which keep their assignment instead of losing it.
fn sections(doc: &Doc, choices: &[Choice], workspaces: Vec<WorkspaceId>) -> Vec<Section> {
    let mut buckets: Vec<Vec<WorkspaceId>> = (0..choices.len()).map(|_| Vec::new()).collect();
    let mut unplugged = Vec::new();
    for workspace in workspaces {
        match displays::index_of(choices, &effective_monitor(doc, workspace)) {
            Some(index) => buckets[index].push(workspace),
            None => unplugged.push(workspace),
        }
    }
    let mut sections = Vec::new();
    for (index, choice) in choices.iter().enumerate() {
        let workspaces = std::mem::take(&mut buckets[index]);
        if workspaces.is_empty() && !choice.connected {
            continue;
        }
        sections.push(Section {
            key: format!("display-{index}"),
            title: choice.title.clone(),
            caption: choice.caption.clone(),
            workspaces,
        });
    }
    if !unplugged.is_empty() {
        sections.push(Section {
            key: "unplugged-display".to_owned(),
            title: "Disconnected displays".to_owned(),
            caption: "Saved in config.toml".to_owned(),
            workspaces: unplugged,
        });
    }
    sections
}

pub fn pane(ui: &Ui, p: Paint) -> El {
    let choices = displays::choices(&ui.displays);
    let sections = sections(
        &ui.doc,
        &choices,
        ui.doc.workspaces_with_referenced(WorkspaceId::numbered()),
    );
    let cards: Vec<El> = sections
        .iter()
        .map(|section| {
            let rows: Vec<El> = if section.workspaces.is_empty() {
                vec![empty_display_row(p)]
            } else {
                section
                    .workspaces
                    .iter()
                    .map(|workspace| {
                        let target = effective_monitor(&ui.doc, *workspace);
                        monitor_row(p, &choices, *workspace, &target)
                    })
                    .collect()
            };
            col()
                .key(section.key.clone())
                .gap(8.0)
                .child(controls::heading(
                    p,
                    "header",
                    &section.title,
                    Some(&section.caption),
                ))
                .child(controls::card(p, "card", rows))
        })
        .collect();
    scroll_body(
        "workspaces-body",
        col().key("sections").gap(24.0).children(cards),
    )
}

fn empty_display_row(p: Paint) -> El {
    row()
        .key("empty")
        .min_h(44.0)
        .px(12.0)
        .py(8.0)
        .center_y()
        .child(
            text("No workspaces assigned")
                .key("label")
                .font(p.k.ty.ui)
                .color(p.faint()),
        )
}

/// The `config.toml` override picking `chosen` writes; `None` clears it, so picking a
/// workspace's built-in default leaves no redundant entry behind.
fn override_for(workspace: WorkspaceId, chosen: &str) -> Option<String> {
    (chosen != default_monitor(workspace)).then(|| chosen.to_owned())
}

/// The row's muted caption: an unplugged preferred display always falls back to Main in
/// `DisplayInventory::resolve`, even for workspaces 6–10 whose built-in default is Secondary.
fn row_description(workspace: WorkspaceId, effective: &str, unplugged: bool) -> Option<String> {
    if unplugged {
        return Some("Not connected · uses Main until it returns".to_owned());
    }
    let default = default_monitor(workspace);
    if effective != default {
        let default_label = if default == SECONDARY {
            "Secondary"
        } else {
            "Main"
        };
        Some(format!("Default: {default_label}"))
    } else {
        None
    }
}

/// The picker's segments and the selected one: every display's label, then `Not connected` when
/// `effective` names a display that is not plugged in.
fn picker(choices: &[Choice], effective: &str) -> (Vec<String>, usize) {
    let mut labels: Vec<String> = choices.iter().map(|choice| choice.label.clone()).collect();
    let selected = displays::index_of(choices, effective).unwrap_or_else(|| {
        labels.push(UNPLUGGED.to_owned());
        choices.len()
    });
    (labels, selected)
}

fn monitor_row(p: Paint, choices: &[Choice], workspace: WorkspaceId, effective: &str) -> El {
    let (labels, selected) = picker(choices, effective);
    let selectors: Vec<String> = choices
        .iter()
        .map(|choice| choice.selector.clone())
        .collect();
    let picker = SegmentedControl::new(&labels, Some(selected))
        .fit()
        .on_pick(move |ui: &mut Ui, index, _: &mut Tx| {
            let Some(chosen) = selectors.get(index) else {
                return;
            };
            let monitor = override_for(workspace, chosen);
            ui.edit(|doc| doc.set_monitor(workspace, monitor.as_deref()));
        })
        .build(p.k)
        .key("monitor");
    let title = match workspace.number() {
        Some(10) => "Workspace 10 (⌃0)".to_owned(),
        Some(_) => format!("Workspace {workspace}"),
        None => format!("Workspace {workspace} (named)"),
    };
    let description = row_description(workspace, effective, selected >= choices.len())
        .map(|copy| (copy, p.faint()));
    controls::setting_row(
        p,
        &format!("workspace-{workspace}"),
        &title,
        description,
        picker,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use displays::{FOURTH_FIXTURE_ID, choices, index_of};

    #[test]
    fn workspace_monitor_defaults_split_main_and_secondary() {
        let doc = Doc::parse("version = 1\n");
        for number in 1..=5 {
            let ws = WorkspaceId::new(number).unwrap();
            assert_eq!(default_monitor(ws), MAIN);
            assert_eq!(effective_monitor(&doc, ws), MAIN);
        }
        for number in 6..=10 {
            let ws = WorkspaceId::new(number).unwrap();
            assert_eq!(default_monitor(ws), SECONDARY);
            assert_eq!(effective_monitor(&doc, ws), SECONDARY);
        }
        let named = WorkspaceId::named('M').unwrap();
        assert_eq!(default_monitor(named), MAIN);
        assert_eq!(effective_monitor(&doc, named), MAIN);

        let mut overridden = doc;
        overridden.set_monitor(WorkspaceId::new(2).unwrap(), Some(SECONDARY));
        overridden.set_monitor(WorkspaceId::new(7).unwrap(), Some(MAIN));
        assert_eq!(
            effective_monitor(&overridden, WorkspaceId::new(2).unwrap()),
            SECONDARY
        );
        assert_eq!(
            effective_monitor(&overridden, WorkspaceId::new(7).unwrap()),
            MAIN
        );
    }

    #[test]
    fn two_displays_keep_the_crisp_main_and_secondary_picker() {
        let choices = choices(&fixture_pair());
        assert_eq!(
            choices
                .iter()
                .map(|choice| choice.selector.as_str())
                .collect::<Vec<_>>(),
            [MAIN, SECONDARY]
        );
        assert_eq!(
            picker(&choices, MAIN).0,
            ["Main".to_owned(), "Secondary".to_owned()]
        );
        assert_eq!(picker(&choices, SECONDARY).1, 1);
        // The section captions name the actual displays.
        assert_eq!(
            choices[0].caption,
            "Built-in Retina Display · Default for 1–5"
        );
        assert_eq!(choices[1].caption, "Studio Display · Default for 6–10");
    }

    #[test]
    fn three_displays_offer_every_display_and_pick_the_third_by_uuid() {
        let choices = choices(&fixture_three());
        assert_eq!(
            choices
                .iter()
                .map(|choice| choice.selector.as_str())
                .collect::<Vec<_>>(),
            [MAIN, SECONDARY, THIRD_FIXTURE_ID]
        );
        // Two identically named external monitors are told apart.
        assert_eq!(
            picker(&choices, THIRD_FIXTURE_ID).0,
            [
                "Main".to_owned(),
                "Secondary".to_owned(),
                "Studio Display 2".to_owned()
            ]
        );
        assert_eq!(picker(&choices, THIRD_FIXTURE_ID).1, 2);
        assert_eq!(choices[2].title, "Studio Display 2");
        assert_eq!(choices[2].caption, "Display 3");

        // Selecting the third display writes its UUID, exactly as Option+Shift+Tab does.
        let mut doc = Doc::parse("version = 1\n");
        let chosen = choose(&choices, 2, WorkspaceId::new(9).unwrap());
        doc.set_monitor(WorkspaceId::new(9).unwrap(), chosen.as_deref());
        assert_eq!(
            doc.monitor(WorkspaceId::new(9).unwrap()).as_deref(),
            Some(THIRD_FIXTURE_ID)
        );
        // Rendering that saved UUID resolves it back to the display's friendly name.
        let saved = effective_monitor(&doc, WorkspaceId::new(9).unwrap());
        assert_eq!(saved, THIRD_FIXTURE_ID);
        assert_eq!(index_of(&choices, &saved), Some(2));
        assert_eq!(picker(&choices, &saved).0[2], "Studio Display 2");
    }

    #[test]
    fn four_displays_number_the_picker_and_keep_names_in_the_headings() {
        let choices = choices(&fixture_quad());
        assert_eq!(
            choices
                .iter()
                .map(|choice| choice.label.as_str())
                .collect::<Vec<_>>(),
            ["Main", "Secondary", "Display 3", "Display 4"]
        );
        assert_eq!(
            choices
                .iter()
                .map(|choice| choice.title.as_str())
                .collect::<Vec<_>>(),
            [
                "Main display",
                "Secondary display",
                "Studio Display 2",
                "LG UltraFine 5K"
            ]
        );
        assert_eq!(
            choose(&choices, 3, WorkspaceId::new(9).unwrap()).as_deref(),
            Some(FOURTH_FIXTURE_ID)
        );
    }

    #[test]
    fn a_saved_uuid_for_the_main_display_resolves_to_main() {
        let choices = choices(&fixture_pair());
        let main_id = choices[0].id.clone().unwrap();
        assert_eq!(index_of(&choices, &main_id), Some(0));
        let secondary_id = choices[1].id.clone().unwrap();
        assert_eq!(index_of(&choices, &secondary_id), Some(1));
    }

    #[test]
    fn unplugged_display_keeps_its_assignment_and_offers_not_connected() {
        // A UUID that no connected display answers to, as after unplugging the third monitor.
        let choices = choices(&fixture_pair());
        let saved = THIRD_FIXTURE_ID;
        assert_eq!(index_of(&choices, saved), None);
        let (labels, selected) = picker(&choices, saved);
        assert_eq!(selected, choices.len());
        assert_eq!(labels.last().map(String::as_str), Some(UNPLUGGED));

        let mut doc = Doc::parse("version = 1\n");
        doc.set_monitor(WorkspaceId::new(7).unwrap(), Some(saved));
        assert_eq!(
            effective_monitor(&doc, WorkspaceId::new(7).unwrap()),
            saved,
            "rendering keeps the config.toml entry"
        );
    }

    #[test]
    fn picking_a_built_in_default_clears_the_override() {
        let choices = choices(&fixture_three());
        let seven = WorkspaceId::new(7).unwrap();
        assert_eq!(choose(&choices, 0, seven).as_deref(), Some(MAIN));
        assert_eq!(choose(&choices, 1, seven), None);
        let three = WorkspaceId::new(3).unwrap();
        assert_eq!(choose(&choices, 0, three), None);
        assert_eq!(choose(&choices, 1, three).as_deref(), Some(SECONDARY));
        // The extra segment is not a choice: picking it leaves the saved assignment alone.
        assert_eq!(choose(&choices, choices.len(), seven), None);
    }

    #[test]
    fn secondary_is_offered_even_when_only_one_display_is_connected() {
        let single = vec![fixture_pair()[0].clone()];
        let choices = choices(&single);
        assert_eq!(
            choices
                .iter()
                .map(|choice| choice.selector.as_str())
                .collect::<Vec<_>>(),
            [MAIN, SECONDARY]
        );
        assert!(!choices[1].connected);
        assert_eq!(choices[1].caption, "Default for 6–10 · main when unplugged");
        assert_eq!(
            picker(&choices, SECONDARY).0,
            ["Main".to_owned(), "Secondary".to_owned()]
        );
    }

    /// The `monitor` override the picker writes for `index`; `None` clears it.
    fn choose(choices: &[Choice], index: usize, workspace: WorkspaceId) -> Option<String> {
        override_for(workspace, &choices.get(index)?.selector)
    }

    /// The pane's cards for `doc`: workspace 9 assigned in every case here.
    fn cards(doc: &Doc, displays: Vec<displays::ConnectedDisplay>) -> Vec<Section> {
        let choices = choices(&displays);
        sections(
            doc,
            &choices,
            doc.workspaces_with_referenced(WorkspaceId::numbered()),
        )
    }

    fn assigned(display: &str) -> Doc {
        let mut doc = Doc::parse("version = 1\n");
        doc.set_monitor(WorkspaceId::new(9).unwrap(), Some(display));
        doc
    }

    #[test]
    fn every_connected_display_gets_its_own_card() {
        let sections = cards(&assigned(THIRD_FIXTURE_ID), fixture_three());
        let titles: Vec<&str> = sections
            .iter()
            .map(|section| section.title.as_str())
            .collect();
        assert_eq!(
            titles,
            ["Main display", "Secondary display", "Studio Display 2"]
        );
        assert_eq!(sections[0].workspaces.len(), 5);
        assert_eq!(sections[1].workspaces.len(), 4);
        assert_eq!(
            sections[2].workspaces,
            [WorkspaceId::new(9).unwrap()],
            "workspace 9 sits on the third display"
        );
        assert_eq!(sections[2].caption, "Display 3");
    }

    #[test]
    fn an_unplugged_display_keeps_its_own_card_and_assignment() {
        let doc = assigned(THIRD_FIXTURE_ID);
        let sections = cards(&doc, fixture_pair());
        let titles: Vec<&str> = sections
            .iter()
            .map(|section| section.title.as_str())
            .collect();
        assert_eq!(
            titles,
            ["Main display", "Secondary display", "Disconnected displays"]
        );
        assert_eq!(sections[2].workspaces, [WorkspaceId::new(9).unwrap()]);
        assert_eq!(sections[2].caption, "Saved in config.toml");
        assert_eq!(
            doc.monitor(WorkspaceId::new(9).unwrap()).as_deref(),
            Some(THIRD_FIXTURE_ID),
            "the assignment survives rendering"
        );
    }

    #[test]
    fn disconnected_workspace_6_to_10_names_main_as_fallback_and_restores_on_reconnect() {
        let seven = WorkspaceId::new(7).unwrap();
        let mut doc = Doc::parse("version = 1\n");
        doc.set_monitor(seven, Some(THIRD_FIXTURE_ID));

        // Third display unplugged while Main and Secondary remain connected.
        let pair_choices = choices(&fixture_pair());
        let saved = effective_monitor(&doc, seven);
        let (_, selected) = picker(&pair_choices, &saved);
        assert_eq!(selected, pair_choices.len());
        assert_eq!(
            row_description(seven, &saved, selected >= pair_choices.len()).as_deref(),
            Some("Not connected · uses Main until it returns")
        );
        assert_eq!(doc.monitor(seven).as_deref(), Some(THIRD_FIXTURE_ID));

        // Reconnecting the third display resumes showing it without losing the assignment.
        let three_choices = choices(&fixture_three());
        let (_, reconnected) = picker(&three_choices, &saved);
        assert_eq!(reconnected, 2);
        assert_eq!(
            row_description(seven, &saved, reconnected >= three_choices.len()).as_deref(),
            Some("Default: Secondary")
        );

        // An ordinary workspace 7 on Secondary still shows Secondary with no override note.
        let default_doc = Doc::parse("version = 1\n");
        let default_effective = effective_monitor(&default_doc, seven);
        let (_, default_selected) = picker(&pair_choices, &default_effective);
        assert_eq!(default_selected, 1);
        assert_eq!(
            row_description(
                seven,
                &default_effective,
                default_selected >= pair_choices.len()
            ),
            None
        );
    }

    #[test]
    fn every_connected_display_has_a_card_by_default() {
        let doc = Doc::parse("version = 1\n");
        let titles = |displays| -> Vec<String> {
            cards(&doc, displays).into_iter().map(|s| s.title).collect()
        };
        let labels: Vec<String> = choices(&fixture_quad())
            .into_iter()
            .map(|c| c.label)
            .collect();
        assert_eq!(
            titles(fixture_three()),
            ["Main display", "Secondary display", "Studio Display 2"]
        );
        assert_eq!(
            titles(fixture_quad()),
            [
                "Main display",
                "Secondary display",
                "Studio Display 2",
                "LG UltraFine 5K"
            ]
        );
        assert_eq!(labels, ["Main", "Secondary", "Display 3", "Display 4"]);

        let three_sections = cards(&doc, fixture_three());
        assert!(three_sections[2].workspaces.is_empty());
        assert_eq!(three_sections[2].caption, "Display 3");

        // Assigning workspace 9 to the third display and moving it back leaves the card in place.
        let nine = WorkspaceId::new(9).unwrap();
        let mut moved = Doc::parse("version = 1\n");
        moved.set_monitor(nine, Some(THIRD_FIXTURE_ID));
        assert_eq!(cards(&moved, fixture_three())[2].workspaces, [nine]);
        moved.set_monitor(nine, None);
        let after_move = cards(&moved, fixture_three());
        assert_eq!(
            after_move
                .iter()
                .map(|s| s.title.as_str())
                .collect::<Vec<_>>(),
            ["Main display", "Secondary display", "Studio Display 2"]
        );
        assert!(after_move[2].workspaces.is_empty());

        // Control: a disconnected third display with no saved workspaces gets no card.
        assert_eq!(
            titles(fixture_pair()),
            ["Main display", "Secondary display"]
        );

        // Rendering the pane with an empty connected display card succeeds cleanly.
        let mut ui = crate::playground::blank_fixture();
        ui.doc = doc;
        ui.displays = fixture_three();
        let p = crate::controls::Paint::new(true, 2.0);
        let _ = pane(&ui, p);
    }
}
