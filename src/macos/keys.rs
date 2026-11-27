use std::{cell::RefCell, ffi::c_void, ptr::NonNull};

use objc2_core_foundation::{
    CFMachPort, CFRetained, CFRunLoop, CFRunLoopSource, kCFRunLoopCommonModes,
};
use objc2_core_graphics::{
    CGEvent, CGEventField, CGEventFlags, CGEventTapLocation, CGEventTapOptions,
    CGEventTapPlacement, CGEventTapProxy, CGEventType,
};

use crate::hotkeys::{COMMAND_KEY, CONTROL_KEY, OPTION_KEY, SHIFT_KEY};

/// Reports Option key presses (virtual key and Carbon modifiers) so the agent can
/// notice a hotkey macOS stopped delivering. The tap only listens, never reads
/// characters, and ignores every press without Option held.
pub struct KeyMonitor {
    tap: CFRetained<CFMachPort>,
    source: CFRetained<CFRunLoopSource>,
    _context: Box<Context>,
}

struct Context {
    report: Box<dyn Fn(u32, u32)>,
    tap: RefCell<Option<CFRetained<CFMachPort>>>,
}

/// The virtual key and Carbon modifiers a [`KeyMonitor`] reports when `chord` is pressed on the
/// active keyboard layout, or `None` when the layout has no key for it.
#[must_use]
pub fn key_press(chord: crate::shortcuts::Chord) -> Option<(u32, u32)> {
    crate::hotkeys::key_press(chord)
}

impl KeyMonitor {
    /// Installs on the main run loop. Fails without Accessibility.
    pub fn install(report: impl Fn(u32, u32) + 'static) -> Result<Self, String> {
        let context = Box::new(Context {
            report: Box::new(report),
            tap: RefCell::default(),
        });
        // SAFETY: `tap_callback` matches `CGEventTapCallBack`, and `context` stays
        // heap-pinned in the monitor until `Drop` invalidates the tap.
        let tap = unsafe {
            CGEvent::tap_create(
                CGEventTapLocation::SessionEventTap,
                CGEventTapPlacement::TailAppendEventTap,
                CGEventTapOptions::ListenOnly,
                1 << CGEventType::KeyDown.0,
                Some(tap_callback),
                (&raw const *context).cast_mut().cast(),
            )
        }
        .ok_or("the key press check needs Accessibility")?;
        let source = CFMachPort::new_run_loop_source(None, Some(&tap), 0)
            .ok_or("could not listen for key presses")?;
        let main = CFRunLoop::main().ok_or("the main run loop is unavailable")?;
        // SAFETY: `kCFRunLoopCommonModes` is a static Core Foundation mode constant.
        main.add_source(Some(&source), unsafe { kCFRunLoopCommonModes });
        context.tap.replace(Some(tap.clone()));
        CGEvent::tap_enable(&tap, true);
        Ok(Self {
            tap,
            source,
            _context: context,
        })
    }
}

impl Drop for KeyMonitor {
    fn drop(&mut self) {
        if let Some(main) = CFRunLoop::main() {
            // SAFETY: `kCFRunLoopCommonModes` is a static Core Foundation mode constant.
            main.remove_source(Some(&self.source), unsafe { kCFRunLoopCommonModes });
        }
        self.tap.invalidate();
    }
}

fn carbon_modifiers(flags: CGEventFlags) -> u32 {
    [
        (CGEventFlags::MaskCommand, COMMAND_KEY),
        (CGEventFlags::MaskShift, SHIFT_KEY),
        (CGEventFlags::MaskAlternate, OPTION_KEY),
        (CGEventFlags::MaskControl, CONTROL_KEY),
    ]
    .into_iter()
    .filter(|(flag, _)| flags.contains(*flag))
    .fold(0, |modifiers, (_, carbon)| modifiers | carbon)
}

unsafe extern "C-unwind" fn tap_callback(
    _: CGEventTapProxy,
    event_type: CGEventType,
    event: NonNull<CGEvent>,
    user_info: *mut c_void,
) -> *mut CGEvent {
    // SAFETY: `user_info` points to the `Context` boxed in `KeyMonitor::install`, which
    // outlives the tap, and the tap calls back only on the main thread.
    let context = unsafe { &*user_info.cast::<Context>() };
    if matches!(
        event_type,
        CGEventType::TapDisabledByTimeout | CGEventType::TapDisabledByUserInput
    ) {
        if let Some(tap) = context.tap.borrow().as_ref() {
            CGEvent::tap_enable(tap, true);
        }
        return event.as_ptr();
    }
    // SAFETY: the tap passes a live event for the duration of the callback.
    let cg_event = unsafe { event.as_ref() };
    let modifiers = carbon_modifiers(CGEvent::flags(Some(cg_event)));
    if event_type == CGEventType::KeyDown
        && modifiers & OPTION_KEY != 0
        && CGEvent::integer_value_field(Some(cg_event), CGEventField::KeyboardEventAutorepeat) == 0
        && let Ok(key_code) = u32::try_from(CGEvent::integer_value_field(
            Some(cg_event),
            CGEventField::KeyboardEventKeycode,
        ))
    {
        (context.report)(key_code, modifiers);
    }
    event.as_ptr()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_flags_become_carbon_modifiers() {
        assert_eq!(
            carbon_modifiers(CGEventFlags::MaskAlternate | CGEventFlags::MaskShift),
            OPTION_KEY | SHIFT_KEY
        );
        assert_eq!(
            carbon_modifiers(CGEventFlags::MaskAlternate | CGEventFlags::MaskAlphaShift),
            OPTION_KEY
        );
    }
}
