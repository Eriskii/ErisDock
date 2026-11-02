use std::cell::OnceCell;
use std::ffi::c_void;
use std::rc::Weak;

use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send};
use objc2_app_kit::{
    NSApplication, NSApplicationDelegate, NSBackingStoreType, NSColor, NSEvent, NSEventModifierFlags, NSPanel,
    NSResponder, NSView, NSWindow, NSWindowAnimationBehavior, NSWindowCollectionBehavior, NSWindowStyleMask,
};
use objc2_core_foundation::CGPoint;
use objc2_core_graphics::{CGWindowLevelForKey, CGWindowLevelKey};
use objc2_foundation::{NSObject, NSObjectProtocol, NSRect, NSString};
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
);

impl DockView {
    pub fn new(mtm: MainThreadMarker, root: &CALayer) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(DockLink::default());
        let this: Retained<Self> = unsafe { msg_send![super(this), initWithFrame: NSRect::ZERO] };
        // Layer-hosting: the view shows `root` and leaves its sublayers to us.
        this.setLayer(Some(root));
        this.setWantsLayer(true);
        this
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
        this.setCollectionBehavior(
            NSWindowCollectionBehavior::CanJoinAllSpaces
                | NSWindowCollectionBehavior::Stationary
                | NSWindowCollectionBehavior::IgnoresCycle
                | NSWindowCollectionBehavior::FullScreenAuxiliary,
        );
        this.setMovable(false);
        this.setHidesOnDeactivate(false);
        this.setAnimationBehavior(NSWindowAnimationBehavior::None);
        this.setIgnoresMouseEvents(true);
        this.setContentView(Some(view));
        this
    }
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
