use std::cell::RefCell;
use std::ffi::c_void;
use std::ptr::NonNull;
use std::rc::{Rc, Weak};

use block2::RcBlock;
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, ProtocolObject};
use objc2::{DefinedClass, MainThreadMarker, msg_send, sel};
use objc2_app_kit::{
    NSApplication, NSApplicationDidChangeScreenParametersNotification, NSEvent, NSEventMask, NSRunningApplication,
    NSScreen, NSWindowWillCloseNotification, NSWorkspace, NSWorkspaceDidActivateApplicationNotification,
    NSWorkspaceDidHideApplicationNotification, NSWorkspaceDidLaunchApplicationNotification,
    NSWorkspaceDidTerminateApplicationNotification, NSWorkspaceDidUnhideApplicationNotification,
};
use objc2_core_foundation::{CGPoint, CGRect, CGSize};
use objc2_foundation::{
    NSNotification, NSNotificationCenter, NSNumber, NSObjectProtocol, NSOperationQueue, NSRunLoop,
    NSRunLoopCommonModes, NSTimer, ns_string,
};
use objc2_quartz_core::CADisplayLink;

use crate::actions::{self, after};
use crate::apps::AppList;
use crate::config::{self, Config};
use crate::menu;
use crate::settings::Settings;
use crate::settings_window::SettingsWindow;
use crate::surface::Surface;
use crate::view::{AppsObserver, DockPanel, DockView, Ticker};

/// Raises the dock when the pointer reaches the bottom edge of a screen near where the dock
/// sits, and tucks it back in once the pointer has left.
///
/// Everything runs on the main thread. A menu's tracking loop runs display link ticks and
/// notifications from inside `show_menu`, so nothing here holds `surface` or `state` across
/// a call that can spin the run loop.
pub struct Dock {
    me: Weak<Dock>,
    mtm: MainThreadMarker,
    config: RefCell<Config>,
    settings: RefCell<Settings>,
    settings_window: RefCell<Option<SettingsWindow>>,
    panel: Retained<DockPanel>,
    view: Retained<DockView>,
    ticker: Retained<Ticker>,
    surface: RefCell<Surface>,
    state: RefCell<State>,
}

#[derive(Default)]
struct State {
    apps: AppList,
    /// The display the dock last rose on.
    screen: Option<u32>,
    shown: bool,
    menu_open: bool,
    /// Held up while the Settings window is open, to show what it changes.
    pinned: bool,
    /// When the config file was last read or written, to notice others' changes.
    config_modified: Option<std::time::SystemTime>,
    config_timer: Option<Retained<NSTimer>>,
    reveal_timer: Option<Retained<NSTimer>>,
    hide_timer: Option<Retained<NSTimer>>,
    link: Option<Retained<CADisplayLink>>,
    last_frame: Option<f64>,
    monitors: Vec<Retained<AnyObject>>,
    observers: Vec<Retained<ProtocolObject<dyn NSObjectProtocol>>>,
    apps_observer: Option<Retained<AppsObserver>>,
}

impl Dock {
    pub fn new(mtm: MainThreadMarker) -> Rc<Self> {
        let config = config::load().unwrap_or_else(|error| {
            eprintln!("ErisDock: {error}; using the defaults");
            Config::default()
        });
        let settings = Settings::from_config(&config, Settings::native_icon_size());
        let scale = NSScreen::screens(mtm).iter().map(|s| s.backingScaleFactor()).fold(1.0, f64::max);
        let surface = Surface::new(settings.clone(), scale);
        let view = DockView::new(mtm, &surface.root);
        let panel = DockPanel::new(mtm, &view);
        let ticker = Ticker::new(mtm);
        let dock = Rc::new_cyclic(|me| Self {
            me: me.clone(),
            mtm,
            config: RefCell::new(config),
            settings: RefCell::new(settings),
            settings_window: RefCell::new(None),
            panel,
            view,
            ticker,
            surface: RefCell::new(surface),
            state: RefCell::new(State::default()),
        });
        dock.view.ivars().set(dock.me.clone());
        dock.ticker.ivars().set(dock.me.clone());
        dock.start();
        dock
    }

