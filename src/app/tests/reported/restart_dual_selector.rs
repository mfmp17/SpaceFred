//! #386: a restart shows the workspace that was up on a display two selectors name.
//! Reproduction contributed by @coygeek.

use super::super::*;

const LEFT: crate::Rect = crate::Rect {
    x: -1440,
    y: -583,
    width: 1440,
    height: 2530,
};
const CENTER: crate::Rect = crate::Rect {
    x: 0,
    y: 30,
    width: 2560,
    height: 1410,
};
const RIGHT: crate::Rect = crate::Rect {
    x: 2560,
    y: -539,
    width: 1440,
    height: 2530,
};

fn trio() -> macos::DisplayOutputs {
    let display = |id: &str, frame, is_main| macos::DisplayOutput {
        id: id.into(),
        name: id.into(),
        frame,
        is_main,
    };
    macos::DisplayOutputs::new(vec![
        display("center", CENTER, true),
        display("left", LEFT, false),
        display("right", RIGHT, false),
    ])
    .unwrap()
}

fn ws(n: u8) -> WorkspaceId {
    WorkspaceId::new(n).unwrap()
}

fn config(six: &str) -> Config {
    let mut text = "version = 1\n".to_owned();
    for (n, monitor) in [(4, "secondary"), (5, "secondary"), (6, six), (9, "right")] {
        text.push_str(&format!(
            "[[workspaces]]\nworkspace = {n}\nmonitor = '{monitor}'\n"
        ));
    }
    Config::parse(&text).unwrap()
}

fn shown(rt: &WorkspaceRuntime<FakeBackend>) -> Vec<String> {
    rt.visible_workspaces(&trio())
        .iter()
        .map(|r| format!("{} {}", r.display_id, r.workspace))
        .collect()
}

fn restart(six: &str) -> bool {
    let cancelled = AtomicBool::new(false);
    let backend = || FakeBackend {
        frames: [(window(60), LEFT)].into(),
        ..FakeBackend::default()
    };
    let mut rt = runtime_with_config(backend(), config(six));
    bind(&mut rt, ws(6), &window(60));
    for n in [9, 4, 6] {
        rt.switch(ws(n), trio(), &cancelled).unwrap();
    }
    let before = shown(&rt);
    let saved = serde_json::to_string(&rt.workspaces.state()).unwrap();

    let mut rt = WorkspaceRuntime::with_backend_config_state(
        ManagementScope::Both,
        backend(),
        HashMap::new(),
        config(six),
        Some(serde_json::from_str(&saved).unwrap()),
    );
    let _ = rt.execute(
        WorkerCommand::Action(WorkerAction::Retile, trio()),
        &AtomicBool::new(false),
    );
    println!(
        "workspace 6 on {six}: before {before:?}, after {:?}, window 60 parked {}",
        shown(&rt),
        rt.parked.contains_key(&window(60))
    );
    rt.is_visible(ws(6), &trio())
}

#[test]
fn restart_keeps_the_workspace_assigned_by_uuid() {
    assert!(restart("left"));
}

#[test]
fn control_restart_keeps_the_workspace_assigned_as_secondary() {
    assert!(restart("secondary"));
}
