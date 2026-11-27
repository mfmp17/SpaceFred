use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
    ptr::NonNull,
    time::Duration,
};

use objc2::rc::autoreleasepool;
use objc2_app_kit::{NSApplicationActivationOptions, NSApplicationActivationPolicy};
use objc2_application_services::{AXError, AXUIElement};
use objc2_core_foundation::{CFArray, CFBoolean, CFRetained, CFString, CGPoint};
use objc2_core_graphics::CGWindowID;

use crate::{AppInstanceId, PhysicalWindowId, Placement, Rect, Settings};

use super::{
    WindowObservation,
    ax::{
        application_element, attribute, cf_array_items, is_non_standard_window_element,
        onscreen_windows, optional_bool_attribute, physical_window_id, set_attribute,
        set_ax_timeout, set_position, set_size, window_frame,
    },
    mouse_down, observe_window, process_incarnation_matches, running_application,
};

mod frame;

use frame::apply_element;

#[derive(Debug, serde::Serialize)]
pub struct ApplyResult {
    pub window: PhysicalWindowId,
    pub target: Rect,
    pub actual: Option<Rect>,
    pub outcome: ApplyOutcome,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ApplyOutcome {
    Applied,
    Constrained,
    Gone,
    Unavailable,
    Deferred,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WindowAccessError {
    Gone,
    Unavailable,
}

thread_local! {
    static MANAGED_WINDOWS: RefCell<HashMap<PhysicalWindowId, CFRetained<AXUIElement>>> =
        RefCell::new(HashMap::new());
}

pub fn apply(placements: &[Placement], settings: &Settings) -> Vec<ApplyResult> {
    apply_while(placements, settings, || true)
}

pub fn apply_while(
    placements: &[Placement],
    settings: &Settings,
    mut should_continue: impl FnMut() -> bool,
) -> Vec<ApplyResult> {
    autoreleasepool(|_| {
        let onscreen = onscreen_windows().unwrap_or_default();
        placements
            .iter()
            .take_while(|_| should_continue())
            .filter_map(|placement| {
                placement.window.as_ref().map(|window| {
                    apply_window(window, placement.frame, settings, &onscreen).unwrap_or_else(
                        || ApplyResult {
                            window: window.clone(),
                            target: placement.frame,
                            actual: None,
                            outcome: ApplyOutcome::Gone,
                        },
                    )
                })
            })
            .collect()
    })
}

pub fn apply_managed(
    placements: &[Placement],
    settings: &Settings,
    dragged: Option<&PhysicalWindowId>,
    mut should_continue: impl FnMut() -> bool,
) -> Vec<ApplyResult> {
    autoreleasepool(|_| {
        let placements: Vec<_> = placements
            .iter()
            .take_while(|_| should_continue())
            .filter_map(|placement| {
                placement
                    .window
                    .as_ref()
                    .map(|window| (window, placement.frame))
            })
            .collect();
        if mouse_down() && dragged.is_none() {
            return placements
                .into_iter()
                .map(|(window, target)| ApplyResult {
                    window: window.clone(),
                    target,
                    actual: None,
                    outcome: ApplyOutcome::Deferred,
                })
                .collect();
        }
        let groups = group_placements_by_app(&placements);
        let mut results: Vec<Option<ApplyResult>> = (0..placements.len()).map(|_| None).collect();
        'applications: for (app_id, indices) in groups {
            if !should_continue() {
                break;
            }
            if !process_incarnation_matches(&app_id) {
                evict_placed_windows(&placements, &indices);
                fill_missing_results(&placements, &indices, &mut results, ApplyOutcome::Gone);
                continue;
            }
            let app = application_element(app_id.pid);
            let windows = match attribute::<CFArray>(&app, "AXWindows") {
                Ok(windows) => windows,
                Err(AXError::InvalidUIElement) => {
                    evict_placed_windows(&placements, &indices);
                    fill_missing_results(&placements, &indices, &mut results, ApplyOutcome::Gone);
                    continue;
                }
                Err(_) => {
                    fill_missing_results(
                        &placements,
                        &indices,
                        &mut results,
                        ApplyOutcome::Unavailable,
                    );
                    continue;
                }
            };
            let _animations = WindowAnimationGuard::new(&app);
            let mut pending = HashMap::<u32, Vec<usize>>::new();
            for index in &indices {
                pending
                    .entry(placements[*index].0.window_id)
                    .or_default()
                    .push(*index);
            }
            let mut lookup_failed = false;
            for window in cf_array_items::<AXUIElement>(&windows) {
                set_ax_timeout(window);
                let Some(window_id) = physical_window_id(&app_id, window) else {
                    if !is_non_standard_window_element(window) {
                        lookup_failed = true;
                    }
                    continue;
                };
                let Some(placement_indices) = pending.remove(&window_id.window_id) else {
                    continue;
                };
                for placement_index in placement_indices {
                    if !should_continue() {
                        break 'applications;
                    }
                    let (id, target) = placements[placement_index];
                    cache_managed_window(id, window);
                    results[placement_index] = Some(if dragged == Some(id) {
                        ApplyResult {
                            window: id.clone(),
                            target,
                            actual: window_frame(window),
                            outcome: ApplyOutcome::Deferred,
                        }
                    } else {
                        apply_element(id, window, target, settings)
                    });
                }
                if pending.is_empty() {
                    break;
                }
            }
            // `AXWindows` lists only the showing desktop's windows, so a window
            // missing from it may live on another macOS desktop. Its cached
            // handle still reaches it: a rolled-back switch parks it again
            // through that handle (#775). A truly closed window's handle is
            // dropped the next time an operation on it fails.
            fill_missing_results(
                &placements,
                &pending.into_values().flatten().collect::<Vec<_>>(),
                &mut results,
                if lookup_failed {
                    ApplyOutcome::Unavailable
                } else {
                    ApplyOutcome::Gone
                },
            );
        }
        results.into_iter().flatten().collect()
    })
}

pub(super) fn group_placements_by_app(
    placements: &[(&PhysicalWindowId, Rect)],
) -> Vec<(AppInstanceId, Vec<usize>)> {
    let mut groups = Vec::<(AppInstanceId, Vec<usize>)>::new();
    let mut group_indices = HashMap::<AppInstanceId, usize>::new();
    for (index, (window, _)) in placements.iter().enumerate() {
        let group = *group_indices.entry(window.app.clone()).or_insert_with(|| {
            groups.push((window.app.clone(), Vec::new()));
            groups.len() - 1
        });
        groups[group].1.push(index);
    }
    groups
}

/// Drop the cached handles of windows whose app is gone or relaunched.
fn evict_placed_windows(placements: &[(&PhysicalWindowId, Rect)], indices: &[usize]) {
    for index in indices {
        evict_managed_window(placements[*index].0);
    }
}

fn fill_missing_results(
    placements: &[(&PhysicalWindowId, Rect)],
    indices: &[usize],
    results: &mut [Option<ApplyResult>],
    outcome: ApplyOutcome,
) {
    for index in indices {
        let (window, target) = placements[*index];
        results[*index] = Some(ApplyResult {
            window: window.clone(),
            target,
            actual: None,
            outcome,
        });
    }
}

pub fn frame(id: &PhysicalWindowId) -> Result<Rect, WindowAccessError> {
    with_managed_window_result(id, |window| {
        window_frame(window).ok_or(WindowAccessError::Unavailable)
    })
}

/// What window rules match on, for a window no observation lists (one an app reopened at a
/// parking spot). Its title stays in memory only; never log it.
pub struct WindowRuleProbe {
    pub bundle_id: String,
    /// The window's title, `None` when it has none or Accessibility could not read it.
    pub title: Option<std::sync::Arc<str>>,
    /// Whether Accessibility resolved the window, so a missing title is an answer.
    pub resolved: bool,
}

/// Reads `id`'s app bundle ID and window title for rule matching.
#[must_use]
pub fn window_rule_probe(id: &PhysicalWindowId) -> Option<WindowRuleProbe> {
    let bundle_id = autoreleasepool(|_| {
        running_application(&id.app)?
            .bundleIdentifier()
            .map(|value| value.to_string())
    })?;
    let read = with_managed_window_result(id, |window| {
        interpret_ax_title(attribute::<CFString>(window, "AXTitle"))
    });
    Some(WindowRuleProbe {
        bundle_id,
        resolved: read.is_ok(),
        title: read.ok().flatten(),
    })
}

fn interpret_ax_title(
    result: Result<CFRetained<CFString>, AXError>,
) -> Result<Option<std::sync::Arc<str>>, WindowAccessError> {
    match result {
        Ok(title) => Ok(Some(std::sync::Arc::from(title.to_string()))),
        Err(AXError::NoValue | AXError::AttributeUnsupported) => Ok(None),
        Err(_) => Err(WindowAccessError::Unavailable),
    }
}

pub fn set_position_only(id: &PhysicalWindowId, target: Rect) -> ApplyOutcome {
    write_position(id, target, position_matches)
}

/// Move a parked window back to its recovery frame `target`: position first,
/// then the recorded size if an app changed it while parked. The size write is
/// best effort (apps may clamp it); arrival is confirmed by position alone.
pub fn set_restore_frame(id: &PhysicalWindowId, target: Rect) -> ApplyOutcome {
    write_window(
        id,
        target,
        |window| {
            let result = set_position(window, target);
            if window_frame(window).is_some_and(|actual| {
                actual.width != target.width || actual.height != target.height
            }) {
                let _: AXError = set_size(window, target);
            }
            result
        },
        position_matches,
    )
}

/// Park a window at `target`, a [`crate::geometry::parked_frame`] position.
/// macOS may hold it above the requested corner to keep its title bar on
/// screen, which still parks it.
pub fn set_parked_position(id: &PhysicalWindowId, target: Rect) -> ApplyOutcome {
    write_position(id, target, |actual, target| {
        crate::geometry::parked_at(actual, target, POSITION_CONFIRM_TOLERANCE)
    })
}

fn write_position(
    id: &PhysicalWindowId,
    target: Rect,
    confirmed: impl Fn(Rect, Rect) -> bool,
) -> ApplyOutcome {
    write_window(id, target, |window| set_position(window, target), confirmed)
}

fn write_window(
    id: &PhysicalWindowId,
    target: Rect,
    write: impl Fn(&AXUIElement) -> AXError,
    confirmed: impl Fn(Rect, Rect) -> bool,
) -> ApplyOutcome {
    match with_managed_window_result(id, |window| {
        let result = without_window_animations(&id.app, || write(window));
        if window_frame(window).is_some_and(|actual| confirmed(actual, target)) {
            Ok(())
        } else if matches!(result, AXError::InvalidUIElement) {
            Err(WindowAccessError::Gone)
        } else {
            Err(WindowAccessError::Unavailable)
        }
    }) {
        Ok(()) => ApplyOutcome::Applied,
        Err(WindowAccessError::Gone) => ApplyOutcome::Gone,
        Err(WindowAccessError::Unavailable) => ApplyOutcome::Unavailable,
    }
}

pub(super) fn position_matches(actual: Rect, target: Rect) -> bool {
    actual.position_approximately_equals(target, POSITION_CONFIRM_TOLERANCE)
}

const POSITION_CONFIRM_TOLERANCE: i32 = 64;

#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {
    fn CGWarpMouseCursorPosition(new_cursor_position: CGPoint) -> i32;
    fn CGAssociateMouseAndMouseCursorPosition(connected: libc::boolean_t) -> i32;
}

