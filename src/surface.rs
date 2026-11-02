use std::collections::HashSet;

use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2_app_kit::{
    NSCompositingOperation, NSFont, NSFontWeightMedium, NSGraphicsContext, NSImage, NSRunningApplication,
};
use objc2_core_foundation::{CFRetained, CFType, CGPoint, CGRect, CGSize};
use objc2_core_graphics::{
    CGBitmapContextCreate, CGBitmapContextCreateImage, CGColor, CGColorSpace, CGImage, CGImageAlphaInfo,
    kCGColorSpaceSRGB,
};
use objc2_foundation::{NSRect, NSString};
use objc2_quartz_core::{
    CALayer, CAShapeLayer, CATextLayer, CATransaction, CATransform3D, kCAAlignmentCenter, kCAFilterTrilinear,
    kCAGravityResizeAspect, kCATruncationEnd,
};

use crate::layout::{self, Motion};
use crate::settings::Settings;
use crate::shape::DockShape;
use crate::spring::Spring;

/// One running app in the dock: its icon, the dot under it, and the springs that move them.
pub struct Item {
    pub app: Retained<NSRunningApplication>,
    pub pid: i32,
    pub name: String,
    icon: Retained<CALayer>,
    dot: Retained<CALayer>,
    /// 0 to 1 as the app arrives and back to 0 as it leaves; its slot in the row grows and
    /// shrinks with it, so its neighbours slide over.
    appear: Spring,
    hover: Spring,
    press: Spring,
    /// 1 while the app is frontmost, which stretches its dot into a bar.
    active: Spring,
    leaving: bool,
    /// Where `render` last put it: the icon's centre, and the slot that answers the pointer.
    pub center: f64,
    pub hit: CGRect,
}

impl Item {
    fn springs(&mut self) -> [&mut Spring; 4] {
        [&mut self.appear, &mut self.hover, &mut self.press, &mut self.active]
    }

    fn is_settled(&self) -> bool {
        self.appear.is_resting() && self.hover.is_resting() && self.press.is_resting() && self.active.is_resting()
    }
}

/// The dock's layers: the black body rising out of the bottom edge, the icons in it, and the
/// name of the app under the pointer above it. Everything moves on springs, stepped once a
/// frame and then drawn by `render`.
pub struct Surface {
    pub settings: Settings,
    /// 0 tucked into the edge, 1 risen to full height.
    rise: Spring,
    /// 0 pinched to `collapsed_width`, 1 at full width.
    spread: Spring,
    pub items: Vec<Item>,
    hovered: Option<i32>,
    pressed: Option<i32>,
    /// Off while a menu is open, which puts the name away.
    pub shows_label: bool,
    pub shape: DockShape,
    pub bounds: CGRect,
    scale: f64,
    pub root: Retained<CALayer>,
    body: Retained<CAShapeLayer>,
    label: Retained<CALayer>,
    label_text: Retained<CATextLayer>,
    label_name: String,
    label_shown: Spring,
    label_x: Spring,
    label_width: Spring,
    white: CFRetained<CGColor>,
}

impl Surface {
    pub fn new(settings: Settings, scale: f64) -> Self {
        let black = CGColor::new_generic_gray(0.0, 1.0);
        let white = CGColor::new_generic_gray(1.0, 1.0);
        let root = CALayer::new();
        let body = CAShapeLayer::new();
        body.setFillColor(Some(&black));
        body.setShadowColor(Some(&black));
        body.setShadowOpacity(0.35);
        body.setShadowRadius(12.0);
        body.setShadowOffset(CGSize::ZERO);
        root.addSublayer(&body);

        let label = CALayer::new();
        label.setBackgroundColor(Some(&black));
        label.setOpacity(0.0);
        let label_text = CATextLayer::new();
        let font = NSFont::systemFontOfSize_weight(12.0, unsafe { NSFontWeightMedium });
        // SAFETY: NSFont is toll-free bridged with CTFont, which CATextLayer takes.
        unsafe { label_text.setFont(Some(&*(Retained::as_ptr(&font) as *const CFType))) };
        label_text.setFontSize(12.0);
        label_text.setForegroundColor(Some(&white));
        label_text.setAlignmentMode(unsafe { kCAAlignmentCenter });
        label_text.setTruncationMode(unsafe { kCATruncationEnd });
        label.addSublayer(&label_text);
        root.addSublayer(&label);

        let mut surface = Self {
            rise: Spring::new(0.0, settings.rise_response, settings.rise_damping),
            spread: Spring::new(0.0, settings.spread_response, settings.spread_damping),
            settings,
            items: Vec::new(),
            hovered: None,
            pressed: None,
            shows_label: true,
            shape: DockShape::default(),
            bounds: CGRect::ZERO,
            scale: 0.0,
            root,
            body,
            label,
            label_text,
            label_name: String::new(),
            label_shown: Spring::new(0.0, 0.24, 0.8),
            label_x: Spring::new(0.0, 0.32, 0.82).with_epsilon(0.05),
            label_width: Spring::new(0.0, 0.32, 0.82).with_epsilon(0.05),
            white,
        };
        surface.set_scale(scale);
        surface
    }

