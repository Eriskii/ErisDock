use std::rc::Weak;

use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{MainThreadMarker, MainThreadOnly, Message, sel};
use objc2_app_kit::{
    NSBackingStoreType, NSButton, NSColor, NSControl, NSControlStateValueOff, NSControlStateValueOn, NSFont,
    NSGridCell, NSGridCellPlacement, NSGridRowAlignment, NSGridView, NSPopUpButton, NSSlider, NSStackView, NSTextField,
    NSView, NSWindow, NSWindowStyleMask, NSWorkspace,
};
use objc2_core_foundation::{CGPoint, CGRect, CGSize};
use objc2_foundation::{NSArray, NSString, NSURL, ns_string};

use crate::actions::LoginItem;
use crate::animation::Style;
use crate::config::{self, Config};
use crate::dock::Dock;
use crate::menu::Action;
use crate::settings::Settings;

/// A small window for the settings people change most. Everything it changes goes into the
/// config file, which holds the rest.
pub struct SettingsWindow {
    pub window: Retained<NSWindow>,
    style: Retained<NSPopUpButton>,
    summary: Retained<NSTextField>,
    icon_size: Slider,
    native_size: Retained<NSButton>,
    smoothing: Slider,
    reveal_delay: Slider,
    hide_delay: Slider,
    pinning: Retained<NSButton>,
    block_self_pinning: Retained<NSButton>,
    login: Retained<NSButton>,
    _actions: Vec<Retained<Action>>,
}

/// A slider and the value shown beside it.
struct Slider {
    slider: Retained<NSSlider>,
    value: Retained<NSTextField>,
    format: fn(f64) -> String,
}

impl Slider {
    fn new(mtm: MainThreadMarker, min: f64, max: f64, format: fn(f64) -> String) -> Self {
        let slider =
            unsafe { NSSlider::sliderWithValue_minValue_maxValue_target_action(min, min, max, None, None, mtm) };
        slider.widthAnchor().constraintEqualToConstant(200.0).setActive(true);
        let value = NSTextField::labelWithString(&NSString::from_str(&format(max)), mtm);
        value.setTextColor(Some(&NSColor::secondaryLabelColor()));
        value.widthAnchor().constraintEqualToConstant(56.0).setActive(true);
        Self { slider, value, format }
    }

    fn set(&self, number: f64) {
        self.slider.setDoubleValue(number);
        self.show(number);
    }

    fn show(&self, number: f64) {
        self.value.setStringValue(&NSString::from_str(&(self.format)(number)));
    }

    fn row(&self, mtm: MainThreadMarker) -> Retained<NSView> {
        let views: [&NSView; 2] = [&self.slider, &self.value];
        let stack = NSStackView::stackViewWithViews(&NSArray::from_slice(&views), mtm);
        stack.setSpacing(8.0);
        Retained::into_super(stack)
    }
}