/// Move the mouse pointer to `point` in global display coordinates and
/// immediately re-associate mouse movement so macOS does not suppress real
/// input for 250 ms afterwards. Never warps while a mouse button is held.
#[must_use]
pub fn warp_pointer(point: (i32, i32)) -> bool {
    let target = CGPoint::new(f64::from(point.0), f64::from(point.1));
    warp_sequence(
        super::any_mouse_button_down,
        || {
            // SAFETY: `CGWarpMouseCursorPosition` takes a plain `CGPoint` by
            // value and does not retain pointers or require AppKit main-thread state.
            unsafe { CGWarpMouseCursorPosition(target) == 0 }
        },
        || {
            // SAFETY: `CGAssociateMouseAndMouseCursorPosition` takes a plain
            // `boolean_t` (`1` for true) to re-enable mouse-to-cursor coupling.
            let _: i32 = unsafe { CGAssociateMouseAndMouseCursorPosition(1) };
        },
    )
}

fn warp_sequence(
    is_mouse_down: impl FnOnce() -> bool,
    warp: impl FnOnce() -> bool,
    reassociate: impl FnOnce(),
) -> bool {
    if is_mouse_down() {
        return false;
    }
    let warped = warp();
    reassociate();
    warped
}

/// Focus a window in another app from the background.
///
/// Since macOS 14, `activateWithOptions` from an inactive app is cooperative:
/// Spaceman is a background accessory when a global hotkey fires, so macOS
/// ignores it and the window comes forward while keys stay in the app focus
/// left (#92). Setting `AXFrontmost` on the app is the public Accessibility
/// route an assistive app is granted, so it brings the app forward; the AppKit
/// call stays as reinforcement for releases before macOS 14.
pub fn focus_window(id: &PhysicalWindowId) -> bool {
    let focused = focus_window_once(id);
    if focused {
        confirm_frontmost(
            || super::frontmost_application().is_some_and(|app| app.pid == id.app.pid),
            || {
                let _: bool = focus_window_once(id);
            },
            std::thread::sleep,
        );
    }
    focused
}

