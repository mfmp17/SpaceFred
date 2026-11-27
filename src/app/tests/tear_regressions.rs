//! Regressions for stateful bugs a model-based torture test found in the
//! runtime. Each drives the plain `FakeBackend` through the minimal sequence.

use super::*;

fn app_window(pid: i32, window_id: u32) -> crate::PhysicalWindowId {
    crate::PhysicalWindowId {
        app: AppInstanceId {
            pid,
            launched_at: 1,
        },
        window_id,
    }
}

/// Complete observations of `windows`, grouped by app, with nothing focused.
fn observe(windows: &[&crate::PhysicalWindowId]) -> crate::NativeObservation {
    let mut apps: Vec<crate::AppObservation> = Vec::new();
    for window in windows {
        let observed = crate::ObservedWindow {
            id: (*window).clone(),
            frame: crate::Rect::default(),
            resizable: true,
            title: None,
            document: None,
        };
        match apps.iter_mut().find(|app| app.app_id() == &window.app) {
            Some(crate::AppObservation::Complete { windows, .. }) => windows.push(observed),
            _ => apps.push(crate::AppObservation::Complete {
                app: crate::ObservedApplication {
                    id: window.app.clone(),
                    bundle_id: format!("com.example.App{}", window.app.pid),
                },
                focused: None,
                windows: vec![observed],
            }),
        }
    }
    crate::NativeObservation {
        cycle: 0,
        mouse_down: false,
        applications: apps,
    }
}

fn main_only() -> macos::DisplayOutputs {
    macos::DisplayOutputs::new(vec![outputs().displays()[0].clone()]).unwrap()
}

/// The widest strip of `frame` showing on any display of `outputs`.
fn visible_px(frame: crate::Rect, outputs: &macos::DisplayOutputs) -> i32 {
    outputs
        .frames()
        .map(|display| {
            let width =
                (frame.x + frame.width).min(display.x + display.width) - frame.x.max(display.x);
            let height =
                (frame.y + frame.height).min(display.y + display.height) - frame.y.max(display.y);
            if height > 0 { width.max(0) } else { 0 }
        })
        .max()
        .unwrap_or(0)
}

fn assert_hidden(runtime: &WorkspaceRuntime<FakeBackend>, window: &crate::PhysicalWindowId) {
    let at = runtime.backend.frames[window];
    let shown = visible_px(at, &outputs());
    assert!(shown <= 1, "hidden window shows {shown}px at {at:?}");
}

/// A stale suppressed window inside the target workspace must not clear the
/// frontmost-app guard: macOS activates the entry window asynchronously, and the
/// first lagging cycle would follow the old app straight back.
#[test]
fn switch_after_moving_a_window_there_survives_async_activation() {
    let moved = app_window(1, 10);
    let other = app_window(2, 20);
    let mut backend = FakeBackend {
        focused: Some(moved.clone()),
        frontmost: Some(moved.app.clone()),
        observation: Some(observe(&[&moved, &other])),
        ..FakeBackend::default()
    };
    backend.frames.insert(moved.clone(), frame(50));
    backend.frames.insert(other.clone(), frame(500));
    let mut runtime = runtime(backend);
    bind(&mut runtime, WorkspaceId::ONE, &moved);
    bind(&mut runtime, WorkspaceId::ONE, &other);
    let four = WorkspaceId::new(4).unwrap();
    let cancelled = AtomicBool::new(false);

    runtime.move_focused(four, outputs(), &cancelled).unwrap();
    // Cmd+Tab to the other app before Accessibility reports its window.
    runtime.backend.focused = None;
    runtime.backend.frontmost = Some(other.app.clone());
    runtime.switch(four, outputs(), &cancelled).unwrap();
    assert_eq!(
        runtime.workspaces.active_for(&DisplaySelector::Main),
        Some(four)
    );
    // One cycle still sees the old app frontmost.
    runtime.backend.focused = None;
    runtime.backend.frontmost = Some(other.app.clone());
    runtime.cycle(outputs(), &cancelled).unwrap();

    assert_eq!(
        runtime.workspaces.active_for(&DisplaySelector::Main),
        Some(four),
        "focus follow snapped the display back to workspace 1"
    );
}

