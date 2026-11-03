use std::collections::HashMap;

use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2_app_kit::{
    NSCompositingOperation, NSFont, NSFontWeightMedium, NSGraphicsContext, NSImage, NSRunningApplication, NSWorkspace,
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

use crate::layout::{self, Motion, Placement};
use crate::row::{FINDER, Spec};
use crate::settings::Settings;
use crate::shape::DockShape;
use crate::spring::Spring;

/// The badge an app puts on its icon: a red pill at its top right.
struct Badge {
    layer: Retained<CALayer>,
    text: Retained<CATextLayer>,
    label: Option<String>,
    width: f64,
    shown: Spring,
}

/// One app in the dock, running or pinned or both, or a gap held open for one being dragged
/// in: its icon, the dot under it, its badge, and the springs that move them.
pub struct Tile {
    id: u64,
    pub key: String,
    /// A gap, with no app.
    placeholder: bool,
    /// The app's process, while it runs.
    pub app: Option<Retained<NSRunningApplication>>,
    /// The app's bundle.
    pub path: Option<String>,
    pub pinned: bool,
    pub name: String,
    /// Opened from the dock and not running yet, so bouncing.
    pub launching: bool,
    icon: Retained<CALayer>,
    dot: Retained<CALayer>,
    badge: Badge,
    /// 0 to 1 as the tile arrives and back to 0 as it leaves; its slot grows and shrinks with
    /// it, so its neighbours slide over.
    appear: Spring,
    hover: Spring,
    press: Spring,
    /// 1 while the app is frontmost, which stretches its dot into a bar.
    active: Spring,
    /// 1 while the app runs, which shows its dot.
    running: Spring,
    shift_x: Spring,
    shift_y: Spring,
    bounce: Spring,
    leaving: bool,
    /// The middle of its slot, and the slot that answers the pointer, as last laid out.
    pub center: f64,
    hit: CGRect,
    /// Where its icon's foot was last drawn, before hovering and bouncing.
    drawn: Option<CGPoint>,
}

impl Tile {
    fn springs(&mut self) -> [&mut Spring; 9] {
        [
            &mut self.appear,
            &mut self.hover,
            &mut self.press,
            &mut self.active,
            &mut self.running,
            &mut self.shift_x,
            &mut self.shift_y,
            &mut self.bounce,
            &mut self.badge.shown,
        ]
    }

    fn is_settled(&self) -> bool {
        [
            &self.appear,
            &self.hover,
            &self.press,
            &self.active,
            &self.running,
            &self.shift_x,
            &self.shift_y,
            &self.bounce,
            &self.badge.shown,
        ]
        .iter()
        .all(|spring| spring.is_resting())
    }

    /// In the row and staying there: not a gap, not on its way out.
    fn is_live(&self) -> bool {
        !self.placeholder && !self.leaving
    }

    pub fn is_finder(&self) -> bool {
        self.key == FINDER
    }

    fn motion(&self) -> Motion {
        Motion {
            appear: self.appear.value,
            hover: self.hover.value,
            press: self.press.value,
            active: self.active.value,
            running: self.running.value,
            shift: (self.shift_x.value, self.shift_y.value),
            bounce: self.bounce.value.max(0.0),
        }
    }

    fn remove_layers(&self) {
        self.icon.removeFromSuperlayer();
        self.dot.removeFromSuperlayer();
    }
}

/// A tile lifted out of the row by the pointer.
struct Dragged {
    tile: Tile,
    /// From the pointer to the icon's foot.
    grab: CGPoint,
    pointer: CGPoint,
    /// Dragged far enough off the dock to be unpinned when dropped.
    outside: bool,
}

/// Where a drag ended: the dragged app, and the place in the row (counting the other apps)
/// it was dropped at.
pub struct DragEnd {
    pub key: String,
    pub index: usize,
    pub outside: bool,
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
    tiles: Vec<Tile>,
    dragged: Option<Dragged>,
    /// Unpinned tiles dropped off the dock, vanishing where they were let go.
    poofs: Vec<Tile>,
    next_id: u64,
    hovered: Option<String>,
    pressed: Option<String>,
    /// Off while a menu is open or something's being dragged, which puts the name away.
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
    red: CFRetained<CGColor>,
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
        set_font(&label_text, &NSFont::systemFontOfSize_weight(12.0, unsafe { NSFontWeightMedium }));
        label_text.setForegroundColor(Some(&white));
        label_text.setAlignmentMode(unsafe { kCAAlignmentCenter });
        label_text.setTruncationMode(unsafe { kCATruncationEnd });
        label.addSublayer(&label_text);
        root.addSublayer(&label);

        let mut surface = Self {
            rise: settings.animation.rise.spring(0.0),
            spread: settings.animation.spread.spring(0.0),
            label_shown: settings.animation.label.spring(0.0),
            label_x: settings.animation.label_move.spring(0.0).with_epsilon(0.05),
            label_width: settings.animation.label_move.spring(0.0).with_epsilon(0.05),
            settings,
            tiles: Vec::new(),
            dragged: None,
            poofs: Vec::new(),
            next_id: 0,
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
            white,
            red: CGColor::new_srgb(1.0, 0.231, 0.188, 1.0),
        };
        surface.set_scale(scale);
        surface
    }

    /// Tall enough for the body's overshoot, a hovered icon and the name above it, and for an
    /// icon dragged up off the dock.
    pub fn panel_height(&self) -> f64 {
        let height = self.settings.height();
        (height * 1.3 + 44.0).max(height + 200.0).ceil()
    }

    /// The body's width once everything has settled.
    pub fn resting_width(&self) -> f64 {
        self.settings.resting_width(self.tiles.iter().filter(|tile| tile.is_live()).count())
    }

    pub fn is_settled(&self) -> bool {
        self.rise.is_resting()
            && self.spread.is_resting()
            && self.label_shown.is_resting()
            && self.label_x.is_resting()
            && self.label_width.is_resting()
            && self.tiles.iter().all(Tile::is_settled)
            && self.dragged.as_ref().is_none_or(|dragged| dragged.tile.is_settled())
            && self.poofs.is_empty()
    }

    pub fn set_risen(&mut self, risen: bool) {
        let a = &self.settings.animation;
        if risen {
            self.rise.animate_spec(1.0, a.rise);
            self.spread.animate_spec(1.0, a.spread);
        } else {
            // Aimed a little past the edge, so it closes instead of creeping the last pixel
            // shut; `step` stops it at 0.
            self.rise.animate_spec(-0.08, a.tuck);
            self.spread.animate_spec(0.0, a.tuck_spread);
        }
    }

    pub fn is_risen(&self) -> bool {
        self.rise.target > 0.0
    }

    /// Starts the next rise over from the edge.
    pub fn tuck_away(&mut self) {
        self.rise.set(0.0);
        self.spread.set(0.0);
    }

    /// Takes on new settings: new sizes at once, new animations from wherever things are.
    pub fn apply(&mut self, settings: Settings) {
        let resized = settings.icon_size != self.settings.icon_size;
        self.settings = settings;
        let a = self.settings.animation;
        self.label_shown.retune(a.label);
        self.label_x.retune(a.label_move);
        self.label_width.retune(a.label_move);
        if self.is_risen() {
            self.set_risen(true);
        }
        let (side, size) = (self.icon_pixels(), self.settings.icon_size);
        let dragged = self.dragged.as_mut().map(|dragged| &mut dragged.tile);
        for tile in self.tiles.iter_mut().chain(dragged).chain(self.poofs.iter_mut()) {
            tile.hover.retune(a.hover);
            tile.press.retune(a.press);
            tile.active.retune(a.active);
            tile.shift_x.retune(a.slide);
            tile.shift_y.retune(a.slide);
            tile.bounce.retune(a.bounce);
            let appear = if tile.placeholder {
                a.slide
            } else if tile.leaving {
                a.leave
            } else {
                a.appear
            };
            tile.appear.retune(appear);
            if resized && !tile.placeholder {
                tile.icon.setBounds(CGRect::new(CGPoint::ZERO, CGSize::new(size, size)));
                set_icon(&tile.icon, tile.app.as_deref(), tile.path.as_deref(), side);
                // Laid out again at the new size with the next poll.
                tile.badge.label = None;
                tile.badge.shown.set(0.0);
            }
        }
    }

    /// Icons are drawn larger than they sit, so they stay sharp while hovered, at the densest
    /// screen's scale.
    fn icon_pixels(&self) -> usize {
        (self.settings.icon_size * 1.3 * max_backing_scale()).ceil() as usize
    }

    pub fn set_scale(&mut self, scale: f64) {
        if scale == self.scale {
            return;
        }
        self.scale = scale;
        for layer in [&*self.root, &self.body, &self.label, &self.label_text] {
            layer.setContentsScale(scale);
        }
        for tile in &self.tiles {
            tile.dot.setContentsScale(scale);
        }
    }

    // The row

    /// Brings the row in line with `row`: new apps pop in, gone ones shrink away where they
    /// were, and ones that moved slide over from where they were drawn. Without `animated`,
    /// everything jumps to where it's headed.
    pub fn set_row(
        &mut self,
        row: &[Spec],
        apps: &HashMap<i32, Retained<NSRunningApplication>>,
        frontmost: Option<i32>,
        animated: bool,
    ) {
        let a = self.settings.animation;
        let order: Vec<u64> = self.tiles.iter().map(|tile| tile.id).collect();
        let mut old: Vec<Option<Tile>> = std::mem::take(&mut self.tiles).into_iter().map(Some).collect();
        let mut tiles = Vec::new();
        for spec in row {
            let app = spec.pid.and_then(|pid| apps.get(&pid)).cloned();
            let found =
                old.iter().position(|tile| tile.as_ref().is_some_and(|tile| tile.key == spec.key && tile.is_live()));
            let mut tile = match found.and_then(|i| old[i].take()) {
                Some(tile) => tile,
                None => self.make_tile(spec, app.clone()),
            };
            tile.app = app;
            tile.pinned = spec.pinned;
            if spec.path.is_some() {
                tile.path = spec.path.clone();
            }
            if let Some(name) = tile.app.as_ref().and_then(|app| app.localizedName()) {
                tile.name = name.to_string();
            }
            tile.running.animate(if tile.app.is_some() { 1.0 } else { 0.0 });
            tile.active.animate(if spec.pid.is_some() && spec.pid == frontmost { 1.0 } else { 0.0 });
            tiles.push(tile);
        }
        // Whatever's left keeps its place, after whatever came before it.
        for mut tile in old.into_iter().flatten() {
            if !tile.leaving && !tile.placeholder {
                tile.leaving = true;
                tile.appear.animate_spec(0.0, a.leave);
            }
            let at = order.iter().position(|id| *id == tile.id).unwrap_or(0);
            let after = order[..at].iter().rev().find_map(|id| tiles.iter().position(|other: &Tile| other.id == *id));
            tiles.insert(after.map_or(0, |i| i + 1), tile);
        }
        if self.hovered.as_ref().is_some_and(|key| !tiles.iter().any(|tile| &tile.key == key && tile.is_live())) {
            self.hovered = None;
        }
        self.tiles = tiles;

        // Tiles that moved slide from where they were drawn into their new slots.
        let (_, placements) = self.layout();
        for (tile, place) in self.tiles.iter_mut().zip(&placements) {
            if let Some(drawn) = tile.drawn {
                tile.shift_x.value = drawn.x - place.slot_base.x;
                tile.shift_y.value = drawn.y - place.slot_base.y;
                tile.shift_x.animate_spec(0.0, a.slide);
                tile.shift_y.animate_spec(0.0, a.slide);
            }
        }
        if !animated {
            for tile in &mut self.tiles {
                tile.springs().into_iter().for_each(Spring::settle);
            }
            self.remove_departed();
        }
    }

    fn make_tile(&mut self, spec: &Spec, app: Option<Retained<NSRunningApplication>>) -> Tile {
        let mut tile = self.blank_tile(spec.key.clone(), false);
        tile.path = spec.path.clone();
        tile.pinned = spec.pinned;
        tile.name = app
            .as_ref()
            .and_then(|app| app.localizedName())
            .map(|name| name.to_string())
            .or_else(|| {
                let path = std::path::Path::new(spec.path.as_deref()?);
                Some(path.file_stem()?.to_string_lossy().into_owned())
            })
            .unwrap_or_default();
        set_icon(&tile.icon, app.as_deref(), spec.path.as_deref(), self.icon_pixels());
        self.root.insertSublayer_below(&tile.icon, Some(&self.label));
        self.root.insertSublayer_below(&tile.dot, Some(&self.label));
        tile.appear.animate(1.0);
        tile
    }

    fn blank_tile(&mut self, key: String, placeholder: bool) -> Tile {
        let a = &self.settings.animation;
        let size = self.settings.icon_size;
        self.next_id += 1;
        let icon = CALayer::new();
        icon.setAnchorPoint(CGPoint::new(0.5, 0.0));
        icon.setBounds(CGRect::new(CGPoint::ZERO, CGSize::new(size, size)));
        icon.setContentsGravity(unsafe { kCAGravityResizeAspect });
        icon.setMinificationFilter(unsafe { kCAFilterTrilinear });
        let dot = CALayer::new();
        dot.setBackgroundColor(Some(&self.white));
        dot.setContentsScale(self.scale);
        let badge = CALayer::new();
        badge.setBackgroundColor(Some(&self.red));
        badge.setOpacity(0.0);
        let text = CATextLayer::new();
        text.setForegroundColor(Some(&self.white));
        text.setAlignmentMode(unsafe { kCAAlignmentCenter });
        // The badge rides the icon, which grows when hovered; drawn finer to stay sharp.
        text.setContentsScale(self.scale.max(2.0) * 1.5);
        badge.setContentsScale(self.scale.max(2.0) * 1.5);
        badge.addSublayer(&text);
        icon.addSublayer(&badge);
        Tile {
            id: self.next_id,
            key,
            placeholder,
            app: None,
            path: None,
            pinned: false,
            name: String::new(),
            launching: false,
            icon,
            dot,
            badge: Badge { layer: badge, text, label: None, width: 0.0, shown: a.appear.spring(0.0) },
            appear: (if placeholder { a.slide } else { a.appear }).spring(0.0),
            hover: a.hover.spring(0.0),
            press: a.press.spring(0.0),
            active: a.active.spring(0.0),
            running: a.active.spring(0.0),
            shift_x: a.slide.spring(0.0).with_epsilon(0.05),
            shift_y: a.slide.spring(0.0).with_epsilon(0.05),
            bounce: a.bounce.spring(0.0).with_epsilon(0.002),
            leaving: false,
            center: 0.0,
            hit: CGRect::ZERO,
            drawn: None,
        }
    }

    pub fn step(&mut self, dt: f64) {
        for spring in
            [&mut self.rise, &mut self.spread, &mut self.label_shown, &mut self.label_x, &mut self.label_width]
        {
            spring.step(dt);
        }
        if self.rise.target < 0.0 && self.rise.value <= 0.0 {
            self.rise.set(0.0);
        }
        let dragged = self.dragged.as_mut().map(|dragged| &mut dragged.tile);
        for tile in self.tiles.iter_mut().chain(dragged).chain(self.poofs.iter_mut()) {
            tile.springs().into_iter().for_each(|spring| spring.step(dt));
        }
        self.remove_departed();
    }

    fn remove_departed(&mut self) {
        let gone =
            |tile: &Tile| (tile.leaving || tile.placeholder) && tile.appear.is_resting() && tile.appear.value <= 0.0;
        for tile in self.tiles.iter().chain(&self.poofs).filter(|tile| gone(tile)) {
            tile.remove_layers();
        }
        self.tiles.retain(|tile| !gone(tile));
        self.poofs.retain(|tile| !gone(tile));
    }

    fn layout(&self) -> (DockShape, Vec<Placement>) {
        let motions: Vec<Motion> = self.tiles.iter().map(Tile::motion).collect();
        let mid_x = self.bounds.origin.x + self.bounds.size.width / 2.0;
        layout::place(&self.settings, mid_x, self.rise.value, self.spread.value, &motions)
    }

    pub fn render(&mut self) {
        let (shape, placements) = self.layout();
        self.shape = shape;
        let path = shape.path();

        CATransaction::begin();
        CATransaction::setDisableActions(true);
        self.body.setFrame(self.bounds);
        self.body.setPath(Some(&path));
        self.body.setShadowPath(Some(&path));
        let s = &self.settings;
        for (tile, place) in self.tiles.iter_mut().zip(&placements) {
            tile.center = place.center;
            tile.hit = place.hit;
            tile.drawn =
                Some(CGPoint::new(place.slot_base.x + tile.shift_x.value, place.slot_base.y + tile.shift_y.value));
            if tile.placeholder {
                continue;
            }
            tile.icon.setPosition(place.icon_base);
            tile.icon.setTransform(CATransform3D::new_scale(place.icon_scale.0, place.icon_scale.1, 1.0));
            let dimmed = if tile.app.as_ref().is_some_and(|app| app.isHidden()) { 0.5 } else { 1.0 };
            tile.icon.setOpacity((place.opacity * dimmed) as f32);
            tile.dot.setBounds(CGRect::new(CGPoint::ZERO, CGSize::new(place.dot_width, s.dot_size)));
            tile.dot.setCornerRadius(s.dot_size / 2.0);
            tile.dot.setPosition(CGPoint::new(place.icon_base.x, place.dot_position.y));
            tile.dot.setOpacity(place.dot_opacity as f32);
            render_badge(&tile.badge, s.icon_size);
        }

        // A dragged icon follows the pointer, kept within the panel, and fades while it's far
        // enough off the dock to be unpinned.
        let height = self.bounds.size.height;
        if let Some(dragged) = &mut self.dragged {
            let tile = &mut dragged.tile;
            let lift = 1.0 + s.animation.hover_scale;
            let foot = CGPoint::new(
                dragged.pointer.x - dragged.grab.x,
                (dragged.pointer.y - dragged.grab.y).min(height - s.icon_size * lift - 2.0),
            );
            tile.drawn = Some(foot);
            tile.icon.setPosition(foot);
            tile.icon.setTransform(CATransform3D::new_scale(lift, lift, 1.0));
            tile.icon.setOpacity(if dragged.outside { 0.45 } else { 1.0 });
            tile.dot.setOpacity(0.0);
            render_badge(&tile.badge, s.icon_size);
        }
        for tile in &self.poofs {
            let shrink = tile.appear.value.max(0.0);
            if let Some(foot) = tile.drawn {
                tile.icon.setPosition(foot);
            }
            tile.icon.setTransform(CATransform3D::new_scale(shrink, shrink, 1.0));
            tile.icon.setOpacity((shrink * 0.45) as f32);
            tile.dot.setOpacity(0.0);
        }

        self.render_label(&placements);
        CATransaction::commit();
    }

    fn render_label(&mut self, placements: &[Placement]) {
        let height = self.shape.height;
        let target = match &self.hovered {
            Some(key) if self.shows_label => self
                .tiles
                .iter()
                .zip(placements)
                .find(|(tile, _)| &tile.key == key && tile.is_live())
                .map(|(tile, place)| (tile.name.clone(), place.icon_base.x)),
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
        self.label_text
            .setFrame(CGRect::new(CGPoint::new(8.0, (side - line) / 2.0), CGSize::new((width - 16.0).max(0.0), line)));
    }

    // Tiles

    pub fn tile(&self, key: &str) -> Option<&Tile> {
        self.tiles.iter().find(|tile| tile.key == key && tile.is_live())
    }

    fn tile_mut(&mut self, key: &str) -> Option<&mut Tile> {
        let dragged = self.dragged.as_mut().map(|dragged| &mut dragged.tile);
        self.tiles.iter_mut().chain(dragged).find(|tile| tile.key == key && tile.is_live())
    }

    /// The running apps' keys and processes.
    pub fn running(&self) -> Vec<(String, i32)> {
        let dragged = self.dragged.as_ref().map(|dragged| &dragged.tile);
        self.tiles
            .iter()
            .chain(dragged)
            .filter(|tile| tile.is_live())
            .filter_map(|tile| Some((tile.key.clone(), tile.app.as_ref()?.processIdentifier())))
            .collect()
    }

    /// The apps opened from the dock that are still on their way.
    pub fn launching(&self) -> Vec<String> {
        self.tiles.iter().filter(|tile| tile.is_live() && tile.launching).map(|tile| tile.key.clone()).collect()
    }

    /// Marks `key`'s app as opening, or done; while it opens, each call bounces it again.
    pub fn set_launching(&mut self, key: &str, launching: bool) {
        if let Some(tile) = self.tile_mut(key) {
            tile.launching = launching;
            if launching {
                // Up off the floor; it falls back, and bounces again with the next call.
                tile.bounce.animate(0.0);
                tile.bounce.value = tile.bounce.value.max(0.0);
                tile.bounce.velocity = 7.5;
            }
        }
    }

    /// Returns whether the badge changed.
    pub fn set_badge(&mut self, key: &str, label: Option<String>) -> bool {
        let size = self.settings.icon_size;
        let Some(tile) = self.tile_mut(key) else { return false };
        let badge = &mut tile.badge;
        if badge.label == label {
            return false;
        }
        badge.shown.animate(if label.is_some() { 1.0 } else { 0.0 });
        if let Some(text) = &label {
            let height = badge_height(size);
            set_font(&badge.text, &NSFont::boldSystemFontOfSize(height * 0.64));
            unsafe { badge.text.setString(Some(&NSString::from_str(text))) };
            badge.width = (badge.text.preferredFrameSize().width.ceil() + height * 0.6).max(height);
        }
        badge.label = label;
        true
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

    pub fn tile_at(&self, point: CGPoint) -> Option<&Tile> {
        if !self.body_contains(point) {
            return None;
        }
        self.tiles.iter().find(|tile| tile.is_live() && contains(tile.hit, point))
    }

    /// The tile whose icon is under `point`, for dropping files on.
    pub fn icon_at(&self, point: CGPoint) -> Option<&Tile> {
        let half = self.settings.icon_size / 2.0;
        self.tile_at(point).filter(|tile| (point.x - tile.center).abs() <= half)
    }

    /// Returns whether the hovered tile changed.
    pub fn hover(&mut self, point: Option<CGPoint>) -> bool {
        let key = point.and_then(|point| self.tile_at(point)).map(|tile| tile.key.clone());
        self.hover_key(key)
    }

    pub fn hover_key(&mut self, key: Option<String>) -> bool {
        if key == self.hovered {
            return false;
        }
        for tile in &mut self.tiles {
            if Some(&tile.key) == self.hovered.as_ref() {
                tile.hover.animate(0.0);
            } else if Some(&tile.key) == key.as_ref() {
                tile.hover.animate(1.0);
            }
        }
        self.hovered = key;
        true
    }

    pub fn press(&mut self, key: Option<String>) {
        for tile in &mut self.tiles {
            if Some(&tile.key) == self.pressed.as_ref() {
                tile.press.animate(0.0);
            }
            if Some(&tile.key) == key.as_ref() {
                tile.press.animate(1.0);
            }
        }
        self.pressed = key;
    }

    pub fn pressed(&self) -> Option<&str> {
        self.pressed.as_deref()
    }

    // Dragging

    pub fn is_dragging(&self) -> bool {
        self.dragged.is_some()
    }

    pub fn dragged(&self) -> Option<&Tile> {
        self.dragged.as_ref().map(|dragged| &dragged.tile)
    }

    /// Lifts `key`'s tile out of the row, leaving a gap where it was.
    pub fn begin_drag(&mut self, key: &str, pointer: CGPoint) -> bool {
        let Some(index) = self.tiles.iter().position(|tile| tile.key == key && tile.is_live()) else { return false };
        let mut tile = self.tiles.remove(index);
        // Above the other icons while it's carried over them.
        self.root.insertSublayer_below(&tile.icon, Some(&self.label));
        let foot = tile.drawn.unwrap_or(pointer);
        tile.hover.animate(0.0);
        tile.press.animate(0.0);
        let mut gap = self.blank_tile(String::new(), true);
        gap.appear.set(1.0);
        self.tiles.insert(index, gap);
        self.hover_key(None);
        self.pressed = None;
        let grab = CGPoint::new(pointer.x - foot.x, pointer.y - foot.y);
        self.dragged = Some(Dragged { tile, grab, pointer, outside: false });
        true
    }

    /// Follows the pointer, moving the gap to where the tile would land. With `removable`, far
    /// enough up off the dock closes the gap instead: let go there, it's unpinned.
    pub fn drag_to(&mut self, pointer: CGPoint, removable: bool) {
        let threshold = self.settings.height() + (self.settings.icon_size * 0.8).max(24.0);
        let outside = removable && pointer.y > threshold;
        if let Some(dragged) = &mut self.dragged {
            dragged.pointer = pointer;
            dragged.outside = outside;
        }
        let index = (!outside).then(|| self.insertion_index(pointer.x));
        self.move_gap(index);
    }

    pub fn end_drag(&self) -> Option<DragEnd> {
        let dragged = self.dragged.as_ref()?;
        let live = self.tiles.iter().filter(|tile| tile.is_live()).count();
        Some(DragEnd {
            key: dragged.tile.key.clone(),
            index: self.gap_index().unwrap_or(live),
            outside: dragged.outside,
        })
    }

    /// Puts the dragged tile down: into the gap (or at the row's end, for the next `set_row` to
    /// move) when `keep`, or gone where it was let go.
    pub fn finish_drag(&mut self, keep: bool) {
        let Some(dragged) = self.dragged.take() else { return };
        let mut tile = dragged.tile;
        let gap = self.tiles.iter().position(|tile| tile.placeholder && !tile.leaving);
        if keep {
            match gap {
                Some(gap) => {
                    self.tiles[gap].remove_layers();
                    self.tiles[gap] = tile;
                }
                None => self.tiles.push(tile),
            }
        } else {
            self.move_gap(None);
            tile.leaving = true;
            tile.appear.animate_spec(0.0, self.settings.animation.leave);
            self.poofs.push(tile);
        }
    }

    /// Where an app let go at `x` would go in the row: never before Finder.
    pub fn insertion_index(&self, x: f64) -> usize {
        let live: Vec<&Tile> = self.tiles.iter().filter(|tile| tile.is_live()).collect();
        let start = usize::from(live.first().is_some_and(|tile| tile.is_finder()));
        live.iter().filter(|tile| tile.center < x).count().max(start)
    }

    /// Where the gap is, counting the apps before it.
    pub fn gap_index(&self) -> Option<usize> {
        let mut count = 0;
        for tile in &self.tiles {
            if tile.placeholder && !tile.leaving {
                return Some(count);
            }
            count += usize::from(tile.is_live());
        }
        None
    }

    /// Opens a gap at `index` in the row, closing any other, or just closes it.
    pub fn move_gap(&mut self, index: Option<usize>) {
        if self.gap_index() == index {
            return;
        }
        let slide = self.settings.animation.slide;
        if let Some(gap) = self.tiles.iter_mut().find(|tile| tile.placeholder && !tile.leaving) {
            gap.leaving = true;
            gap.appear.animate_spec(0.0, slide);
        }
        if let Some(index) = index {
            let mut count = 0;
            let mut position = self.tiles.len();
            for (i, tile) in self.tiles.iter().enumerate() {
                if tile.is_live() {
                    if count == index {
                        position = i;
                        break;
                    }
                    count += 1;
                }
            }
            let mut gap = self.blank_tile(String::new(), true);
            gap.appear.animate(1.0);
            self.tiles.insert(position, gap);
        }
    }

    /// For the snapshot: the middle of the `index`th app's slot.
    pub fn slot_center(&self, index: usize) -> Option<(String, CGPoint)> {
        let tile = self.tiles.iter().filter(|tile| tile.is_live()).nth(index)?;
        Some((tile.key.clone(), CGPoint::new(tile.center, tile.hit.size.height / 2.0)))
    }
}

fn badge_height(icon_size: f64) -> f64 {
    (icon_size * 0.36).max(12.0)
}

/// Lays the badge out at its icon's top right, in the icon's own coordinates.
fn render_badge(badge: &Badge, icon_size: f64) {
    let shown = badge.shown.value.max(0.0);
    let height = badge_height(icon_size);
    badge.layer.setBounds(CGRect::new(CGPoint::ZERO, CGSize::new(badge.width, height)));
    badge.layer.setCornerRadius(height / 2.0);
    badge.layer.setPosition(CGPoint::new(icon_size - badge.width / 2.0 + height * 0.3, icon_size - height * 0.3));
    badge.layer.setTransform(CATransform3D::new_scale(shown, shown, 1.0));
    badge.layer.setOpacity(shown.min(1.0) as f32);
    let line = badge.text.preferredFrameSize().height.ceil();
    badge.text.setFrame(CGRect::new(CGPoint::new(0.0, (height - line) / 2.0), CGSize::new(badge.width, line)));
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

fn set_font(layer: &CATextLayer, font: &NSFont) {
    // SAFETY: NSFont is toll-free bridged with CTFont, which CATextLayer takes.
    unsafe { layer.setFont(Some(&*(font as *const NSFont as *const CFType))) };
    layer.setFontSize(font.pointSize());
}

fn max_backing_scale() -> f64 {
    objc2::MainThreadMarker::new()
        .map(|mtm| objc2_app_kit::NSScreen::screens(mtm).iter().map(|s| s.backingScaleFactor()).fold(1.0, f64::max))
        .unwrap_or(2.0)
}

/// Shows the running app's icon, or the icon of the bundle at `path`.
fn set_icon(layer: &CALayer, app: Option<&NSRunningApplication>, path: Option<&str>, side: usize) {
    let image = app
        .and_then(|app| app.icon())
        .or_else(|| path.map(|path| NSWorkspace::sharedWorkspace().iconForFile(&NSString::from_str(path))));
    if let Some(image) = image.and_then(|image| rasterize(&image, side)) {
        // SAFETY: a CGImage is valid layer contents.
        unsafe { layer.setContents(Some(cf_object(&image))) };
    }
}

/// Draws `image` into a square bitmap `side` pixels across, letting NSImage pick its best
/// representation for that size.
fn rasterize(image: &NSImage, side: usize) -> Option<CFRetained<CGImage>> {
    let space = CGColorSpace::with_name(Some(unsafe { kCGColorSpaceSRGB }))?;
    // SAFETY: a null data pointer has the context allocate its own memory.
    let context = unsafe {
        CGBitmapContextCreate(
            std::ptr::null_mut(),
            side,
            side,
            8,
            0,
            Some(&space),
            CGImageAlphaInfo::PremultipliedLast.0,
        )
    }?;
    let graphics = NSGraphicsContext::graphicsContextWithCGContext_flipped(&context, false);
    NSGraphicsContext::saveGraphicsState_class();
    NSGraphicsContext::setCurrentContext(Some(&graphics));
    let rect = NSRect::new(CGPoint::ZERO, CGSize::new(side as f64, side as f64));
    image.drawInRect_fromRect_operation_fraction(rect, NSRect::ZERO, NSCompositingOperation::Copy, 1.0);
    NSGraphicsContext::restoreGraphicsState_class();
    CGBitmapContextCreateImage(Some(&context))
}