const FRONTMOST_POLL: Duration = Duration::from_millis(10);
const FRONTMOST_REASSERT_EVERY: u32 = 5;
const FRONTMOST_REASSERTS: u32 = 3;

/// Some apps (Arc) answer `AXFrontmost` with success yet stay behind while the
/// app focus left keeps the keyboard. Poll the frontmost app briefly and
/// repeat the focus every `FRONTMOST_REASSERT_EVERY` polls, at most
/// `FRONTMOST_REASSERTS` times. Returns how many repeats were needed.
fn confirm_frontmost(
    mut is_frontmost: impl FnMut() -> bool,
    mut reassert: impl FnMut(),
    mut sleep: impl FnMut(Duration),
) -> u32 {
    let mut reasserts = 0;
    for poll in 1..=FRONTMOST_REASSERT_EVERY * (FRONTMOST_REASSERTS + 1) {
        sleep(FRONTMOST_POLL);
        if is_frontmost() {
            break;
        }
        if poll % FRONTMOST_REASSERT_EVERY == 0 && reasserts < FRONTMOST_REASSERTS {
            reasserts += 1;
            reassert();
        }
    }
    reasserts
}

fn focus_window_once(id: &PhysicalWindowId) -> bool {
    autoreleasepool(|_| {
        focus_sequence(
            || raise_window_with_focus(id, true),
            || {
                set_attribute(
                    &application_element(id.app.pid),
                    "AXFrontmost",
                    CFBoolean::new(true),
                )
            },
            || {
                running_application(&id.app).is_some_and(|application| {
                    #[allow(deprecated)]
                    let options = NSApplicationActivationOptions::ActivateIgnoringOtherApps;
                    application.activateWithOptions(options)
                })
            },
        )
    })
}

