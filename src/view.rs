use std::cell::OnceCell;
use std::ffi::{CStr, c_void};
use std::rc::Weak;

use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::runtime::ProtocolObject;
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send};
use objc2_app_kit::{
    NSApplication, NSApplicationDelegate, NSBackingStoreType, NSColor, NSDragOperation, NSDraggingDestination,
    NSDraggingInfo, NSEvent, NSEventModifierFlags, NSPanel, NSPasteboardTypeFileURL, NSResponder, NSView, NSWindow,
    NSWindowAnimationBehavior, NSWindowCollectionBehavior, NSWindowStyleMask,
};
use objc2_core_foundation::CGPoint;
use objc2_core_graphics::{CGWindowLevelForKey, CGWindowLevelKey};
use objc2_foundation::{NSArray, NSMutableArray, NSNumber, NSObject, NSObjectProtocol, NSRect, NSString, NSURL, ns_string};
use objc2_quartz_core::{CADisplayLink, CALayer};

use crate::dock::Dock;

/// The way back from an Objective-C object to the dock that made it.
#[derive(Default)]
pub struct DockLink(OnceCell<Weak<Dock>>);

impl DockLink {
    pub fn set(&self, dock: Weak<Dock>) {
        let _ = self.0.set(dock);
    }

    fn with(&self, f: impl FnOnce(&Dock)) {
        if let Some(dock) = self.0.get().and_then(Weak::upgrade) {
            f(&dock);
        }
    }
}

define_class!(
    /// Hosts the surface's layers and hands the dock its clicks.
    #[unsafe(super(NSView, NSResponder, NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "ErisDockView"]
    #[ivars = DockLink]
    pub struct DockView;

    impl DockView {
        #[unsafe(method(acceptsFirstMouse:))]
        fn accepts_first_mouse(&self, _event: Option<&NSEvent>) -> bool {
            true
        }

        #[unsafe(method(mouseDown:))]
        fn mouse_down(&self, event: &NSEvent) {
            let point = self.point(event);
            let control = event.modifierFlags().contains(NSEventModifierFlags::Control);
            self.ivars().with(|dock| dock.mouse_down(point, control));
        }

        #[unsafe(method(mouseDragged:))]
        fn mouse_dragged(&self, event: &NSEvent) {
            let point = self.point(event);
            self.ivars().with(|dock| dock.mouse_dragged(point));
        }

        #[unsafe(method(mouseUp:))]
        fn mouse_up(&self, event: &NSEvent) {
            let point = self.point(event);
            self.ivars().with(|dock| dock.mouse_up(point));
        }

        #[unsafe(method(rightMouseDown:))]
        fn right_mouse_down(&self, event: &NSEvent) {
            let point = self.point(event);
            self.ivars().with(|dock| dock.show_menu_at(point));
        }
    }

    unsafe impl NSObjectProtocol for DockView {}

    // Files dragged in from elsewhere.
    unsafe impl NSDraggingDestination for DockView {
        #[unsafe(method(draggingEntered:))]
        fn dragging_entered(&self, sender: &ProtocolObject<dyn NSDraggingInfo>) -> NSDragOperation {
            self.drag_over(sender)
        }

        #[unsafe(method(draggingUpdated:))]
        fn dragging_updated(&self, sender: &ProtocolObject<dyn NSDraggingInfo>) -> NSDragOperation {
            self.drag_over(sender)
        }

        #[unsafe(method(draggingExited:))]
        fn dragging_exited(&self, _sender: Option<&ProtocolObject<dyn NSDraggingInfo>>) {
            self.ivars().with(|dock| dock.drag_left());
        }

        #[unsafe(method(performDragOperation:))]
        fn perform_drag_operation(&self, sender: &ProtocolObject<dyn NSDraggingInfo>) -> bool {
            let (point, paths) = (self.convertPoint_fromView(sender.draggingLocation(), None), dragged_paths(sender));
            let mut done = false;
            self.ivars().with(|dock| done = dock.drop_files(point, &paths));
            done
        }
    }
);