    /// Tall enough for the body's overshoot, a hovered icon and the name above it.
    pub fn panel_height(&self) -> f64 {
        (self.settings.height() * 1.3 + 44.0).ceil()
    }

    /// The body's width once everything has settled.
    pub fn resting_width(&self) -> f64 {
        self.settings.resting_width(self.items.iter().filter(|item| !item.leaving).count())
    }

    pub fn is_settled(&self) -> bool {
        self.rise.is_resting()
            && self.spread.is_resting()
            && self.label_shown.is_resting()
            && self.label_x.is_resting()
            && self.label_width.is_resting()
            && self.items.iter().all(Item::is_settled)
    }

    pub fn set_risen(&mut self, risen: bool) {
        let s = &self.settings;
        if risen {
            self.rise.animate_with(1.0, s.rise_response, s.rise_damping);
            self.spread.animate_with(1.0, s.spread_response, s.spread_damping);
        } else {
            // Aimed a little past the edge, so it closes instead of creeping the last pixel
            // shut; `step` stops it at 0.
            self.rise.animate_with(-0.08, 0.3, 0.92);
            self.spread.animate_with(0.0, 0.34, 1.0);
        }
    }

    /// Starts the next rise over from the edge.
    pub fn tuck_away(&mut self) {
        self.rise.set(0.0);
        self.spread.set(0.0);
    }

    pub fn set_scale(&mut self, scale: f64) {
        if scale == self.scale {
            return;
        }
        self.scale = scale;
        for layer in [&*self.root, &self.body, &self.label, &self.label_text] {
            layer.setContentsScale(scale);
        }
        for item in &self.items {
            item.dot.setContentsScale(scale);
        }
    }

    /// Brings the row in line with `apps`: new ones pop in at the end, gone ones shrink away
    /// where they were. Without `animated`, everything jumps to where it's headed.
    pub fn set_apps(&mut self, apps: &[Retained<NSRunningApplication>], frontmost: Option<i32>, animated: bool) {
        let live: HashSet<i32> = apps.iter().map(|app| app.processIdentifier()).collect();
        for item in &mut self.items {
            if !item.leaving && !live.contains(&item.pid) {
                item.leaving = true;
                item.appear.animate_with(0.0, 0.3, 1.0);
            }
        }
        if self.hovered.is_some_and(|pid| !live.contains(&pid)) {
            self.hover(None);
        }
        let known: HashSet<i32> = self.items.iter().map(|item| item.pid).collect();
        for app in apps.iter().filter(|app| !known.contains(&app.processIdentifier())) {
            let item = self.make_item(app.clone());
            self.root.insertSublayer_below(&item.icon, Some(&self.label));
            self.root.insertSublayer_below(&item.dot, Some(&self.label));
            self.items.push(item);
        }
        for item in &mut self.items {
            item.active.animate(if Some(item.pid) == frontmost { 1.0 } else { 0.0 });
        }
        if !animated {
            for item in &mut self.items {
                item.springs().into_iter().for_each(Spring::settle);
            }
            self.remove_departed();
        }
    }