    fn start(&self) {
        let mut monitors = Vec::new();
        // Mouse movement can be watched without Accessibility access. Moves over other apps
        // reach the global monitor, and moves while ErisDock is active the local one.
        let moves = NSEventMask::MouseMoved
            | NSEventMask::LeftMouseDragged
            | NSEventMask::RightMouseDragged
            | NSEventMask::OtherMouseDragged;
        let me = self.me.clone();
        let global = RcBlock::new(move |_: NonNull<NSEvent>| {
            if let Some(dock) = me.upgrade() {
                dock.pointer_moved();
            }
        });
        monitors.extend(NSEvent::addGlobalMonitorForEventsMatchingMask_handler(moves, &global));
        let me = self.me.clone();
        let local = RcBlock::new(move |event: NonNull<NSEvent>| -> *mut NSEvent {
            if let Some(dock) = me.upgrade() {
                dock.pointer_moved();
            }
            event.as_ptr()
        });
        monitors.extend(unsafe { NSEvent::addLocalMonitorForEventsMatchingMask_handler(moves, &local) });

        let mut observers = Vec::new();
        let workspace = NSWorkspace::sharedWorkspace();
        let names = unsafe {
            [
                NSWorkspaceDidLaunchApplicationNotification,
                NSWorkspaceDidTerminateApplicationNotification,
                NSWorkspaceDidActivateApplicationNotification,
                NSWorkspaceDidHideApplicationNotification,
                NSWorkspaceDidUnhideApplicationNotification,
            ]
        };
        for name in names {
            observers.push(self.observe(&workspace.notificationCenter(), name, Dock::apps_changed));
        }
        observers.push(self.observe(
            &NSNotificationCenter::defaultCenter(),
            unsafe { NSApplicationDidChangeScreenParametersNotification },
            Dock::screens_changed,
        ));

        let apps_observer = AppsObserver::new(self.mtm);
        apps_observer.ivars().set(self.me.clone());
        unsafe {
            let _: () = msg_send![&*workspace, addObserver: &*apps_observer, forKeyPath: ns_string!("runningApplications"),
                                  options: 0usize, context: std::ptr::null_mut::<c_void>()];
        }

        let mut state = self.state.borrow_mut();
        state.monitors = monitors;
        state.observers = observers;
        state.apps_observer = Some(apps_observer);
        state.config_modified = config::modified();
        let me = self.me.clone();
        let check = RcBlock::new(move |_: NonNull<NSTimer>| {
            if let Some(dock) = me.upgrade() {
                dock.check_config();
            }
        });
        state.config_timer = Some(unsafe { NSTimer::scheduledTimerWithTimeInterval_repeats_block(1.0, true, &check) });
        drop(state);
        self.apps_changed();
    }

    fn observe(
        &self,
        center: &NSNotificationCenter,
        name: &objc2_foundation::NSNotificationName,
        handler: fn(&Dock),
    ) -> Retained<ProtocolObject<dyn NSObjectProtocol>> {
        let me = self.me.clone();
        let block = RcBlock::new(move |_: NonNull<NSNotification>| {
            if let Some(dock) = me.upgrade() {
                handler(&dock);
            }
        });
        unsafe {
            center.addObserverForName_object_queue_usingBlock(
                Some(name),
                None,
                Some(&NSOperationQueue::mainQueue()),
                &block,
            )
        }
    }

    // Showing and hiding

    fn pointer_moved(&self) {
        let mouse = NSEvent::mouseLocation();
        let (shown, screen, held) = {
            let state = self.state.borrow();
            (state.shown, state.screen, state.menu_open || state.pinned)
        };
        if shown {
            let frame = self.panel.frame();
            let point = CGPoint::new(mouse.x - frame.origin.x, mouse.y - frame.origin.y);
            let (inside, near, changed) = {
                let mut surface = self.surface.borrow_mut();
                let inside = surface.body_contains(point);
                let changed = surface.hover(inside.then_some(point));
                (inside, surface.is_near(point), changed)
            };
            // Only the body takes clicks; the rest of the panel's strip passes them through.
            self.panel.setIgnoresMouseEvents(!inside);
            if held || near {
                self.cancel_hide();
            } else {
                self.schedule_hide();
            }
            if changed {
                self.animate();
            }
        }
        match self.edge_screen(mouse) {
            Some((_, id)) if !(shown && Some(id) == screen) => self.schedule_reveal(),
            _ => self.cancel_reveal(),
        }
    }

