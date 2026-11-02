use std::cell::Cell;
use std::rc::Weak;

use objc2::rc::Retained;
use objc2::runtime::{AnyObject, ProtocolObject};
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::{
    NSApplication, NSControlStateValueMixed, NSControlStateValueOn, NSEventModifierFlags, NSFontWeightRegular, NSImage,
    NSImageSymbolConfiguration, NSMenu, NSMenuDelegate, NSMenuItem, NSRunningApplication, NSScreen, NSView,
    NSWorkspace,
};
use objc2_core_foundation::{CGPoint, CGRect, CGSize};
use objc2_foundation::{NSArray, NSObject, NSObjectProtocol, NSString, ns_string};

use crate::actions::{self, AppWindows, CoreDock, LoginItem};
use crate::dock::Dock;

define_class!(
    /// A control's or menu item's target, running a closure.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "ErisDockAction"]
    #[ivars = Box<dyn Fn()>]
    pub struct Action;

    impl Action {
        #[unsafe(method(run:))]
        fn run(&self, _sender: Option<&AnyObject>) {
            (self.ivars())();
        }
    }
);

impl Action {
    pub fn new(mtm: MainThreadMarker, action: impl Fn() + 'static) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(Box::new(action) as Box<dyn Fn()>);
        unsafe { msg_send![super(this), init] }
    }
}

define_class!(
    /// Keeps a menu to the part of the screen above the dock.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "ErisDockMenuPlacement"]
    #[ivars = Cell<CGRect>]
    struct MenuPlacement;

    unsafe impl NSObjectProtocol for MenuPlacement {}

    unsafe impl NSMenuDelegate for MenuPlacement {
        #[unsafe(method(confinementRectForMenu:onScreen:))]
        fn confinement(&self, _menu: &NSMenu, _screen: Option<&NSScreen>) -> CGRect {
            self.ivars().get()
        }
    }
);

/// A menu whose items run closures. Items hold their targets weakly, so the menu keeps them
/// for as long as it lives.
pub struct Menu {
    mtm: MainThreadMarker,
    pub menu: Retained<NSMenu>,
    actions: Vec<Retained<Action>>,
    placement: Option<Retained<MenuPlacement>>,
}

impl Menu {
    pub fn new(mtm: MainThreadMarker) -> Self {
        let menu = NSMenu::new(mtm);
        menu.setAutoenablesItems(false);
        Self { mtm, menu, actions: Vec::new(), placement: None }
    }

    pub fn item(&mut self, title: &str, action: impl Fn() + 'static) -> Retained<NSMenuItem> {
        let target = Action::new(self.mtm, action);
        let item = unsafe {
            NSMenuItem::initWithTitle_action_keyEquivalent(
                NSMenuItem::alloc(self.mtm),
                &NSString::from_str(title),
                Some(sel!(run:)),
                ns_string!(""),
            )
        };
        let object: &AnyObject = &target;
        unsafe { item.setTarget(Some(object)) };
        item.setKeyEquivalentModifierMask(NSEventModifierFlags::empty());
        self.menu.addItem(&item);
        self.actions.push(target);
        item
    }

    /// Opens the menu centred on `x` with its bottom on `bottom`, both in `view`'s coordinates,
    /// and runs it until it closes. AppKit's own estimate of a menu's height runs a couple of
    /// dozen points over, so rather than placing its top from that, the menu is kept above
    /// `bottom` and opened there: with no room below, AppKit opens it upward against it.
    pub fn pop_up_above(&mut self, view: &NSView, x: f64, bottom: f64) {
        if let Some((window, screen)) =
            view.window().and_then(|window| Some((window.frame(), window.screen()?.frame())))
        {
            let floor = window.origin.y + bottom;
            let above = CGRect::new(
                CGPoint::new(screen.origin.x, floor),
                CGSize::new(screen.size.width, screen.origin.y + screen.size.height - floor),
            );
            let placement = MenuPlacement::alloc(self.mtm).set_ivars(Cell::new(above));
            let placement: Retained<MenuPlacement> = unsafe { msg_send![super(placement), init] };
            self.menu.setDelegate(Some(ProtocolObject::from_ref(&*placement)));
            self.placement = Some(placement);
        }
        let width = self.menu.size().width;
        self.menu.popUpMenuPositioningItem_atLocation_inView(None, CGPoint::new(x - width / 2.0, bottom), Some(view));
    }

