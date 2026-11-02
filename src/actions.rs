use std::cell::Cell;
use std::ffi::c_void;
use std::ptr::{self, NonNull};
use std::sync::OnceLock;

use block2::RcBlock;
use objc2::rc::Retained;
use objc2_app_kit::{NSApplicationActivationOptions, NSRunningApplication, NSWorkspace, NSWorkspaceOpenConfiguration};
use objc2_application_services::{AXError, AXIsProcessTrusted, AXIsProcessTrustedWithOptions, AXUIElement};
use objc2_core_foundation::{CFArray, CFBoolean, CFDictionary, CFRetained, CFString, CFType};
use objc2_foundation::{NSString, NSTimer, NSURL};
use objc2_service_management::{SMAppService, SMAppServiceStatus};

/// Brings `app` forward as clicking it in the Dock does: unhidden, active, and asked to
/// reopen, so one with no windows open makes one and a minimized one comes back.
pub fn open(app: &NSRunningApplication) {
    let Some(url) = app.bundleURL() else {
        app.unhide();
        app.activateWithOptions(NSApplicationActivationOptions::empty());
        return;
    };
    let configuration = NSWorkspaceOpenConfiguration::configuration();
    NSWorkspace::sharedWorkspace().openApplicationAtURL_configuration_completionHandler(&url, &configuration, None);
}

pub fn hide_others(app: &NSRunningApplication) {
    let pid = app.processIdentifier();
    for other in NSWorkspace::sharedWorkspace().runningApplications().iter() {
        if other.activationPolicy() == objc2_app_kit::NSApplicationActivationPolicy::Regular
            && other.processIdentifier() != pid
        {
            other.hide();
        }
    }
    open(app);
}

/// App Exposé shows the frontmost app's windows, so `app` comes forward first.
pub fn show_all_windows(app: &Retained<NSRunningApplication>) {
    open(app);
    wait_until_frontmost(app.processIdentifier(), 20);
}

fn wait_until_frontmost(pid: i32, tries: u32) {
    let frontmost = NSWorkspace::sharedWorkspace().frontmostApplication().map(|app| app.processIdentifier());
    if frontmost == Some(pid) || tries == 0 {
        after(0.05, CoreDock::show_front_app_windows);
    } else {
        after(0.05, move || wait_until_frontmost(pid, tries - 1));
    }
}

pub fn new_finder_window() {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/".into());
    NSWorkspace::sharedWorkspace().openURL(&NSURL::fileURLWithPath_isDirectory(&NSString::from_str(&home), true));
}

/// Runs `f` once on the main run loop after `seconds`.
pub fn after(seconds: f64, f: impl FnOnce() + 'static) -> Retained<NSTimer> {
    let f = Cell::new(Some(f));
    let block = RcBlock::new(move |_: NonNull<NSTimer>| {
        if let Some(f) = f.take() {
            f();
        }
    });
    unsafe { NSTimer::scheduledTimerWithTimeInterval_repeats_block(seconds, false, &block) }
}

/// The native Dock's own calls, for what only it does, like App Exposé. Private API in
/// HIServices, looked up at run time so ErisDock goes without it if it's gone.
pub struct CoreDock;

type SendNotification = unsafe extern "C" fn(&CFString, *mut c_void);

impl CoreDock {
    fn send_notification() -> Option<SendNotification> {
        static SEND: OnceLock<Option<usize>> = OnceLock::new();
        let address = *SEND.get_or_init(|| {
            let symbol = unsafe { libc::dlsym(libc::RTLD_DEFAULT, c"CoreDockSendNotification".as_ptr()) };
            (!symbol.is_null()).then_some(symbol as usize)
        });
        // SAFETY: the symbol has had this signature since 10.5.
        address.map(|address| unsafe { std::mem::transmute::<usize, SendNotification>(address) })
    }

    pub fn can_show_app_windows() -> bool {
        Self::send_notification().is_some()
    }

    pub fn show_front_app_windows() {
        if let Some(send) = Self::send_notification() {
            unsafe { send(&CFString::from_static_str("com.apple.expose.front.awake"), ptr::null_mut()) };
        }
    }
}

