use std::{cell::RefCell, sync::mpsc::Sender};

use block2::RcBlock;
use objc2::{
    DefinedClass, MainThreadMarker, MainThreadOnly, msg_send, rc::Retained, runtime::Bool, sel,
};
use objc2_app_kit::{
    NSBezierPath, NSColor, NSControlStateValueOff, NSControlStateValueOn, NSImage, NSMenu,
    NSMenuItem, NSStatusBar, NSStatusBarButton, NSStatusItem, NSStringDrawing,
    NSVariableStatusItemLength,
};
use objc2_foundation::{NSObject, NSObjectProtocol, NSPoint, NSRect, NSSize, NSString};

use crate::{
    WorkspaceId,
    app::{Action, HUD_PREVIEW_RECT, UiHudLayout, WorkerAction},
    login_item,
};

mod workspaces;

pub(crate) use workspaces::{DisplayWorkspaces, MenuWorkspace};

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct WorkspaceLayout {
    pub(crate) workspace: String,
    pub(crate) mode: UiHudLayout,
}

struct TargetIvars {
    actions: Sender<Action>,
}

objc2::define_class!(
    #[unsafe(super = NSObject)]
    #[thread_kind = MainThreadOnly]
    #[ivars = TargetIvars]
    struct Target;

    unsafe impl NSObjectProtocol for Target {}

    impl Target {
        #[unsafe(method(openSetup:))]
        fn open_setup(&self, _sender: &NSObject) {
            drop(self.ivars().actions.send(Action::OpenSetup));
        }

        #[unsafe(method(toggleAutomatic:))]
        fn toggle_automatic(&self, _sender: &NSObject) {
            drop(self.ivars().actions.send(Action::ToggleAutomatic));
        }

        #[unsafe(method(toggleHud:))]
        fn toggle_hud(&self, _sender: &NSObject) {
            drop(self.ivars().actions.send(Action::ToggleHud));
        }

        #[unsafe(method(openSettings:))]
        fn open_settings(&self, _sender: &NSObject) {
            drop(self.ivars().actions.send(Action::OpenSettings));
        }

        /// Checks and opens Settings, whose sidebar shows the check's progress.
        #[unsafe(method(checkForUpdates:))]
        fn check_for_updates(&self, _sender: &NSObject) {
            drop(self.ivars().actions.send(Action::CheckForUpdates));
            drop(self.ivars().actions.send(Action::OpenSettings));
        }

        #[unsafe(method(installUpdate:))]
        fn install_update(&self, _sender: &NSObject) {
            drop(self.ivars().actions.send(Action::InstallUpdate));
        }

        #[unsafe(method(toggleLaunchAtLogin:))]
        fn toggle_launch_at_login(&self, _sender: &NSObject) {
            drop(self.ivars().actions.send(Action::ToggleLaunchAtLogin));
        }

        #[unsafe(method(switchWorkspace:))]
        fn switch_workspace(&self, sender: &NSMenuItem) {
            if let Ok(index) = usize::try_from(sender.tag()) {
                drop(self.ivars().actions.send(Action::Worker(
                    WorkerAction::SelectWorkspace(WorkspaceId::from_index(index)),
                )));
            }
        }

        #[unsafe(method(quit:))]
        fn quit(&self, _sender: &NSObject) {
            drop(self.ivars().actions.send(Action::Quit));
        }
    }
);

impl Target {
    fn new(actions: Sender<Action>, marker: MainThreadMarker) -> Retained<Self> {
        let this = Self::alloc(marker).set_ivars(TargetIvars { actions });
        // SAFETY: `NSObject`'s `-init` takes no arguments and returns an initialized `Self`.
        unsafe { msg_send![super(this), init] }
    }
}

