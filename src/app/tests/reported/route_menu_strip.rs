//! #498: A new window matching a `route` rule stayed unowned when its center was in the display's
//! menu bar strip, while the same window a little lower routed to its hidden workspace.
//! Reported by @coygeek.

use super::super::*;

fn rect(x: i32, y: i32, width: i32, height: i32) -> crate::Rect {
    crate::Rect {
        x,
        y,
        width,
        height,
    }
}

fn display() -> macos::DisplayOutputs {
    macos::DisplayOutputs::with_bounds(
        vec![
            macos::DisplayOutput {
                id: "main-id".into(),
                name: "Main".into(),
                frame: rect(0, 30, 1000, 770),
                is_main: true,
            },
            macos::DisplayOutput {
                id: "secondary-id".into(),
                name: "Secondary".into(),
                frame: rect(1000, 0, 1000, 800),
                is_main: false,
            },
        ],
        vec![rect(0, 0, 1000, 800), rect(1000, 0, 1000, 800)],
    )
    .unwrap()
}

const RULE: &str =
    "version = 1\n[[rules]]\nbundle_id = 'com.example.Test'\naction = 'route'\nworkspace = 2";

#[test]
fn route_rule_owns_a_new_window_centered_in_the_menu_strip() {
    let target = WorkspaceId::new(2).unwrap();
    // Centers 8 points above and below the usable top.
    for y in [-10, 6] {
        let created = window(10);
        let frame = rect(200, y, 420, 64);
        let outputs = display();
        assert!(outputs.screens()[0].contains_center(frame));
        let backend = FakeBackend {
            observation: Some(observation(&created, frame)),
            frames: [(created.clone(), frame)].into(),
            ..FakeBackend::default()
        };
        let config = Config::parse(RULE).unwrap();
        let mut runtime = runtime_with_config(backend, config);

        runtime.cycle(outputs, &AtomicBool::new(false)).unwrap();

        assert_eq!(
            runtime.workspaces.workspace_of(&created),
            Some(target),
            "y={y}"
        );
        assert!(runtime.parked.contains_key(&created), "y={y}");
        assert!(
            runtime
                .workspaces
                .engine(WorkspaceId::ONE)
                .bound_windows()
                .is_empty(),
            "y={y}"
        );
    }
}

/// #586: the reporter's layout, a landscape main display between two portraits whose physical
/// tops sit above it. The left portrait's usable top is the highest of any display, so a window
/// centered in its menu bar strip lay above every usable frame and was never observed.
#[test]
fn route_rule_owns_a_new_window_centered_in_the_topmost_menu_strip() {
    let outputs = macos::DisplayOutputs::with_bounds(
        vec![
            macos::DisplayOutput {
                id: "main-id".into(),
                name: "Main".into(),
                frame: rect(0, 30, 2560, 1410),
                is_main: true,
            },
            macos::DisplayOutput {
                id: "left-id".into(),
                name: "Left".into(),
                frame: rect(-1440, -583, 1440, 2530),
                is_main: false,
            },
            macos::DisplayOutput {
                id: "right-id".into(),
                name: "Right".into(),
                frame: rect(2560, -539, 1440, 2530),
                is_main: false,
            },
        ],
        vec![
            rect(0, 0, 2560, 1440),
            rect(-1440, -613, 1440, 2560),
            rect(2560, -569, 1440, 2560),
        ],
    )
    .unwrap();
    let target = WorkspaceId::new(9).unwrap();
    // Centers 8 points above and below the left portrait's usable top.
    for y in [-623, -607] {
        let created = window(10);
        let frame = rect(-930, y, 420, 64);
        let backend = FakeBackend {
            observation: Some(observation(&created, frame)),
            frames: [(created.clone(), frame)].into(),
            ..FakeBackend::default()
        };
        let config = Config::parse(
            "version = 1\n[settings]\nlayout = 'columns'\n\
             [[workspaces]]\nworkspace = 5\nmonitor = 'left-id'\n\
             [[workspaces]]\nworkspace = 8\nmonitor = 'right-id'\n\
             [[workspaces]]\nworkspace = 9\nmonitor = 'right-id'\n\
             [[rules]]\nbundle_id = 'com.example.Test'\naction = 'route'\nworkspace = 9",
        )
        .unwrap();
        let mut runtime = runtime_with_config(backend, config);

        runtime
            .cycle(outputs.clone(), &AtomicBool::new(false))
            .unwrap();

        assert_eq!(
            runtime.workspaces.workspace_of(&created),
            Some(target),
            "y={y}"
        );
        assert!(runtime.parked.contains_key(&created), "y={y}");
    }
}
