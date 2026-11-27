//! #423: renaming a workspace's display selector to the same display keeps it in front.
//! Reproduction contributed by @coygeek.

use super::super::*;

fn execute(
    rt: &mut WorkspaceRuntime<FakeBackend>,
    action: WorkerAction,
    outputs: &macos::DisplayOutputs,
) -> Result<serde_json::Value, String> {
    let Some(WorkerResult::Status { result, debug }) = rt.execute(
        WorkerCommand::Action(action, outputs.clone()),
        &AtomicBool::new(false),
    ) else {
        panic!("missing worker status");
    };
    assert_eq!(result?.failed, 0);
    Ok(serde_json::to_value(debug).unwrap())
}

fn portrait_displays() -> macos::DisplayOutputs {
    let mut displays = three_outputs().displays().to_vec();
    for (display, (x, y, width, height)) in displays.iter_mut().zip([
        (0, 30, 2560, 1410),
        (-1440, -583, 1440, 2530),
        (2560, -539, 1440, 2530),
    ]) {
        display.frame = crate::Rect {
            x,
            y,
            width,
            height,
        };
    }
    let bounds = displays
        .iter()
        .map(|d| crate::Rect {
            y: d.frame.y - 30,
            height: d.frame.height + 30,
            ..d.frame
        })
        .collect();
    macos::DisplayOutputs::with_bounds(displays, bounds).unwrap()
}

fn configured_rig(
    config: Config,
    displays: &macos::DisplayOutputs,
    tiles: &[(u8, u32, usize)],
) -> WorkspaceRuntime<FakeBackend> {
    let mut applications = Vec::new();
    let mut backend = FakeBackend::default();
    for &(_, id, display) in tiles {
        let window = window(id);
        let output = displays.displays()[display].frame;
        let frame = crate::Rect {
            x: output.x + 50,
            y: output.y + 50,
            width: 400,
            height: 300,
        };
        backend.frames.insert(window.clone(), frame);
    }
    if !tiles.is_empty() {
        let app = crate::ObservedApplication {
            id: window(tiles[0].1).app,
            bundle_id: "com.example.Test".into(),
        };
        applications.push(crate::AppObservation::Complete {
            app,
            focused: Some(window(tiles[0].1)),
            windows: tiles
                .iter()
                .map(|&(_, id, _)| crate::ObservedWindow {
                    id: window(id),
                    frame: backend.frames[&window(id)],
                    title: None,
                    document: None,
                    resizable: true,
                })
                .collect(),
        });
        backend.focused = Some(window(tiles[0].1));
        backend.frontmost = Some(window(tiles[0].1).app);
    }
    backend.observation = Some(crate::NativeObservation {
        cycle: 0,
        applications,
        mouse_down: false,
    });
    let mut rt = runtime_with_config(backend, config);
    for &(n, id, _) in tiles {
        bind(&mut rt, WorkspaceId::new(n).unwrap(), &window(id));
    }
    execute(&mut rt, WorkerAction::Retile, displays).unwrap();
    rt
}

fn ws(n: u8) -> WorkspaceId {
    WorkspaceId::new(n).unwrap()
}

fn alias_config(monitor: &str, layout: &str, inner: u32) -> Config {
    let mut source = format!(
        "version = 1\n[settings]\nlayout = '{layout}'\nmouse_follows_focus = 'display'\n[settings.gaps]\ninner = {inner}\n"
    );
    for (n, selector) in [
        (4, "secondary"),
        (5, monitor),
        (6, "secondary-id"),
        (7, "third-id"),
        (8, "third-id"),
        (9, "third-id"),
        (10, "third-id"),
    ] {
        source.push_str(&format!(
            "\n[[workspaces]]\nworkspace = {n}\nmonitor = '{selector}'\n"
        ));
    }
    Config::parse(&source).unwrap()
}

#[test]
fn config_research_alias() {
    let displays = portrait_displays();
    for layout in ["columns", "dwindle"] {
        for (monitor, inner) in [("secondary", 8), ("secondary-id", 0)] {
            let mut rt = configured_rig(
                alias_config("secondary", layout, 0),
                &displays,
                &[(1, 10, 0), (4, 40, 1), (5, 50, 1), (6, 60, 1), (9, 90, 2)],
            );
            for n in [9, 6, 5, 1] {
                execute(&mut rt, WorkerAction::SwitchWorkspace(ws(n)), &displays).unwrap();
            }
            assert!(rt.is_visible(ws(5), &displays));
            assert_eq!(
                displays.resolve(&DisplaySelector::Secondary).id,
                displays.resolve(&DisplaySelector::parse(monitor)).id,
            );
            rt.replace_config(alias_config(monitor, layout, inner));
            let debug = execute(&mut rt, WorkerAction::Retile, &displays).unwrap();
            assert_eq!(
                debug["active_workspaces"],
                serde_json::json!([1, 5, 9]),
                "{layout} monitor={monitor}"
            );
            assert!(!rt.parked.contains_key(&window(50)), "{layout} {monitor}");
            // Workspaces 4 and 6 on that display still switch in place.
            for n in [4, 6] {
                let shown =
                    execute(&mut rt, WorkerAction::SwitchWorkspace(ws(n)), &displays).unwrap();
                assert_eq!(shown["active_workspaces"], serde_json::json!([1, n, 9]));
            }
        }
    }
}

