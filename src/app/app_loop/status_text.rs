//! How a worker cycle's outcome maps onto `last_error` (what `ctl status` and
//! Settings read) and the menu bar status line, kept in step with each other.

use super::*;

pub(super) fn is_update_failed_error(error: Option<&str>) -> bool {
    error.is_some_and(|error| error.starts_with(UPDATE_FAILED_PREFIX))
}

pub(super) fn is_shortcuts_not_applied_error(error: Option<&str>) -> bool {
    error.is_some_and(|error| error.starts_with(live_shortcuts::NOT_APPLIED))
}

/// Updates `last_error` when automatic tiling starts and returns the status text
/// to show in the menu bar. A failed update installation is unrelated to tiling
/// and survives the automatic resume that follows it; an unapplied shortcut table
/// also stays retained while the previous table remains active.
pub(super) fn automatic_started_status<'a>(
    last_error: &'a mut Option<String>,
    automatic_status: &'a str,
) -> &'a str {
    if is_update_failed_error(last_error.as_deref()) {
        last_error.as_deref().unwrap_or(automatic_status)
    } else {
        if !is_shortcuts_not_applied_error(last_error.as_deref()) {
            *last_error = None;
        }
        automatic_status
    }
}

pub(super) fn is_cancelled_error(error: &str) -> bool {
    error.ends_with("cancelled")
}

/// Drops a retained window-operation failure when tiling stops: it described the
/// last tiling cycle, so neither the paused menu nor `ctl status` should keep it.
pub(super) fn clear_window_operations_failed_error(last_error: &mut Option<String>) {
    if last_error
        .as_deref()
        .is_some_and(is_window_operations_failed_error)
    {
        *last_error = None;
    }
}

/// Records a worker error that arrived after automatic tiling paused, ignoring
/// routine cancellations caused by pausing or superseding input.
pub(super) fn record_paused_worker_error(last_error: &mut Option<String>, error: String) -> bool {
    if is_cancelled_error(&error) {
        return false;
    }
    *last_error = Some(error);
    true
}