/// Re-parking a window that grew while hidden must use its current size, or
/// the growth stays on screen.
#[test]
fn re_parking_a_grown_window_hides_all_of_it() {
    let visible = app_window(1, 10);
    let electron = app_window(2, 20);
    let mut backend = FakeBackend {
        focused: Some(visible.clone()),
        frontmost: Some(visible.app.clone()),
        observation: Some(observe(&[&visible, &electron])),
        ..FakeBackend::default()
    };
    backend.frames.insert(visible.clone(), frame(50));
    // The app restored larger saved bounds while its workspace was hidden.
    backend.frames.insert(
        electron.clone(),
        crate::Rect {
            x: 100,
            y: 20,
            width: 900,
            height: 300,
        },
    );
    let mut runtime = runtime_with_parked(backend, [(electron.clone(), frame(50))]);
    bind(&mut runtime, WorkspaceId::ONE, &visible);
    bind(&mut runtime, WorkspaceId::new(2).unwrap(), &electron);

    runtime.cycle(outputs(), &AtomicBool::new(false)).unwrap();

    assert_hidden(&runtime, &electron);
}

/// Reconnecting a display must not uncover parked windows, nor let heal re-home
/// one into the new display's workspace.
#[test]
fn reconnecting_a_display_keeps_parked_windows_hidden_and_owned() {
    let (mut runtime, hidden) = parked_on_laptop();
    let cancelled = AtomicBool::new(false);

    // Dock, then wait out the park grace.
    runtime.backend.advance(Duration::from_secs(3));
    runtime.cycle(outputs(), &cancelled).unwrap();
    runtime.cycle(outputs(), &cancelled).unwrap();

    assert_eq!(
        runtime.workspaces.workspace_of(&hidden),
        Some(WorkspaceId::new(2).unwrap()),
        "re-homed by the reconnect"
    );
    assert_hidden(&runtime, &hidden);
}

/// A re-park after a display change that fails while the app is briefly
/// inaccessible is retried on later cycles, and the window is not healed into
/// the new display's workspace meanwhile.
#[test]
fn a_failed_re_park_after_a_display_change_is_retried() {
    let (mut runtime, hidden) = parked_on_laptop();
    let cancelled = AtomicBool::new(false);

    runtime.backend.advance(Duration::from_secs(3));
    runtime.backend.position_outcome = Some(macos::ApplyOutcome::Unavailable);
    drop(runtime.cycle(outputs(), &cancelled));
    assert!(runtime.parked.awaits_display(&hidden));
    runtime.backend.position_outcome = None;
    runtime.cycle(outputs(), &cancelled).unwrap();
    runtime.cycle(outputs(), &cancelled).unwrap();

    assert!(!runtime.parked.awaits_display(&hidden));
    assert_eq!(
        runtime.workspaces.workspace_of(&hidden),
        Some(WorkspaceId::new(2).unwrap())
    );
    assert_hidden(&runtime, &hidden);
}

/// macOS holds a parked window up to keep its title bar on screen. A display
/// change that leaves its parking corner where it was does not write it again.
#[test]
fn a_display_change_leaves_a_held_up_parked_window_alone() {
    let (mut runtime, hidden) = parked_on_laptop();
    let cancelled = AtomicBool::new(false);
    runtime.backend.advance(Duration::from_secs(3));
    runtime.cycle(outputs(), &cancelled).unwrap();
    runtime.cycle(outputs(), &cancelled).unwrap();
    for frames in [
        &mut runtime.backend.frames,
        &mut runtime.backend.server_frames,
    ] {
        if let Some(frame) = frames.get_mut(&hidden) {
            frame.y -= 31;
        }
    }
    runtime.backend.parked_writes.clear();
    runtime.backend.advance(Duration::from_secs(3));

    runtime.cycle(three_outputs(), &cancelled).unwrap();
    runtime.cycle(three_outputs(), &cancelled).unwrap();

    assert!(!runtime.backend.parked_writes.contains(&hidden));
    assert!(!runtime.parked.awaits_display(&hidden));
    assert_hidden(&runtime, &hidden);
}

/// A display whose visible frame grows at the bottom (its Dock moved away) moves
/// a held-up parked window down to the new corner rather than leaving more of
/// it showing.
#[test]
fn a_display_that_grows_at_the_bottom_re_parks_its_held_up_windows() {
    for growth in [40, 70] {
        let (mut runtime, hidden) = parked_on_laptop();
        let cancelled = AtomicBool::new(false);
        for frames in [
            &mut runtime.backend.frames,
            &mut runtime.backend.server_frames,
        ] {
            if let Some(frame) = frames.get_mut(&hidden) {
                frame.y -= 31;
            }
        }
        runtime.backend.parked_writes.clear();
        runtime.backend.advance(Duration::from_secs(3));
        let mut grown = main_only().displays().to_vec();
        grown[0].frame.height += growth;
        let grown = macos::DisplayOutputs::new(grown).unwrap();

        runtime.cycle(grown.clone(), &cancelled).unwrap();

        assert!(runtime.backend.parked_writes.contains(&hidden), "{growth}");
        let main = grown.main().frame;
        assert_eq!(runtime.backend.frames[&hidden].y, main.y + main.height - 1);
    }
}