/// Raise the target window and make it main first, so the app brings that
/// window (not whichever of its windows was main before) forward when it
/// activates; then bring the app forward both ways. Either app step is
/// enough: `AXFrontmost` is the one macOS 14 and later honour, the AppKit
/// call the one older releases do.
fn focus_sequence(
    raise: impl FnOnce() -> bool,
    bring_frontmost: impl FnOnce() -> AXError,
    activate: impl FnOnce() -> bool,
) -> bool {
    let raised = raise();
    let frontmost = bring_frontmost() == AXError::Success;
    let activated = activate();
    raised && (frontmost || activated)
}

pub fn raise_window(id: &PhysicalWindowId) -> bool {
    raise_window_with_focus(id, false)
}

fn raise_window_with_focus(id: &PhysicalWindowId, focus: bool) -> bool {
    with_managed_window_result(id, |window| {
        if focus {
            // Best-effort attributes before `AXRaise`; some apps ignore them.
            // `AXMain` picks which of the app's windows becomes key when the
            // app activates.
            let _: AXError = set_attribute(window, "AXMain", CFBoolean::new(true));
            let _: AXError = set_attribute(window, "AXFocused", CFBoolean::new(true));
        }
        // SAFETY: `window` is a live `AXUIElement` and `"AXRaise"` is a standard AX action.
        if (unsafe { window.perform_action(&CFString::from_static_str("AXRaise")) })
            == AXError::Success
        {
            Ok(())
        } else {
            Err(WindowAccessError::Unavailable)
        }
    })
    .is_ok()
}

