use std::collections::{HashMap, HashSet};

use objc2::rc::Retained;
use objc2_app_kit::{NSApplicationActivationPolicy, NSRunningApplication, NSWorkspace};

use crate::row::{self, Running};

/// The running apps the native Dock would show: those with a regular activation policy, in the
/// order they launched, unless they've been dragged into another.
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
    }

    pub fn running(&self) -> Vec<Running> {
        self.apps
            .iter()
            .map(|app| {
                let path = app.bundleURL().and_then(|url| url.path()).map(|path| path.to_string());
                Running { key: key(app), pid: app.processIdentifier(), path }
            })
            .collect()
    }

    pub fn by_pid(&self) -> HashMap<i32, Retained<NSRunningApplication>> {
        self.apps.iter().map(|app| (app.processIdentifier(), app.clone())).collect()
    }

    /// Puts the apps named in `keys` first, in that order, the rest after them as they were.
    pub fn reorder(&mut self, keys: &[String]) {
        let mut rest = std::mem::take(&mut self.apps);
        for wanted in keys {
            if let Some(i) = rest.iter().position(|app| &key(app) == wanted) {
                self.apps.push(rest.remove(i));
            }
        }
        self.apps.append(&mut rest);
    }
}

pub fn key(app: &NSRunningApplication) -> String {
    let id = app.bundleIdentifier().map(|id| id.to_string());
    let path = app.bundleURL().and_then(|url| url.path()).map(|path| path.to_string());
    row::key(id.as_deref(), path.as_deref(), Some(app.processIdentifier()))
}