/// The files being dragged.
fn dragged_paths(sender: &ProtocolObject<dyn NSDraggingInfo>) -> Vec<String> {
    let Some(items) = sender.draggingPasteboard().pasteboardItems() else { return Vec::new() };
    items
        .iter()
        .filter_map(|item| {
            let url = item.stringForType(unsafe { NSPasteboardTypeFileURL })?;
            NSURL::URLWithString(&url)?.path().map(|path| path.to_string())
        })
        .collect()
}

impl DockView {
    pub fn new(mtm: MainThreadMarker, root: &CALayer) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(DockLink::default());
        let this: Retained<Self> = unsafe { msg_send![super(this), initWithFrame: NSRect::ZERO] };
        // Layer-hosting: the view shows `root` and leaves its sublayers to us.
        this.setLayer(Some(root));
        this.setWantsLayer(true);
        this.registerForDraggedTypes(&NSArray::from_slice(&[unsafe { NSPasteboardTypeFileURL }]));
        this
    }

    fn drag_over(&self, sender: &ProtocolObject<dyn NSDraggingInfo>) -> NSDragOperation {
        let (point, paths) = (self.convertPoint_fromView(sender.draggingLocation(), None), dragged_paths(sender));
        let mut accept = false;
        self.ivars().with(|dock| accept = dock.drag_over(point, &paths));
        if accept { NSDragOperation::Copy } else { NSDragOperation::None }
    }

    fn point(&self, event: &NSEvent) -> CGPoint {
        self.convertPoint_fromView(event.locationInWindow(), None)
    }
}

define_class!(
    /// A clear strip along the bottom of a screen, at the Dock's level and on every Space, that
    /// takes clicks without activating ErisDock or taking focus from the frontmost app.
    #[unsafe(super(NSPanel, NSWindow, NSResponder, NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "ErisDockPanel"]
    pub struct DockPanel;

    impl DockPanel {
        #[unsafe(method(canBecomeKeyWindow))]
        fn can_become_key(&self) -> bool {
            false
        }

        #[unsafe(method(canBecomeMainWindow))]
        fn can_become_main(&self) -> bool {
            false
        }
    }
);

impl DockPanel {
    pub fn new(mtm: MainThreadMarker, view: &NSView) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(());
        let style = NSWindowStyleMask::Borderless | NSWindowStyleMask::NonactivatingPanel;
        let this: Retained<Self> = unsafe {
            msg_send![super(this), initWithContentRect: NSRect::ZERO, styleMask: style,
                      backing: NSBackingStoreType::Buffered, defer: false]
        };
        unsafe { this.setReleasedWhenClosed(false) };
        this.setOpaque(false);
        this.setBackgroundColor(Some(&NSColor::clearColor()));
        this.setHasShadow(false);
        this.setLevel(CGWindowLevelForKey(CGWindowLevelKey::DockWindowLevelKey) as isize);
        this.setCollectionBehavior(on_every_space());
        this.setMovable(false);
        this.setHidesOnDeactivate(false);
        this.setAnimationBehavior(NSWindowAnimationBehavior::None);
        this.setIgnoresMouseEvents(true);
        this.setContentView(Some(view));
        this
    }

    /// Puts the panel back on every space when it isn't on the current one. Something outside
    /// ErisDock (a window manager, most likely) can leave it listed on a single space while still
    /// marked as joining all of them, and the window server then shows it there alone. Setting the
    /// collection behavior again doesn't undo that; adding it to every space does.
    pub fn rejoin_every_space(&self) {
        if !self.isOnActiveSpace() {
            add_to_every_space(self.windowNumber());
        }
    }
}

type MainConnection = unsafe extern "C" fn() -> i32;
type CopyDisplaySpaces = unsafe extern "C" fn(i32) -> *mut AnyObject;
type AddWindowsToSpaces = unsafe extern "C" fn(i32, *const AnyObject, *const AnyObject);

/// A SkyLight function, looked up at run time (AppKit has SkyLight loaded), so a macOS without it
/// just skips this.
fn skylight<F>(name: &CStr) -> Option<F> {
    let symbol = unsafe { libc::dlsym(libc::RTLD_DEFAULT, name.as_ptr()) };
    // SAFETY: F is the function pointer type the symbol has (see the types above).
    (!symbol.is_null()).then(|| unsafe { std::mem::transmute_copy::<*mut c_void, F>(&symbol) })
}

