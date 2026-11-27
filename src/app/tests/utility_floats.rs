//! Built-in float defaults (Stickies), `title_contains` rules and pinned floats.

use super::*;

const STICKIES: &str = "com.apple.Stickies";
const SAFARI: &str = "com.apple.Safari";

fn sticky(id: u32) -> crate::PhysicalWindowId {
    crate::PhysicalWindowId {
        app: AppInstanceId {
            pid: 5,
            launched_at: 5,
        },
        window_id: id,
    }
}

fn note(x: i32) -> crate::Rect {
    crate::Rect {
        x,
        y: 100,
        width: 220,
        height: 180,
    }
}

fn stickies_backend(notes: &[(crate::PhysicalWindowId, crate::Rect)]) -> FakeBackend {
    FakeBackend {
        frames: notes.iter().cloned().collect(),
        observation: Some(observation_for_windows(STICKIES, &notes[0].0, notes)),
        ..FakeBackend::default()
    }
}

fn observe_notes(
    runtime: &mut WorkspaceRuntime<FakeBackend>,
    notes: &[(crate::PhysicalWindowId, crate::Rect)],
) {
    for (window, frame) in notes {
        runtime
            .backend
            .frames
            .entry(window.clone())
            .or_insert(*frame);
    }
    runtime.backend.observation = Some(observation_for_windows(STICKIES, &notes[0].0, notes));
}

fn active(runtime: &WorkspaceRuntime<FakeBackend>) -> Option<WorkspaceId> {
    runtime.workspaces.active_for(&DisplaySelector::Main)
}

#[test]
fn stickies_float_at_their_own_size_and_live_on_their_own_workspaces() {
    let first = sticky(1);
    let second = sticky(2);
    let mut runtime = runtime(stickies_backend(&[(first.clone(), note(600))]));
    let cancelled = AtomicBool::new(false);

    runtime.cycle(outputs(), &cancelled).unwrap();
    assert_eq!(runtime.workspaces.workspace_of(&first), None);
    assert_eq!(
        runtime.workspaces.floating_workspace_of(&first),
        Some(WorkspaceId::ONE)
    );
    assert_eq!(runtime.backend.frames[&first], note(600), "never stretched");

    runtime
        .switch(WorkspaceId::TWO, outputs(), &cancelled)
        .unwrap();
    assert!(runtime.parked.contains_key(&first));

    observe_notes(
        &mut runtime,
        &[(first.clone(), note(600)), (second.clone(), note(300))],
    );
    runtime.cycle(outputs(), &cancelled).unwrap();
    assert_eq!(
        runtime.workspaces.floating_workspace_of(&second),
        Some(WorkspaceId::TWO)
    );
    assert_eq!(runtime.backend.frames[&second], note(300));
    assert_eq!(
        runtime.workspaces.floating_workspace_of(&first),
        Some(WorkspaceId::ONE)
    );
    assert!(runtime.parked.contains_key(&first));

    runtime
        .switch(WorkspaceId::ONE, outputs(), &cancelled)
        .unwrap();
    assert!(!runtime.parked.contains_key(&first));
    assert_eq!(runtime.backend.frames[&first], note(600));
    assert!(runtime.parked.contains_key(&second));
}

#[test]
fn a_tile_rule_overrides_the_built_in_stickies_default() {
    let window = sticky(1);
    let config = Config::parse(&format!(
        "version = 1\n[[rules]]\nbundle_id = '{STICKIES}'\naction = 'tile'"
    ))
    .unwrap();
    let mut runtime = runtime_with_config(stickies_backend(&[(window.clone(), note(600))]), config);

    runtime.cycle(outputs(), &AtomicBool::new(false)).unwrap();

    assert_eq!(
        runtime.workspaces.workspace_of(&window),
        Some(WorkspaceId::ONE)
    );
    assert_eq!(runtime.workspaces.floating_workspace_of(&window), None);
    assert_eq!(runtime.backend.frames[&window], outputs().main().frame);
}

