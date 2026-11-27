use crate::{AppInstanceId, PhysicalWindowId, Rect};

use super::*;

fn window(title: Option<&str>) -> ObservedWindow {
    ObservedWindow {
        id: PhysicalWindowId {
            app: AppInstanceId {
                pid: 1,
                launched_at: 1,
            },
            window_id: 1,
        },
        frame: Rect::default(),
        resizable: true,
        title: title.map(std::sync::Arc::from),
        document: None,
    }
}

#[test]
fn ordered_rules_match_all_supplied_fields() {
    let config = Config::parse(
        r#"
version = 1

[[rules]]
bundle_id = "com.example.App"
title = "Settings"
action = "float"

[[rules]]
bundle_id = "com.example.App"
action = "route"
workspace = 7
"#,
    )
    .unwrap();

    assert_eq!(
        config.disposition("com.example.App", &window(Some("Settings"))),
        WindowDisposition::Floating
    );
    assert_eq!(
        config.disposition("com.example.App", &window(Some("Document"))),
        WindowDisposition::Route(WorkspaceId::new(7).unwrap())
    );
}

#[test]
fn tile_rule_parses_and_rejects_a_workspace() {
    let config = Config::parse(
        "version = 1\n[[rules]]\nbundle_id = 'com.apple.systempreferences'\naction = 'tile'",
    )
    .unwrap();
    assert_eq!(
        config.disposition("com.apple.systempreferences", &window(None)),
        WindowDisposition::Tiled
    );
    assert_eq!(
        config.disposition("com.example.Other", &window(None)),
        WindowDisposition::Managed
    );
    assert_eq!(
        Config::parse("version = 1\n[[rules]]\nbundle_id = 'x'\naction = 'tile'\nworkspace = 2")
            .err()
            .as_deref(),
        Some("tile rule cannot specify workspace")
    );
}

#[test]
fn built_in_float_defaults_apply_only_after_user_rules() {
    let defaults = Config::parse("version = 1").unwrap();
    assert_eq!(
        defaults.disposition("com.apple.Stickies", &window(None)),
        WindowDisposition::Floating
    );
    assert_eq!(
        defaults.disposition("com.apple.Safari", &window(None)),
        WindowDisposition::Managed
    );
    let tiled =
        Config::parse("version = 1\n[[rules]]\nbundle_id = 'com.apple.Stickies'\naction = 'tile'")
            .unwrap();
    assert_eq!(
        tiled.disposition("com.apple.Stickies", &window(None)),
        WindowDisposition::Tiled
    );
    assert_eq!(
        tiled.disposition("com.apple.calculator", &window(None)),
        WindowDisposition::Floating
    );
}

#[test]
fn pin_rule_parses_and_rejects_a_workspace() {
    let config =
        Config::parse("version = 1\n[[rules]]\nbundle_id = 'com.apple.Stickies'\naction = 'pin'")
            .unwrap();
    assert_eq!(
        config.disposition("com.apple.Stickies", &window(None)),
        WindowDisposition::Pinned
    );
    assert_eq!(
        Config::parse("version = 1\n[[rules]]\nbundle_id = 'x'\naction = 'pin'\nworkspace = 2")
            .err()
            .as_deref(),
        Some("pin rule cannot specify workspace")
    );
}

#[test]
fn route_windows_key_is_route_only_and_waits_for_its_lab() {
    let rule = "[[rules]]\nbundle_id = 'com.google.Chrome'\naction = 'route'\nworkspace = 4";
    let routes_all = |source: &str| {
        Config::parse(source)
            .unwrap()
            .routes_every_window("com.google.Chrome", &window(Some("tab")))
    };
    assert!(!routes_all(&format!("version = 1\n{rule}\n")));
    assert!(!routes_all(&format!(
        "version = 1\n{rule}\nwindows = 'all'\n"
    )));
    let lab = "version = 1\n[labs]\nroute_all_windows = true\n";
    assert!(routes_all(&format!("{lab}{rule}\nwindows = 'all'\n")));
    assert!(!routes_all(&format!("{lab}{rule}\nwindows = 'first'\n")));
    assert!(!routes_all(&format!("{lab}{rule}\n")));
    let float = "version = 1\n[[rules]]\nbundle_id = 'a'\naction = 'float'\nwindows = 'all'";
    let error = Config::parse(float).unwrap_err();
    assert_eq!(error, "float rule cannot specify windows");
    assert!(Config::parse(&format!("version = 1\n{rule}\nwindows = 'some'\n")).is_err());
}