/// A re-park that fails after a display grows is not forgotten when another
/// display change arrives before the retry and leaves the new corner in place.
#[test]
fn a_failed_re_park_survives_a_second_display_change() {
    let (mut runtime, hidden) = parked_on_laptop();
    let cancelled = AtomicBool::new(false);
    for frames in [
        &mut runtime.backend.frames,
        &mut runtime.backend.server_frames,
    ] {
        if let Some(frame) = frames.get_mut(&hidden) {
            frame.y -= 31;
        }
    }
    runtime.backend.advance(Duration::from_secs(3));
    let mut grown = main_only().displays().to_vec();
    grown[0].frame.height += 40;
    runtime.backend.position_outcome = Some(macos::ApplyOutcome::Unavailable);
    drop(runtime.cycle(
        macos::DisplayOutputs::new(grown.clone()).unwrap(),
        &cancelled,
    ));
    assert!(runtime.parked.awaits_display(&hidden));
    runtime.backend.position_outcome = None;
    runtime.backend.parked_writes.clear();
    let mut left = outputs().displays()[1].clone();
    left.frame.x = -1000;
    grown.push(left);
    let both = macos::DisplayOutputs::new(grown).unwrap();

    runtime.cycle(both.clone(), &cancelled).unwrap();

    assert!(runtime.backend.parked_writes.contains(&hidden));
    let main = both.main().frame;
    assert_eq!(runtime.backend.frames[&hidden].y, main.y + main.height - 1);
}

/// Laptop only: a window sent to workspace 2 while its app stays frontmost on 1.
fn parked_on_laptop() -> (WorkspaceRuntime<FakeBackend>, crate::PhysicalWindowId) {
    let hidden = app_window(1, 10);
    let working = app_window(1, 11);
    let mut backend = FakeBackend {
        focused: Some(hidden.clone()),
        frontmost: Some(hidden.app.clone()),
        observation: Some(observe(&[&hidden, &working])),
        ..FakeBackend::default()
    };
    backend.frames.insert(hidden.clone(), frame(50));
    backend.frames.insert(working.clone(), frame(500));
    let mut runtime = runtime(backend);
    bind(&mut runtime, WorkspaceId::ONE, &hidden);
    bind(&mut runtime, WorkspaceId::ONE, &working);
    let cancelled = AtomicBool::new(false);
    runtime
        .move_focused(WorkspaceId::new(2).unwrap(), main_only(), &cancelled)
        .unwrap();
    runtime.backend.focused = Some(working);
    runtime.cycle(main_only(), &cancelled).unwrap();
    assert!(visible_px(runtime.backend.frames[&hidden], &main_only()) <= 1);
    (runtime, hidden)
}

/// Accessibility reports "gone" for a window it merely cannot resolve while the
/// window server still lists it. Restoring must keep its only recovery frame.
#[test]
fn restore_all_keeps_recovery_for_a_window_the_window_server_still_lists() {
    let window = window(10);
    let normal = frame(50);
    let mut backend = FakeBackend::default();
    backend
        .server_frames
        .insert(window.clone(), main_parked_frame(normal));
    let mut runtime = runtime_with_parked(backend, [(window.clone(), normal)]);
    bind(&mut runtime, WorkspaceId::new(2).unwrap(), &window);

    let failed = runtime.restore_all();

    assert_eq!(failed, 1);
    assert_eq!(runtime.parked.get(&window), Some(&normal));
}

/// After the display a window was parked from is unplugged, restoring puts it
/// on a connected display.
#[test]
fn restore_all_after_unplugging_lands_windows_on_a_connected_display() {
    let window = window(10);
    let normal = frame(1050);
    let mut backend = FakeBackend {
        observation: Some(observe(&[])),
        ..FakeBackend::default()
    };
    backend
        .frames
        .insert(window.clone(), crate::Rect { x: 1999, ..normal });
    let mut runtime = runtime_with_parked(backend, [(window.clone(), normal)]);
    bind(&mut runtime, WorkspaceId::new(7).unwrap(), &window);
    let cancelled = AtomicBool::new(false);

    runtime.cycle(outputs(), &cancelled).unwrap();
    runtime.cycle(main_only(), &cancelled).unwrap();
    assert_eq!(runtime.restore_all(), 0);

    let at = runtime.backend.frames[&window];
    assert!(visible_px(at, &main_only()) > 1, "restored to {at:?}");
}

