use objc2::AnyThread;
use objc2_foundation::{NSUserDefaults, ns_string};

use crate::animation::{Animation, Style};
use crate::config::Config;

/// Sizes, timings and animations, worked out from the config file. Sizes scale from the icon
/// size, which is the native Dock's unless the config sets one.
#[derive(Clone, Debug, PartialEq)]
pub struct Settings {
    pub style: Style,
    pub animation: Animation,
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
    /// How long the pointer rests on the edge before the dock rises.
    pub reveal_delay: f64,
    /// How long the pointer stays away before the dock tucks back in.
    pub hide_delay: f64,
    /// How far beside the dock the bottom edge still raises it.
    pub trigger_margin: f64,
    /// Whether pinned apps show, running or not.
    pub pinning: bool,
    /// Whether apps that pin themselves to the native Dock are kept out of this one.
    pub block_self_pinning: bool,
}

impl Settings {
    pub const DEFAULT_ICON_SIZE: f64 = 48.0;
    pub const DEFAULT_SMOOTHING: f64 = 0.6;
    pub const DEFAULT_REVEAL_DELAY: f64 = 0.06;
    pub const DEFAULT_HIDE_DELAY: f64 = 0.35;

    #[cfg(test)]
    pub fn with_icon_size(icon_size: f64) -> Self {
        Self::from_config(&Config { icon_size: Some(icon_size), ..Config::default() }, None)
    }

    /// `native_icon_size` is the native Dock's, used when the config doesn't set one.
    pub fn from_config(config: &Config, native_icon_size: Option<f64>) -> Self {
        let icon_size = config.icon_size.or(native_icon_size).unwrap_or(Self::DEFAULT_ICON_SIZE).clamp(16.0, 256.0);
        let top_padding = config.top_padding.unwrap_or((icon_size * 0.2).round());
        let dot_size = config.dot_size.unwrap_or((icon_size * 0.08).round().max(3.0));
        Self {
            style: config.animation,
            animation: config.animation.animation(),
            icon_size,
            spacing: config.spacing.unwrap_or((icon_size * 0.18).round()),
            padding: config.padding.unwrap_or((icon_size * 0.26).round()),
            top_padding,
            bottom_padding: config.bottom_padding.unwrap_or(top_padding + dot_size + 2.0),
            dot_size,
            corner_radius: config.corner_radius.unwrap_or(icon_size * 0.46),
            flare_radius: config.flare_radius.unwrap_or(icon_size * 0.26),
            smoothing: config.smoothing.unwrap_or(Self::DEFAULT_SMOOTHING).clamp(0.0, 1.0),
            reveal_delay: config.reveal_delay.unwrap_or(Self::DEFAULT_REVEAL_DELAY).max(0.0),
            hide_delay: config.hide_delay.unwrap_or(Self::DEFAULT_HIDE_DELAY).max(0.0),
            trigger_margin: config.trigger_margin.unwrap_or(48.0).max(0.0),
            pinning: config.pinning.unwrap_or(true),
            block_self_pinning: config.block_self_pinning.unwrap_or(true),
        }
    }

    /// The native Dock's icon size, its `tilesize`.
    pub fn native_icon_size() -> Option<f64> {
        NSUserDefaults::initWithSuiteName(NSUserDefaults::alloc(), Some(ns_string!("com.apple.dock")))
            .map(|dock| dock.doubleForKey(ns_string!("tilesize")))
            .filter(|size| *size > 0.0)
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes_follow_the_icon_unless_set() {
        let config = Config { icon_size: Some(50.0), spacing: Some(3.0), ..Config::default() };
        let settings = Settings::from_config(&config, Some(30.0));
        assert_eq!(settings.icon_size, 50.0);
        assert_eq!(settings.spacing, 3.0);
        assert_eq!(settings.padding, 13.0);
    }

    #[test]
    fn the_native_dock_sets_the_icon_size_when_the_config_does_not() {
        assert_eq!(Settings::from_config(&Config::default(), Some(30.0)).icon_size, 30.0);
        assert_eq!(Settings::from_config(&Config::default(), None).icon_size, Settings::DEFAULT_ICON_SIZE);
    }
}