#[test]
fn title_contains_matches_case_insensitively_in_memory() {
    let config = Config::parse(
        r#"
version = 1

[[rules]]
bundle_id = "com.apple.Safari"
title_contains = "WORK —"
action = "route"
workspace = 2

[[rules]]
title_contains = "Claude"
action = "route"
workspace = 3
"#,
    )
    .unwrap();
    let two = WorkspaceId::TWO;
    let three = WorkspaceId::new(3).unwrap();
    assert_eq!(
        config.disposition("com.apple.Safari", &window(Some("Work — Pull Request #42"))),
        WindowDisposition::Route(two)
    );
    assert_eq!(
        config.disposition("com.apple.Safari", &window(Some("Fix layout - claude"))),
        WindowDisposition::Route(three)
    );
    assert_eq!(
        config.disposition("com.apple.Safari", &window(Some("Personal"))),
        WindowDisposition::Managed
    );
    assert_eq!(
        config.disposition("com.apple.Safari", &window(None)),
        WindowDisposition::Managed
    );
    // A title filter covers only some windows: it is not an app-wide route.
    assert!(!config.has_bundle_route("com.apple.Safari"));
    assert!(config.routes_every_window("com.apple.Safari", &window(Some("work — notes"))));
    assert_eq!(
        Config::parse("version = 1\n[[rules]]\ntitle_contains = ''\naction = 'float'")
            .err()
            .as_deref(),
        Some("title_contains cannot be empty")
    );
}

#[test]
fn rejects_invalid_rules_and_workspaces() {
    assert!(Config::parse("version = 2").is_err());
    assert!(Config::parse("version = 1\n[[rules]]\naction = 'float'").is_err());
    assert!(
        Config::parse("version = 1\n[[rules]]\nbundle_id = 'x'\naction = 'route'\nworkspace = 11")
            .is_err()
    );
    assert!(
        Config::parse(
            "version = 1\n[[rules]]\nbundle_id = 'x'\naction = 'route'\nworkspace = 'AA'"
        )
        .is_err()
    );
}

#[test]
fn workspace_order_accepts_numbers_and_letters_in_the_given_order() {
    let config = Config::parse(
        "version = 1\n[settings]\nworkspace_order = [1, \"Q\", \"w\", \"10\", \"E\"]",
    )
    .unwrap();
    assert_eq!(
        config.workspace_order(),
        [
            WorkspaceId::ONE,
            WorkspaceId::named('Q').unwrap(),
            WorkspaceId::named('W').unwrap(),
            WorkspaceId::new(10).unwrap(),
            WorkspaceId::named('E').unwrap(),
        ]
    );
    assert!(
        Config::parse("version = 1")
            .unwrap()
            .workspace_order()
            .is_empty()
    );
}

#[test]
fn workspace_order_rejects_duplicate_and_unknown_workspaces() {
    let duplicate =
        Config::parse("version = 1\n[settings]\nworkspace_order = [\"Q\", \"W\", \"q\"]")
            .unwrap_err();
    assert!(
        duplicate.contains("workspace Q appears more than once in settings.workspace_order"),
        "{duplicate}"
    );
    let numbered =
        Config::parse("version = 1\n[settings]\nworkspace_order = [3, \"3\"]").unwrap_err();
    assert!(
        numbered.contains("workspace 3 appears more than once"),
        "{numbered}"
    );
    let unknown = Config::parse("version = 1\n[settings]\nworkspace_order = [\"AA\"]").unwrap_err();
    assert!(
        unknown.contains("workspace must be 1 through 10 or A through Z"),
        "{unknown}"
    );
}

#[test]
fn accepts_named_workspace_routes_and_assignments() {
    let config = Config::parse(
        r#"
version = 1
[[rules]]
bundle_id = "ai.opencode.desktop"
action = "route"
workspace = "D"
[[workspaces]]
workspace = "D"
monitor = "main"
"#,
    )
    .unwrap();
    assert_eq!(
        config.disposition("ai.opencode.desktop", &window(None)),
        WindowDisposition::Route(WorkspaceId::named('D').unwrap())
    );
    assert_eq!(
        config.named_workspaces().collect::<Vec<_>>(),
        vec![
            WorkspaceId::named('D').unwrap(),
            WorkspaceId::named('D').unwrap()
        ]
    );
}

#[test]
fn rejects_obsolete_keyboard_mapping_override() {
    assert!(Config::parse("version = 1\nkey_mapping = 'dvorak'").is_err());
}