/// Pause and quit carry the current displays, so a restore lands on a
/// connected display even when no cycle has seen the unplug.
#[test]
fn restore_all_command_moves_recovery_frames_onto_the_current_displays() {
    let window = window(10);
    let normal = frame(1050);
    let mut backend = FakeBackend::default();
    backend
        .frames
        .insert(window.clone(), crate::Rect { x: 1999, ..normal });
    let mut runtime = runtime_with_parked(backend, [(window.clone(), normal)]);
    bind(&mut runtime, WorkspaceId::new(7).unwrap(), &window);

    let result = runtime.execute(
        WorkerCommand::RestoreAll(Some(main_only())),
        &AtomicBool::new(false),
    );

    assert!(matches!(
        result,
        Some(WorkerResult::Restored { failed: 0, .. })
    ));
    assert!(runtime.parked.is_empty());
    let at = runtime.backend.frames[&window];
    assert_eq!(at, crate::Rect { x: 600, ..normal });
}

/// An app with a floating window on the visible workspace and a tile on a
/// hidden one is ambiguous; focus follow must not switch away from it.
#[test]
fn frontmost_app_with_a_visible_floating_window_does_not_switch_away() {
    let floating = app_window(3, 30);
    let tiled = app_window(3, 31);
    let other = app_window(1, 10);
    let mut backend = FakeBackend {
        focused: None,
        frontmost: Some(floating.app.clone()),
        observation: Some(observe(&[&floating, &other])),
        ..FakeBackend::default()
    };
    backend.frames.insert(other.clone(), frame(50));
    backend.frames.insert(floating.clone(), frame(500));
    backend
        .frames
        .insert(tiled.clone(), main_parked_frame(frame(100)));
    let mut runtime = runtime_with_parked(backend, [(tiled.clone(), frame(100))]);
    bind(&mut runtime, WorkspaceId::ONE, &other);
    runtime
        .workspaces
        .own_floating(floating.clone(), WorkspaceId::ONE);
    bind(&mut runtime, WorkspaceId::new(2).unwrap(), &tiled);

    runtime.cycle(outputs(), &AtomicBool::new(false)).unwrap();

    assert_eq!(
        runtime.workspaces.active_for(&DisplaySelector::Main),
        Some(WorkspaceId::ONE)
    );
}

/// A layout that places a window still recorded as parked (a config reload made
/// its workspace visible) retires the stale recovery frame, so the next park and
/// restore use where the window really is.
#[test]
fn laying_out_a_parked_window_retires_its_stale_recovery_frame() {
    let window = window(10);
    let stale = frame(1050);
    let mut backend = FakeBackend {
        observation: Some(observe(&[&window])),
        ..FakeBackend::default()
    };
    backend
        .frames
        .insert(window.clone(), main_parked_frame(stale));
    let mut runtime = runtime_with_parked(backend, [(window.clone(), stale)]);
    bind(&mut runtime, WorkspaceId::ONE, &window);
    let cancelled = AtomicBool::new(false);
    let main = outputs().main().frame;

    runtime.layout_workspace(WorkspaceId::ONE, main, &cancelled);
    assert!(runtime.parked.is_empty());
    assert!(runtime.backend.persisted.is_empty());
    let laid_out = runtime.backend.frames[&window];

    let two = WorkspaceId::new(2).unwrap();
    runtime.switch(two, outputs(), &cancelled).unwrap();
    assert_eq!(runtime.parked.get(&window), Some(&laid_out));
    assert_eq!(runtime.restore_all(), 0);
    assert_eq!(runtime.backend.frames[&window], laid_out);
}

/// A window wider than its tile overflows the display edge with its center
/// outside; it is still on screen and must be parked.
#[test]
fn switching_away_parks_a_window_that_overflows_the_display_edge() {
    let window = window(10);
    let mut backend = FakeBackend {
        observation: Some(observe(&[&window])),
        ..FakeBackend::default()
    };
    backend.frames.insert(
        window.clone(),
        crate::Rect {
            x: 700,
            y: 20,
            width: 700,
            height: 300,
        },
    );
    let mut runtime = runtime(backend);
    bind(&mut runtime, WorkspaceId::ONE, &window);
    let cancelled = AtomicBool::new(false);

    runtime
        .switch(WorkspaceId::new(2).unwrap(), main_only(), &cancelled)
        .unwrap();
    runtime.cycle(main_only(), &cancelled).unwrap();

    let at = runtime.backend.frames[&window];
    let shown = visible_px(at, &main_only());
    assert!(
        shown <= 1,
        "hidden workspace's window shows {shown}px at {at:?}"
    );
}