/// Lists `window` on every space of every display, as the window server does for a window that
/// joins all spaces.
fn add_to_every_space(window: isize) {
    let (Some(connection), Some(copy_spaces), Some(add)) = (
        skylight::<MainConnection>(c"SLSMainConnectionID"),
        skylight::<CopyDisplaySpaces>(c"SLSCopyManagedDisplaySpaces"),
        skylight::<AddWindowsToSpaces>(c"SLSAddWindowsToSpaces"),
    ) else {
        return;
    };
    let cid = unsafe { connection() };
    // SAFETY: a +1 CFArray of dictionaries, toll-free bridged.
    let Some(displays) = (unsafe { Retained::from_raw(copy_spaces(cid).cast::<NSArray<AnyObject>>()) }) else {
        return;
    };
    let ids = NSMutableArray::<AnyObject>::new();
    for display in displays.iter() {
        let spaces: Option<Retained<NSArray<AnyObject>>> = unsafe { msg_send![&*display, objectForKey: ns_string!("Spaces")] };
        for space in spaces.iter().flat_map(|spaces| spaces.iter()) {
            let id: Option<Retained<AnyObject>> = unsafe { msg_send![&*space, objectForKey: ns_string!("ManagedSpaceID")] };
            if let Some(id) = id {
                ids.addObject(&id);
            }
        }
    }
    let windows = NSArray::from_retained_slice(&[Retained::into_super(Retained::into_super(NSNumber::new_isize(window)))]);
    unsafe { add(cid, Retained::as_ptr(&windows).cast(), Retained::as_ptr(&ids).cast()) };
}

fn on_every_space() -> NSWindowCollectionBehavior {
    NSWindowCollectionBehavior::CanJoinAllSpaces
        | NSWindowCollectionBehavior::Stationary
        | NSWindowCollectionBehavior::IgnoresCycle
        | NSWindowCollectionBehavior::FullScreenAuxiliary
}

define_class!(
    /// The display link's target, a frame at a time.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "ErisDockTicker"]
    #[ivars = DockLink]
    pub struct Ticker;

    impl Ticker {
        #[unsafe(method(tick:))]
        fn tick(&self, link: &CADisplayLink) {
            self.ivars().with(|dock| dock.tick(link));
        }
    }
);

impl Ticker {
    pub fn new(mtm: MainThreadMarker) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(DockLink::default());
        unsafe { msg_send![super(this), init] }
    }
}

define_class!(
    /// Watches NSWorkspace's runningApplications, which an app joins as soon as it starts
    /// launching, well before didLaunchApplication is posted.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "ErisDockAppsObserver"]
    #[ivars = DockLink]
    pub struct AppsObserver;

    impl AppsObserver {
        #[unsafe(method(observeValueForKeyPath:ofObject:change:context:))]
        fn observe(
            &self,
            _key_path: Option<&NSString>,
            _object: Option<&AnyObject>,
            _change: Option<&AnyObject>,
            _context: *mut c_void,
        ) {
            // NSWorkspace changes the list on the main run loop; anything else is ignored, and
            // the launch notifications catch up.
            if MainThreadMarker::new().is_some() {
                self.ivars().with(|dock| dock.apps_changed());
            }
        }
    }
);

impl AppsObserver {
    pub fn new(mtm: MainThreadMarker) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(DockLink::default());
        unsafe { msg_send![super(this), init] }
    }
}

define_class!(
    /// Opening ErisDock again, from Spotlight or Finder, while it's running opens its settings.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "ErisDockAppDelegate"]
    #[ivars = DockLink]
    pub struct AppDelegate;

    unsafe impl NSObjectProtocol for AppDelegate {}

    unsafe impl NSApplicationDelegate for AppDelegate {
        #[unsafe(method(applicationShouldHandleReopen:hasVisibleWindows:))]
        fn reopen(&self, _app: &NSApplication, _has_visible_windows: bool) -> bool {
            self.ivars().with(|dock| dock.open_settings());
            false
        }
    }
);

impl AppDelegate {
    pub fn new(mtm: MainThreadMarker) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(DockLink::default());
        unsafe { msg_send![super(this), init] }
    }
}
