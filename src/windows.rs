//! What each app's windows are doing, from the window server: showing on a desktop that's on
//! screen, minimized, or on another desktop. Asked through SkyLight's private calls, looked up
//! at run time, which need no permissions; without them every app counts as windowless.

use std::collections::{HashMap, HashSet};
use std::ffi::{CStr, c_void};
use std::sync::OnceLock;

use objc2::rc::Retained;
use objc2_core_foundation::CFRetained;
use objc2_core_graphics::{CGWindowListCopyWindowInfo, CGWindowListOption};
use objc2_foundation::{NSArray, NSDictionary, NSNumber, ns_string};

use objc2_app_kit::NSWorkspace;

use crate::apps::AppList;
use crate::config;
use crate::indicators::{Indicators, Presence, Status};

type MainConnection = unsafe extern "C" fn() -> i32;
type CopyDisplaySpaces = unsafe extern "C" fn(i32) -> *mut NSArray;
type CopySpacesForWindows = unsafe extern "C" fn(i32, i32, *const NSArray<NSNumber>) -> *mut NSArray;
type IsOrderedIn = unsafe extern "C" fn(i32, u32, *mut u8) -> i32;
type GetTags = unsafe extern "C" fn(i32, u32, *mut u64, i32) -> i32;

/// Every kind of space a window can be in: desktops, fullscreen apps' and the system's.
const ALL_SPACES: i32 = 7;
/// The tag the window server puts on minimized windows.
const MINIMIZED: u64 = 1 << 60;

struct SkyLight {
    connection: i32,
    display_spaces: CopyDisplaySpaces,
    spaces_for_windows: CopySpacesForWindows,
    is_ordered_in: IsOrderedIn,
    tags: GetTags,
}

fn skylight() -> Option<&'static SkyLight> {
    static SKYLIGHT: OnceLock<Option<SkyLight>> = OnceLock::new();
    SKYLIGHT
        .get_or_init(|| {
            fn find<T>(name: &CStr) -> Option<T> {
                let symbol = unsafe { libc::dlsym(libc::RTLD_DEFAULT, name.as_ptr()) };
                // SAFETY: T is the function's type, the size of a pointer.
                (!symbol.is_null()).then(|| unsafe { std::mem::transmute_copy::<*mut c_void, T>(&symbol) })
            }
            let main: MainConnection = find(c"SLSMainConnectionID")?;
            Some(SkyLight {
                connection: unsafe { main() },
                display_spaces: find(c"SLSCopyManagedDisplaySpaces")?,
                spaces_for_windows: find(c"SLSCopySpacesForWindows")?,
                is_ordered_in: find(c"SLSWindowIsOrderedIn")?,
                tags: find(c"SLSGetWindowTags")?,
            })
        })
        .as_ref()
}

impl SkyLight {
    /// The space on screen on each display.
    fn current_spaces(&self) -> Vec<i64> {
        // SAFETY: returns a +1 array of dictionaries, or null.
        let Some(displays) = (unsafe { Retained::from_raw((self.display_spaces)(self.connection)) }) else {
            return Vec::new();
        };
        displays
            .iter()
            .filter_map(|display| {
                let display = display.downcast::<NSDictionary>().ok()?;
                let current = display.objectForKey(ns_string!("Current Space"))?.downcast::<NSDictionary>().ok()?;
                Some(current.objectForKey(ns_string!("ManagedSpaceID"))?.downcast::<NSNumber>().ok()?.longLongValue())
            })
            .collect()
    }

    fn spaces(&self, window: u32) -> Vec<i64> {
        let windows = NSArray::from_retained_slice(&[NSNumber::new_u32(window)]);
        // SAFETY: returns a +1 array of numbers, or null.
        let spaces = unsafe { Retained::from_raw((self.spaces_for_windows)(self.connection, ALL_SPACES, &*windows)) };
        spaces
            .map(|spaces| {
                spaces.iter().filter_map(|space| Some(space.downcast::<NSNumber>().ok()?.longLongValue())).collect()
            })
            .unwrap_or_default()
    }

    fn is_ordered_in(&self, window: u32) -> bool {
        let mut ordered_in = 0;
        unsafe { (self.is_ordered_in)(self.connection, window, &mut ordered_in) == 0 && ordered_in != 0 }
    }

    fn tags(&self, window: u32) -> u64 {
        let mut tags = [0u64; 2];
        if unsafe { (self.tags)(self.connection, window, tags.as_mut_ptr(), 64) } == 0 { tags[0] } else { 0 }
    }
}

/// Where the windows of the apps in `pids` are. Apps with no windows are left out.
pub fn survey(pids: &HashSet<i32>) -> HashMap<i32, Presence> {
    let mut presence = HashMap::new();
    let Some(skylight) = skylight() else { return presence };
    let current = skylight.current_spaces();
    let Some(list) =
        CGWindowListCopyWindowInfo(CGWindowListOption::OptionAll | CGWindowListOption::ExcludeDesktopElements, 0)
    else {
        return presence;
    };
    // SAFETY: CFArray is toll-free bridged to NSArray, and this one holds dictionaries.
    let list: &NSArray<NSDictionary> = unsafe { &*CFRetained::as_ptr(&list).as_ptr().cast() };
    for info in list {
        let number = |key| info.objectForKey(key).and_then(|number| number.downcast::<NSNumber>().ok());
        let (Some(pid), Some(window), Some(layer)) = (
            number(ns_string!("kCGWindowOwnerPID")),
            number(ns_string!("kCGWindowNumber")),
            number(ns_string!("kCGWindowLayer")),
        ) else {
            continue;
        };
        // Ordinary windows are on layer 0; menus, panels and the like sit above.
        let pid = pid.intValue();
        if layer.intValue() != 0 || !pids.contains(&pid) {
            continue;
        }
        let window = window.unsignedIntValue();
        let minimized = skylight.tags(window) & MINIMIZED != 0;
        // Closed windows an app keeps around are neither ordered in nor minimized, though
        // they still belong to a space.
        if !minimized && !skylight.is_ordered_in(window) {
            continue;
        }
        let spaces = skylight.spaces(window);
        if spaces.is_empty() {
            continue;
        }
        let here = spaces.iter().any(|space| current.contains(space));
        let app: &mut Presence = presence.entry(pid).or_default();
        match (here, minimized) {
            (true, false) => app.open += 1,
            (true, true) => app.minimized += 1,
            (false, _) => app.elsewhere += 1,
        }
    }
    presence
}

/// `ErisDock --windows` lists the running apps with what the dock makes of their windows, and
/// the mark that gets them.
pub fn print() {
    let indicators = Indicators::from_config(&config::load().unwrap_or_default().indicators);
    let mut apps = AppList::default();
    apps.refresh();
    let presence = survey(&apps.apps.iter().map(|app| app.processIdentifier()).collect());
    let frontmost = NSWorkspace::sharedWorkspace().frontmostApplication().map(|app| app.processIdentifier());
    for app in &apps.apps {
        let pid = app.processIdentifier();
        let here = presence.get(&pid).copied().unwrap_or_default();
        let status = Status::of(here, Some(pid) == frontmost, app.isHidden());
        let name = app.localizedName().map(|name| name.to_string()).unwrap_or_default();
        println!(
            "{name:24} {:16} {:16} {here:?}",
            status.key(),
            indicators.get(status).for_windows(here.windows(status)).to_string()
        );
    }
}
