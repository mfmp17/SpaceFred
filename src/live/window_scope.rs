//! `watch-window`'s scope: the one window a scoped probe follows across cycles.

use std::collections::HashSet;

use crate::{AppInstanceId, AppObservation, NativeObservation, ObservedApplication, Rect};

/// How far an unknown window may sit from the followed window's last frame and still count as
/// its replacement (a native tab taking its place).
const REPLACEMENT_TOLERANCE: i32 = 100;
/// Cycles a missing window reports as settling before the scope reports it absent.
const SETTLING_CYCLES: u8 = 8;

pub struct WindowScope {
    app: AppInstanceId,
    bundle_id: String,
    /// The window's frame when the watch started: the scoped probe's layout output.
    pub frame: Rect,
    /// The display the window was found on; later snapshots are taken in it.
    pub output: Rect,
    /// The followed window's latest frame, where a replacement is expected to appear.
    anchor: Rect,
    excluded_window_ids: HashSet<u32>,
    authorized_window_ids: HashSet<u32>,
    missing_cycles: u8,
}

impl WindowScope {
    /// Scopes to `window_id` as it appears in `observation`.
    ///
    /// # Errors
    ///
    /// Fails when no complete application in `observation` reports the window.
    pub fn discover(
        observation: &NativeObservation,
        window_id: u32,
        output: Rect,
    ) -> Result<Self, String> {
        observation
            .applications
            .iter()
            .find_map(|app| match app {
                AppObservation::Complete { app, windows, .. } => windows
                    .iter()
                    .find(|window| window.id.window_id == window_id)
                    .map(|window| Self {
                        app: app.id.clone(),
                        bundle_id: app.bundle_id.clone(),
                        frame: window.frame,
                        output,
                        anchor: window.frame,
                        excluded_window_ids: windows
                            .iter()
                            .filter(|existing| existing.id.window_id != window_id)
                            .map(|existing| existing.id.window_id)
                            .collect(),
                        authorized_window_ids: HashSet::from([window_id]),
                        missing_cycles: 0,
                    }),
                _ => None,
            })
            .ok_or_else(|| format!("window {window_id} is not currently eligible"))
    }

    /// Narrows `observation` to the followed window. A window already followed stays in scope
    /// wherever it moves; an unknown one is taken only near the followed window's last frame.
    pub fn filter(&mut self, observation: &mut NativeObservation) {
        let mut scoped = observation
            .applications
            .drain(..)
            .find(|app| app.app_id() == &self.app);
        if let Some(AppObservation::Complete {
            focused, windows, ..
        }) = &mut scoped
        {
            windows.retain(|window| {
                self.authorized_window_ids.contains(&window.id.window_id)
                    || (!self.excluded_window_ids.contains(&window.id.window_id)
                        && window
                            .frame
                            .approximately_equals(self.anchor, REPLACEMENT_TOLERANCE))
            });
            let selected = focused
                .as_ref()
                .and_then(|focused| windows.iter().find(|window| window.id == *focused))
                .or_else(|| {
                    windows
                        .iter()
                        .find(|window| self.authorized_window_ids.contains(&window.id.window_id))
                })
                .or_else(|| (windows.len() == 1).then(|| &windows[0]))
                .map(|window| (window.id.clone(), window.frame));
            if let Some((selected, frame)) = selected {
                windows.retain(|window| window.id == selected);
                self.authorized_window_ids.insert(selected.window_id);
                self.anchor = frame;
                *focused = focused.take().filter(|focused| focused == &selected);
                self.missing_cycles = 0;
                if let Some(scoped) = scoped {
                    observation.applications = vec![scoped];
                }
                return;
            }
        }
        self.missing_cycles = self.missing_cycles.saturating_add(1);
        observation.applications = vec![if self.missing_cycles <= SETTLING_CYCLES {
            AppObservation::Unavailable {
                app: self.app.clone(),
                error: "scoped window is settling or absent".into(),
            }
        } else {
            AppObservation::Complete {
                app: ObservedApplication {
                    id: self.app.clone(),
                    bundle_id: self.bundle_id.clone(),
                },
                focused: None,
                windows: Vec::new(),
            }
        }];
    }
}

#[cfg(test)]
mod tests {
    use crate::{ObservedWindow, PhysicalWindowId};

    use super::*;

    fn app() -> AppInstanceId {
        AppInstanceId {
            pid: 10,
            launched_at: 1,
        }
    }

    fn window(window_id: u32, x: i32, y: i32) -> ObservedWindow {
        ObservedWindow {
            id: PhysicalWindowId {
                app: app(),
                window_id,
            },
            frame: Rect {
                x,
                y,
                width: 560,
                height: 400,
            },
            resizable: true,
            title: None,
            document: None,
        }
    }

    fn observation(windows: Vec<ObservedWindow>) -> NativeObservation {
        NativeObservation {
            cycle: 1,
            mouse_down: false,
            applications: vec![AppObservation::Complete {
                app: ObservedApplication {
                    id: app(),
                    bundle_id: "com.example.Editor".into(),
                },
                focused: None,
                windows,
            }],
        }
    }

    fn scoped_ids(observation: &NativeObservation) -> Vec<u32> {
        match &observation.applications[..] {
            [AppObservation::Complete { windows, .. }] => {
                windows.iter().map(|window| window.id.window_id).collect()
            }
            _ => Vec::new(),
        }
    }

    #[test]
    fn a_followed_window_stays_in_scope_after_moving_far_on_its_display() {
        let start = observation(vec![window(20, -1200, 0)]);
        let mut scope = WindowScope::discover(&start, 20, Rect::default()).unwrap();

        let mut moved = observation(vec![window(20, -880, 320)]);
        scope.filter(&mut moved);

        assert_eq!(scoped_ids(&moved), vec![20]);
        assert_eq!(scope.frame.x, -1200, "the probe's layout output stays put");
    }

    #[test]
    fn a_replacement_is_found_near_the_followed_windows_latest_frame() {
        let start = observation(vec![window(20, -1200, 0)]);
        let mut scope = WindowScope::discover(&start, 20, Rect::default()).unwrap();
        let mut moved = observation(vec![window(20, -880, 320)]);
        scope.filter(&mut moved);

        let mut replaced = observation(vec![window(30, -1200, 0), window(31, -870, 330)]);
        scope.filter(&mut replaced);

        assert_eq!(scoped_ids(&replaced), vec![31]);
    }
}