    /// The screen whose bottom edge the pointer is on, if it's within reach of the dock there.
    fn edge_screen(&self, mouse: CGPoint) -> Option<(Retained<NSScreen>, u32)> {
        let reach = self.surface.borrow().resting_width() / 2.0 + self.settings.borrow().trigger_margin;
        let screens: Vec<_> = NSScreen::screens(self.mtm).iter().collect();
        let frames: Vec<CGRect> = screens.iter().map(|screen| screen.frame()).collect();
        let index = frames.iter().enumerate().position(|(i, frame)| {
            let (min_x, max_x, min_y) = (frame.origin.x, frame.origin.x + frame.size.width, frame.origin.y);
            let on_edge = mouse.x >= min_x && mouse.x < max_x && mouse.y >= min_y - 1.0 && mouse.y < min_y + 2.0;
            let in_reach = (mouse.x - (min_x + frame.size.width / 2.0)).abs() <= reach;
            // Where another screen carries on below, the pointer doesn't stop at the edge.
            let continues = frames.iter().enumerate().any(|(j, other)| {
                j != i
                    && other.origin.y + other.size.height == min_y
                    && other.origin.x <= mouse.x
                    && mouse.x < other.origin.x + other.size.width
            });
            on_edge && in_reach && !continues
        })?;
        let screen = screens[index].clone();
        let id = display_id(&screen)?;
        Some((screen, id))
    }

    fn schedule_reveal(&self) {
        if self.state.borrow().reveal_timer.is_some() {
            return;
        }
        let me = self.me.clone();
        let timer = after(self.settings.borrow().reveal_delay, move || {
            if let Some(dock) = me.upgrade() {
                dock.state.borrow_mut().reveal_timer = None;
                if let Some((screen, id)) = dock.edge_screen(NSEvent::mouseLocation()) {
                    dock.reveal(&screen, id);
                }
            }
        });
        self.state.borrow_mut().reveal_timer = Some(timer);
    }

    fn cancel_reveal(&self) {
        let timer = self.state.borrow_mut().reveal_timer.take();
        if let Some(timer) = timer {
            timer.invalidate();
        }
    }

    fn schedule_hide(&self) {
        if self.state.borrow().hide_timer.is_some() {
            return;
        }
        let me = self.me.clone();
        let timer = after(self.settings.borrow().hide_delay, move || {
            if let Some(dock) = me.upgrade() {
                dock.state.borrow_mut().hide_timer = None;
                dock.hide();
            }
        });
        self.state.borrow_mut().hide_timer = Some(timer);
    }

    fn cancel_hide(&self) {
        let timer = self.state.borrow_mut().hide_timer.take();
        if let Some(timer) = timer {
            timer.invalidate();
        }
    }

    fn reveal(&self, screen: &NSScreen, id: u32) {
        self.cancel_hide();
        let moved = {
            let mut state = self.state.borrow_mut();
            let moved = state.screen != Some(id);
            state.screen = Some(id);
            state.shown = true;
            moved
        };
        self.panel.setFrame_display(self.panel_frame(screen), false);
        {
            let mut surface = self.surface.borrow_mut();
            if moved {
                surface.tuck_away();
            }
            surface.bounds = self.view.bounds();
            surface.set_scale(screen.backingScaleFactor());
            surface.set_risen(true);
        }
        self.panel.orderFrontRegardless();
        self.animate();
        self.pointer_moved();
    }

    fn hide(&self) {
        {
            let mut state = self.state.borrow_mut();
            if !state.shown || state.menu_open || state.pinned {
                return;
            }
            state.shown = false;
        }
        self.cancel_hide();
        self.panel.setIgnoresMouseEvents(true);
        {
            let mut surface = self.surface.borrow_mut();
            surface.hover(None);
            surface.press(None);
            surface.set_risen(false);
        }
        self.animate();
    }

    fn panel_frame(&self, screen: &NSScreen) -> CGRect {
        let frame = screen.frame();
        CGRect::new(frame.origin, CGSize::new(frame.size.width, self.surface.borrow().panel_height()))
    }

