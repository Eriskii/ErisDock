//! ErisDock: a dock for macOS that rises out of the bottom edge of the screen.

mod actions;
mod animation;
mod apps;
mod badges;
mod config;
mod dock;
mod layout;
mod menu;
mod pins;
mod row;
mod settings;
mod settings_window;
mod shape;
mod snapshot;
mod spring;
mod surface;
mod view;

use std::path::Path;
use std::rc::Rc;

use objc2::runtime::ProtocolObject;
use objc2::{DefinedClass, MainThreadMarker};
use objc2_app_kit::{NSApplication, NSApplicationActivationPolicy, NSRunningApplication};

fn main() {
    let mtm = MainThreadMarker::new().expect("ErisDock runs on the main thread");
    let arguments: Vec<String> = std::env::args().collect();
    if let Some(flag) = arguments.iter().position(|argument| argument == "--snapshot") {
        let path = arguments.get(flag + 1).map_or("erisdock-snapshot.png", String::as_str);
        let style = arguments.iter().position(|argument| argument == "--style").and_then(|i| arguments.get(i + 1));
        let style = style
            .and_then(|name| animation::Style::ALL.into_iter().find(|style| style.title().eq_ignore_ascii_case(name)));
        snapshot::write(Path::new(path), style);
        return;
    }

    let current = NSRunningApplication::currentApplication();
    if let Some(id) = current.bundleIdentifier() {
        let pid = current.processIdentifier();
        if NSRunningApplication::runningApplicationsWithBundleIdentifier(&id)
            .iter()
            .any(|app| app.processIdentifier() != pid)
        {
            eprintln!("ErisDock is already running");
            return;
        }
    }

    let app = NSApplication::sharedApplication(mtm);
    app.setActivationPolicy(NSApplicationActivationPolicy::Accessory);
    let dock = dock::Dock::new(mtm);
    let delegate = view::AppDelegate::new(mtm);
    delegate.ivars().set(Rc::downgrade(&dock));
    app.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
    app.run();
}