#[test]
fn title_contains_routes_matching_windows_without_persisting_titles() {
    let work = window(10);
    let personal = window(11);
    let config = Config::parse(&format!(
        "version = 1\n[[rules]]\nbundle_id = '{SAFARI}'\ntitle_contains = 'work'\naction = 'route'\nworkspace = 2"
    ))
    .unwrap();
    let mut observation = observation_for_windows(
        SAFARI,
        &work,
        &[(work.clone(), frame(50)), (personal.clone(), frame(500))],
    );
    let crate::AppObservation::Complete { windows, .. } = &mut observation.applications[0] else {
        unreachable!()
    };
    windows[0].title = Some("WORK — Pull Request #42".into());
    windows[1].title = Some("Weekend plans".into());
    let backend = FakeBackend {
        frames: HashMap::from([(work.clone(), frame(50)), (personal.clone(), frame(500))]),
        observation: Some(observation),
        ..FakeBackend::default()
    };
    let mut runtime = runtime_with_config(backend, config);

    runtime.cycle(outputs(), &AtomicBool::new(false)).unwrap();

    assert_eq!(
        runtime.workspaces.workspace_of(&work),
        Some(WorkspaceId::TWO)
    );
    assert_eq!(
        runtime.workspaces.workspace_of(&personal),
        Some(WorkspaceId::ONE)
    );
    let persisted = serde_json::to_string(&runtime.backend.workspace_state).unwrap();
    assert!(!persisted.to_lowercase().contains("work —"), "{persisted}");
    assert!(!persisted.contains("Weekend"), "{persisted}");
}

fn pin_config() -> Config {
    Config::parse(&format!(
        "version = 1\n[[rules]]\nbundle_id = '{STICKIES}'\naction = 'pin'"
    ))
    .unwrap()
}

#[test]
fn a_pinned_float_stays_visible_across_switches_and_never_pulls_focus() {
    let window = sticky(1);
    let mut runtime = runtime_with_config(
        stickies_backend(&[(window.clone(), note(600))]),
        pin_config(),
    );
    let cancelled = AtomicBool::new(false);
    runtime.cycle(outputs(), &cancelled).unwrap();
    assert_eq!(
        runtime.workspaces.floating_workspace_of(&window),
        Some(WorkspaceId::ONE)
    );
    assert!(runtime.workspaces.is_pinned(&window));

    for workspace in [WorkspaceId::TWO, WorkspaceId::new(3).unwrap()] {
        runtime.switch(workspace, outputs(), &cancelled).unwrap();
        runtime.cycle(outputs(), &cancelled).unwrap();
        assert!(!runtime.parked.contains_key(&window));
        assert_eq!(runtime.backend.frames[&window], note(600));
        assert!(!runtime.backend.parked_writes.contains(&window));
    }

    // Clicking the note on workspace 3 does not switch back to its owner.
    runtime.backend.focused = Some(window.clone());
    runtime.backend.frontmost = Some(window.app.clone());
    runtime.cycle(outputs(), &cancelled).unwrap();
    assert_eq!(active(&runtime), WorkspaceId::new(3).ok());

    // Pause and quit restore everything parked; the pinned note never was.
    assert_eq!(runtime.restore_all(), 0);
    assert!(runtime.parked.is_empty());
    assert_eq!(runtime.backend.frames[&window], note(600));
}