    /// An item that stands in for the one before it while `modifiers` are held.
    pub fn alternate(&mut self, title: &str, modifiers: NSEventModifierFlags, action: impl Fn() + 'static) {
        let item = self.item(title, action);
        item.setKeyEquivalentModifierMask(modifiers);
        item.setAlternate(true);
    }

    pub fn separator(&self) {
        self.menu.addItem(&NSMenuItem::separatorItem(self.mtm));
    }

    pub fn submenu(&mut self, title: &str, build: impl FnOnce(&mut Menu)) {
        let mut submenu = Menu::new(self.mtm);
        build(&mut submenu);
        let item = NSMenuItem::new(self.mtm);
        item.setTitle(&NSString::from_str(title));
        item.setSubmenu(Some(&submenu.menu));
        self.menu.addItem(&item);
        self.actions.append(&mut submenu.actions);
    }
}

/// The menu an app's icon opens, after the native Dock's: its windows, Options, Show All
/// Windows, Hide and Quit, with Option turning those two into Hide Others and Force Quit.
pub fn app_menu(mtm: MainThreadMarker, app: &Retained<NSRunningApplication>) -> Menu {
    let mut menu = Menu::new(mtm);
    let is_finder = app.bundleIdentifier().is_some_and(|id| id.to_string() == "com.apple.finder");

    if AppWindows::is_trusted() {
        let windows = AppWindows::list(app.processIdentifier());
        let minimized = minimized_image();
        for window in &windows {
            let (raised, owner) = (window.clone(), app.clone());
            let item = menu.item(&window.title, move || AppWindows::raise(&raised, &owner));
            if window.minimized {
                item.setState(NSControlStateValueMixed);
                unsafe { item.setMixedStateImage(minimized.as_deref()) };
            } else if window.main && app.isActive() {
                item.setState(NSControlStateValueOn);
            }
        }
        if !windows.is_empty() {
            menu.separator();
        }
    } else {
        menu.item("Allow Window List…", AppWindows::request_access);
        menu.separator();
    }

    if let Some(url) = app.bundleURL() {
        menu.submenu("Options", |options| {
            options.item("Show in Finder", move || {
                NSWorkspace::sharedWorkspace()
                    .activateFileViewerSelectingURLs(&NSArray::from_retained_slice(std::slice::from_ref(&url)));
            });
        });
        menu.separator();
    }

    if is_finder {
        menu.item("New Finder Window", actions::new_finder_window);
    }
    if CoreDock::can_show_app_windows() {
        let app = app.clone();
        menu.item("Show All Windows", move || actions::show_all_windows(&app));
    }
    let shown = app.clone();
    if app.isHidden() {
        menu.item("Show", move || actions::open(&shown));
    } else {
        menu.item("Hide", move || {
            shown.hide();
        });
    }
    let others = app.clone();
    menu.alternate("Hide Others", NSEventModifierFlags::Option, move || actions::hide_others(&others));
    if !is_finder {
        let (quit, force) = (app.clone(), app.clone());
        menu.item("Quit", move || {
            quit.terminate();
        });
        menu.alternate("Force Quit", NSEventModifierFlags::Option, move || {
            force.forceTerminate();
        });
    }
    menu
}

/// The menu the dock's own background opens.
pub fn dock_menu(mtm: MainThreadMarker, dock: Weak<Dock>) -> Menu {
    let mut menu = Menu::new(mtm);
    menu.item("Settings…", move || {
        if let Some(dock) = dock.upgrade() {
            dock.open_settings();
        }
    });
    let login = menu.item("Open at Login", LoginItem::toggle);
    if LoginItem::is_enabled() {
        login.setState(NSControlStateValueOn);
    }
    if !AppWindows::is_trusted() {
        menu.item("Allow Window Lists…", AppWindows::request_access);
    }
    menu.separator();
    menu.item("Quit ErisDock", move || NSApplication::sharedApplication(mtm).terminate(None));
    menu
}

fn minimized_image() -> Option<Retained<NSImage>> {
    let image = NSImage::imageWithSystemSymbolName_accessibilityDescription(
        ns_string!("diamond.fill"),
        Some(ns_string!("Minimized")),
    )?;
    let configuration =
        NSImageSymbolConfiguration::configurationWithPointSize_weight(7.0, unsafe { NSFontWeightRegular });
    image.imageWithSymbolConfiguration(&configuration)
}