fn apply_window(
    id: &PhysicalWindowId,
    target: Rect,
    settings: &Settings,
    onscreen: &HashMap<i32, HashMap<CGWindowID, i64>>,
) -> Option<ApplyResult> {
    if mouse_down() {
        return Some(ApplyResult {
            window: id.clone(),
            target,
            actual: None,
            outcome: ApplyOutcome::Deferred,
        });
    }
    let Some(application) = running_application(&id.app) else {
        return Some(ApplyResult {
            window: id.clone(),
            target,
            actual: None,
            outcome: ApplyOutcome::Gone,
        });
    };
    if !onscreen
        .get(&id.app.pid)
        .is_some_and(|windows| windows.contains_key(&id.window_id))
    {
        return Some(ApplyResult {
            window: id.clone(),
            target,
            actual: None,
            outcome: ApplyOutcome::Gone,
        });
    }
    let accessory = application.activationPolicy() == NSApplicationActivationPolicy::Accessory;
    let app = application_element(id.app.pid);
    let windows = attribute::<CFArray>(&app, "AXWindows").ok()?;
    for window in cf_array_items::<AXUIElement>(&windows) {
        set_ax_timeout(window);
        if physical_window_id(&id.app, window).as_ref() != Some(id) {
            continue;
        }
        if !matches!(
            observe_window(
                id.clone(),
                accessory,
                window,
                onscreen.get(&id.app.pid)?,
                settings.output,
            ),
            Ok(WindowObservation::Observed(_))
        ) {
            return Some(ApplyResult {
                window: id.clone(),
                target,
                actual: window_frame(window),
                outcome: ApplyOutcome::Unavailable,
            });
        }

        let _animations = WindowAnimationGuard::new(&app);
        return Some(apply_element(id, window, target, settings));
    }
    None
}

fn without_window_animations<T>(app: &AppInstanceId, operation: impl FnOnce() -> T) -> T {
    let application = application_element(app.pid);
    let _animations = WindowAnimationGuard::new(&application);
    operation()
}

struct WindowAnimationGuard<'a> {
    application: &'a AXUIElement,
    restore_enhanced_ui: bool,
}

impl<'a> WindowAnimationGuard<'a> {
    fn new(application: &'a AXUIElement) -> Self {
        let restore_enhanced_ui = optional_bool_attribute(application, "AXEnhancedUserInterface")
            .ok()
            .unwrap_or(false);
        if restore_enhanced_ui {
            let _: AXError = set_attribute(
                application,
                "AXEnhancedUserInterface",
                CFBoolean::new(false),
            );
        }
        Self {
            application,
            restore_enhanced_ui,
        }
    }
}

impl Drop for WindowAnimationGuard<'_> {
    fn drop(&mut self) {
        if self.restore_enhanced_ui {
            let _: AXError = set_attribute(
                self.application,
                "AXEnhancedUserInterface",
                CFBoolean::new(true),
            );
        }
    }
}

/// How resolving a window afresh through its app's `AXWindows` went, after an
/// operation on its cached handle failed.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WindowLookup {
    /// The app did not answer (busy or hung); nothing shows the handle is stale.
    Unanswered,
    /// The app is gone, relaunched, or no longer lists the window.
    Missing,
    /// The app listed the window, so its fresh handle replaces the cached one.
    Found,
}

/// Whether a cached handle whose operation failed stays cached after `lookup`.
/// A handle can go stale while the window lives on (an app rebuilds its
/// accessibility tree), so it is dropped once the app answers. An app that does
/// not answer keeps it: writes sent on it still land, in order, when the app
/// resumes, and without it no later write (a rolled-back switch's park, #722)
/// could reach the app at all until it answers again.
#[must_use]
pub fn keeps_failed_handle(lookup: WindowLookup) -> bool {
    lookup == WindowLookup::Unanswered
}

fn with_managed_window_result<T>(
    id: &PhysicalWindowId,
    mut operation: impl FnMut(&AXUIElement) -> Result<T, WindowAccessError>,
) -> Result<T, WindowAccessError> {
    let cached = MANAGED_WINDOWS.with(|windows| windows.borrow().get(id).cloned());
    if let Some(window) = &cached
        && let Ok(value) = operation(window)
    {
        return Ok(value);
    }
    // Resolve the window afresh once before reporting it unavailable or gone.
    let (lookup, result) = resolve_managed_window(id, &mut operation);
    if cached.is_some() && !keeps_failed_handle(lookup) && result.is_err() {
        evict_managed_window(id);
    }
    result
}

