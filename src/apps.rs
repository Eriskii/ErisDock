use std::collections::HashSet;

use objc2::rc::Retained;
use objc2_app_kit::{NSApplicationActivationPolicy, NSRunningApplication, NSWorkspace};

/// The running apps the native Dock would show: those with a regular activation policy,
/// Finder first and the rest in the order they launched.
#[derive(Default)]
pub struct AppList {
    pub apps: Vec<Retained<NSRunningApplication>>,
}

impl AppList {
    pub fn refresh(&mut self) {
        let running: Vec<_> = NSWorkspace::sharedWorkspace()
            .runningApplications()
            .iter()
            .filter(|app| app.activationPolicy() == NSApplicationActivationPolicy::Regular && !app.isTerminated())
            .collect();
        let pids: HashSet<i32> = running.iter().map(|app| app.processIdentifier()).collect();
        self.apps.retain(|app| pids.contains(&app.processIdentifier()));
        let known: HashSet<i32> = self.apps.iter().map(|app| app.processIdentifier()).collect();
        self.apps.extend(running.into_iter().filter(|app| !known.contains(&app.processIdentifier())));
        let finder = self
            .apps
            .iter()
            .position(|app| app.bundleIdentifier().is_some_and(|id| id.to_string() == "com.apple.finder"));
        if let Some(finder) = finder.filter(|&index| index > 0) {
            let app = self.apps.remove(finder);
            self.apps.insert(0, app);
        }
    }
}
