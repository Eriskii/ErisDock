use std::path::PathBuf;
use std::time::SystemTime;
use std::{fs, io};

use objc2::AnyThread;
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2_foundation::{NSBundle, NSDictionary, NSString, NSURL, NSUserDefaults, ns_string};
use serde::{Deserialize, Serialize};

use crate::row::{self, Pinned};

/// The pinned apps, kept in `pinned.toml` beside the config file. Only ErisDock changes it:
/// pinning, unpinning and moving apps in the dock, or by hand.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Pins {
    /// The pinned apps' bundles, in order.
    #[serde(default)]
    pub apps: Vec<String>,
    /// The apps the native Dock had pinned when ErisDock last looked, so that an app pinning
    /// itself there is noticed.
    #[serde(default)]
    pub native_dock: Vec<String>,
}

const HEADER: &str = "# The apps pinned to ErisDock, in order. ErisDock rewrites this file when an app is pinned,\n\
# unpinned or moved, and picks up changes made to it by hand.\n\n";

impl Pins {
    pub fn path() -> PathBuf {
        crate::config::path().with_file_name("pinned.toml")
    }

    pub fn modified() -> Option<SystemTime> {
        fs::metadata(Self::path()).and_then(|metadata| metadata.modified()).ok()
    }

    /// Reads the file. The first time, with no file yet, the native Dock's pinned apps become
    /// ErisDock's.
    pub fn load() -> Self {
        match fs::read_to_string(Self::path()) {
            Ok(text) => toml::from_str(&text).unwrap_or_else(|error| {
                eprintln!("ErisDock: {}: {error}", Self::path().display());
                Self::default()
            }),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                let native = native_dock_apps();
                let pins = Self { apps: native.clone(), native_dock: native };
                pins.save();
                pins
            }
            Err(error) => {
                eprintln!("ErisDock: {}: {error}", Self::path().display());
                Self::default()
            }
        }
    }

    pub fn save(&self) {
        let path = Self::path();
        let text = format!("{HEADER}{}", toml::to_string(self).unwrap_or_default());
        let written = path.parent().map_or(Ok(()), fs::create_dir_all).and_then(|()| fs::write(&path, text));
        if let Err(error) = written {
            eprintln!("ErisDock: couldn't save {}: {error}", path.display());
        }
    }

    /// Takes note of the native Dock's pinned apps. Ones added there since the last look are
    /// pinned here too only when `follow` allows it: apps pin themselves to the native Dock,
    /// with installers and "Keep in Dock?" prompts. Returns whether anything changed.
    pub fn observe_native(&mut self, current: Vec<String>, follow: bool) -> bool {
        let before = self.clone();
        if follow {
            for app in &current {
                if !self.native_dock.contains(app) && !self.apps.contains(app) {
                    self.apps.push(app.clone());
                }
            }
        }
        self.native_dock = current;
        *self != before
    }

    pub fn pinned(&self) -> Vec<Pinned> {
        self.apps.iter().map(|path| Pinned { key: key_for_bundle(path), path: path.clone() }).collect()
    }

    /// Puts the pins in `keys`' order, `paths` naming any newly pinned ones.
    pub fn set_order(&mut self, keys: &[String], paths: &[(String, String)]) {
        let current = self.pinned();
        self.apps = keys
            .iter()
            .filter_map(|key| {
                current
                    .iter()
                    .find(|pin| &pin.key == key)
                    .map(|pin| pin.path.clone())
                    .or_else(|| paths.iter().find(|(k, _)| k == key).map(|(_, path)| path.clone()))
            })
            .collect();
    }

    pub fn remove(&mut self, key: &str) {
        self.apps.retain(|path| key_for_bundle(path) != key);
    }

    pub fn contains(&self, key: &str) -> bool {
        self.apps.iter().any(|path| key_for_bundle(path) == key)
    }
}

/// The key of the app bundle at `path`.
pub fn key_for_bundle(path: &str) -> String {
    let id = NSBundle::bundleWithPath(&NSString::from_str(path)).and_then(|bundle| bundle.bundleIdentifier());
    row::key(id.map(|id| id.to_string()).as_deref(), Some(path), None)
}

/// The paths of the apps pinned to the native Dock.
pub fn native_dock_apps() -> Vec<String> {
    let Some(dock) = NSUserDefaults::initWithSuiteName(NSUserDefaults::alloc(), Some(ns_string!("com.apple.dock")))
    else {
        return Vec::new();
    };
    let Some(tiles) = dock.arrayForKey(ns_string!("persistent-apps")) else { return Vec::new() };
    tiles
        .iter()
        .filter_map(|tile| {
            let tile = tile.downcast::<NSDictionary>().ok()?;
            let data = object(&tile, "tile-data")?.downcast::<NSDictionary>().ok()?;
            let file = object(&data, "file-data")?.downcast::<NSDictionary>().ok()?;
            let url = object(&file, "_CFURLString")?.downcast::<NSString>().ok()?;
            NSURL::URLWithString(&url)?.path().map(|path| path.to_string())
        })
        .collect()
}

fn object(dictionary: &NSDictionary, key: &str) -> Option<Retained<AnyObject>> {
    dictionary.objectForKey(&NSString::from_str(key))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pins(apps: &[&str], native: &[&str]) -> Pins {
        Pins {
            apps: apps.iter().map(|s| s.to_string()).collect(),
            native_dock: native.iter().map(|s| s.to_string()).collect(),
        }
    }

    fn list(apps: &[&str]) -> Vec<String> {
        apps.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn apps_pinning_themselves_to_the_native_dock_are_ignored() {
        let mut pins = pins(&["/Mail.app"], &["/Mail.app"]);
        assert!(pins.observe_native(list(&["/Mail.app", "/Chrome.app"]), false));
        assert_eq!(pins.apps, list(&["/Mail.app"]));
        // Seen, so letting apps pin themselves later doesn't bring it in after all.
        assert!(!pins.observe_native(list(&["/Mail.app", "/Chrome.app"]), true));
        assert_eq!(pins.apps, list(&["/Mail.app"]));
    }

    #[test]
    fn when_allowed_new_native_pins_are_followed() {
        let mut pins = pins(&["/Mail.app"], &["/Mail.app"]);
        assert!(pins.observe_native(list(&["/Mail.app", "/Chrome.app"]), true));
        assert_eq!(pins.apps, list(&["/Mail.app", "/Chrome.app"]));
    }

    #[test]
    fn unpinning_from_the_native_dock_leaves_erisdock_alone() {
        let mut pins = pins(&["/Mail.app"], &["/Mail.app"]);
        pins.observe_native(Vec::new(), true);
        assert_eq!(pins.apps, list(&["/Mail.app"]));
    }

    #[test]
    fn the_file_round_trips() {
        let pins = pins(&["/Applications/Safari.app"], &[]);
        let text = format!("{HEADER}{}", toml::to_string(&pins).unwrap());
        assert_eq!(toml::from_str::<Pins>(&text).unwrap(), pins);
    }
}