    fn make_item(&self, app: Retained<NSRunningApplication>) -> Item {
        let size = self.settings.icon_size;
        let icon = CALayer::new();
        icon.setAnchorPoint(CGPoint::new(0.5, 0.0));
        icon.setBounds(CGRect::new(CGPoint::ZERO, CGSize::new(size, size)));
        icon.setContentsGravity(unsafe { kCAGravityResizeAspect });
        icon.setMinificationFilter(unsafe { kCAFilterTrilinear });
        // Drawn larger than it sits, so it stays sharp while hovered, at the densest screen's scale.
        let side = (size * 1.3 * max_backing_scale()).ceil() as usize;
        if let Some(image) = app.icon().and_then(|image| rasterize(&image, side)) {
            // SAFETY: a CGImage is valid layer contents.
            unsafe { icon.setContents(Some(cf_object(&image))) };
        }
        let dot = CALayer::new();
        dot.setBackgroundColor(Some(&self.white));
        dot.setContentsScale(self.scale);
        let mut appear = Spring::new(0.0, 0.42, 0.6);
        appear.animate(1.0);
        Item {
            pid: app.processIdentifier(),
            name: app.localizedName().map(|name| name.to_string()).unwrap_or_default(),
            app,
            icon,
            dot,
            appear,
            hover: Spring::new(0.0, 0.3, 0.55),
            press: Spring::new(0.0, 0.16, 0.5),
            active: Spring::new(0.0, 0.3, 0.75),
            leaving: false,
            center: 0.0,
            hit: CGRect::ZERO,
        }
    }

    pub fn step(&mut self, dt: f64) {
        for spring in [&mut self.rise, &mut self.spread, &mut self.label_shown, &mut self.label_x, &mut self.label_width] {
            spring.step(dt);
        }
        if self.rise.target < 0.0 && self.rise.value <= 0.0 {
            self.rise.set(0.0);
        }
        for item in &mut self.items {
            item.springs().into_iter().for_each(|spring| spring.step(dt));
        }
        self.remove_departed();
    }

    fn remove_departed(&mut self) {
        self.items.retain(|item| {
            let gone = item.leaving && item.appear.is_resting();
            if gone {
                item.icon.removeFromSuperlayer();
                item.dot.removeFromSuperlayer();
            }
            !gone
        });
    }

    pub fn render(&mut self) {
        let motions: Vec<Motion> = self
            .items
            .iter()
            .map(|item| Motion {
                appear: item.appear.value,
                hover: item.hover.value,
                press: item.press.value,
                active: item.active.value,
            })
            .collect();
        let mid_x = self.bounds.origin.x + self.bounds.size.width / 2.0;
        let (shape, placements) = layout::place(&self.settings, mid_x, self.rise.value, self.spread.value, &motions);
        self.shape = shape;
        let path = shape.path();

        CATransaction::begin();
        CATransaction::setDisableActions(true);
        self.body.setFrame(self.bounds);
        self.body.setPath(Some(&path));
        self.body.setShadowPath(Some(&path));
        let dot_size = self.settings.dot_size;
        for (item, place) in self.items.iter_mut().zip(&placements) {
            item.center = place.center;
            item.hit = place.hit;
            item.icon.setPosition(place.icon_base);
            item.icon.setTransform(CATransform3D::new_scale(place.icon_scale.0, place.icon_scale.1, 1.0));
            let dimmed = if item.app.isHidden() { 0.5 } else { 1.0 };
            item.icon.setOpacity((place.opacity * dimmed) as f32);
            item.dot.setBounds(CGRect::new(CGPoint::ZERO, CGSize::new(place.dot_width, dot_size)));
            item.dot.setCornerRadius(dot_size / 2.0);
            item.dot.setPosition(place.dot_position);
            item.dot.setOpacity(place.dot_opacity as f32);
        }
        self.render_label();
        CATransaction::commit();
    }

    fn render_label(&mut self) {
        let height = self.shape.height;
        let target = match self.hovered {
            Some(pid) if self.shows_label => self.live_item(pid).map(|item| (item.name.clone(), item.center)),
            _ => None,
        };
        self.label_shown.animate(if target.is_some() { 1.0 } else { 0.0 });
        if let Some((name, center)) = target {
            if name != self.label_name {
                unsafe { self.label_text.setString(Some(&NSString::from_str(&name))) };
                self.label_name = name;
            }
            let width = (self.label_text.preferredFrameSize().width.ceil() + 20.0).min(320.0);
            if self.label_shown.value < 0.05 {
                self.label_x.set(center);
                self.label_width.set(width);
            } else {
                self.label_x.animate(center);
                self.label_width.animate(width);
            }
        }
        let (shown, width, side) = (self.label_shown.value, self.label_width.value, 22.0);
        let line = self.label_text.preferredFrameSize().height.ceil();
        let x = self.label_x.value.min(self.bounds.size.width - width / 2.0 - 4.0).max(width / 2.0 + 4.0);
        self.label.setBounds(CGRect::new(CGPoint::ZERO, CGSize::new(width, side)));
        self.label.setCornerRadius(side / 2.0);
        self.label.setPosition(CGPoint::new(x, height + 10.0 + side / 2.0 - 6.0 * (1.0 - shown)));
        self.label.setOpacity(shown.clamp(0.0, 1.0) as f32);
        let scale = 0.85 + 0.15 * shown;
        self.label.setTransform(CATransform3D::new_scale(scale, scale, 1.0));
        self.label_text.setFrame(CGRect::new(
            CGPoint::new(8.0, (side - line) / 2.0),
            CGSize::new((width - 16.0).max(0.0), line),
        ));
    }

