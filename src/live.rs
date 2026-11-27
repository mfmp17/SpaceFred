pub mod window_scope;

use std::collections::{HashMap, HashSet};

use crate::{
    AppInstanceId, AppObservation, NativeObservation, Placement, Reconciliation, Settings, macos,
};

pub fn add_absent_applications(
    observation: &mut NativeObservation,
    known_apps: &mut HashMap<AppInstanceId, String>,
) {
    add_absent_applications_with(
        observation,
        known_apps,
        macos::is_process_incarnation_running,
        |_| true,
    );
}

pub fn add_absent_applications_with(
    observation: &mut NativeObservation,
    known_apps: &mut HashMap<AppInstanceId, String>,
    mut is_running: impl FnMut(&AppInstanceId) -> bool,
    mut has_window_server_evidence: impl FnMut(&AppInstanceId) -> bool,
) {
    let mut present = HashSet::with_capacity(observation.applications.len());
    for app in &observation.applications {
        present.insert(app.app_id().clone());
        if let AppObservation::Complete { app, .. } = app {
            known_apps.insert(app.id.clone(), app.bundle_id.clone());
        }
    }
    let missing: Vec<_> = known_apps
        .iter()
        .filter(|(app, _)| !present.contains(*app))
        .map(|(app, bundle_id)| (app.clone(), bundle_id.clone()))
        .collect();
    for (app, bundle_id) in missing {
        if !is_running(&app) {
            known_apps.remove(&app);
            observation
                .applications
                .push(AppObservation::Terminated { app });
        } else if has_window_server_evidence(&app) {
            observation.applications.push(AppObservation::Unavailable {
                app,
                error: "application was absent from the Core Graphics snapshot".into(),
            });
        } else {
            observation.applications.push(AppObservation::Complete {
                app: crate::ObservedApplication { id: app, bundle_id },
                focused: None,
                windows: Vec::new(),
            });
        }
    }
}

pub fn applicable_placements(
    observation: &NativeObservation,
    reconciliation: &Reconciliation,
    settings: &Settings,
) -> Vec<Placement> {
    let observed_frames = observed_frames(observation);
    reconciliation
        .placements
        .iter()
        .filter(|placement| {
            needs_application(placement, &observed_frames, settings.frame_tolerance)
        })
        .cloned()
        .collect()
}

pub fn observed_frames(
    observation: &NativeObservation,
) -> HashMap<crate::PhysicalWindowId, crate::Rect> {
    observation
        .applications
        .iter()
        .flat_map(|app| match app {
            AppObservation::Complete { windows, .. } => windows.as_slice(),
            AppObservation::Unavailable { .. } | AppObservation::Terminated { .. } => &[],
        })
        .map(|window| (window.id.clone(), window.frame))
        .collect()
}

#[must_use]
pub fn needs_application(
    placement: &Placement,
    observed_frames: &HashMap<crate::PhysicalWindowId, crate::Rect>,
    tolerance: i32,
) -> bool {
    placement
        .window
        .as_ref()
        .and_then(|window| observed_frames.get(window))
        .is_some_and(|frame| !frame.approximately_equals(placement.frame, tolerance))
}

#[cfg(test)]
mod tests {
    use crate::{
        Model, ObservedApplication, ObservedWindow, PhysicalWindowId, ReconciliationEvidence, Rect,
        reconcile,
    };

    use super::*;

    #[test]
    fn only_mismatched_observed_frames_need_application() {
        let app = AppInstanceId {
            pid: 10,
            launched_at: 1,
        };
        let window = PhysicalWindowId {
            app: app.clone(),
            window_id: 20,
        };
        let settings = Settings {
            output: Rect {
                width: 200,
                height: 100,
                ..Rect::default()
            },
            frame_tolerance: 2,
            spacing: crate::spacing::Spacing::NONE,
            layout: crate::config::LayoutMode::default(),
        };
        let mut observation = NativeObservation {
            cycle: 1,
            mouse_down: false,
            applications: vec![AppObservation::Complete {
                app: ObservedApplication {
                    id: app,
                    bundle_id: "com.example.Editor".into(),
                },
                focused: Some(window.clone()),
                windows: vec![ObservedWindow {
                    id: window,
                    frame: Rect {
                        width: 100,
                        height: 100,
                        ..Rect::default()
                    },
                    resizable: true,
                    title: None,
                    document: None,
                }],
            }],
        };
        let reconciliation = reconcile(
            &Model::default(),
            &observation,
            &ReconciliationEvidence::default(),
            &settings,
        );

        assert_eq!(
            applicable_placements(&observation, &reconciliation, &settings).len(),
            1
        );
        let AppObservation::Complete { windows, .. } = &mut observation.applications[0] else {
            unreachable!()
        };
        windows[0].frame = reconciliation.placements[0].frame;
        assert!(applicable_placements(&observation, &reconciliation, &settings).is_empty());
    }

    #[test]
    fn one_missing_snapshot_does_not_authoritatively_remove_a_running_apps_windows() {
        let app = AppInstanceId {
            pid: 10,
            launched_at: 1,
        };
        let window = PhysicalWindowId {
            app: app.clone(),
            window_id: 20,
        };
        let settings = Settings {
            output: Rect {
                width: 200,
                height: 100,
                ..Rect::default()
            },
            frame_tolerance: 2,
            spacing: crate::spacing::Spacing::NONE,
            layout: crate::config::LayoutMode::default(),
        };
        let initial = reconcile(
            &Model::default(),
            &NativeObservation {
                cycle: 1,
                mouse_down: false,
                applications: vec![AppObservation::Complete {
                    app: ObservedApplication {
                        id: app.clone(),
                        bundle_id: "com.example.Editor".into(),
                    },
                    focused: Some(window.clone()),
                    windows: vec![ObservedWindow {
                        id: window.clone(),
                        frame: settings.output,
                        resizable: true,
                        title: None,
                        document: None,
                    }],
                }],
            },
            &ReconciliationEvidence::default(),
            &settings,
        );
        let mut known = HashMap::from([(app.clone(), "com.example.Editor".into())]);
        let mut missing = NativeObservation {
            cycle: 2,
            mouse_down: false,
            applications: Vec::new(),
        };

        add_absent_applications_with(&mut missing, &mut known, |_| true, |_| true);
        let result = reconcile(
            &initial.model,
            &missing,
            &ReconciliationEvidence::default(),
            &settings,
        );

        assert!(matches!(
            missing.applications.as_slice(),
            [AppObservation::Unavailable { app: missing, .. }] if missing == &app
        ));
        assert_eq!(result.model.workspace.slots, initial.model.workspace.slots);
        assert_eq!(
            result.model.workspace.slots[0].bound_window(),
            Some(&window)
        );

        let mut confirmed_absent = NativeObservation {
            cycle: 3,
            mouse_down: false,
            applications: Vec::new(),
        };
        add_absent_applications_with(&mut confirmed_absent, &mut known, |_| true, |_| false);
        let result = reconcile(
            &initial.model,
            &confirmed_absent,
            &ReconciliationEvidence::default(),
            &settings,
        );

        assert!(matches!(
            confirmed_absent.applications.as_slice(),
            [AppObservation::Complete { windows, .. }] if windows.is_empty()
        ));
        assert!(result.model.workspace.slots.is_empty());
    }
}