#[test]
fn trackpad_swipe_turns_off_only_with_its_lab_on() {
    let lab = "[labs]\nswipe_switch = true\n";
    let off = "[settings]\ntrackpad_swipe = false\n";
    assert!(Config::default().trackpad_swipe());
    assert!(Config::parse("version = 1").unwrap().trackpad_swipe());
    // Without the Lab, the setting is ignored and swipes behave as before.
    assert!(
        Config::parse(&format!("version = 1\n{off}"))
            .unwrap()
            .trackpad_swipe()
    );
    assert!(
        Config::parse(&format!("version = 1\n{lab}"))
            .unwrap()
            .trackpad_swipe()
    );
    assert!(
        !Config::parse(&format!("version = 1\n{off}{lab}"))
            .unwrap()
            .trackpad_swipe()
    );
    assert!(Config::parse("version = 1\n[settings]\ntrackpad_swipe = \"off\"\n").is_err());
}

#[test]
fn swipe_switch_lab_parses_and_round_trips_its_key() {
    assert_eq!(
        LabFeature::parse("swipe_switch"),
        Some(LabFeature::SwipeSwitch)
    );
    assert_eq!(LabFeature::SwipeSwitch.key(), "swipe_switch");
    for feature in LabFeature::ALL {
        assert_eq!(LabFeature::parse(feature.key()), Some(feature));
    }
    assert!(!Config::default().labs().enabled(LabFeature::SwipeSwitch));
    let on = Config::parse("version = 1\n[labs]\nswipe_switch = true\n").unwrap();
    assert!(on.labs().enabled(LabFeature::SwipeSwitch));
    assert!(!on.labs().enabled(LabFeature::Scratchpad));
    assert!(Config::parse("version = 1\n[labs]\nswipe_switch = 1\n").is_err());
}

#[test]
fn parses_hud_setting_with_enabled_default() {
    assert!(Config::default().hud());
    assert!(Config::parse("version = 1").unwrap().hud());
    assert!(Config::parse("version = 1\n[settings]\n").unwrap().hud());
    assert!(
        !Config::parse("version = 1\n[settings]\nhud = false\n")
            .unwrap()
            .hud()
    );
    assert!(Config::parse("version = 1\n[settings]\nunknown = true\n").is_err());
}

#[test]
fn parses_hud_style_with_icons_default() {
    assert_eq!(Config::default().hud_style(), HudStyle::Icons);
    assert_eq!(
        Config::parse("version = 1").unwrap().hud_style(),
        HudStyle::Icons
    );
    assert_eq!(
        Config::parse("version = 1\n[settings]\nhud = false\n")
            .unwrap()
            .hud_style(),
        HudStyle::Icons
    );
    for style in [HudStyle::Layouts, HudStyle::Icons] {
        let source = format!(
            "version = 1\n[settings]\nhud_style = {:?}\n",
            style.as_str()
        );
        assert_eq!(Config::parse(&source).unwrap().hud_style(), style);
        assert_eq!(HudStyle::parse(style.as_str()), Some(style));
    }
    assert!(Config::parse("version = 1\n[settings]\nhud_style = \"grid\"\n").is_err());
    assert!(Config::parse("version = 1\n[settings]\nhud_style = true\n").is_err());
    assert_eq!(HudStyle::parse("Icons"), None);
}

#[test]
fn parses_hud_position_with_bottom_default() {
    assert_eq!(Config::default().hud_position(), HudPosition::Bottom);
    assert_eq!(
        Config::parse("version = 1").unwrap().hud_position(),
        HudPosition::Bottom
    );
    assert_eq!(
        Config::parse("version = 1\n[settings]\nhud = false\n")
            .unwrap()
            .hud_position(),
        HudPosition::Bottom
    );
    for position in [HudPosition::Bottom, HudPosition::Top] {
        let source = format!(
            "version = 1\n[settings]\nhud_position = {:?}\n",
            position.as_str()
        );
        assert_eq!(Config::parse(&source).unwrap().hud_position(), position);
        assert_eq!(HudPosition::parse(position.as_str()), Some(position));
    }
    assert!(Config::parse("version = 1\n[settings]\nhud_position = \"center\"\n").is_err());
    assert!(Config::parse("version = 1\n[settings]\nhud_position = true\n").is_err());
    assert_eq!(HudPosition::parse("Top"), None);
}

