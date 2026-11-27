//! #664: a bundle route without `windows = "all"` sent an app's second window to the rule's
//! workspace once a float rule added while its first, routed window was open floated that
//! window, because a rule-made float did not count as the app's first window. A window that
//! floats from the start (a splash) still does not use up the route. Reported by @coygeek.

use super::super::*;

const ROUTE: &str = "[[rules]]\nbundle_id = 'com.example.Test'\naction = 'route'\nworkspace = 6\n";
const FLOAT: &str =
    "[[rules]]\nbundle_id = 'com.example.Test'\ntitle = 'Primary'\naction = 'float'\n";

fn titled(
    focused: &crate::PhysicalWindowId,
    windows: &[(crate::PhysicalWindowId, crate::Rect, &str)],
) -> crate::NativeObservation {
    let frames: Vec<_> = windows
        .iter()
        .map(|(window, frame, _)| (window.clone(), *frame))
        .collect();
    let mut observation = observation_for_windows("com.example.Test", focused, &frames);
    if let crate::AppObservation::Complete { windows: seen, .. } = &mut observation.applications[0]
    {
        for (window, (_, _, title)) in seen.iter_mut().zip(windows) {
            window.title = Some((*title).into());
        }
    }
    observation
}

/// Where a second window opened on workspace 1 lands after the app's first window, titled
/// `Primary`, met a float rule: added while it was tiled on the routed workspace 6 when
/// `late`, present from its launch otherwise.
fn second_window_after_rule_float(late: bool) -> Option<WorkspaceId> {
    let (first, second) = (window(10), window(11));
    let initial = if late {
        format!("version = 1\n{ROUTE}")
    } else {
        format!("version = 1\n{FLOAT}{ROUTE}")
    };
    let mut backend = FakeBackend {
        observation: Some(titled(&first, &[(first.clone(), frame(50), "Primary")])),
        focused: Some(first.clone()),
        ..FakeBackend::default()
    };
    backend.frames.insert(first.clone(), frame(50));
    let mut runtime = runtime_with_config(backend, Config::parse(&initial).unwrap());
    let cancel = AtomicBool::new(false);
    runtime.cycle(outputs(), &cancel).unwrap();
    if late {
        assert_eq!(
            runtime.workspaces.workspace_of(&first),
            Some(WorkspaceId::SIX)
        );
        let routed = runtime.backend.frames[&first];
        runtime.backend.observation = Some(titled(&first, &[(first.clone(), routed, "Primary")]));
        runtime.replace_config(Config::parse(&format!("version = 1\n{FLOAT}{ROUTE}")).unwrap());
        runtime.cycle(outputs(), &cancel).unwrap();
    }
    assert!(runtime.workspaces.floating_workspace_of(&first).is_some());

    let floated = runtime.backend.frames[&first];
    runtime.backend.frames.insert(second.clone(), frame(100));
    runtime.backend.focused = Some(second.clone());
    runtime.backend.observation = Some(titled(
        &second,
        &[
            (first.clone(), floated, "Primary"),
            (second.clone(), frame(100), "Secondary"),
        ],
    ));
    runtime.cycle(outputs(), &cancel).unwrap();
    runtime.workspaces.workspace_of(&second)
}

#[test]
fn a_second_window_stays_put_after_a_late_rule_floats_the_routed_first_window() {
    assert_eq!(second_window_after_rule_float(true), Some(WorkspaceId::ONE));
}

#[test]
fn a_first_window_that_floats_from_launch_leaves_the_route_to_the_second() {
    assert_eq!(
        second_window_after_rule_float(false),
        Some(WorkspaceId::SIX)
    );
}