#[derive(Clone)]
pub struct AppWindow {
    element: CFRetained<AXUIElement>,
    pub title: String,
    pub minimized: bool,
    pub main: bool,
}

/// An app's windows, through the Accessibility API. Needs ErisDock to be allowed under
/// Privacy & Security › Accessibility.
pub struct AppWindows;

impl AppWindows {
    pub fn is_trusted() -> bool {
        unsafe { AXIsProcessTrusted() }
    }

    pub fn request_access() {
        let key = CFString::from_static_str("AXTrustedCheckOptionPrompt");
        let options = CFDictionary::<CFString, CFBoolean>::from_slices(&[&*key], &[CFBoolean::new(true)]);
        unsafe { AXIsProcessTrustedWithOptions(Some(options.as_opaque())) };
    }

    pub fn list(pid: i32) -> Vec<AppWindow> {
        let app = unsafe { AXUIElement::new_application(pid) };
        // A hung app shouldn't hold up its own menu for long.
        unsafe { app.set_messaging_timeout(0.25) };
        let Some(windows) = attribute(&app, "AXWindows") else { return Vec::new() };
        let Some(windows) = windows.downcast_ref::<CFArray>() else { return Vec::new() };
        // SAFETY: AXWindows is an array of AXUIElements.
        let windows = unsafe { &*(windows as *const CFArray as *const CFArray<AXUIElement>) };
        windows
            .iter()
            .filter_map(|window| {
                let subrole = string_attribute(&window, "AXSubrole");
                let title = string_attribute(&window, "AXTitle").filter(|title| !title.is_empty());
                if subrole.as_deref() != Some("AXStandardWindow") {
                    return None;
                }
                Some(AppWindow {
                    title: title?,
                    minimized: bool_attribute(&window, "AXMinimized"),
                    main: bool_attribute(&window, "AXMain"),
                    element: window,
                })
            })
            .collect()
    }

    pub fn raise(window: &AppWindow, app: &NSRunningApplication) {
        if app.isHidden() {
            app.unhide();
        }
        let element = &window.element;
        unsafe {
            if window.minimized {
                element.set_attribute_value(&CFString::from_static_str("AXMinimized"), CFBoolean::new(false));
            }
            element.perform_action(&CFString::from_static_str("AXRaise"));
            element.set_attribute_value(&CFString::from_static_str("AXMain"), CFBoolean::new(true));
            AXUIElement::new_application(app.processIdentifier())
                .set_attribute_value(&CFString::from_static_str("AXFrontmost"), CFBoolean::new(true));
        }
    }
}

fn attribute(element: &AXUIElement, name: &'static str) -> Option<CFRetained<CFType>> {
    let mut value: *const CFType = ptr::null();
    let error = unsafe { element.copy_attribute_value(&CFString::from_static_str(name), NonNull::from(&mut value)) };
    if error != AXError::Success {
        return None;
    }
    // SAFETY: a copied value comes retained.
    NonNull::new(value as *mut CFType).map(|value| unsafe { CFRetained::from_raw(value) })
}

fn string_attribute(element: &AXUIElement, name: &'static str) -> Option<String> {
    attribute(element, name)?.downcast_ref::<CFString>().map(|s| s.to_string())
}

fn bool_attribute(element: &AXUIElement, name: &'static str) -> bool {
    attribute(element, name)
        .and_then(|value| value.downcast_ref::<CFBoolean>().map(CFBoolean::as_bool))
        .unwrap_or(false)
}

pub struct LoginItem;

impl LoginItem {
    pub fn is_enabled() -> bool {
        unsafe { SMAppService::mainAppService().status() == SMAppServiceStatus::Enabled }
    }

    pub fn toggle() {
        let service = unsafe { SMAppService::mainAppService() };
        let result = if Self::is_enabled() {
            unsafe { service.unregisterAndReturnError() }
        } else {
            unsafe { service.registerAndReturnError() }
        };
        if let Err(error) = result {
            eprintln!("ErisDock: couldn't change the login item: {}", error.localizedDescription());
        }
    }
}