struct Controller {
    _item: Retained<NSStatusItem>,
    menu: Retained<NSMenu>,
    target: Retained<Target>,
    button: Retained<NSStatusBarButton>,
    status: Retained<NSMenuItem>,
    setup: Retained<NSMenuItem>,
    setup_separator: Retained<NSMenuItem>,
    automatic: Retained<NSMenuItem>,
    hud: Retained<NSMenuItem>,
    launch_at_login: Retained<NSMenuItem>,
    update: Retained<NSMenuItem>,
    status_title: String,
    workspace_layouts: Vec<WorkspaceLayout>,
    workspace_menu: Vec<DisplayWorkspaces>,
    workspace_items: Vec<Retained<NSMenuItem>>,
}

thread_local! {
    static CONTROLLER: RefCell<Option<Controller>> = const { RefCell::new(None) };
}

pub fn install(sender: Sender<Action>) -> Result<(), String> {
    let marker = MainThreadMarker::new().ok_or("status item requires the main thread")?;
    let target = Target::new(sender, marker);
    let menu = NSMenu::initWithTitle(NSMenu::alloc(marker), &NSString::from_str("SpaceFred"));
    let status = plain_item("Starting...", marker);
    status.setEnabled(false);
    status.setHidden(true);
    menu.addItem(&status);
    menu.addItem(&NSMenuItem::separatorItem(marker));
    let setup = action_item("Setup Tour…", &target, sel!(openSetup:), "", marker);
    setup.setHidden(true);
    menu.addItem(&setup);
    let setup_separator = NSMenuItem::separatorItem(marker);
    setup_separator.setHidden(true);
    menu.addItem(&setup_separator);
    let automatic = action_item("Pause Tiling", &target, sel!(toggleAutomatic:), "", marker);
    menu.addItem(&automatic);
    menu.addItem(&NSMenuItem::separatorItem(marker));
    let hud = action_item("Workspace HUD", &target, sel!(toggleHud:), "", marker);
    hud.setState(NSControlStateValueOn);
    menu.addItem(&hud);
    let launch_at_login = action_item(
        "Launch at Login",
        &target,
        sel!(toggleLaunchAtLogin:),
        "",
        marker,
    );
    menu.addItem(&launch_at_login);
    menu.addItem(&NSMenuItem::separatorItem(marker));
    menu.addItem(&action_item(
        "Settings...",
        &target,
        sel!(openSettings:),
        ",",
        marker,
    ));
    let update = action_item(
        CHECK_FOR_UPDATES,
        &target,
        sel!(checkForUpdates:),
        "",
        marker,
    );
    menu.addItem(&update);
    menu.addItem(&NSMenuItem::separatorItem(marker));
    menu.addItem(&action_item(
        "Quit SpaceFred",
        &target,
        sel!(quit:),
        "q",
        marker,
    ));

    let item = NSStatusBar::systemStatusBar().statusItemWithLength(NSVariableStatusItemLength);
    let button = item
        .button(marker)
        .ok_or("status item button is unavailable")?;
    item.setMenu(Some(&menu));

    CONTROLLER.with(|controller| {
        *controller.borrow_mut() = Some(Controller {
            _item: item,
            menu,
            target,
            button,
            status,
            setup,
            setup_separator,
            automatic,
            hud,
            launch_at_login,
            update,
            status_title: String::new(),
            workspace_layouts: Vec::new(),
            workspace_menu: Vec::new(),
            workspace_items: Vec::new(),
        });
    });
    Ok(())
}

const CHECK_FOR_UPDATES: &str = "Check for Updates...";

/// The update item checks, or installs a ready update.
pub fn set_update(ready: Option<&str>) {
    CONTROLLER.with(|controller| {
        if let Some(controller) = controller.borrow().as_ref() {
            let (title, action) = match ready {
                Some(version) => (
                    format!("Install SpaceFred {version} and Restart"),
                    sel!(installUpdate:),
                ),
                None => (CHECK_FOR_UPDATES.to_owned(), sel!(checkForUpdates:)),
            };
            controller.update.setTitle(&NSString::from_str(&title));
            // SAFETY: Both selectors are methods on `Target`, the item's retained target.
            unsafe { controller.update.setAction(Some(action)) };
        }
    });
}