#[test]
fn activating_an_app_whose_only_window_is_pinned_never_switches_to_its_owner() {
    let window = sticky(1);
    let mut runtime = runtime_with_config(
        stickies_backend(&[(window.clone(), note(600))]),
        pin_config(),
    );
    let cancelled = AtomicBool::new(false);
    runtime.cycle(outputs(), &cancelled).unwrap();
    assert_eq!(
        runtime.workspaces.floating_workspace_of(&window),
        Some(WorkspaceId::ONE)
    );
    assert!(runtime.workspaces.is_pinned(&window));
    runtime
        .switch(WorkspaceId::TWO, outputs(), &cancelled)
        .unwrap();
    runtime.cycle(outputs(), &cancelled).unwrap();

    // Clicking the app in the Dock (or unminimizing it) makes it frontmost
    // before Accessibility reports its window as focused (#465).
    runtime.backend.focused = None;
    runtime.backend.frontmost = Some(window.app.clone());
    runtime.cycle(outputs(), &cancelled).unwrap();
    assert_eq!(active(&runtime), Some(WorkspaceId::TWO));
    assert!(!runtime.parked.contains_key(&window));
}

#[test]
fn a_pin_rule_added_while_a_note_is_parked_brings_it_back() {
    let window = sticky(1);
    let mut runtime = runtime(stickies_backend(&[(window.clone(), note(600))]));
    let cancelled = AtomicBool::new(false);
    runtime.cycle(outputs(), &cancelled).unwrap();
    runtime
        .switch(WorkspaceId::TWO, outputs(), &cancelled)
        .unwrap();
    assert!(runtime.parked.contains_key(&window));

    // Some time later (past the restore retry throttle) the user adds a pin rule.
    runtime.backend.advance(Duration::from_secs(3));
    runtime.replace_config(pin_config());
    runtime.cycle(outputs(), &cancelled).unwrap();

    assert!(runtime.workspaces.is_pinned(&window));
    assert!(!runtime.parked.contains_key(&window));
    assert_eq!(runtime.backend.frames[&window], note(600));
    assert_eq!(active(&runtime), Some(WorkspaceId::TWO));
}

