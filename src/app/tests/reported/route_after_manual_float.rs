//! #551: a bundle route without `windows = "all"` sent an app's second window to the rule's
//! workspace once its first, routed window was floated by hand, because only tiles counted as
//! the app's first window. Reported by @coygeek.

use super::super::*;
use crate::workspace::FloatCause;

const RULE: &str = "[[rules]]\nbundle_id = 'com.example.Test'\naction = 'route'\nworkspace = 6\n";

/// Where a second window opened on workspace 1 lands after the app's first window, routed to
/// workspace 6, was floated by hand there.
fn second_window_after_float(config: &str) -> Option<WorkspaceId> {
    let (first, second) = (window(10), window(11));
    let mut backend = FakeBackend {
        observation: Some(observation(&first, frame(50))),
        focused: Some(first.clone()),
        ..FakeBackend::default()
    };
    backend.frames.insert(first.clone(), frame(50));
    let mut runtime = runtime_with_config(backend, Config::parse(config).unwrap());
    let cancel = AtomicBool::new(false);
    runtime.cycle(outputs(), &cancel).unwrap();
    assert_eq!(
        runtime.workspaces.workspace_of(&first),
        Some(WorkspaceId::SIX)
    );

    runtime.forget_window(WorkspaceId::SIX, &first);
    runtime
        .workspaces
        .float(first.clone(), WorkspaceId::SIX, FloatCause::Manual);
    let floated = runtime.backend.frames[&first];
    runtime.backend.frames.insert(second.clone(), frame(100));
    runtime.backend.focused = Some(second.clone());
    runtime.backend.observation = Some(observation_for_windows(
        "com.example.Test",
        &second,
        &[(first.clone(), floated), (second.clone(), frame(100))],
    ));
    runtime.cycle(outputs(), &cancel).unwrap();

    assert_eq!(
        runtime.workspaces.floating_workspace_of(&first),
        Some(WorkspaceId::SIX)
    );
    runtime.workspaces.workspace_of(&second)
}

#[test]
fn a_second_window_stays_put_after_the_routed_first_window_floats_by_hand() {
    let config = format!("version = 1\n{RULE}");
    assert_eq!(second_window_after_float(&config), Some(WorkspaceId::ONE));
}

#[test]
fn a_route_all_rule_still_routes_the_second_window_after_the_first_floats() {
    let config = format!("version = 1\n[labs]\nroute_all_windows = true\n{RULE}windows = 'all'\n");
    assert_eq!(second_window_after_float(&config), Some(WorkspaceId::SIX));
}
