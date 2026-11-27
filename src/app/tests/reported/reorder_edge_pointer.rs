//! #517: Option+Shift+L off the workspace edge moves the tile onto the next
//! display's active workspace, and Mouse follows focus brings the pointer
//! along, exactly like any other keyboard move onto a visible workspace.
//! Reported by @coygeek.

use super::super::*;

#[test]
fn edge_reorder_onto_another_display_follows_the_pointer() {
    for (mode, held) in [("display", false), ("off", false), ("display", true)] {
        let config = Config::parse(&format!(
            "version = 1\n[settings]\nmouse_follows_focus = '{mode}'\n"
        ))
        .unwrap();
        let (main_tile, side_tile) = (window(10), window(60));
        let backend = FakeBackend {
            frames: HashMap::from([
                (main_tile.clone(), frame(50)),
                (side_tile.clone(), frame(1050)),
            ]),
            focused: Some(main_tile.clone()),
            frontmost: Some(main_tile.app.clone()),
            pointer: Some((200, 200)),
            mouse_down: held,
            ..FakeBackend::default()
        };
        let mut rt = runtime_with_config(backend, config);
        bind(&mut rt, WorkspaceId::ONE, &main_tile);
        bind(&mut rt, WorkspaceId::SIX, &side_tile);
        let Some(WorkerResult::Status { result, .. }) = rt.execute(
            WorkerCommand::Action(WorkerAction::Reorder(Direction::Right), outputs()),
            &AtomicBool::new(false),
        ) else {
            panic!("missing worker status");
        };
        result.unwrap();
        assert_eq!(
            rt.workspaces.workspace_of_any(&main_tile),
            Some(WorkspaceId::SIX),
            "{mode} {held}"
        );
        assert_eq!(rt.backend.focused, Some(main_tile.clone()), "{mode} {held}");
        let warps: Vec<_> = rt
            .backend
            .calls
            .iter()
            .filter_map(|call| match call {
                BackendCall::WarpPointer(point) => Some(*point),
                _ => None,
            })
            .collect();
        if mode == "display" && !held {
            assert_eq!(warps.len(), 1, "{mode} {held}");
            assert!(warps[0].0 >= 1000, "pointer lands on the secondary display");
        } else {
            assert!(warps.is_empty(), "{mode} {held}: {warps:?}");
        }
    }
}