    fn live_item(&self, pid: i32) -> Option<&Item> {
        self.items.iter().find(|item| item.pid == pid && !item.leaving)
    }

    // Pointer

    pub fn body_contains(&self, point: CGPoint) -> bool {
        let body = self.shape.body_rect();
        self.shape.height > 1.0
            && point.y >= 0.0
            && point.y <= self.shape.height
            && point.x >= body.origin.x
            && point.x <= body.origin.x + body.size.width
    }

    /// Close enough to the dock, as it will be once risen, to keep it up.
    pub fn is_near(&self, point: CGPoint) -> bool {
        let mid_x = self.bounds.origin.x + self.bounds.size.width / 2.0;
        (point.x - mid_x).abs() <= self.resting_width() / 2.0 + self.settings.trigger_margin.max(24.0)
            && point.y <= self.shape.height.max(self.settings.height()) + 32.0
    }

    pub fn item_at(&self, point: CGPoint) -> Option<&Item> {
        if !self.body_contains(point) {
            return None;
        }
        self.items.iter().find(|item| !item.leaving && contains(item.hit, point))
    }

    /// Returns whether the hovered item changed.
    pub fn hover(&mut self, point: Option<CGPoint>) -> bool {
        let pid = point.and_then(|point| self.item_at(point)).map(|item| item.pid);
        if pid == self.hovered {
            return false;
        }
        let hovered = self.hovered;
        for item in &mut self.items {
            if Some(item.pid) == hovered {
                item.hover.animate(0.0);
            } else if Some(item.pid) == pid {
                item.hover.animate(1.0);
            }
        }
        self.hovered = pid;
        true
    }

    pub fn press(&mut self, pid: Option<i32>) {
        let pressed = self.pressed;
        for item in &mut self.items {
            if Some(item.pid) == pressed {
                item.press.animate(0.0);
            }
            if Some(item.pid) == pid {
                item.press.animate(1.0);
            }
        }
        self.pressed = pid;
    }

    pub fn pressed(&self) -> Option<i32> {
        self.pressed
    }
}

fn contains(rect: CGRect, point: CGPoint) -> bool {
    point.x >= rect.origin.x
        && point.x < rect.origin.x + rect.size.width
        && point.y >= rect.origin.y
        && point.y < rect.origin.y + rect.size.height
}

pub fn cf_object(cf: &CFType) -> &AnyObject {
    cf.as_ref()
}

fn max_backing_scale() -> f64 {
    objc2::MainThreadMarker::new()
        .map(|mtm| objc2_app_kit::NSScreen::screens(mtm).iter().map(|s| s.backingScaleFactor()).fold(1.0, f64::max))
        .unwrap_or(2.0)
}

/// Draws `image` into a square bitmap `side` pixels across, letting NSImage pick its best
/// representation for that size.
fn rasterize(image: &NSImage, side: usize) -> Option<CFRetained<CGImage>> {
    let space = CGColorSpace::with_name(Some(unsafe { kCGColorSpaceSRGB }))?;
    // SAFETY: a null data pointer has the context allocate its own memory.
    let context = unsafe {
        CGBitmapContextCreate(std::ptr::null_mut(), side, side, 8, 0, Some(&space), CGImageAlphaInfo::PremultipliedLast.0)
    }?;
    let graphics = NSGraphicsContext::graphicsContextWithCGContext_flipped(&context, false);
    NSGraphicsContext::saveGraphicsState_class();
    NSGraphicsContext::setCurrentContext(Some(&graphics));
    let rect = NSRect::new(CGPoint::ZERO, CGSize::new(side as f64, side as f64));
    image.drawInRect_fromRect_operation_fraction(rect, NSRect::ZERO, NSCompositingOperation::Copy, 1.0);
    NSGraphicsContext::restoreGraphicsState_class();
    CGBitmapContextCreateImage(Some(&context))
}