fn resolve_managed_window<T>(
    id: &PhysicalWindowId,
    operation: &mut impl FnMut(&AXUIElement) -> Result<T, WindowAccessError>,
) -> (WindowLookup, Result<T, WindowAccessError>) {
    if !process_incarnation_matches(&id.app) {
        evict_managed_app(&id.app);
        return (WindowLookup::Missing, Err(WindowAccessError::Gone));
    }
    let app = application_element(id.app.pid);
    let windows = match attribute::<CFArray>(&app, "AXWindows") {
        Ok(windows) => windows,
        Err(AXError::InvalidUIElement) => {
            return (WindowLookup::Missing, Err(WindowAccessError::Gone));
        }
        Err(_) => {
            return (
                WindowLookup::Unanswered,
                Err(WindowAccessError::Unavailable),
            );
        }
    };
    for window in cf_array_items::<AXUIElement>(&windows) {
        set_ax_timeout(window);
        if physical_window_id(&id.app, window).as_ref() == Some(id) {
            let result = operation(window);
            if result.is_ok() {
                cache_managed_window(id, window);
            }
            return (WindowLookup::Found, result);
        }
    }
    (WindowLookup::Missing, Err(WindowAccessError::Gone))
}

/// Reads `AXMinimized` from the cached handle only, with a short timeout, so
/// an unresponsive app cannot hold up the cycle; no handle means no evidence.
pub(super) fn cached_window_is_minimized(id: &PhysicalWindowId) -> bool {
    cached_window_flag(id, "AXMinimized")
}

/// Whether the cached window is in macOS native fullscreen (`AXFullScreen`).
pub(super) fn cached_window_is_fullscreen(id: &PhysicalWindowId) -> bool {
    cached_window_flag(id, "AXFullScreen")
}

fn cached_window_flag(id: &PhysicalWindowId, attribute: &'static str) -> bool {
    let Some(window) = MANAGED_WINDOWS.with(|windows| windows.borrow().get(id).cloned()) else {
        return false;
    };
    // SAFETY: `window` is a live cached `AXUIElement`; the timeout is restored below.
    let _: AXError = unsafe { window.set_messaging_timeout(0.05) };
    let value = optional_bool_attribute(&window, attribute).unwrap_or(false);
    set_ax_timeout(&window);
    value
}

pub(super) fn cache_managed_window(id: &PhysicalWindowId, window: &AXUIElement) {
    // SAFETY: `window` is a valid `AXUIElement` reference backed by Core Foundation refcounting.
    let retained = unsafe { CFRetained::retain(NonNull::from(window)) };
    MANAGED_WINDOWS.with(|windows| {
        windows.borrow_mut().insert(id.clone(), retained);
    });
}

fn evict_managed_window(id: &PhysicalWindowId) {
    MANAGED_WINDOWS.with(|windows| {
        windows.borrow_mut().remove(id);
    });
}

pub(super) fn evict_managed_app(app: &AppInstanceId) {
    MANAGED_WINDOWS.with(|windows| {
        windows.borrow_mut().retain(|window, _| &window.app != app);
    });
}

pub(super) fn prune_managed_app(app: &AppInstanceId, current: &HashSet<PhysicalWindowId>) {
    MANAGED_WINDOWS.with(|windows| {
        windows
            .borrow_mut()
            .retain(|window, _| keep_cached_window(window, app, current));
    });
}