    fn screens_changed(&self) {
        let Some(id) = self.state.borrow().screen else { return };
        let screen = NSScreen::screens(self.mtm).iter().find(|screen| display_id(screen) == Some(id));
        if let Some(screen) = screen {
            self.panel.setFrame_display(self.panel_frame(&screen), false);
            {
                let mut surface = self.surface.borrow_mut();
                surface.bounds = self.view.bounds();
                surface.set_scale(screen.backingScaleFactor());
            }
            self.animate();
        } else {
            {
                let mut state = self.state.borrow_mut();
                state.screen = None;
                state.shown = false;
            }
            self.panel.setIgnoresMouseEvents(true);
            {
                let mut surface = self.surface.borrow_mut();
                surface.tuck_away();
                surface.render();
            }
            self.panel.orderOut(None);
        }
    }

    // Frames

    fn animate(&self) {
        if !self.panel.isVisible() {
            self.surface.borrow_mut().render();
            return;
        }
        let mut state = self.state.borrow_mut();
        let link = state.link.get_or_insert_with(|| {
            let target: &AnyObject = &self.ticker;
            let link = unsafe { self.view.displayLinkWithTarget_selector(target, sel!(tick:)) };
            unsafe { link.addToRunLoop_forMode(&NSRunLoop::mainRunLoop(), NSRunLoopCommonModes) };
            link
        });
        link.setPaused(false);
    }

    pub fn tick(&self, link: &CADisplayLink) {
        let now = link.timestamp();
        let dt = {
            let mut state = self.state.borrow_mut();
            let dt = state.last_frame.map_or(1.0 / 120.0, |last| (now - last).min(1.0 / 30.0));
            state.last_frame = Some(now);
            dt
        };
        let settled = {
            let mut surface = self.surface.borrow_mut();
            surface.step(dt);
            surface.render();
            surface.is_settled()
        };
        if settled {
            link.setPaused(true);
            let shown = {
                let mut state = self.state.borrow_mut();
                state.last_frame = None;
                state.shown
            };
            if !shown {
                self.panel.orderOut(None);
            }
        }
    }

    // Apps, clicks and menus

    pub fn apps_changed(&self) {
        let (Ok(mut state), Ok(mut surface)) = (self.state.try_borrow_mut(), self.surface.try_borrow_mut()) else {
            // Posted from inside a change already under way; catch up once it's done.
            let me = self.me.clone();
            after(0.0, move || {
                if let Some(dock) = me.upgrade() {
                    dock.apps_changed();
                }
            });
            return;
        };
        state.apps.refresh();
        let frontmost = NSWorkspace::sharedWorkspace().frontmostApplication().map(|app| app.processIdentifier());
        surface.set_apps(&state.apps.apps, frontmost, self.panel.isVisible());
        drop((state, surface));
        self.animate();
    }

    pub fn mouse_down(&self, point: CGPoint, control: bool) {
        if control {
            return self.show_menu_at(point);
        }
        {
            let mut surface = self.surface.borrow_mut();
            let pid = surface.item_at(point).map(|item| item.pid);
            surface.press(pid);
        }
        self.animate();
    }

    pub fn mouse_up(&self, point: CGPoint) {
        let app = {
            let mut surface = self.surface.borrow_mut();
            let Some(pressed) = surface.pressed() else { return };
            surface.press(None);
            surface.item_at(point).filter(|item| item.pid == pressed).map(|item| item.app.clone())
        };
        self.animate();
        if let Some(app) = app {
            actions::open(&app);
        }
    }

    pub fn show_menu_at(&self, point: CGPoint) {
        let target = self.surface.borrow().item_at(point).map(|item| (item.app.clone(), item.center));
        self.show_menu(target, point);
    }

    fn show_menu(&self, target: Option<(Retained<NSRunningApplication>, f64)>, point: CGPoint) {
        let mut menu = match &target {
            Some((app, _)) => menu::app_menu(self.mtm, app),
            None => menu::dock_menu(self.mtm, self.me.clone()),
        };
        self.state.borrow_mut().menu_open = true;
        self.cancel_hide();
        let top = {
            let mut surface = self.surface.borrow_mut();
            surface.shows_label = false;
            surface.shape.height.max(self.settings.borrow().height())
        };
        self.animate();
        // Just above the icon, clear of the body, as the native Dock's menus open.
        let x = target.as_ref().map_or(point.x, |(_, center)| *center);
        menu.pop_up_above(&self.view, x, top + 6.0);
        self.state.borrow_mut().menu_open = false;
        self.surface.borrow_mut().shows_label = true;
        self.animate();
        self.pointer_moved();
    }

    // Settings

