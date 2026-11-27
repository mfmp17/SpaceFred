//! #750: an app's native next-tab action while a tile's title-bar drag was held over its
//! neighbor handed the incoming tab a new tile on release, so the committed swap lost its
//! split sizes. No cycle runs while the button is down, so the incoming tab, where the drag
//! left the group, matched neither the tile's new target nor its last observed frame.
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
fn a_native_tab_switch_during_a_held_swap_keeps_the_split_sizes() {
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
    let sized = |width| crate::Rect { width, ..start };

    // Resize the tab's tile to 400 wide, leaving its neighbor 600.
    rt.backend.mouse_down = true;
    rt.backend.frames.insert(tab.clone(), sized(380));
    dispatch(
        &mut rt,
        WorkerCommand::Gesture(
            FinalGesture::Resize(tab.clone(), sized(380)),
            true,
            one_output(),
        ),
    );
    rt.backend.mouse_down = false;
    rt.backend.frames.insert(tab.clone(), sized(400));
    dispatch(
        &mut rt,
        WorkerCommand::Gesture(
            FinalGesture::Resize(tab.clone(), sized(400)),
            false,
            one_output(),
        ),
    );
    assert_eq!(span(rt.backend.frames[&neighbor]), (400, 600));
    let slot = rt.workspaces.engine(WorkspaceId::ONE).slot_for_window(&tab);

    // Drag the tab over its neighbor and hold: the preview shows the swap.
    let point = (700, 400);
    let dragged = crate::Rect {
        x: 550,
        ..sized(400)
    };
    rt.backend.mouse_down = true;
    rt.backend.frames.insert(tab.clone(), dragged);
    dispatch(
        &mut rt,
        WorkerCommand::Gesture(FinalGesture::drop(tab.clone(), point), true, one_output()),
    );
    assert_eq!(span(rt.backend.frames[&neighbor]), (0, 400));

    // Still held, the app selects its next tab, which takes the dragged frame.
    rt.backend.frames.remove(&tab);
    rt.backend.frames.insert(next_tab.clone(), dragged);
    rt.backend.focused = Some(next_tab.clone());
    rt.backend.mouse_down = false;
    dispatch(
        &mut rt,
        WorkerCommand::Gesture(FinalGesture::drop(tab.clone(), point), false, one_output()),
    );
    let swapped_neighbor = rt.backend.frames[&neighbor];
    assert_eq!(span(swapped_neighbor), (0, 400));
    // The release's write went to the outgoing tab, now off screen with the group's other tabs.
    rt.backend.frames.remove(&tab);

    rt.backend.observation = Some(observation_for_windows(
        BUNDLE,
        &next_tab,
        &[
            (next_tab.clone(), dragged),
            (neighbor.clone(), swapped_neighbor),
        ],
    ));
    rt.cycle(one_output(), &AtomicBool::new(false)).unwrap();

    assert_eq!(
        rt.workspaces
            .engine(WorkspaceId::ONE)
            .slot_for_window(&next_tab),
        slot
    );
    assert_eq!(span(rt.backend.frames[&neighbor]), (0, 400));
    assert_eq!(span(rt.backend.frames[&next_tab]), (400, 600));
}
