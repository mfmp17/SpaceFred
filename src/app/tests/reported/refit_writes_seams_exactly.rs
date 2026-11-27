//! #629: A minimum-size refit moves the other seams by a point or two. A tile
//! an earlier round had already written then sat inside the write tolerance of
//! its new target and was left there, overlapping a neighbor that did move.
//! Reported by @coygeek.

use super::super::*;

fn columns(minimum: i32) -> (WorkspaceRuntime<FakeBackend>, Vec<crate::PhysicalWindowId>) {
    let windows: Vec<_> = (10..14).map(window).collect();
    let mut backend = FakeBackend {
        honors_tolerance: true,
        ..FakeBackend::default()
    };
    for (index, window) in windows.iter().enumerate() {
        backend
            .frames
            .insert(window.clone(), frame(index as i32 * 10));
    }
    backend.minimum_widths.insert(windows[3].clone(), minimum);
    let mut rt = runtime(backend);
    for window in &windows {
        bind(&mut rt, WorkspaceId::ONE, window);
    }
    (rt, windows)
}

#[test]
fn a_minimum_refit_writes_every_moved_seam_exactly() {
    let output = outputs().main().frame;
    for minimum in 253..=262 {
        let (mut rt, windows) = columns(minimum);
        rt.layout_workspace(WorkspaceId::ONE, output, &AtomicBool::new(false));
        let native: Vec<_> = windows.iter().map(|w| rt.backend.frames[w]).collect();
        let targets: Vec<_> = rt
            .workspaces
            .engine(WorkspaceId::ONE)
            .placements(&Settings::for_output(output))
            .into_iter()
            .map(|placement| placement.frame)
            .collect();
        assert_eq!(native, targets, "minimum={minimum}");
        for pair in native.windows(2) {
            assert_eq!(pair[0].x + pair[0].width, pair[1].x, "minimum={minimum}");
        }
        assert_eq!(native[3].width, minimum, "minimum={minimum}");
        assert!(!rt.exact_spacing_workspaces.contains(&WorkspaceId::ONE));
    }
}
