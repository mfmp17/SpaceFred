//! #417: move-tab --follow moves the pointer onto the detached tab's display.
//! Reproduction contributed by @coygeek.

use super::super::*;

const CENTER: crate::Rect = crate::Rect {
    x: 0,
    y: 30,
    width: 2560,
    height: 1410,
};
const LEFT: crate::Rect = crate::Rect {
    x: -1440,
    y: -583,
    width: 1440,
    height: 2530,
};
const RIGHT: crate::Rect = crate::Rect {
    x: 2560,
    y: -539,
    width: 1440,
    height: 2530,
};

fn monitors() -> macos::DisplayOutputs {
    let rows = [
        ("center", CENTER, true),
        ("left", LEFT, false),
        ("right", RIGHT, false),
    ];
    macos::DisplayOutputs::with_bounds(
        rows.iter()
            .map(|(id, frame, is_main)| macos::DisplayOutput {
                id: (*id).into(),
                name: (*id).into(),
                frame: *frame,
                is_main: *is_main,
            })
            .collect(),
        rows.into_iter()
            .map(|(_, frame, _)| crate::Rect {
                y: frame.y - 30,
                height: frame.height + 30,
                ..frame
            })
            .collect(),
    )
    .unwrap()
}

fn ws(number: u8) -> WorkspaceId {
    WorkspaceId::new(number).unwrap()
}

fn id(number: u32) -> crate::PhysicalWindowId {
    crate::PhysicalWindowId {
        app: AppInstanceId {
            pid: number as i32,
            launched_at: 1,
        },
        window_id: number,
    }
}

fn inset(output: crate::Rect) -> crate::Rect {
    crate::Rect {
        x: output.x + 100,
        y: output.y + 100,
        width: 400,
        height: 300,
    }
}

fn open(
    rt: &mut WorkspaceRuntime<FakeBackend>,
    window: &crate::PhysicalWindowId,
    frame: crate::Rect,
) {
    rt.backend.frames.insert(window.clone(), frame);
    rt.backend
        .observation
        .as_mut()
        .unwrap()
        .applications
        .extend(
            observation_for(
                &format!("com.example.Fixture{}", window.app.pid),
                window,
                frame,
            )
            .applications,
        );
}

fn act(rt: &mut WorkspaceRuntime<FakeBackend>, action: WorkerAction) {
    let Some(WorkerResult::Status { result, .. }) = rt.execute(
        WorkerCommand::Action(action, monitors()),
        &AtomicBool::new(false),
    ) else {
        panic!("expected command status");
    };
    assert_eq!(result.unwrap().failed, 0);
}

fn fixture(rules: &str, windows: &[(u32, crate::Rect)]) -> WorkspaceRuntime<FakeBackend> {
    let config = Config::parse(&format!(
        "version = 1\n[settings]\nlayout = 'columns'\nmouse_follows_focus = 'display'\n[labs]\nscratchpad = true\n[[workspaces]]\nworkspace = 9\nmonitor = 'right'\n[[workspaces]]\nworkspace = 10\nmonitor = 'right'\n{rules}"
    )).unwrap();
    let mut rt = runtime_with_config(
        FakeBackend {
            observation: Some(crate::NativeObservation {
                cycle: 0,
                mouse_down: false,
                applications: Vec::new(),
            }),
            ..FakeBackend::default()
        },
        config,
    );
    for (number, output) in windows {
        open(&mut rt, &id(*number), inset(*output));
    }
    act(&mut rt, WorkerAction::Retile);
    rt
}

fn focus(rt: &mut WorkspaceRuntime<FakeBackend>, window: &crate::PhysicalWindowId) {
    rt.backend.focused = Some(window.clone());
    rt.backend.frontmost = Some(window.app.clone());
}

#[test]
fn move_tab_follow_pointer_reaches_target_display() {
    for detached in [false, true] {
        let mut rt = fixture("", &[(11, CENTER), (61, LEFT), (91, RIGHT)]);
        let source = id(11);
        let tab = crate::PhysicalWindowId {
            app: source.app.clone(),
            window_id: 100,
        };
        focus(&mut rt, &source);
        rt.backend.pointer = Some((200, 200));
        if detached {
            rt.backend.tab_detach = Some(FakeTabDetach::NewWindow(tab.clone(), inset(CENTER)));
        }
        act(
            &mut rt,
            WorkerAction::MoveTab {
                target: ws(9),
                follow: true,
            },
        );
        let moved = if detached { tab } else { source };
        assert_eq!(rt.backend.focused.as_ref(), Some(&moved));
        assert!(RIGHT.contains_center(rt.backend.frames[&moved]));
        let (x, y) = rt.backend.pointer.unwrap();
        println!("detached={detached} pointer=({x}, {y})");
        assert!(RIGHT.contains(x, y));
    }
}