/// #544: after a same-display selector edit, moving the workspace away reveals
/// the one its display showed just before it, not an older one the new
/// selector had covered. Reported by @coygeek.
#[test]
fn move_after_alias_edit_reveals_the_workspace_shown_before() {
    let displays = portrait_displays();
    for target in [
        DisplayTarget::Next,
        DisplayTarget::Selector(DisplaySelector::parse("third-id")),
        DisplayTarget::Selector(DisplaySelector::parse("Projector")),
    ] {
        let mut rt = configured_rig(
            alias_config("secondary", "dwindle", 0),
            &displays,
            &[(2, 20, 0), (4, 40, 1), (5, 50, 1), (6, 60, 1), (7, 70, 2)],
        );
        for n in [6, 4, 5] {
            execute(&mut rt, WorkerAction::SwitchWorkspace(ws(n)), &displays).unwrap();
        }
        rt.replace_config(alias_config("secondary-id", "dwindle", 0));
        execute(&mut rt, WorkerAction::Retile, &displays).unwrap();
        assert!(rt.is_visible(ws(5), &displays), "{target:?}");
        rt.backend.focused = Some(window(50));

        execute(
            &mut rt,
            WorkerAction::MoveWorkspaceToDisplay(target.clone()),
            &displays,
        )
        .unwrap();
        assert!(rt.is_visible(ws(5), &displays), "{target:?}");
        assert!(rt.is_visible(ws(4), &displays), "{target:?}");
        assert!(!rt.is_visible(ws(6), &displays), "{target:?}");
        assert!(!rt.parked.contains_key(&window(40)), "{target:?}");
        assert!(rt.parked.contains_key(&window(60)), "{target:?}");
    }
}

/// #542: the edited workspace is hidden behind another selector's workspace
/// on the same display; it stays hidden and the shown one stays in front.
#[test]
fn hidden_workspace_selector_edit_keeps_the_shown_one_in_front() {
    let displays = portrait_displays();
    // (switches, edited workspace, new selector, shown workspace)
    for (switches, edited, selector, shown) in [
        ([6, 5, 7], 6, "secondary", 5),
        ([5, 6, 7], 5, "secondary-id", 6),
    ] {
        let mut rt = configured_rig(
            alias_config("secondary", "dwindle", 0),
            &displays,
            &[(1, 10, 0), (5, 50, 1), (6, 60, 1), (7, 70, 2)],
        );
        for n in switches {
            execute(&mut rt, WorkerAction::SwitchWorkspace(ws(n)), &displays).unwrap();
        }
        assert!(rt.is_visible(ws(shown), &displays));
        let mut config = alias_config("secondary", "dwindle", 0);
        config.set_workspace_monitor(ws(edited), DisplaySelector::parse(selector));
        rt.replace_config(config);
        let debug = execute(&mut rt, WorkerAction::Retile, &displays).unwrap();
        assert_eq!(
            debug["active_workspaces"],
            serde_json::json!([1, shown, 7]),
            "editing {edited} to {selector}"
        );
        let shown_window = window(u32::from(shown) * 10);
        let edited_window = window(u32::from(edited) * 10);
        assert!(
            !rt.parked.contains_key(&shown_window),
            "{edited} {selector}"
        );
        assert!(
            rt.parked.contains_key(&edited_window),
            "{edited} {selector}"
        );
        // An explicit switch still brings the edited workspace forward.
        let debug = execute(
            &mut rt,
            WorkerAction::SwitchWorkspace(ws(edited)),
            &displays,
        )
        .unwrap();
        assert_eq!(
            debug["active_workspaces"],
            serde_json::json!([1, edited, 7])
        );
    }
}

/// #542 review: a display that showed `secondary` while the left display was
/// unplugged holds a newer record on that lane. After a reconnect the left
/// display still shows 6 through its UUID, so renaming 6's selector to
/// `secondary` keeps 6 in front (#423).
#[test]
fn shown_workspace_selector_edit_ignores_another_displays_newer_record() {
    let displays = portrait_displays();
    let all = displays.displays();
    let unplugged = macos::DisplayOutputs::new(vec![all[0].clone(), all[2].clone()]).unwrap();
    let mut rt = configured_rig(alias_config("secondary", "dwindle", 0), &displays, &[]);
    for n in [9, 6] {
        execute(&mut rt, WorkerAction::SwitchWorkspace(ws(n)), &displays).unwrap();
    }
    // With the left display gone, 6 (newer than 1) promotes onto main while
    // secondary names the right display, which shows 4.
    let debug = execute(&mut rt, WorkerAction::SwitchWorkspace(ws(4)), &unplugged).unwrap();
    assert_eq!(debug["active_workspaces"], serde_json::json!([6, 4]));
    let debug = execute(&mut rt, WorkerAction::Retile, &displays).unwrap();
    assert_eq!(debug["active_workspaces"], serde_json::json!([1, 6, 9]));
    let mut config = alias_config("secondary", "dwindle", 0);
    config.set_workspace_monitor(ws(6), DisplaySelector::Secondary);
    rt.replace_config(config);
    let debug = execute(&mut rt, WorkerAction::Retile, &displays).unwrap();
    assert_eq!(debug["active_workspaces"], serde_json::json!([1, 6, 9]));
}
