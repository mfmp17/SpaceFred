//! #730: a new movable, non-resizable window under a `route` rule, centered in the empty space
//! between staggered displays, floated on the rule's hidden workspace and was parked for it,
//! then revealed on another display. A window centered on no attached display is unmanaged; a
//! tiled route already skipped it. Reported by @coygeek.

use super::super::*;

fn rect(x: i32, y: i32, width: i32, height: i32) -> crate::Rect {
    crate::Rect {
        x,
        y,
        width,
        height,
    }
}

/// The reporter's layout: a landscape main display between two portraits whose tops sit above it.
fn staggered() -> macos::DisplayOutputs {
    macos::DisplayOutputs::with_bounds(
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
    .unwrap()
}

/// Opens a non-resizable window at `frame` under a route to hidden workspace 6 on the left.
fn open_routed_float(
    frame: crate::Rect,
) -> (WorkspaceRuntime<FakeBackend>, crate::PhysicalWindowId) {
    let created = window(10);
    let mut observed = observation(&created, frame);
    let crate::AppObservation::Complete { windows, .. } = &mut observed.applications[0] else {
        unreachable!();
    };
    windows[0].resizable = false;
    let backend = FakeBackend {
        observation: Some(observed),
        frames: [(created.clone(), frame)].into(),
        ..FakeBackend::default()
    };
    let config = Config::parse(
        "version = 1\n\
         [[workspaces]]\nworkspace = 5\nmonitor = 'left-id'\n\
         [[workspaces]]\nworkspace = 6\nmonitor = 'left-id'\n\
         [[workspaces]]\nworkspace = 8\nmonitor = 'right-id'\n\
         [[rules]]\nbundle_id = 'com.example.Test'\naction = 'route'\nworkspace = 6",
    )
    .unwrap();
    let mut runtime = runtime_with_config(backend, config);
    runtime.cycle(staggered(), &AtomicBool::new(false)).unwrap();
    (runtime, created)
}

#[test]
fn a_routed_float_centered_between_displays_stays_unmanaged() {
    // Center (2530, -340): above Main, left of Right; its right 150 points overlap Right.
    let frame = rect(2350, -490, 360, 300);
    let screens = staggered().screens();
    assert!(!screens.iter().any(|screen| screen.contains_center(frame)));

    let (runtime, created) = open_routed_float(frame);

    assert_eq!(runtime.workspaces.floating_workspace_of(&created), None);
    assert_eq!(runtime.workspaces.workspace_of_any(&created), None);
    assert!(!runtime.parked.contains_key(&created));
    assert_eq!(runtime.backend.frames[&created], frame);
}

#[test]
fn a_routed_float_centered_on_a_display_still_parks_for_its_workspace() {
    // Center (2600, -340), on Right.
    let (runtime, created) = open_routed_float(rect(2420, -490, 360, 300));

    assert_eq!(
        runtime.workspaces.floating_workspace_of(&created),
        Some(WorkspaceId::SIX)
    );
    assert!(runtime.parked.contains_key(&created));
}