pub fn set_status(status: &str) {
    CONTROLLER.with(|controller| {
        let mut controller = controller.borrow_mut();
        if let Some(controller) = controller.as_mut()
            && controller.status_title != status
        {
            controller.status.setHidden(false);
            controller.status.setTitle(&NSString::from_str(status));
            status.clone_into(&mut controller.status_title);
        }
    });
}

pub fn clear_status() {
    CONTROLLER.with(|controller| {
        let mut controller = controller.borrow_mut();
        if let Some(controller) = controller.as_mut() {
            controller.status.setHidden(true);
            controller.status_title.clear();
        }
    });
}

pub fn set_accessibility(trusted: bool) {
    CONTROLLER.with(|controller| {
        if let Some(controller) = controller.borrow().as_ref() {
            controller.setup.setHidden(trusted);
            controller.setup_separator.setHidden(trusted);
        }
    });
}

pub fn set_hud(enabled: bool) {
    CONTROLLER.with(|controller| {
        if let Some(controller) = controller.borrow().as_ref() {
            controller.hud.setState(if enabled {
                NSControlStateValueOn
            } else {
                NSControlStateValueOff
            });
        }
    });
}

pub fn set_automatic(enabled: bool) {
    if !enabled {
        // Switching is ignored while paused, and the list goes stale.
        set_workspace_menu(&[]);
    }
    CONTROLLER.with(|controller| {
        if let Some(controller) = controller.borrow().as_ref() {
            controller
                .automatic
                .setTitle(&NSString::from_str(if enabled {
                    "Pause Tiling"
                } else {
                    "Resume Tiling"
                }));
        }
    });
}

pub fn set_launch_at_login(status: login_item::Status) {
    CONTROLLER.with(|controller| {
        if let Some(controller) = controller.borrow().as_ref() {
            let (title, state) = match status {
                login_item::Status::Disabled => ("Launch at Login", NSControlStateValueOff),
                login_item::Status::Enabled => ("Launch at Login", NSControlStateValueOn),
                login_item::Status::RequiresApproval => (
                    "Launch at Login (Approval Required...)",
                    NSControlStateValueOff,
                ),
            };
            controller
                .launch_at_login
                .setTitle(&NSString::from_str(title));
            controller.launch_at_login.setState(state);
        }
    });
}

pub fn set_workspace_numbers(workspaces: &[u8]) {
    let title = if workspaces.is_empty() {
        String::new()
    } else {
        format!(
            " {}",
            workspaces
                .iter()
                .map(u8::to_string)
                .collect::<Vec<_>>()
                .join(" · ")
        )
    };
    CONTROLLER.with(|controller| {
        if let Some(controller) = controller.borrow_mut().as_mut() {
            controller.button.setImage(None);
            controller.button.setTitle(&NSString::from_str(&title));
            controller.workspace_layouts.clear();
        }
    });
}

pub fn set_workspace_layouts(layouts: &[WorkspaceLayout]) {
    CONTROLLER.with(|controller| {
        let mut controller = controller.borrow_mut();
        if let Some(controller) = controller.as_mut()
            && controller.workspace_layouts != layouts
        {
            let image = workspace_layout_image(layouts);
            controller.button.setTitle(&NSString::new());
            controller.button.setImage(Some(&image));
            controller.workspace_layouts = layouts.to_vec();
        }
    });
}

/// Lists `displays`' workspaces above the tiling toggle.
pub fn set_workspace_menu(displays: &[DisplayWorkspaces]) {
    let Some(marker) = MainThreadMarker::new() else {
        return;
    };
    CONTROLLER.with(|controller| {
        let mut controller = controller.borrow_mut();
        let Some(controller) = controller.as_mut() else {
            return;
        };
        if controller.workspace_menu == displays {
            return;
        }
        for item in controller.workspace_items.drain(..) {
            controller.menu.removeItem(&item);
        }
        let items = workspaces::items(displays, &controller.target, marker);
        let start = controller.menu.indexOfItem(&controller.automatic);
        for (offset, item) in (start..).zip(&items) {
            controller.menu.insertItem_atIndex(item, offset);
        }
        controller.workspace_items = items;
        controller.workspace_menu = displays.to_vec();
    });
}