/// Applies a worker cycle result to `last_error` and returns the status line for
/// the menu bar (`None` hides the status item).
pub(super) fn worker_status_update(
    last_error: &mut Option<String>,
    keep_shortcuts: bool,
    result: Result<usize, &str>,
) -> Option<String> {
    match result {
        Err(error) if is_cancelled_error(error) => {
            // Routine cancellation is not a failure: keep any retained status
            // error intact instead of overwriting it or flashing an error. A
            // retained window-operation failure stays in the menu too, so the
            // menu and `ctl status` agree (#694).
            let retained = last_error.as_deref().is_some_and(|error| {
                is_update_failed_error(Some(error)) || is_window_operations_failed_error(error)
            });
            retained.then(|| last_error.clone()).flatten()
        }
        Err(error) => {
            *last_error = Some(error.to_owned());
            Some(error.to_owned())
        }
        Ok(failed) => {
            let keep_update = is_update_failed_error(last_error.as_deref());
            let keep = keep_shortcuts || keep_update;
            if failed > 0 {
                let status = window_operations_failed_status(failed);
                // `ctl status` must report the same partial failure the menu
                // shows (#694); a retained update or shortcut error wins.
                if !keep {
                    *last_error = Some(status.clone());
                }
                return Some(status);
            }
            if !keep {
                *last_error = None;
            }
            if keep_update {
                last_error.clone()
            } else {
                None
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn automatic_resume_and_clean_cycles_preserve_update_install_failure() {
        let reason = format!("{UPDATE_FAILED_PREFIX}could not write update receipt");
        let mut last_error = Some(reason.clone());

        let resumed_status = automatic_started_status(
            &mut last_error,
            "Automatic tiling enabled · Workspaces 1 / 6",
        );
        assert_eq!(resumed_status, reason);
        assert_eq!(last_error.as_deref(), Some(reason.as_str()));

        let cycle_status = worker_status_update(&mut last_error, false, Ok(0));
        assert_eq!(cycle_status.as_deref(), Some(reason.as_str()));
        assert_eq!(last_error.as_deref(), Some(reason.as_str()));

        let failed_ops_status = worker_status_update(&mut last_error, false, Ok(2));
        assert_eq!(
            failed_ops_status.as_deref(),
            Some("2 window operations failed")
        );
        assert_eq!(last_error.as_deref(), Some(reason.as_str()));
    }

    #[test]
    fn automatic_start_and_clean_cycles_still_clear_non_update_errors() {
        let mut last_error = Some("Accessibility permission is required".to_owned());
        let status = automatic_started_status(
            &mut last_error,
            "Automatic tiling enabled · Workspaces 1 / 6",
        );
        assert_eq!(status, "Automatic tiling enabled · Workspaces 1 / 6");
        assert_eq!(last_error, None);

        let mut worker_error = Some("Workspace 2 could not restore all of its windows".to_owned());
        assert_eq!(worker_status_update(&mut worker_error, false, Ok(0)), None);
        assert_eq!(worker_error, None);

        let mut shortcuts_error = Some("Shortcuts not applied: conflict".to_owned());
        assert_eq!(
            automatic_started_status(
                &mut shortcuts_error,
                "Automatic tiling enabled · Workspaces 1 / 6",
            ),
            "Automatic tiling enabled · Workspaces 1 / 6"
        );
        assert_eq!(
            shortcuts_error.as_deref(),
            Some("Shortcuts not applied: conflict")
        );
        assert_eq!(
            worker_status_update(&mut shortcuts_error, true, Ok(0)),
            None
        );
        assert_eq!(
            shortcuts_error.as_deref(),
            Some("Shortcuts not applied: conflict")
        );
    }

    #[test]
    fn failed_window_operations_reach_last_error_until_a_clean_cycle() {
        let mut last_error = None;
        assert_eq!(
            worker_status_update(&mut last_error, false, Ok(1)).as_deref(),
            Some("1 window operation failed")
        );
        assert_eq!(last_error.as_deref(), Some("1 window operation failed"));

        assert_eq!(worker_status_update(&mut last_error, false, Ok(0)), None);
        assert_eq!(last_error, None);

        let mut shortcuts_error = Some("Shortcuts not applied: conflict".to_owned());
        assert_eq!(
            worker_status_update(&mut shortcuts_error, true, Ok(3)).as_deref(),
            Some("3 window operations failed")
        );
        assert_eq!(
            shortcuts_error.as_deref(),
            Some("Shortcuts not applied: conflict")
        );
    }

    #[test]
    fn window_operation_failures_are_recognized_and_cleared_when_tiling_stops() {
        assert!(is_window_operations_failed_error(
            &window_operations_failed_status(1)
        ));
        assert!(is_window_operations_failed_error(
            &window_operations_failed_status(12)
        ));
        assert!(!is_window_operations_failed_error(
            "window operation failed"
        ));
        assert!(!is_window_operations_failed_error(
            "Workspace 2 could not restore all of its windows"
        ));

        let mut last_error = None;
        worker_status_update(&mut last_error, false, Ok(1));
        clear_window_operations_failed_error(&mut last_error);
        assert_eq!(last_error, None);

        let reason = format!("{UPDATE_FAILED_PREFIX}could not write update receipt");
        let mut update_error = Some(reason.clone());
        clear_window_operations_failed_error(&mut update_error);
        assert_eq!(update_error.as_deref(), Some(reason.as_str()));
    }

    #[test]
    fn routine_cancellations_keep_a_retained_operation_failure_in_both_surfaces() {
        let mut last_error = None;
        worker_status_update(&mut last_error, false, Ok(2));
        assert_eq!(
            worker_status_update(&mut last_error, false, Err("workspace operation cancelled"))
                .as_deref(),
            Some("2 window operations failed")
        );
        assert_eq!(last_error.as_deref(), Some("2 window operations failed"));
    }

    #[test]
    fn routine_cancellations_preserve_retained_errors_and_never_store_cancelled() {
        let reason = format!("{UPDATE_FAILED_PREFIX}could not write update receipt");
        let mut last_error = Some(reason.clone());

        // Routine cancellation while automatic preserves the retained install failure.
        assert_eq!(
            worker_status_update(&mut last_error, false, Err("workspace operation cancelled")),
            Some(reason.clone())
        );
        assert_eq!(last_error.as_deref(), Some(reason.as_str()));

        // The following clean cycle still retains the install failure.
        assert_eq!(
            worker_status_update(&mut last_error, false, Ok(0)),
            Some(reason.clone())
        );
        assert_eq!(last_error.as_deref(), Some(reason.as_str()));

        // Routine cancellation arriving after pause does not overwrite or populate last_error.
        assert!(!record_paused_worker_error(
            &mut last_error,
            "workspace operation cancelled".into()
        ));
        assert_eq!(last_error.as_deref(), Some(reason.as_str()));

        let mut empty = None;
        assert_eq!(
            worker_status_update(&mut empty, false, Err("workspace switch cancelled")),
            None
        );
        assert_eq!(empty, None);
        assert!(!record_paused_worker_error(
            &mut empty,
            "workspace operation cancelled".into()
        ));
        assert_eq!(empty, None);

        // Genuine failures still report and store their error in both states.
        assert_eq!(
            worker_status_update(&mut empty, false, Err("Observation error: unavailable")),
            Some("Observation error: unavailable".into())
        );
        assert_eq!(empty.as_deref(), Some("Observation error: unavailable"));
        assert!(record_paused_worker_error(
            &mut empty,
            "Restore error".into()
        ));
        assert_eq!(empty.as_deref(), Some("Restore error"));
    }
}
