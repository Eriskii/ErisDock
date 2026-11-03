//! The badges apps put on their Dock icons (`NSDockTile.badgeLabel`), which LaunchServices
//! keeps as each app's "StatusLabel". Read through private LaunchServices calls, looked up at
//! run time, so badges just don't show if they're gone.

use std::ffi::c_void;
use std::sync::OnceLock;

use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2_foundation::{NSArray, NSDictionary, NSString, ns_string};

type CreateAsn = unsafe extern "C" fn(*const c_void, i32) -> *mut AnyObject;
type CopyInformation = unsafe extern "C" fn(i32, *const AnyObject, *const NSArray<NSString>) -> *mut NSDictionary;

/// LaunchServices' default session.
const SESSION: i32 = -2;

fn functions() -> Option<(CreateAsn, CopyInformation)> {
    static FUNCTIONS: OnceLock<Option<(usize, usize)>> = OnceLock::new();
    let (create, copy) = (*FUNCTIONS.get_or_init(|| {
        let find = |name: &std::ffi::CStr| {
            let symbol = unsafe { libc::dlsym(libc::RTLD_DEFAULT, name.as_ptr()) };
            (!symbol.is_null()).then_some(symbol as usize)
        };
        Some((find(c"_LSASNCreateWithPid")?, find(c"_LSCopyApplicationInformation")?))
    }))?;
    // SAFETY: the symbols have had these signatures since 10.6.
    unsafe {
        Some((std::mem::transmute::<usize, CreateAsn>(create), std::mem::transmute::<usize, CopyInformation>(copy)))
    }
}

/// The badge on `pid`'s icon, if it has one.
pub fn label(pid: i32) -> Option<String> {
    let (create, copy) = functions()?;
    // SAFETY: both return +1 references, or null.
    let asn = unsafe { Retained::from_raw(create(std::ptr::null(), pid)) }?;
    let keys = NSArray::from_slice(&[ns_string!("StatusLabel")]);
    let information = unsafe { Retained::from_raw(copy(SESSION, &*asn, &*keys)) }?;
    let status = information.objectForKey(ns_string!("StatusLabel"))?.downcast::<NSDictionary>().ok()?;
    let label = status.objectForKey(ns_string!("label"))?.downcast::<NSString>().ok()?.to_string();
    (!label.is_empty()).then_some(label)
}