    /// Changes the config, writes it to the file, and puts it into effect.
    pub fn edit_config(&self, edit: impl FnOnce(&mut Config)) {
        let mut config = self.config.borrow().clone();
        edit(&mut config);
        if config == *self.config.borrow() {
            return;
        }
        if let Err(error) = config::save(&config) {
            eprintln!("ErisDock: couldn't save {}: {error}", config::path().display());
        }
        self.state.borrow_mut().config_modified = config::modified();
        self.apply_config(config);
    }

    /// Reads the config file again if something else has changed it.
    fn check_config(&self) {
        let modified = config::modified();
        if modified == self.state.borrow().config_modified {
            return;
        }
        self.state.borrow_mut().config_modified = modified;
        match config::load() {
            Ok(config) if config != *self.config.borrow() => self.apply_config(config),
            Ok(_) => {}
            Err(error) => eprintln!("ErisDock: {error}; keeping the settings as they were"),
        }
    }

    fn apply_config(&self, config: Config) {
        let settings = Settings::from_config(&config, Settings::native_icon_size());
        *self.config.borrow_mut() = config;
        *self.settings.borrow_mut() = settings.clone();
        self.surface.borrow_mut().apply(settings);
        // A new icon size changes the panel's height.
        let screen = self.state.borrow().screen;
        if let Some(screen) = screen.and_then(|id| self.screen(id)) {
            self.panel.setFrame_display(self.panel_frame(&screen), false);
            self.surface.borrow_mut().bounds = self.view.bounds();
        }
        self.animate();
        if let Some(window) = &*self.settings_window.borrow() {
            window.sync(&self.config.borrow(), &self.settings.borrow());
        }
    }

    pub fn open_settings(&self) {
        if self.settings_window.borrow().is_none() {
            let window = SettingsWindow::new(self.mtm, self.me.clone());
            let me = self.me.clone();
            let closing = RcBlock::new(move |_: NonNull<NSNotification>| {
                if let Some(dock) = me.upgrade() {
                    dock.set_pinned(false);
                }
            });
            let observer = unsafe {
                NSNotificationCenter::defaultCenter().addObserverForName_object_queue_usingBlock(
                    Some(NSWindowWillCloseNotification),
                    Some(&window.window),
                    Some(&NSOperationQueue::mainQueue()),
                    &closing,
                )
            };
            self.state.borrow_mut().observers.push(observer);
            *self.settings_window.borrow_mut() = Some(window);
        }
        let window = {
            let settings_window = self.settings_window.borrow();
            let settings_window = settings_window.as_ref().expect("made above");
            settings_window.sync(&self.config.borrow(), &self.settings.borrow());
            settings_window.window.clone()
        };
        NSApplication::sharedApplication(self.mtm).activate();
        window.makeKeyAndOrderFront(None);
        self.set_pinned(true);
    }

    /// Holds the dock up, on the Settings window's screen, or lets it go.
    fn set_pinned(&self, pinned: bool) {
        self.state.borrow_mut().pinned = pinned;
        if !pinned {
            // Tucks away shortly unless the pointer is near.
            self.pointer_moved();
            return;
        }
        let window_screen = self.settings_window.borrow().as_ref().and_then(|window| window.window.screen());
        if let Some(screen) = window_screen.or_else(|| NSScreen::mainScreen(self.mtm))
            && let Some(id) = display_id(&screen)
        {
            self.reveal(&screen, id);
        }
    }

    /// Tucks the dock away and raises it again, to show off its animation.
    pub fn replay(&self) {
        if !self.state.borrow().shown {
            return;
        }
        self.surface.borrow_mut().set_risen(false);
        self.animate();
        let me = self.me.clone();
        after(0.45, move || {
            if let Some(dock) = me.upgrade()
                && dock.state.borrow().shown
            {
                dock.surface.borrow_mut().set_risen(true);
                dock.animate();
            }
        });
    }

    fn screen(&self, id: u32) -> Option<Retained<NSScreen>> {
        NSScreen::screens(self.mtm).iter().find(|screen| display_id(screen) == Some(id))
    }
}

fn display_id(screen: &NSScreen) -> Option<u32> {
    let number = screen.deviceDescription().objectForKey(ns_string!("NSScreenNumber"))?;
    number.downcast::<NSNumber>().ok().map(|number| number.unsignedIntValue())
}
