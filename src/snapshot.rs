use std::path::Path;

use objc2::AnyThread;
use objc2_app_kit::{NSBitmapImageFileType, NSBitmapImageRep, NSWorkspace};
use objc2_core_foundation::{CFRetained, CGPoint, CGRect, CGSize};
use objc2_core_graphics::{
    CGBitmapContextCreate, CGBitmapContextCreateImage, CGColor, CGColorSpace, CGContext, CGImage, CGImageAlphaInfo,
    kCGColorSpaceSRGB,
};
use objc2_foundation::{NSDictionary, NSString};

use crate::apps::AppList;
use crate::settings::Settings;
use crate::surface::Surface;

const SCALE: f64 = 2.0;

/// `ErisDock --snapshot <file.png>` draws the dock rising, hovered, pressed and tucking away,
/// frame by frame, one under another in a single PNG, without putting anything on screen.
pub fn write(path: &Path) {
    let mut surface = Surface::new(Settings::load(), SCALE);
    surface.bounds = CGRect::new(CGPoint::ZERO, CGSize::new(640.0, surface.panel_height()));
    surface.root.setFrame(surface.bounds);
    let mut apps = AppList::default();
    apps.refresh();
    apps.apps.truncate(8);
    let frontmost = NSWorkspace::sharedWorkspace().frontmostApplication().map(|app| app.processIdentifier());
    surface.set_apps(&apps.apps, frontmost, false);
    surface.render();

    let mut frames = Vec::new();
    let mut run = |surface: &mut Surface, seconds: f64, every: usize| {
        for i in 1..=(seconds * 60.0) as usize {
            surface.step(1.0 / 60.0);
            surface.render();
            if i % every == 0 {
                frames.extend(capture(surface));
            }
        }
    };
    surface.set_risen(true);
    run(&mut surface, 0.6, 2);
    let hovered = surface.items.get(2).map(|item| (item.pid, item.hit));
    if let Some((pid, hit)) = hovered {
        surface.hover(Some(CGPoint::new(hit.origin.x + hit.size.width / 2.0, hit.size.height / 2.0)));
        run(&mut surface, 0.36, 9);
        surface.press(Some(pid));
        run(&mut surface, 0.12, 4);
        surface.press(None);
        run(&mut surface, 0.3, 10);
        surface.hover(None);
    }
    surface.set_risen(false);
    run(&mut surface, 0.45, 3);

    let Some(sheet) = stack(&frames) else {
        eprintln!("ErisDock: couldn't draw the snapshot");
        return;
    };
    let rep = NSBitmapImageRep::initWithCGImage(NSBitmapImageRep::alloc(), &sheet);
    let png = unsafe { rep.representationUsingType_properties(NSBitmapImageFileType::PNG, &NSDictionary::new()) };
    match png {
        Some(png) if png.writeToFile_atomically(&NSString::from_str(&path.to_string_lossy()), true) => {
            println!("{} frames in {}", frames.len(), path.display());
        }
        _ => eprintln!("ErisDock: couldn't write {}", path.display()),
    }
}

fn bitmap(width: f64, height: f64) -> Option<CFRetained<CGContext>> {
    let space = CGColorSpace::with_name(Some(unsafe { kCGColorSpaceSRGB }))?;
    unsafe {
        CGBitmapContextCreate(
            std::ptr::null_mut(),
            (width * SCALE) as usize,
            (height * SCALE) as usize,
            8,
            0,
            Some(&space),
            CGImageAlphaInfo::PremultipliedLast.0,
        )
    }
}

/// One frame, over a mid grey so the black body shows.
fn capture(surface: &Surface) -> Option<CFRetained<CGImage>> {
    let size = surface.bounds.size;
    let context = bitmap(size.width, size.height)?;
    CGContext::scale_ctm(Some(&context), SCALE, SCALE);
    CGContext::set_fill_color_with_color(Some(&context), Some(&CGColor::new_generic_gray(0.62, 1.0)));
    CGContext::fill_rect(Some(&context), surface.bounds);
    surface.root.renderInContext(&context);
    CGBitmapContextCreateImage(Some(&context))
}

fn stack(frames: &[CFRetained<CGImage>]) -> Option<CFRetained<CGImage>> {
    let first = frames.first()?;
    let (width, height) = (CGImage::width(Some(first)) as f64, CGImage::height(Some(first)) as f64);
    let context = bitmap(width / SCALE, height / SCALE * frames.len() as f64)?;
    for (i, frame) in frames.iter().enumerate() {
        let y = height * (frames.len() - 1 - i) as f64;
        CGContext::draw_image(Some(&context), CGRect::new(CGPoint::new(0.0, y), CGSize::new(width, height)), Some(frame));
    }
    CGBitmapContextCreateImage(Some(&context))
}
