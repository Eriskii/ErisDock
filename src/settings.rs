use objc2::AnyThread;
use objc2_foundation::{NSString, NSUserDefaults, ns_string};

/// Sizes and timings. Sizes scale from the icon size, which is the native Dock's unless set;
/// any of them can be set with `defaults write com.eriskii.ErisDock <key> -float <value>`.
#[derive(Clone, Debug, PartialEq)]
pub struct Settings {
    pub icon_size: f64,
    /// Between neighbouring icons.
    pub spacing: f64,
    /// Between the outer icons and the body's sides.
    pub padding: f64,
    pub top_padding: f64,
    /// Below the icons, holding the dots.
    pub bottom_padding: f64,
    pub dot_size: f64,
    pub corner_radius: f64,
    /// The concave fillets where the body meets the screen's edge.
    pub flare_radius: f64,
    /// Figma's corner smoothing, 0 for circular corners.
    pub smoothing: f64,
    /// The body's width, as a fraction of its full width, while it's tucked into the edge.
    /// It rises faster than it widens, which is the squish.
    pub collapsed_width: f64,
    pub rise_response: f64,
    pub rise_damping: f64,
    pub spread_response: f64,
    pub spread_damping: f64,
    /// How long the pointer rests on the edge before the dock rises.
    pub reveal_delay: f64,
    /// How long the pointer stays away before the dock tucks back in.
    pub hide_delay: f64,
    /// How far beside the dock the bottom edge still raises it.
    pub trigger_margin: f64,
}

impl Settings {
    pub fn with_icon_size(icon_size: f64) -> Self {
        let top_padding = (icon_size * 0.2).round();
        let dot_size = (icon_size * 0.08).round().max(3.0);
        Self {
            icon_size,
            spacing: (icon_size * 0.18).round(),
            padding: (icon_size * 0.26).round(),
            top_padding,
            bottom_padding: top_padding + dot_size + 2.0,
            dot_size,
            corner_radius: icon_size * 0.46,
            flare_radius: icon_size * 0.26,
            smoothing: 0.6,
            collapsed_width: 0.45,
            rise_response: 0.42,
            rise_damping: 0.58,
            spread_response: 0.5,
            spread_damping: 0.62,
            reveal_delay: 0.06,
            hide_delay: 0.35,
            trigger_margin: 48.0,
        }
    }

    /// The body's height once risen.
    pub fn height(&self) -> f64 {
        self.top_padding + self.icon_size + self.bottom_padding
    }

    /// The body's width once settled with `count` apps in it.
    pub fn resting_width(&self, count: usize) -> f64 {
        let count = count as f64;
        count * (self.icon_size + self.spacing) - if count > 0.0 { self.spacing } else { 0.0 } + 2.0 * self.padding
    }

    pub fn load() -> Self {
        let defaults = NSUserDefaults::standardUserDefaults();
        let read = |key: &str| {
            let key = NSString::from_str(key);
            defaults.objectForKey(&key).map(|_| defaults.doubleForKey(&key))
        };
        let native = NSUserDefaults::initWithSuiteName(NSUserDefaults::alloc(), Some(ns_string!("com.apple.dock")))
            .map(|dock| dock.doubleForKey(ns_string!("tilesize")))
            .filter(|size| *size > 0.0);
        let mut settings = Self::with_icon_size(read("iconSize").or(native).unwrap_or(48.0));
        let keys: [(&str, &mut f64); 16] = [
            ("spacing", &mut settings.spacing),
            ("padding", &mut settings.padding),
            ("topPadding", &mut settings.top_padding),
            ("bottomPadding", &mut settings.bottom_padding),
            ("dotSize", &mut settings.dot_size),
            ("cornerRadius", &mut settings.corner_radius),
            ("flareRadius", &mut settings.flare_radius),
            ("smoothing", &mut settings.smoothing),
            ("collapsedWidth", &mut settings.collapsed_width),
            ("riseResponse", &mut settings.rise_response),
            ("riseDamping", &mut settings.rise_damping),
            ("spreadResponse", &mut settings.spread_response),
            ("spreadDamping", &mut settings.spread_damping),
            ("revealDelay", &mut settings.reveal_delay),
            ("hideDelay", &mut settings.hide_delay),
            ("triggerMargin", &mut settings.trigger_margin),
        ];
        for (key, field) in keys {
            if let Some(value) = read(key) {
                *field = value;
            }
        }
        settings
    }
}