fn keep_cached_window(
    window: &PhysicalWindowId,
    app: &AppInstanceId,
    current: &HashSet<PhysicalWindowId>,
) -> bool {
    if window.app == *app {
        current.contains(window)
    } else {
        window.app.pid != app.pid || window.app.launched_at > app.launched_at
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use std::collections::HashSet;

    use objc2_application_services::AXError;
    use objc2_core_foundation::CFString;

    use super::{
        ApplyOutcome, FRONTMOST_POLL, FRONTMOST_REASSERTS, WindowAccessError, WindowLookup,
        confirm_frontmost, focus_sequence,
        frame::{FrameWrite, arrived_short, frame_outcome, write_frame},
        interpret_ax_title, keep_cached_window, keeps_failed_handle, warp_sequence,
    };
    use crate::{AppInstanceId, PhysicalWindowId, Rect, Settings};

    /// #722: a busy app keeps its window's handle, so a park written after a
    /// timed-out restore is still sent and lands behind it; once the app
    /// answers, a failed handle is dropped as before.
    #[test]
    fn a_failed_handle_is_kept_only_while_its_app_does_not_answer() {
        assert!(keeps_failed_handle(WindowLookup::Unanswered));
        assert!(!keeps_failed_handle(WindowLookup::Missing));
        assert!(!keeps_failed_handle(WindowLookup::Found));
    }

    #[test]
    fn a_window_from_another_display_that_lands_short_is_sized_again() {
        // #428: center display above, a taller portrait display to its left.
        let center = Rect {
            x: 0,
            y: 30,
            width: 2560,
            height: 1410,
        };
        let portrait = Rect {
            x: -1440,
            y: -583,
            width: 1440,
            height: 2530,
        };
        let settings = Settings {
            output: portrait,
            frame_tolerance: crate::reconcile::DEFAULT_FRAME_TOLERANCE,
            spacing: crate::spacing::Spacing::default(),
            layout: crate::config::LayoutMode::default(),
        };
        let clamped = Rect {
            height: 2023,
            ..portrait
        };
        assert!(arrived_short(
            Some(center),
            portrait,
            Some(clamped),
            &settings
        ));
        // Already on the display: a short frame is the app's own answer.
        assert!(!arrived_short(
            Some(portrait),
            portrait,
            Some(clamped),
            &settings
        ));
        // A full-size landing, or one that did not reach its place, needs nothing.
        assert!(!arrived_short(
            Some(center),
            portrait,
            Some(portrait),
            &settings
        ));
        let elsewhere = Rect { x: 0, ..clamped };
        assert!(!arrived_short(
            Some(center),
            portrait,
            Some(elsewhere),
            &settings
        ));
        assert!(!arrived_short(None, portrait, Some(clamped), &settings));
    }

    #[test]
    fn focus_raises_the_window_before_bringing_its_app_forward() {
        let calls = RefCell::new(Vec::new());

        let focused = focus_sequence(
            || {
                calls.borrow_mut().push("raise");
                true
            },
            || {
                calls.borrow_mut().push("frontmost");
                AXError::Success
            },
            || {
                calls.borrow_mut().push("activate");
                false
            },
        );

        assert!(focused);
        assert_eq!(*calls.borrow(), ["raise", "frontmost", "activate"]);
    }

    #[test]
    fn either_app_step_brings_the_app_forward() {
        // macOS 14+ ignores the AppKit call from a background app.
        assert!(focus_sequence(|| true, || AXError::Success, || false));
        // An app that refuses `AXFrontmost` but honours AppKit.
        assert!(focus_sequence(|| true, || AXError::CannotComplete, || true));
    }

    #[test]
    fn focus_fails_when_the_app_stays_behind_or_the_window_cannot_raise() {
        assert!(!focus_sequence(
            || true,
            || AXError::CannotComplete,
            || false
        ));
        assert!(!focus_sequence(|| false, || AXError::Success, || true));
    }

    #[test]
    fn an_app_that_comes_forward_needs_no_repeat() {
        let polls = RefCell::new(0);
        let repeats = confirm_frontmost(
            || {
                *polls.borrow_mut() += 1;
                *polls.borrow() >= 2
            },
            || panic!("no repeat expected"),
            |_| {},
        );
        assert_eq!(repeats, 0);
        assert_eq!(*polls.borrow(), 2);
    }

    #[test]
    fn an_app_that_stays_behind_gets_the_focus_repeated() {
        // Arc accepts `AXFrontmost` and stays behind; a repeat brings it forward.
        let repeats = RefCell::new(0);
        let count = confirm_frontmost(
            || *repeats.borrow() >= 1,
            || *repeats.borrow_mut() += 1,
            |_| {},
        );
        assert_eq!(count, 1);
    }

    #[test]
    fn an_app_that_never_comes_forward_stops_after_a_bounded_wait() {
        let waited = RefCell::new(std::time::Duration::ZERO);
        let count = confirm_frontmost(|| false, || {}, |step| *waited.borrow_mut() += step);
        assert_eq!(count, FRONTMOST_REASSERTS);
        assert!(*waited.borrow() <= FRONTMOST_POLL * 20);
    }

    #[test]
    fn a_refused_size_before_the_move_still_moves_the_window() {
        let calls = RefCell::new(Vec::new());
        let sizes = RefCell::new(vec![AXError::Success, AXError::CannotComplete]);

        let wrote = write_frame(
            || {
                calls.borrow_mut().push("size");
                sizes.borrow_mut().pop().unwrap()
            },
            || {
                calls.borrow_mut().push("position");
                AXError::Success
            },
        );

        assert!(wrote.written());
        assert_eq!(*calls.borrow(), ["size", "position", "size"]);
    }

    #[test]
    fn a_refused_move_or_final_size_is_not_written() {
        let refused_move = write_frame(|| AXError::Success, || AXError::CannotComplete);
        assert!(!refused_move.written());
        let sizes = RefCell::new(vec![AXError::CannotComplete, AXError::Success]);
        let write = write_frame(|| sizes.borrow_mut().pop().unwrap(), || AXError::Success);
        assert_eq!(
            write,
            FrameWrite {
                moved: true,
                sized: false,
            }
        );
        assert!(!write.written());
    }

    /// #726: a fixed-size window tiled by a `tile` rule refuses every size
    /// write; moved to its tile's corner it keeps its own size there, which
    /// must not block the layout of a workspace it is revealed with.
    #[test]
    fn a_window_that_moves_but_cannot_resize_is_constrained_not_unavailable() {
        let tile = Rect {
            x: 0,
            y: 30,
            width: 1280,
            height: 1410,
        };
        let native = Rect {
            width: 620,
            height: 420,
            ..tile
        };
        let moved_only = FrameWrite {
            moved: true,
            sized: false,
        };
        assert_eq!(
            frame_outcome(moved_only, tile, Some(native), 0),
            ApplyOutcome::Constrained
        );
        let elsewhere = Rect {
            x: 900,
            y: 600,
            ..native
        };
        assert_eq!(
            frame_outcome(moved_only, tile, Some(elsewhere), 0),
            ApplyOutcome::Unavailable
        );
        let refused = FrameWrite {
            moved: false,
            sized: false,
        };
        assert_eq!(
            frame_outcome(refused, tile, Some(native), 0),
            ApplyOutcome::Unavailable
        );
        assert_eq!(
            frame_outcome(refused, tile, Some(tile), 0),
            ApplyOutcome::Applied
        );
        assert_eq!(
            frame_outcome(moved_only, tile, None, 0),
            ApplyOutcome::Unavailable
        );
    }

    #[test]
    fn warp_reassociates_immediately_after_warping_and_skips_while_mouse_is_down() {
        let calls = RefCell::new(Vec::new());
        assert!(!warp_sequence(
            || true,
            || {
                calls.borrow_mut().push("warp");
                true
            },
            || calls.borrow_mut().push("reassociate"),
        ));
        assert!(calls.borrow().is_empty());

        assert!(warp_sequence(
            || false,
            || {
                calls.borrow_mut().push("warp");
                true
            },
            || calls.borrow_mut().push("reassociate"),
        ));
        assert_eq!(*calls.borrow(), ["warp", "reassociate"]);
    }

    #[test]
    fn ax_title_timeout_marks_rule_probe_unresolved_instead_of_untitled() {
        assert_eq!(
            interpret_ax_title(Ok(CFString::from_str("Work")))
                .unwrap()
                .as_deref(),
            Some("Work")
        );
        assert_eq!(interpret_ax_title(Err(AXError::NoValue)), Ok(None));
        assert_eq!(
            interpret_ax_title(Err(AXError::AttributeUnsupported)),
            Ok(None)
        );
        assert_eq!(
            interpret_ax_title(Err(AXError::CannotComplete)),
            Err(WindowAccessError::Unavailable)
        );
        assert_eq!(
            interpret_ax_title(Err(AXError::APIDisabled)),
            Err(WindowAccessError::Unavailable)
        );
    }

    #[test]
    fn prune_managed_app_evicts_older_incarnations_reusing_the_same_pid() {
        let older_app = AppInstanceId {
            pid: 42,
            launched_at: 100,
        };
        let current_app = AppInstanceId {
            pid: 42,
            launched_at: 200,
        };
        let other_app = AppInstanceId {
            pid: 99,
            launched_at: 50,
        };
        let kept_current = PhysicalWindowId {
            app: current_app.clone(),
            window_id: 1,
        };
        let closed_current = PhysicalWindowId {
            app: current_app.clone(),
            window_id: 2,
        };
        let stale_reused_pid = PhysicalWindowId {
            app: older_app,
            window_id: 3,
        };
        let unrelated = PhysicalWindowId {
            app: other_app,
            window_id: 4,
        };
        let current = HashSet::from([kept_current.clone()]);

        assert!(keep_cached_window(&kept_current, &current_app, &current));
        assert!(!keep_cached_window(&closed_current, &current_app, &current));
        assert!(!keep_cached_window(
            &stale_reused_pid,
            &current_app,
            &current
        ));
        assert!(keep_cached_window(&unrelated, &current_app, &current));
    }
}