/// As after a restart with every Stickies note minimized: no observation lists the app, so
/// neither its bundle nor its notes' titles are known, and only the rule probe can read them.
fn forget_stickies_as_after_a_restart(
    runtime: &mut WorkspaceRuntime<FakeBackend>,
    probes: &[(u32, Option<&'static str>, bool)],
) {
    runtime.memory.bundles.clear();
    runtime.memory.identities.clear();
    runtime.backend.observation = Some(crate::NativeObservation {
        cycle: 0,
        mouse_down: false,
        applications: Vec::new(),
    });
    runtime.backend.rule_probes = probes
        .iter()
        .map(|&(id, title, resolved)| (id, (STICKIES, title, resolved)))
        .collect();
}

#[test]
fn a_pin_rule_matches_a_parked_window_whose_app_was_never_observed() {
    // After a restart, an app whose every window is minimized is in no observation, so its
    // bundle is not known yet; the pin rule still finds the window by asking its app (#697).
    let window = sticky(1);
    let mut runtime = runtime(stickies_backend(&[(window.clone(), note(600))]));
    let cancelled = AtomicBool::new(false);
    runtime.cycle(outputs(), &cancelled).unwrap();
    runtime
        .switch(WorkspaceId::TWO, outputs(), &cancelled)
        .unwrap();
    assert!(runtime.parked.contains_key(&window));

    forget_stickies_as_after_a_restart(&mut runtime, &[(1, None, true)]);
    runtime.backend.advance(Duration::from_secs(3));
    runtime.replace_config(pin_config());
    runtime.cycle(outputs(), &cancelled).unwrap();
    runtime.cycle(outputs(), &cancelled).unwrap();

    assert!(runtime.workspaces.is_pinned(&window));
    assert!(!runtime.parked.contains_key(&window));
    // Read once; the answer is remembered.
    assert_eq!(runtime.backend.rule_probe_calls, vec![1]);
}

#[test]
fn a_never_observed_parked_note_keeps_an_earlier_title_rule_over_a_pin_rule() {
    // The title read from the running app decides as an observed one would (#549, #697); a
    // title it could not read yet waits rather than let the bundle-wide pin win.
    let (quiet, public, unread) = (sticky(1), sticky(2), sticky(3));
    let notes = [
        (quiet.clone(), note(600)),
        (public.clone(), note(300)),
        // Keep the center on main, whose right edge is x=1000.
        (unread.clone(), note(750)),
    ];
    let rules = |action: &str| {
        Config::parse(&format!(
            "version = 1\n[[rules]]\nbundle_id = '{STICKIES}'\ntitle_contains = 'qUiEt'\naction = 'float'\n[[rules]]\nbundle_id = '{STICKIES}'\naction = '{action}'"
        ))
        .unwrap()
    };
    let mut runtime = runtime_with_config(stickies_backend(&notes), rules("float"));
    let cancelled = AtomicBool::new(false);
    runtime.cycle(outputs(), &cancelled).unwrap();
    for (window, _) in &notes {
        assert_eq!(
            runtime.workspaces.floating_workspace_of(window),
            Some(WorkspaceId::ONE)
        );
    }
    runtime
        .switch(WorkspaceId::TWO, outputs(), &cancelled)
        .unwrap();
    for (window, _) in &notes {
        assert!(runtime.parked.contains_key(window));
    }

    forget_stickies_as_after_a_restart(
        &mut runtime,
        &[
            (1, Some("C1 Quiet"), true),
            (2, Some("C2 Public"), true),
            (3, None, false),
        ],
    );
    runtime.backend.advance(Duration::from_secs(3));
    runtime.replace_config(rules("pin"));
    for _ in 0..3 {
        runtime.cycle(outputs(), &cancelled).unwrap();
        assert!(runtime.workspaces.is_pinned(&public));
        assert!(!runtime.parked.contains_key(&public));
        for window in [&quiet, &unread] {
            assert!(!runtime.workspaces.is_pinned(window));
            assert!(runtime.parked.contains_key(window));
        }
        runtime.backend.advance(Duration::from_secs(3));
    }
    assert_eq!(active(&runtime), Some(WorkspaceId::TWO));
}

#[test]
fn a_pin_rule_added_while_a_tile_is_parked_floats_and_brings_it_back() {
    let editor = window(10);
    let mut runtime = runtime(FakeBackend {
        frames: HashMap::from([(editor.clone(), frame(50))]),
        observation: Some(observation(&editor, frame(50))),
        ..FakeBackend::default()
    });
    let cancelled = AtomicBool::new(false);
    runtime.cycle(outputs(), &cancelled).unwrap();
    assert_eq!(
        runtime.workspaces.workspace_of(&editor),
        Some(WorkspaceId::ONE)
    );
    let tiled_at = runtime.backend.frames[&editor];
    runtime
        .switch(WorkspaceId::TWO, outputs(), &cancelled)
        .unwrap();
    assert!(runtime.parked.contains_key(&editor));

    runtime.backend.advance(Duration::from_secs(3));
    runtime.replace_config(
        Config::parse("version = 1\n[[rules]]\nbundle_id = 'com.example.Test'\naction = 'pin'")
            .unwrap(),
    );
    runtime.cycle(outputs(), &cancelled).unwrap();

    assert_eq!(runtime.workspaces.workspace_of(&editor), None);
    assert_eq!(
        runtime.workspaces.floating_workspace_of(&editor),
        Some(WorkspaceId::ONE)
    );
    assert!(runtime.workspaces.is_pinned(&editor));
    assert!(!runtime.parked.contains_key(&editor));
    assert_eq!(runtime.backend.frames[&editor], tiled_at);
    // It came from a tile, so its app's first-window route stays used (#664).
    assert!(runtime.workspaces.app_holds_route(&editor.app));
    assert_eq!(active(&runtime), Some(WorkspaceId::TWO));
}

#[test]
fn ctl_pin_floats_and_pins_the_focused_window_then_unpins_where_it_is_seen() {
    let editor = window(10);
    let mut runtime = runtime(FakeBackend {
        frames: HashMap::from([(editor.clone(), frame(50))]),
        observation: Some(observation(&editor, frame(50))),
        focused: Some(editor.clone()),
        ..FakeBackend::default()
    });
    let cancelled = AtomicBool::new(false);
    runtime.cycle(outputs(), &cancelled).unwrap();
    assert_eq!(
        runtime.workspaces.workspace_of(&editor),
        Some(WorkspaceId::ONE)
    );

    runtime.toggle_pin(outputs(), &cancelled).unwrap();
    assert_eq!(runtime.workspaces.workspace_of(&editor), None);
    assert!(runtime.workspaces.is_pinned_by_hand(&editor));
    assert!(
        runtime
            .backend
            .workspace_state
            .as_ref()
            .is_some_and(|state| state.validate().is_ok() && state != &Default::default())
    );

    runtime.backend.focused = None;
    runtime
        .switch(WorkspaceId::TWO, outputs(), &cancelled)
        .unwrap();
    assert!(!runtime.parked.contains_key(&editor));

    runtime.backend.focused = Some(editor.clone());
    runtime.toggle_pin(outputs(), &cancelled).unwrap();
    assert!(!runtime.workspaces.is_pinned(&editor));
    assert_eq!(
        runtime.workspaces.floating_workspace_of(&editor),
        Some(WorkspaceId::TWO)
    );
    runtime.cycle(outputs(), &cancelled).unwrap();
    assert!(!runtime.parked.contains_key(&editor));
}

#[test]
fn a_pin_rule_reload_leaves_a_parked_note_an_earlier_title_rule_floats() {
    a_pin_rule_reload_leaves_a_parked_note_titled("B1 Quiet");
}

#[test]
fn a_pin_rule_reload_leaves_a_parked_note_matched_past_the_remembered_title_length() {
    let long = format!(
        "{} Quiet",
        "a".repeat(crate::window_memory::MAX_TITLE_LEN + 8)
    );
    a_pin_rule_reload_leaves_a_parked_note_titled(&long);
}

fn a_pin_rule_reload_leaves_a_parked_note_titled(quiet_title: &str) {
    let quiet = sticky(1);
    let public = sticky(2);
    let notes = [(quiet.clone(), note(600)), (public.clone(), note(300))];
    let rules = |action: &str| {
        Config::parse(&format!(
            "version = 1\n[[rules]]\nbundle_id = '{STICKIES}'\ntitle_contains = 'qUiEt'\naction = 'float'\n[[rules]]\nbundle_id = '{STICKIES}'\naction = '{action}'"
        ))
        .unwrap()
    };
    let mut backend = stickies_backend(&notes);
    let crate::AppObservation::Complete { windows, .. } =
        &mut backend.observation.as_mut().unwrap().applications[0]
    else {
        unreachable!()
    };
    windows[0].title = Some(quiet_title.into());
    windows[1].title = Some("B2 Public".into());
    let mut runtime = runtime_with_config(backend, rules("float"));
    let cancelled = AtomicBool::new(false);
    runtime.cycle(outputs(), &cancelled).unwrap();
    runtime
        .switch(WorkspaceId::TWO, outputs(), &cancelled)
        .unwrap();
    assert!(runtime.parked.contains_key(&quiet));
    assert!(runtime.parked.contains_key(&public));
    let parked_at = runtime.backend.frames[&quiet];

    runtime.backend.advance(Duration::from_secs(3));
    runtime.replace_config(rules("pin"));
    for _ in 0..4 {
        runtime.cycle(outputs(), &cancelled).unwrap();
        assert!(runtime.workspaces.is_pinned(&public));
        assert!(!runtime.parked.contains_key(&public));
        assert!(!runtime.workspaces.is_pinned(&quiet));
        assert!(runtime.parked.contains_key(&quiet));
        assert_eq!(runtime.backend.frames[&quiet], parked_at, "never revealed");
        runtime.backend.advance(Duration::from_secs(3));
    }
    assert_eq!(active(&runtime), Some(WorkspaceId::TWO));
}