impl SettingsWindow {
    pub fn new(mtm: MainThreadMarker, dock: Weak<Dock>) -> Self {
        let window = unsafe {
            NSWindow::initWithContentRect_styleMask_backing_defer(
                NSWindow::alloc(mtm),
                CGRect::ZERO,
                NSWindowStyleMask::Titled | NSWindowStyleMask::Closable,
                NSBackingStoreType::Buffered,
                false,
            )
        };
        window.setTitle(ns_string!("ErisDock Settings"));
        unsafe { window.setReleasedWhenClosed(false) };

        let style = NSPopUpButton::initWithFrame_pullsDown(NSPopUpButton::alloc(mtm), CGRect::ZERO, false);
        let titles: Vec<_> = Style::ALL.iter().map(|style| NSString::from_str(style.title())).collect();
        style.addItemsWithTitles(&NSArray::from_retained_slice(&titles));
        // Measured with the longest description, so none of them needs more room.
        let longest = Style::ALL.iter().map(|style| style.summary()).max_by_key(|summary| summary.len()).unwrap_or("");
        let summary = small(NSTextField::wrappingLabelWithString(&NSString::from_str(longest), mtm));
        summary.setPreferredMaxLayoutWidth(264.0);

        let icon_size = Slider::new(mtm, 20.0, 96.0, |size| format!("{size:.0} pt"));
        let native_size = checkbox(mtm, "Match the native Dock");
        let smoothing = Slider::new(mtm, 0.0, 1.0, |smoothing| format!("{smoothing:.2}"));
        let reveal_delay = Slider::new(mtm, 0.0, 0.5, |seconds| format!("{seconds:.2} s"));
        let hide_delay = Slider::new(mtm, 0.0, 2.0, |seconds| format!("{seconds:.2} s"));
        let pinning = checkbox(mtm, "Keep pinned apps in the dock");
        let block_self_pinning = checkbox(mtm, "Never let apps pin themselves");
        let login = checkbox(mtm, "Open at login");
        let replay =
            unsafe { NSButton::buttonWithTitle_target_action(ns_string!("Replay Animation"), None, None, mtm) };
        let open = unsafe { NSButton::buttonWithTitle_target_action(ns_string!("Open Config File"), None, None, mtm) };
        let buttons: [&NSView; 2] = [&replay, &open];
        let buttons = NSStackView::stackViewWithViews(&NSArray::from_slice(&buttons), mtm);
        let note = small(NSTextField::wrappingLabelWithString(
            &NSString::from_str(&format!(
                "Saved to {}, along with sizes and spacing that only the file has.",
                config::path().display().to_string().replacen(&std::env::var("HOME").unwrap_or_default(), "~", 1)
            )),
            mtm,
        ));
        note.setPreferredMaxLayoutWidth(264.0);

        let label = |text: &str| {
            Retained::into_super(Retained::into_super(NSTextField::labelWithString(&NSString::from_str(text), mtm)))
        };
        let empty = || NSGridCell::emptyContentView(mtm);
        let rows: Vec<[Retained<NSView>; 2]> = vec![
            [label("Animation:"), view(&style)],
            [empty(), view(&summary)],
            [label("Icon size:"), icon_size.row(mtm)],
            [empty(), view(&native_size)],
            [label("Corner smoothing:"), smoothing.row(mtm)],
            [label("Rise after:"), reveal_delay.row(mtm)],
            [label("Tuck away after:"), hide_delay.row(mtm)],
            [label("Pinning:"), view(&pinning)],
            [empty(), view(&block_self_pinning)],
            [empty(), view(&login)],
            [empty(), Retained::into_super(buttons)],
            [empty(), view(&note)],
        ];
        let rows: Vec<_> = rows.iter().map(|row| NSArray::from_retained_slice(row)).collect();
        let grid = NSGridView::gridViewWithViews(&NSArray::from_retained_slice(&rows), mtm);
        grid.setRowSpacing(10.0);
        grid.setColumnSpacing(10.0);
        grid.setRowAlignment(NSGridRowAlignment::FirstBaseline);
        grid.columnAtIndex(0).setXPlacement(NSGridCellPlacement::Trailing);
        let size = grid.fittingSize();
        grid.setFrame(CGRect::new(CGPoint::new(20.0, 20.0), size));
        window.setContentSize(CGSize::new(size.width + 40.0, size.height + 40.0));
        if let Some(content) = window.contentView() {
            content.addSubview(&grid);
        }
        window.center();

        let mut actions = Vec::new();
        let mut connect = |control: &NSControl, action: Box<dyn Fn(&Dock)>| {
            let dock = dock.clone();
            let target = Action::new(mtm, move || {
                if let Some(dock) = dock.upgrade() {
                    action(&dock);
                }
            });
            let object: &AnyObject = &target;
            unsafe {
                control.setTarget(Some(object));
                control.setAction(Some(sel!(run:)));
            }
            actions.push(target);
        };
        let popup = style.clone();
        connect(
            &style,
            Box::new(move |dock| {
                let style = Style::ALL.get(popup.indexOfSelectedItem().max(0) as usize).copied().unwrap_or_default();
                dock.edit_config(|config| config.animation = style);
                dock.replay();
            }),
        );
        let (slider, native) = (icon_size.slider.clone(), native_size.clone());
        connect(
            &icon_size.slider,
            Box::new(move |dock| {
                native.setState(NSControlStateValueOff);
                dock.edit_config(|config| config.icon_size = Some(slider.doubleValue().round()));
            }),
        );
        let (checkbox, slider) = (native_size.clone(), icon_size.slider.clone());
        connect(
            &native_size,
            Box::new(move |dock| {
                let native = checkbox.state() == NSControlStateValueOn;
                dock.edit_config(|config| config.icon_size = (!native).then(|| slider.doubleValue().round()));
            }),
        );
        let slider = smoothing.slider.clone();
        connect(
            &smoothing.slider,
            Box::new(move |dock| {
                dock.edit_config(|config| config.smoothing = Some(round(slider.doubleValue())));
            }),
        );
        let slider = reveal_delay.slider.clone();
        connect(
            &reveal_delay.slider,
            Box::new(move |dock| {
                dock.edit_config(|config| config.reveal_delay = Some(round(slider.doubleValue())));
            }),
        );
        let slider = hide_delay.slider.clone();
        connect(
            &hide_delay.slider,
            Box::new(move |dock| {
                dock.edit_config(|config| config.hide_delay = Some(round(slider.doubleValue())));
            }),
        );
        let checkbox = pinning.clone();
        connect(
            &pinning,
            Box::new(move |dock| {
                dock.edit_config(|config| config.pinning = Some(checkbox.state() == NSControlStateValueOn))
            }),
        );
        let checkbox = block_self_pinning.clone();
        connect(
            &block_self_pinning,
            Box::new(move |dock| {
                dock.edit_config(|config| config.block_self_pinning = Some(checkbox.state() == NSControlStateValueOn))
            }),
        );
        let checkbox = login.clone();
        connect(
            &login,
            Box::new(move |_| {
                LoginItem::toggle();
                checkbox.setState(if LoginItem::is_enabled() { NSControlStateValueOn } else { NSControlStateValueOff });
            }),
        );
        connect(&replay, Box::new(Dock::replay));
        connect(&open, Box::new(|_| open_config_file()));

        Self {
            window,
            style,
            summary,
            icon_size,
            native_size,
            smoothing,
            reveal_delay,
            hide_delay,
            pinning,
            block_self_pinning,
            login,
            _actions: actions,
        }
    }

