use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::{
    NSApplication, NSControlStateValueMixed, NSControlStateValueOn, NSEventModifierFlags, NSFontWeightRegular,
    NSImage, NSImageSymbolConfiguration, NSMenu, NSMenuItem, NSRunningApplication, NSWorkspace,
};
use objc2_foundation::{NSArray, NSObject, NSString, ns_string};

use crate::actions::{self, AppWindows, CoreDock, LoginItem};

define_class!(
    /// A menu item's target, running a closure.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "ErisDockMenuAction"]
    #[ivars = Box<dyn Fn()>]
    struct MenuAction;

    impl MenuAction {
        #[unsafe(method(run:))]
        fn run(&self, _sender: Option<&AnyObject>) {
            (self.ivars())();
        }
    }
);

/// A menu whose items run closures. Items hold their targets weakly, so the menu keeps them
/// for as long as it lives.
pub struct Menu {
    mtm: MainThreadMarker,
    pub menu: Retained<NSMenu>,
    actions: Vec<Retained<MenuAction>>,
}

impl Menu {
    pub fn new(mtm: MainThreadMarker) -> Self {
        let menu = NSMenu::new(mtm);
        menu.setAutoenablesItems(false);
        Self { mtm, menu, actions: Vec::new() }
    }

    pub fn item(&mut self, title: &str, action: impl Fn() + 'static) -> Retained<NSMenuItem> {
        let target = MenuAction::alloc(self.mtm).set_ivars(Box::new(action) as Box<dyn Fn()>);
        let target: Retained<MenuAction> = unsafe { msg_send![super(target), init] };
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
                NSWorkspace::sharedWorkspace().activateFileViewerSelectingURLs(&NSArray::from_retained_slice(std::slice::from_ref(&url)));
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
pub fn dock_menu(mtm: MainThreadMarker) -> Menu {
    let mut menu = Menu::new(mtm);
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
    let configuration = NSImageSymbolConfiguration::configurationWithPointSize_weight(7.0, unsafe { NSFontWeightRegular });
    image.imageWithSymbolConfiguration(&configuration)
}
