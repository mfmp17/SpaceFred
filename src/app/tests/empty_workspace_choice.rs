//! #360, `[labs] empty_workspace_focus`: an empty workspace chosen on another
//! display is the focused one for commands until another window takes focus.
use super::*;

const MAIN_WINDOW: u32 = 10;
const OTHER_MAIN_WINDOW: u32 = 11;

fn ws(n: u8) -> WorkspaceId {
    WorkspaceId::new(n).unwrap()
}

fn act(
    rt: &mut WorkspaceRuntime<FakeBackend>,
    action: WorkerAction,
) -> Result<CycleStatus, String> {
    let Some(WorkerResult::Status { result, .. }) = rt.execute(
        WorkerCommand::Action(action, outputs()),
        &AtomicBool::new(false),
    ) else {
        panic!("missing action status");
    };
    result
}

/// Workspace 1 on Main holds two windows, the first focused; workspace 6 on
/// Secondary holds one. Then empty workspace 7 is chosen on Secondary with a key.
fn chosen(lab: bool) -> WorkspaceRuntime<FakeBackend> {
    chosen_with(lab, WorkerAction::SwitchWorkspace(ws(7)))
}

fn chosen_with(lab: bool, choose: WorkerAction) -> WorkspaceRuntime<FakeBackend> {
    let (focused, other, secondary) = (window(MAIN_WINDOW), window(OTHER_MAIN_WINDOW), window(60));
    let mut backend = FakeBackend {
        focused: Some(focused.clone()),
        frontmost: Some(focused.app.clone()),
        ..FakeBackend::default()
    };
    backend.frames.insert(focused.clone(), frame(50));
    backend.frames.insert(other.clone(), frame(500));
    backend.frames.insert(secondary.clone(), frame(1050));
    let text = format!("version = 1\n[labs]\nempty_workspace_focus = {lab}\n");
    let mut rt = runtime_with_config(backend, Config::parse(&text).unwrap());
    bind(&mut rt, ws(1), &focused);
    bind(&mut rt, ws(1), &other);
    bind(&mut rt, ws(6), &secondary);
    rt.workspaces.switch(ws(6));
    rt.workspaces.switch(ws(1));
    act(&mut rt, choose).unwrap();
    assert_eq!(rt.focus.history().current, Some(ws(7)));
    // Showing an empty workspace focuses nothing: Main's window keeps focus.
    assert_eq!(rt.backend.focused, Some(window(MAIN_WINDOW)));
    rt
}

fn moved(rt: &mut WorkspaceRuntime<FakeBackend>) -> Option<WorkspaceId> {
    act(
        rt,
        WorkerAction::MoveWorkspaceToDisplay(DisplayTarget::Next),
    )
    .unwrap();
    rt.backend.persisted_monitors.first().map(|moved| moved.0)
}

#[test]
fn moving_the_focused_workspace_moves_the_empty_one_chosen() {
    let mut rt = chosen(true);
    assert_eq!(moved(&mut rt), Some(ws(7)));
}

#[test]
fn choosing_it_from_the_menu_or_hud_counts_too() {
    let mut rt = chosen_with(true, WorkerAction::SelectWorkspace(ws(7)));
    assert!(rt.focus.empty_choice().is_some());
    assert_eq!(moved(&mut rt), Some(ws(7)));
}

#[test]
fn with_the_lab_off_the_window_left_focused_still_decides() {
    let mut rt = chosen(false);
    assert!(rt.focus.empty_choice().is_none());
    assert_eq!(moved(&mut rt), Some(ws(1)));
}

#[test]
fn another_window_taking_focus_ends_the_choice() {
    let mut rt = chosen(true);
    rt.backend.focused = Some(window(OTHER_MAIN_WINDOW));
    assert_eq!(moved(&mut rt), Some(ws(1)));
    assert!(rt.focus.empty_choice().is_none());
}

#[test]
fn commands_that_need_a_window_find_none() {
    let mut rt = chosen(true);
    assert!(act(&mut rt, WorkerAction::ToggleFullscreenLayout).is_err());
    assert!(!rt.workspaces.deck(ws(1)).is_monocle());
    assert!(rt.focus.empty_choice().is_some());
}

#[test]
fn focusing_toward_main_reaches_its_window_and_ends_the_choice() {
    let mut rt = chosen(true);
    act(&mut rt, WorkerAction::Focus(Direction::Left)).unwrap();
    assert_eq!(rt.focus.history().current, Some(ws(1)));
    assert!(rt.focus.empty_choice().is_none());
    let on_main = [MAIN_WINDOW, OTHER_MAIN_WINDOW];
    assert!(rt.backend.calls.iter().any(|call| matches!(
        call,
        BackendCall::Focus(window) if on_main.contains(&window.window_id)
    )));
}