    /// Shows what `config` sets, and `settings`' values for what it leaves out.
    pub fn sync(&self, config: &Config, settings: &Settings) {
        let index = Style::ALL.iter().position(|style| *style == config.animation).unwrap_or(0);
        self.style.selectItemAtIndex(index as isize);
        self.summary.setStringValue(&NSString::from_str(config.animation.summary()));
        self.icon_size.set(settings.icon_size);
        let native = config.icon_size.is_none();
        self.native_size.setState(if native { NSControlStateValueOn } else { NSControlStateValueOff });
        self.smoothing.set(settings.smoothing);
        self.reveal_delay.set(settings.reveal_delay);
        self.hide_delay.set(settings.hide_delay);
        self.pinning.setState(state(settings.pinning));
        self.block_self_pinning.setState(state(settings.block_self_pinning));
        self.block_self_pinning.setEnabled(settings.pinning);
        self.login.setState(if LoginItem::is_enabled() { NSControlStateValueOn } else { NSControlStateValueOff });
    }
}

fn view<T: AsRef<NSView>>(control: &Retained<T>) -> Retained<NSView> {
    let view: &NSView = (**control).as_ref();
    view.retain()
}

fn state(on: bool) -> isize {
    if on { NSControlStateValueOn } else { NSControlStateValueOff }
}

fn checkbox(mtm: MainThreadMarker, title: &str) -> Retained<NSButton> {
    unsafe { NSButton::checkboxWithTitle_target_action(&NSString::from_str(title), None, None, mtm) }
}

fn small(label: Retained<NSTextField>) -> Retained<NSTextField> {
    label.setFont(Some(&NSFont::systemFontOfSize(NSFont::smallSystemFontSize())));
    label.setTextColor(Some(&NSColor::secondaryLabelColor()));
    label
}

fn round(number: f64) -> f64 {
    (number * 100.0).round() / 100.0
}

fn open_config_file() {
    let url = NSURL::fileURLWithPath(&NSString::from_str(&config::path().to_string_lossy()));
    let workspace = NSWorkspace::sharedWorkspace();
    // With no app for .toml files, show it in Finder instead.
    if !workspace.openURL(&url) {
        workspace.activateFileViewerSelectingURLs(&NSArray::from_retained_slice(&[url]));
    }
}
