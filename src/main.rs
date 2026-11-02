//! ErisDock: a dock for macOS that rises out of the bottom edge of the screen.

mod actions;
mod apps;
mod dock;
mod layout;
mod menu;
mod settings;
mod shape;
mod snapshot;
mod spring;
mod surface;
mod view;

use std::path::Path;

use objc2::MainThreadMarker;
use objc2_app_kit::{NSApplication, NSApplicationActivationPolicy, NSRunningApplication};

fn main() {
    let mtm = MainThreadMarker::new().expect("ErisDock runs on the main thread");
    let arguments: Vec<String> = std::env::args().collect();
    if let Some(flag) = arguments.iter().position(|argument| argument == "--snapshot") {
        let path = arguments.get(flag + 1).map_or("erisdock-snapshot.png", String::as_str);
        snapshot::write(Path::new(path));
        return;
    }

    let current = NSRunningApplication::currentApplication();
    if let Some(id) = current.bundleIdentifier() {
        let pid = current.processIdentifier();
        if NSRunningApplication::runningApplicationsWithBundleIdentifier(&id).iter().any(|app| app.processIdentifier() != pid) {
            eprintln!("ErisDock is already running");
            return;
        }
    }

    let app = NSApplication::sharedApplication(mtm);
    app.setActivationPolicy(NSApplicationActivationPolicy::Accessory);
    let _dock = dock::Dock::new(mtm);
    app.run();
}