#[test]
fn choosing_a_workspace_with_focus_on_main_ends_the_choice() {
    let mut rt = chosen(true);
    act(&mut rt, WorkerAction::SwitchWorkspace(ws(1))).unwrap();
    assert!(rt.focus.empty_choice().is_none());
    assert_eq!(moved(&mut rt), Some(ws(1)));
}

fn last_focus_call(rt: &WorkspaceRuntime<FakeBackend>) -> Option<u32> {
    rt.backend.calls.iter().rev().find_map(|call| match call {
        BackendCall::Focus(window) => Some(window.window_id),
        _ => None,
    })
}

#[test]
fn a_lagging_report_of_the_window_that_kept_focus_stands_for_the_new_target() {
    let mut rt = chosen(true);
    act(&mut rt, WorkerAction::Focus(Direction::Left)).unwrap();
    // From Secondary, the nearest tile to the left is Main's right-hand one.
    assert_eq!(last_focus_call(&rt), Some(OTHER_MAIN_WINDOW));
    // Accessibility still reports the window that kept focus.
    rt.backend.focused = Some(window(MAIN_WINDOW));
    rt.backend.frontmost = Some(window(MAIN_WINDOW).app);
    act(&mut rt, WorkerAction::Focus(Direction::Left)).unwrap();
    // The second press steps from the window just focused, not the stale one
    // (from which there is nothing further left).
    assert_eq!(last_focus_call(&rt), Some(MAIN_WINDOW));
}

fn main_only() -> macos::DisplayOutputs {
    macos::DisplayOutputs::new(vec![outputs().main().clone()]).unwrap()
}

#[test]
fn unplugging_the_chosen_workspace_display_ends_the_choice() {
    let mut rt = chosen(true);
    // Main keeps showing workspace 1 once Secondary is gone.
    rt.workspaces.show_on("main-id", ws(1));
    let Some(WorkerResult::Status { result, .. }) = rt.execute(
        WorkerCommand::Action(WorkerAction::ToggleFullscreenLayout, main_only()),
        &AtomicBool::new(false),
    ) else {
        panic!("missing action status");
    };
    result.unwrap();
    assert!(rt.focus.empty_choice().is_none());
    assert!(rt.workspaces.deck(ws(1)).is_monocle());
}

#[test]
fn an_observation_cycle_keeps_the_choice() {
    let mut rt = chosen(true);
    let seen: Vec<_> = [MAIN_WINDOW, OTHER_MAIN_WINDOW, 60]
        .into_iter()
        .map(|id| (window(id), rt.backend.frames[&window(id)]))
        .collect();
    rt.backend.observation = Some(observation_for_windows(
        "com.example.Test",
        &window(MAIN_WINDOW),
        &seen,
    ));
    act(&mut rt, WorkerAction::Retile).unwrap();
    assert!(rt.focus.empty_choice().is_some());
    assert_eq!(moved(&mut rt), Some(ws(7)));
}

#[test]
fn an_unmanaged_window_keeping_focus_counts_by_its_display() {
    // #532: the window that keeps focus belongs to no workspace (an `ignore`
    // rule), so the display it is on decides where it is.
    let (unmanaged, tile, secondary) = (window(12), window(MAIN_WINDOW), window(60));
    let mut backend = FakeBackend {
        focused: Some(unmanaged.clone()),
        frontmost: Some(unmanaged.app.clone()),
        ..FakeBackend::default()
    };
    backend.frames.insert(unmanaged.clone(), frame(300));
    backend.frames.insert(tile.clone(), frame(50));
    backend.frames.insert(secondary.clone(), frame(1050));
    let config = Config::parse("version = 1\n[labs]\nempty_workspace_focus = true\n").unwrap();
    let mut rt = runtime_with_config(backend, config);
    bind(&mut rt, ws(1), &tile);
    bind(&mut rt, ws(6), &secondary);
    rt.workspaces.switch(ws(6));
    rt.workspaces.switch(ws(1));
    assert_eq!(rt.workspaces.workspace_of_any(&unmanaged), None);
    act(&mut rt, WorkerAction::SwitchWorkspace(ws(7))).unwrap();
    assert_eq!(rt.backend.focused, Some(unmanaged));
    assert!(rt.focus.empty_choice().is_some());
    assert_eq!(moved(&mut rt), Some(ws(7)));
}