#[test]
fn saving_a_workspace_monitor_keeps_hud_position() {
    let dir = std::env::temp_dir().join(format!(
        "spaceman-config-hud-position-{}-{}",
        std::process::id(),
        now().as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("config.toml");
    std::fs::write(&path, "version = 1\n[settings]\nhud_position = \"top\"\n").unwrap();
    let saved =
        save_workspace_monitor_at(&path, WorkspaceId::TWO, &DisplaySelector::Secondary).unwrap();
    assert_eq!(saved.hud_position(), HudPosition::Top);
    assert!(
        std::fs::read_to_string(&path)
            .unwrap()
            .contains("hud_position = \"top\"")
    );
    drop(std::fs::remove_dir_all(dir));
}

#[test]
fn parses_layout_with_dwindle_default() {
    assert_eq!(Config::default().layout(), LayoutMode::Dwindle);
    assert_eq!(
        Config::parse("version = 1").unwrap().layout(),
        LayoutMode::Dwindle
    );
    assert_eq!(
        Config::parse("version = 1\n[settings]\nhud = false\n")
            .unwrap()
            .layout(),
        LayoutMode::Dwindle
    );
    for mode in [LayoutMode::Dwindle, LayoutMode::Columns] {
        let source = format!("version = 1\n[settings]\nlayout = {:?}\n", mode.as_str());
        assert_eq!(Config::parse(&source).unwrap().layout(), mode);
        assert_eq!(LayoutMode::parse(mode.as_str()), Some(mode));
    }
    assert!(Config::parse("version = 1\n[settings]\nlayout = \"bsp\"\n").is_err());
    assert!(Config::parse("version = 1\n[settings]\nlayout = true\n").is_err());
    assert_eq!(LayoutMode::parse("Dwindle"), None);
}

#[test]
fn saving_a_workspace_monitor_keeps_layout() {
    let dir = std::env::temp_dir().join(format!(
        "spaceman-config-layout-{}-{}",
        std::process::id(),
        now().as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("config.toml");
    std::fs::write(&path, "version = 1\n[settings]\nlayout = \"columns\"\n").unwrap();
    let saved =
        save_workspace_monitor_at(&path, WorkspaceId::TWO, &DisplaySelector::Secondary).unwrap();
    assert_eq!(saved.layout(), LayoutMode::Columns);
    assert!(
        std::fs::read_to_string(&path)
            .unwrap()
            .contains("layout = \"columns\"")
    );
    drop(std::fs::remove_dir_all(dir));
}

#[test]
fn parses_labs_with_workspace_steps_off_by_default() {
    let config = Config::parse("version = 1").unwrap();
    assert!(!config.labs().workspace_steps);
    assert!(!config.shortcuts().workspace_steps);
    let config = Config::parse("version = 1\n[labs]\nworkspace_steps = true\n").unwrap();
    assert!(config.labs().enabled(LabFeature::WorkspaceSteps));
    assert!(config.shortcuts().workspace_steps);
}

#[test]
fn parses_labs_with_the_scratchpad_off_by_default() {
    for source in ["version = 1", "version = 1\n[labs]\n"] {
        let config = Config::parse(source).unwrap();
        assert!(!config.labs().scratchpad);
        assert!(!config.shortcuts().scratchpad);
    }
    assert!(!Config::default().labs().scratchpad);
    let config = Config::parse("version = 1\n[labs]\nscratchpad = true\n").unwrap();
    assert!(config.labs().scratchpad);
    assert!(
        config.shortcuts().scratchpad,
        "the Lab's shortcuts follow its switch"
    );
    assert!(Config::parse("version = 1\n[labs]\nwarp_drive = true\n").is_err());
    assert!(Config::parse("version = 1\n[labs]\nscratchpad = 1\n").is_err());
}

#[test]
fn parses_labs_with_recent_focus_off_by_default() {
    assert!(!Config::default().labs().recent_focus);
    assert!(
        !Config::parse("version = 1\n[labs]\n")
            .unwrap()
            .labs()
            .recent_focus
    );
    let config = Config::parse("version = 1\n[labs]\nrecent_focus = true\n").unwrap();
    assert!(config.labs().enabled(LabFeature::RecentFocus));
    assert_eq!(
        LabFeature::parse("recent_focus"),
        Some(LabFeature::RecentFocus)
    );
    assert!(!config.labs().enabled(LabFeature::Scratchpad));
}

#[test]
fn parses_labs_with_activity_off_by_default() {
    for source in ["version = 1", "version = 1\n[labs]\n"] {
        let config = Config::parse(source).unwrap();
        assert!(!config.labs().activity);
        assert!(!config.labs().enabled(LabFeature::Activity));
    }
    assert!(!Config::default().labs().activity);
    let config = Config::parse("version = 1\n[labs]\nactivity = true\n").unwrap();
    assert!(config.labs().activity);
    assert!(config.labs().enabled(LabFeature::Activity));
    assert!(
        !config.labs().enabled(LabFeature::Scratchpad),
        "each switch stands alone"
    );
    // Both switches share the table.
    let both = Config::parse("version = 1\n[labs]\nscratchpad = true\nactivity = true\n").unwrap();
    assert!(both.labs().enabled(LabFeature::Scratchpad));
    assert!(both.labs().enabled(LabFeature::Activity));
    assert!(!both.labs().enabled(LabFeature::PortraitEdgeRows));
    let rows = Config::parse("version = 1\n[labs]\nportrait_edge_rows = true\n").unwrap();
    assert!(rows.labs().portrait_edge_rows);
}

#[test]
fn lab_features_round_trip_their_keys() {
    for feature in LabFeature::ALL {
        assert_eq!(LabFeature::parse(feature.key()), Some(feature));
    }
    assert_eq!(LabFeature::parse("activity").unwrap().key(), "activity");
    assert_eq!(LabFeature::parse("scroll"), None);
    assert_eq!(LabFeature::parse(""), None);
}

#[test]
fn saving_a_workspace_monitor_keeps_the_labs_table() {
    let directory = std::env::temp_dir().join(format!(
        "spaceman-labs-{}-{}",
        std::process::id(),
        now().as_nanos()
    ));
    std::fs::create_dir_all(&directory).unwrap();
    let path = directory.join("config.toml");
    std::fs::write(&path, "version = 1\n[labs]\nscratchpad = true\n").unwrap();

    let saved =
        save_workspace_monitor_at(&path, WorkspaceId::ONE, &DisplaySelector::Secondary).unwrap();

    assert!(saved.labs().scratchpad);
    assert!(load_path(&path).unwrap().labs().scratchpad);
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn parses_update_channel_with_stable_default() {
    assert_eq!(Config::default().update_channel(), UpdateChannel::Stable);
    assert_eq!(
        Config::parse("version = 1").unwrap().update_channel(),
        UpdateChannel::Stable
    );
    for channel in [UpdateChannel::Stable, UpdateChannel::Dev] {
        let source = format!(
            "version = 1\n[settings]\nupdate_channel = {:?}\n",
            channel.as_str()
        );
        assert_eq!(Config::parse(&source).unwrap().update_channel(), channel);
        assert_eq!(UpdateChannel::parse(channel.as_str()), Some(channel));
    }
    assert!(Config::parse("version = 1\n[settings]\nupdate_channel = \"beta\"\n").is_err());
}

#[test]
fn parses_mouse_follows_focus_with_off_default() {
    assert_eq!(
        Config::default().mouse_follows_focus(),
        MouseFollowsFocus::Off
    );
    assert_eq!(
        Config::parse("version = 1").unwrap().mouse_follows_focus(),
        MouseFollowsFocus::Off
    );
    for mode in [
        MouseFollowsFocus::Off,
        MouseFollowsFocus::Display,
        MouseFollowsFocus::Window,
    ] {
        let source = format!(
            "version = 1\n[settings]\nmouse_follows_focus = {:?}\n",
            mode.as_str()
        );
        assert_eq!(Config::parse(&source).unwrap().mouse_follows_focus(), mode);
        assert_eq!(MouseFollowsFocus::parse(mode.as_str()), Some(mode));
    }
    assert!(Config::parse("version = 1\n[settings]\nmouse_follows_focus = \"monitor\"\n").is_err());
    assert!(Config::parse("version = 1\n[settings]\nmouse_follows_focus = true\n").is_err());
    assert_eq!(MouseFollowsFocus::parse("Display"), None);
}

#[test]
fn menu_bar_icon_waits_for_its_lab() {
    assert_eq!(Config::default().menu_bar_icon(), MenuBarIcon::Full);
    for icon in [MenuBarIcon::Full, MenuBarIcon::Compact, MenuBarIcon::Off] {
        let source = format!(
            "version = 1\n[settings]\nmenu_bar_icon = {:?}\n",
            icon.as_str()
        );
        assert_eq!(
            Config::parse(&source).unwrap().menu_bar_icon(),
            MenuBarIcon::Full,
            "the setting is ignored while the Lab is off"
        );
        let lab = format!("{source}[labs]\nmenu_bar_icon_style = true\n");
        assert_eq!(Config::parse(&lab).unwrap().menu_bar_icon(), icon);
        assert_eq!(MenuBarIcon::parse(icon.as_str()), Some(icon));
    }
    assert!(Config::parse("version = 1\n[settings]\nmenu_bar_icon = \"tiny\"\n").is_err());
}

#[test]
fn saving_a_workspace_monitor_keeps_mouse_follows_focus() {
    let dir = std::env::temp_dir().join(format!(
        "spaceman-config-mff-{}-{}",
        std::process::id(),
        now().as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("config.toml");
    std::fs::write(
        &path,
        "version = 1\n[settings]\nmouse_follows_focus = \"display\"\n",
    )
    .unwrap();
    let saved =
        save_workspace_monitor_at(&path, WorkspaceId::TWO, &DisplaySelector::Secondary).unwrap();
    assert_eq!(saved.mouse_follows_focus(), MouseFollowsFocus::Display);
    drop(std::fs::remove_dir_all(dir));
}

#[test]
fn saving_a_workspace_monitor_keeps_the_workspace_order() {
    let dir = std::env::temp_dir().join(format!(
        "spaceman-config-order-{}-{}",
        std::process::id(),
        now().as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("config.toml");
    std::fs::write(
        &path,
        "version = 1\n[settings]\nworkspace_order = [1, \"Q\", \"W\"]\n",
    )
    .unwrap();
    let saved =
        save_workspace_monitor_at(&path, WorkspaceId::TWO, &DisplaySelector::Secondary).unwrap();
    assert_eq!(
        saved.workspace_order(),
        [
            WorkspaceId::ONE,
            WorkspaceId::named('Q').unwrap(),
            WorkspaceId::named('W').unwrap(),
        ]
    );
    drop(std::fs::remove_dir_all(dir));
}

#[test]
fn save_workspace_monitor_persists_overrides_and_clears_built_in_defaults() {
    let dir = std::env::temp_dir().join(format!(
        "spaceman-config-save-{}-{}",
        std::process::id(),
        now().as_nanos()
    ));
    let path = dir.join("config.toml");

    let saved =
        save_workspace_monitor_at(&path, WorkspaceId::ONE, &DisplaySelector::Secondary).unwrap();
    let mut workspaces = Workspaces::default();
    saved.configure_workspaces(&mut workspaces);
    assert_eq!(
        workspaces.assignment(WorkspaceId::ONE),
        &DisplaySelector::Secondary
    );
    let written = std::fs::read_to_string(&path).unwrap();
    assert!(written.contains("monitor = \"secondary\""), "{written}");

    let reset = save_workspace_monitor_at(&path, WorkspaceId::ONE, &DisplaySelector::Main).unwrap();
    reset.configure_workspaces(&mut workspaces);
    assert_eq!(
        workspaces.assignment(WorkspaceId::ONE),
        &DisplaySelector::Main
    );
    let written = std::fs::read_to_string(&path).unwrap();
    assert!(!written.contains("[[workspaces]]"), "{written}");

    let named = WorkspaceId::named('D').unwrap();
    let kept = save_workspace_monitor_at(&path, named, &DisplaySelector::Main).unwrap();
    assert_eq!(kept.named_workspaces().collect::<Vec<_>>(), vec![named]);
    let written = std::fs::read_to_string(&path).unwrap();
    assert!(written.contains("workspace = \"D\""), "{written}");

    drop(std::fs::remove_dir_all(dir));
}

#[test]
fn save_workspace_monitor_preserves_symlink_target_without_sidecar_files() {
    let dir = std::env::temp_dir().join(format!(
        "spaceman-config-symlink-{}-{}",
        std::process::id(),
        now().as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let target = dir.join("dotfiles.toml");
    let link = dir.join("config.toml");
    std::fs::write(&target, "version = 1\n").unwrap();
    std::os::unix::fs::symlink("dotfiles.toml", &link).unwrap();

    save_workspace_monitor_at(&link, WorkspaceId::TWO, &DisplaySelector::Secondary).unwrap();

    assert!(
        std::fs::symlink_metadata(&link)
            .unwrap()
            .file_type()
            .is_symlink()
    );
    let written = std::fs::read_to_string(&target).unwrap();
    assert!(written.contains("monitor = \"secondary\""), "{written}");
    assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 2);

    drop(std::fs::remove_dir_all(dir));
}

#[test]
fn spacing_defaults_to_none_and_parses_every_key() {
    assert!(Config::default().spacing().is_none());
    assert!(Config::parse("version = 1").unwrap().spacing().is_none());
    assert!(
        Config::parse("version = 1\n[settings]\nhud = false\n")
            .unwrap()
            .spacing()
            .is_none()
    );
    let config = Config::parse(
        r#"
version = 1
[settings]
single_window_max_width = 1600
[settings.gaps]
inner = 8
top = 32
bottom = 4
left = 0
right = 128
"#,
    )
    .unwrap();
    assert_eq!(
        config.spacing(),
        Spacing {
            inner: 8,
            top: 32,
            bottom: 4,
            left: 0,
            right: 128,
            single_window_max_width: 1600,
            single_window_max_height: 0,
        }
    );
    let height = "version = 1\n[settings]\nsingle_window_max_height = 1050\n";
    assert_eq!(
        Config::parse(height)
            .unwrap()
            .spacing()
            .single_window_max_height,
        0,
        "the height cap waits for its Lab"
    );
    let lab = format!("{height}[labs]\nsingle_window_height = true\n");
    assert_eq!(
        Config::parse(&lab)
            .unwrap()
            .spacing()
            .single_window_max_height,
        1050
    );
    let partial = Config::parse("version = 1\n[settings.gaps]\ninner = 12\n").unwrap();
    assert_eq!(partial.spacing().inner, 12);
    assert_eq!(partial.spacing().top, 0);
}

#[test]
fn spacing_rejects_out_of_range_and_unknown_keys_with_clear_messages() {
    for (source, message) in [
        (
            "[settings.gaps]\ninner = -1",
            "settings.gaps.inner must be between 0 and 48 px, got -1",
        ),
        (
            "[settings.gaps]\ninner = 49",
            "settings.gaps.inner must be between 0 and 48 px, got 49",
        ),
        (
            "[settings.gaps]\ntop = 129",
            "settings.gaps.top must be between 0 and 128 px, got 129",
        ),
        (
            "[settings.gaps]\nright = -4",
            "settings.gaps.right must be between 0 and 128 px, got -4",
        ),
        (
            "[settings]\nsingle_window_max_width = 100",
            "settings.single_window_max_width must be 0 (off) or between 320 and 16384 px, got 100",
        ),
        (
            "[settings]\nsingle_window_max_width = 99999",
            "settings.single_window_max_width must be 0 (off) or between 320 and 16384 px, got 99999",
        ),
        (
            "[settings]\nsingle_window_max_height = 200",
            "settings.single_window_max_height must be 0 (off) or between 320 and 16384 px, got 200",
        ),
    ] {
        let error = Config::parse(&format!("version = 1\n{source}\n")).unwrap_err();
        assert_eq!(error, message);
    }
    assert!(Config::parse("version = 1\n[settings.gaps]\nvertical = 4\n").is_err());
    assert!(Config::parse("version = 1\n[settings.gaps]\ninner = 1.5\n").is_err());
}

/// Top, bottom, left and right as `gaps` (the keys under `[settings.gaps]`) resolve them.
fn edges(gaps: &str) -> [i32; 4] {
    let source = format!("version = 1\n[settings.gaps]\n{gaps}\n");
    let spacing = Config::parse(&source).unwrap().spacing();
    SpacingKey::EDGES.map(|key| key.get(&spacing))
}

#[test]
fn outer_pads_only_the_edges_the_file_leaves_out() {
    assert_eq!(edges("inner = 6\nouter = 8\ntop = 0"), [0, 8, 8, 8]);
    assert_eq!(edges("outer = 8"), [8, 8, 8, 8]);
    assert_eq!(edges("outer = 128"), [128, 128, 128, 128]);
    assert_eq!(
        edges("outer = 8\ntop = 1\nbottom = 2\nleft = 3\nright = 4"),
        [1, 2, 3, 4]
    );
    assert_eq!(
        edges("outer = 8\ntop = 0\nbottom = 0\nleft = 0\nright = 0"),
        [0, 0, 0, 0]
    );
    // It pads edges only, and an unset or zero `outer` leaves them where they were.
    let gaps = "version = 1\n[settings.gaps]\nouter = 8\n";
    let spacing = Config::parse(gaps).unwrap().spacing();
    assert_eq!((spacing.inner, spacing.single_window_max_width), (0, 0));
    assert_eq!(edges("top = 32\nright = 12"), [32, 0, 0, 12]);
    assert_eq!(edges("outer = 0\ntop = 32\nright = 12"), [32, 0, 0, 12]);
}

#[test]
fn outer_takes_the_same_range_as_an_edge() {
    for (gaps, message) in [
        (
            "outer = -1",
            "settings.gaps.outer must be between 0 and 128 px, got -1",
        ),
        (
            "outer = 129",
            "settings.gaps.outer must be between 0 and 128 px, got 129",
        ),
        (
            "outer = 129\ntop = 1\nbottom = 2\nleft = 3\nright = 4",
            "settings.gaps.outer must be between 0 and 128 px, got 129",
        ),
        (
            "outer = 8\ntop = 129",
            "settings.gaps.top must be between 0 and 128 px, got 129",
        ),
    ] {
        let error = Config::parse(&format!("version = 1\n[settings.gaps]\n{gaps}\n"));
        assert_eq!(error.unwrap_err(), message);
    }
    for outer in ["8.0", "\"8\"", "true"] {
        let source = format!("version = 1\n[settings.gaps]\nouter = {outer}\n");
        assert!(Config::parse(&source).is_err(), "{source}");
    }
}

#[test]
fn saving_a_workspace_monitor_keeps_spacing() {
    let dir = std::env::temp_dir().join(format!(
        "spaceman-config-spacing-{}-{}",
        std::process::id(),
        now().as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("config.toml");
    std::fs::write(
        &path,
        "version = 1\n[settings]\nsingle_window_max_width = 1440\n[settings.gaps]\ninner = 6\nouter = 8\ntop = 0\nleft = 20\n",
    )
    .unwrap();
    let saved =
        save_workspace_monitor_at(&path, WorkspaceId::TWO, &DisplaySelector::Secondary).unwrap();
    assert_eq!(saved.spacing().inner, 6);
    assert_eq!(saved.spacing().left, 20);
    assert_eq!(saved.spacing().single_window_max_width, 1440);
    let written = std::fs::read_to_string(&path).unwrap();
    let written: toml::Table = toml::from_str(&written).unwrap();
    assert_eq!(
        serde_json::to_value(&written["settings"]["gaps"]).unwrap(),
        serde_json::json!({ "inner": 6, "outer": 8, "top": 0, "left": 20 }),
        "the shorthand stays a shorthand"
    );
    drop(std::fs::remove_dir_all(dir));
}

#[test]
fn saving_a_workspace_monitor_keeps_shortcuts() {
    let dir = std::env::temp_dir().join(format!(
        "spaceman-config-shortcuts-{}-{}",
        std::process::id(),
        now().as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("config.toml");
    std::fs::write(
        &path,
        "version = 1\n[shortcuts]\ndefaults = false\n[shortcuts.bindings]\n\"alt+left\" = \"move-direction left\"\n",
    )
    .unwrap();
    let saved =
        save_workspace_monitor_at(&path, WorkspaceId::TWO, &DisplaySelector::Secondary).unwrap();
    assert!(!saved.shortcuts().defaults);
    assert_eq!(saved.shortcuts().bindings.len(), 1);
    drop(std::fs::remove_dir_all(dir));
}

#[test]
fn shortcuts_default_to_built_ins_and_reject_bad_bindings() {
    assert_eq!(
        Config::parse("version = 1").unwrap().shortcuts(),
        &crate::shortcuts::Shortcuts::default()
    );
    for (source, message) in [
        (
            "[shortcuts.bindings]\n\"super+q\" = \"float\"",
            "unknown modifier",
        ),
        (
            "[shortcuts.bindings]\n\"alt+q\" = \"close\"",
            "unknown command",
        ),
        (
            "[shortcuts]\ndefaults = false",
            "needs at least one binding",
        ),
        ("[shortcuts]\nmode = \"replace\"", "unknown field"),
    ] {
        let error = Config::parse(&format!("version = 1\n{source}")).unwrap_err();
        assert!(error.contains(message), "{source}: {error}");
    }
}

/// A global shortcut fires in every app, so a chord without cmd, ctrl, or alt would eat
/// ordinary typing. Unbinding is the only exception, and it never registers a chord.
#[test]
fn shortcuts_never_bind_a_plain_or_shift_only_key() {
    for chord in [
        "\"h\"",
        "\"shift+h\"",
        "\"shift+left\"",
        "\"shift+/\"",
        "\"shift+1\"",
        "\"space\"",
    ] {
        let source = format!("version = 1\n[shortcuts.bindings]\n{chord} = \"float\"");
        let error = Config::parse(&source).unwrap_err();
        assert!(
            error.contains("needs at least one of alt, cmd, or ctrl"),
            "{chord}: {error}"
        );
    }
    assert!(Config::parse("version = 1\n[shortcuts.bindings]\n\"alt+h\" = \"none\"").is_ok());
}