fn workspace_layout_image(layouts: &[WorkspaceLayout]) -> Retained<NSImage> {
    let labels = layouts
        .iter()
        .map(|layout| NSString::from_str(&layout.workspace))
        .collect::<Vec<_>>();
    let label_widths = labels
        .iter()
        .map(|label| {
            // SAFETY: `sizeWithAttributes(None)` measures `label` using AppKit's default font.
            unsafe { label.sizeWithAttributes(None) }.width.ceil()
        })
        .collect::<Vec<_>>();
    let width = label_widths.iter().sum::<f64>() + layouts.len() as f64 * 24.0 - 6.0;
    let layouts = layouts.to_vec();
    let image = NSImage::imageWithSize_flipped_drawingHandler(
        NSSize::new(width, 16.0),
        false,
        &RcBlock::new(move |_: NSRect| {
            draw_workspace_layouts(&layouts, &labels, &label_widths);
            Bool::YES
        }),
    );
    image.setTemplate(true);
    image
}

fn draw_workspace_layouts(
    layouts: &[WorkspaceLayout],
    labels: &[Retained<NSString>],
    label_widths: &[f64],
) {
    NSColor::blackColor().setStroke();
    NSColor::blackColor().setFill();
    let mut x = 0.5;
    for ((layout, label), label_width) in
        layouts.iter().zip(labels).zip(label_widths.iter().copied())
    {
        let frame = NSRect::new(NSPoint::new(x, 2.5), NSSize::new(15.0, 11.0));
        let outline = NSBezierPath::bezierPathWithRoundedRect_xRadius_yRadius(frame, 2.5, 2.5);
        match &layout.mode {
            UiHudLayout::Fullscreen => outline.fill(),
            UiHudLayout::Split(tiles) => {
                outline.setLineWidth(1.25);
                outline.stroke();
                for tile in tiles {
                    let tile = NSRect::new(
                        NSPoint::new(
                            x + f64::from(tile.x) / f64::from(HUD_PREVIEW_RECT.width) * 15.0,
                            // Preview frames are top-down; the image is unflipped.
                            2.5 + (f64::from(HUD_PREVIEW_RECT.height)
                                - f64::from(tile.y)
                                - f64::from(tile.height))
                                / f64::from(HUD_PREVIEW_RECT.height)
                                * 11.0,
                        ),
                        NSSize::new(
                            f64::from(tile.width) / f64::from(HUD_PREVIEW_RECT.width) * 15.0,
                            f64::from(tile.height) / f64::from(HUD_PREVIEW_RECT.height) * 11.0,
                        ),
                    );
                    let tile = NSBezierPath::bezierPathWithRect(tile);
                    tile.setLineWidth(0.75);
                    tile.stroke();
                }
            }
        }
        // SAFETY: Called inside `NSImage` drawing handler with a active graphics context.
        unsafe { label.drawAtPoint_withAttributes(NSPoint::new(x + 19.0, 0.5), None) };
        x += 24.0 + label_width;
    }
}

fn plain_item(title: &str, marker: MainThreadMarker) -> Retained<NSMenuItem> {
    // SAFETY: `NSMenuItem` is allocated on the main thread with no target/action selector.
    unsafe {
        NSMenuItem::initWithTitle_action_keyEquivalent(
            NSMenuItem::alloc(marker),
            &NSString::from_str(title),
            None,
            &NSString::new(),
        )
    }
}

fn action_item(
    title: &str,
    target: &Target,
    action: objc2::runtime::Sel,
    key: &str,
    marker: MainThreadMarker,
) -> Retained<NSMenuItem> {
    // SAFETY: `action` matches a method declared on `Target` taking a single sender object.
    let item = unsafe {
        NSMenuItem::initWithTitle_action_keyEquivalent(
            NSMenuItem::alloc(marker),
            &NSString::from_str(title),
            Some(action),
            &NSString::from_str(key),
        )
    };
    // SAFETY: `target` is retained inside `Controller` for the lifetime of the menu.
    unsafe { item.setTarget(Some(target)) };
    item
}
