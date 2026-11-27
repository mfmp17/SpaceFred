//! #624: an app's native next-tab action right after a pointer resize was released handed the
//! incoming tab a new tile beside its neighbor, losing the resized tile's side and width. The
//! release wrote the new layout without remembering it as the tiles' targets, so the incoming
//! tab, at the resized frame, matched neither the old target nor the last observed frame.
//! Reported by @coygeek.

use super::super::*;

const BUNDLE: &str = "com.example.Tabs";

fn one_output() -> macos::DisplayOutputs {
    macos::DisplayOutputs::new(vec![outputs().main().clone()]).unwrap()
}

fn dispatch(rt: &mut WorkspaceRuntime<FakeBackend>, command: WorkerCommand) {
    let Some(WorkerResult::Status { result, .. }) = rt.execute(command, &AtomicBool::new(false))
    else {
        panic!("expected worker status");
    };
    result.unwrap();
}

/// A window's horizontal span: the only thing a column resize changes.
fn span(frame: crate::Rect) -> (i32, i32) {
    (frame.x, frame.width)
}

fn column(x: i32, width: i32) -> crate::Rect {
    crate::Rect {
        x,
        y: 0,
        width,
        height: 800,
    }
}

#[test]
fn a_native_tab_switch_right_after_a_resize_release_keeps_the_resized_tile() {
    let (tab, neighbor, next_tab) = (window(10), window(20), window(11));
    let frames = [
        (tab.clone(), column(0, 500)),
        (neighbor.clone(), column(500, 500)),
    ];
    let backend = FakeBackend {
        frames: frames.iter().cloned().collect(),
        focused: Some(tab.clone()),
        observation: Some(observation_for_windows(BUNDLE, &tab, &frames)),
        ..FakeBackend::default()
    };
    let mut rt = runtime(backend);
    bind(&mut rt, WorkspaceId::ONE, &tab);
    bind(&mut rt, WorkspaceId::ONE, &neighbor);
    dispatch(
        &mut rt,
        WorkerCommand::Action(WorkerAction::Retile, one_output()),
    );
    let start = rt.backend.frames[&tab];
    assert_eq!(span(start), (0, 500));
    let sized = |width| crate::Rect { width, ..start };
    assert_eq!(span(rt.backend.frames[&neighbor]), (500, 500));
    let slot = rt.workspaces.engine(WorkspaceId::ONE).slot_for_window(&tab);

    // Drag the tab's right edge out to 580 and release.
    rt.backend.mouse_down = true;
    rt.backend.frames.insert(tab.clone(), sized(550));
    dispatch(
        &mut rt,
        WorkerCommand::Gesture(
            FinalGesture::Resize(tab.clone(), sized(550)),
            true,
            one_output(),
        ),
    );
    rt.backend.mouse_down = false;
    rt.backend.frames.insert(tab.clone(), sized(580));
    dispatch(
        &mut rt,
        WorkerCommand::Gesture(
            FinalGesture::Resize(tab.clone(), sized(580)),
            false,
            one_output(),
        ),
    );
    let resized_neighbor = rt.backend.frames[&neighbor];
    assert_eq!(span(resized_neighbor), (580, 420));

    // Before any cycle observes the release, the next tab takes the old one's frame.
    let resized = rt.backend.frames.remove(&tab).unwrap();
    rt.backend.frames.insert(next_tab.clone(), resized);
    rt.backend.focused = Some(next_tab.clone());
    rt.backend.observation = Some(observation_for_windows(
        BUNDLE,
        &next_tab,
        &[
            (next_tab.clone(), resized),
            (neighbor.clone(), resized_neighbor),
        ],
    ));
    rt.cycle(one_output(), &AtomicBool::new(false)).unwrap();

    assert_eq!(
        rt.workspaces
            .engine(WorkspaceId::ONE)
            .slot_for_window(&next_tab),
        slot
    );
    assert_eq!(span(rt.backend.frames[&next_tab]), (0, 580));
    assert_eq!(span(rt.backend.frames[&neighbor]), (580, 420));
}
