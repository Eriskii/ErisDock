//! The marks under running apps' icons, which say what each app's windows are up to: focused,
//! open on the desktop on screen, minimized there, only on other desktops, without windows,
//! or hidden. Which mark each of those gets is picked in the config.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

/// How bright a dim mark is, next to a bright one.
pub const DIM: f64 = 0.45;

/// From one dot to the next, in dot sizes.
const PITCH: f64 = 1.8;
/// A bar's length, in dot sizes.
const BAR: f64 = 3.5;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shape {
    None,
    Dot,
    TwoDots,
    ThreeDots,
    /// A dot for each of the app's windows, up to three.
    WindowDots,
    Bar,
}

impl Shape {
    /// In the order the settings list them.
    pub const ALL: [Shape; 6] =
        [Shape::Dot, Shape::TwoDots, Shape::ThreeDots, Shape::WindowDots, Shape::Bar, Shape::None];

    fn name(self) -> &'static str {
        match self {
            Shape::None => "none",
            Shape::Dot => "dot",
            Shape::TwoDots => "two-dots",
            Shape::ThreeDots => "three-dots",
            Shape::WindowDots => "window-dots",
            Shape::Bar => "bar",
        }
    }

    pub fn title(self) -> &'static str {
        match self {
            Shape::None => "Nothing",
            Shape::Dot => "Dot",
            Shape::TwoDots => "Two dots",
            Shape::ThreeDots => "Three dots",
            Shape::WindowDots => "Dot per window",
            Shape::Bar => "Bar",
        }
    }
}

/// A mark's colour.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tone {
    Bright,
    Dim,
    /// The accent colour picked in System Settings.
    Accent,
}

impl Tone {
    pub const ALL: [Tone; 3] = [Tone::Bright, Tone::Dim, Tone::Accent];

    pub fn title(self) -> &'static str {
        match self {
            Tone::Bright => "Bright",
            Tone::Dim => "Dim",
            Tone::Accent => "Accent",
        }
    }
}

/// A mark: its shape and colour, filled or a ring. Written as its shape, `"dot"`, `"two-dots"`,
/// `"three-dots"`, `"window-dots"`, `"bar"` or `"none"`, with `"dim-"` or `"accent-"` in front
/// for those colours and `"ring-"` for a ring, as in `"accent-ring-bar"`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct Indicator {
    pub shape: Shape,
    pub tone: Tone,
    pub ring: bool,
}

impl Indicator {
    pub const fn new(shape: Shape, tone: Tone, ring: bool) -> Self {
        Self { shape, tone, ring }
    }

    pub const NONE: Indicator = Indicator::new(Shape::None, Tone::Bright, false);

    /// The mark with `windows` worked into it: a dot per window, at least one and at most three.
    pub fn for_windows(self, windows: usize) -> Self {
        let shape = match (self.shape, windows) {
            (Shape::WindowDots, 0 | 1) => Shape::Dot,
            (Shape::WindowDots, 2) => Shape::TwoDots,
            (Shape::WindowDots, _) => Shape::ThreeDots,
            (shape, _) => shape,
        };
        Self { shape, ..self }
    }

    /// Where its three pills sit and how long they are, in dot sizes from the middle. A mark
    /// with fewer stacks some on top of each other, so one mark turns into another by sliding
    /// its pills over.
    pub fn pills(self) -> [(f64, f64); 3] {
        match self.shape {
            Shape::None | Shape::Dot | Shape::WindowDots => [(0.0, 1.0); 3],
            Shape::TwoDots => [(-PITCH / 2.0, 1.0), (PITCH / 2.0, 1.0), (PITCH / 2.0, 1.0)],
            Shape::ThreeDots => [(-PITCH, 1.0), (0.0, 1.0), (PITCH, 1.0)],
            Shape::Bar => [(0.0, BAR); 3],
        }
    }

    pub fn is_shown(self) -> bool {
        self.shape != Shape::None
    }
}

impl fmt::Display for Indicator {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        if !self.is_shown() {
            return f.write_str("none");
        }
        let tone = match self.tone {
            Tone::Bright => "",
            Tone::Dim => "dim-",
            Tone::Accent => "accent-",
        };
        write!(f, "{tone}{}{}", if self.ring { "ring-" } else { "" }, self.shape.name())
    }
}

impl FromStr for Indicator {
    type Err = String;

    fn from_str(text: &str) -> Result<Self, String> {
        let mut mark = Indicator::new(Shape::None, Tone::Bright, false);
        let mut rest = text;
        loop {
            let (word, after) = rest.split_once('-').unwrap_or((rest, ""));
            match word {
                "bright" => mark.tone = Tone::Bright,
                "dim" => mark.tone = Tone::Dim,
                "accent" => mark.tone = Tone::Accent,
                "ring" => mark.ring = true,
                _ => break,
            }
            rest = after;
        }
        let Some(shape) = Shape::ALL.into_iter().find(|shape| shape.name() == rest) else {
            return Err(format!(
                "unknown mark \"{text}\"; use \"dot\", \"two-dots\", \"three-dots\", \"window-dots\", \"bar\" or \
                 \"none\", with \"dim-\" or \"accent-\" and \"ring-\" in front for those"
            ));
        };
        mark.shape = shape;
        Ok(if mark.is_shown() { mark } else { Indicator::NONE })
    }
}

impl TryFrom<String> for Indicator {
    type Error = String;

    fn try_from(text: String) -> Result<Self, String> {
        text.parse()
    }
}

impl From<Indicator> for String {
    fn from(indicator: Indicator) -> String {
        indicator.to_string()
    }
}

/// What an app's windows are doing, from the window server.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Presence {
    /// Windows showing on a desktop that's on screen.
    pub open: usize,
    /// Windows minimized from a desktop that's on screen.
    pub minimized: usize,
    /// Windows on desktops that aren't on screen, minimized or not.
    pub elsewhere: usize,
}

impl Presence {
    /// The windows a dot per window counts: those elsewhere for an app only there, otherwise
    /// those here.
    pub fn windows(self, status: Status) -> usize {
        match status {
            Status::OtherDesktops => self.elsewhere,
            _ => self.open + self.minimized,
        }
    }
}

/// What a running app is up to, as its mark shows it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    Focused,
    Open,
    Minimized,
    OtherDesktops,
    NoWindows,
    Hidden,
}

impl Status {
    pub const ALL: [Status; 6] =
        [Status::Focused, Status::Open, Status::Minimized, Status::OtherDesktops, Status::NoWindows, Status::Hidden];

    /// The first that fits, in that order: hidden, focused, then where its windows are.
    pub fn of(presence: Presence, focused: bool, hidden: bool) -> Status {
        if hidden {
            Status::Hidden
        } else if focused {
            Status::Focused
        } else if presence.open > 0 {
            Status::Open
        } else if presence.minimized > 0 {
            Status::Minimized
        } else if presence.elsewhere > 0 {
            Status::OtherDesktops
        } else {
            Status::NoWindows
        }
    }

    /// Its name in the config file.
    pub fn key(self) -> &'static str {
        match self {
            Status::Focused => "focused",
            Status::Open => "open",
            Status::Minimized => "minimized",
            Status::OtherDesktops => "other_desktops",
            Status::NoWindows => "no_windows",
            Status::Hidden => "hidden",
        }
    }

    pub fn title(self) -> &'static str {
        match self {
            Status::Focused => "Focused:",
            Status::Open => "Open here:",
            Status::Minimized => "Minimized here:",
            Status::OtherDesktops => "On other desktops:",
            Status::NoWindows => "No windows:",
            Status::Hidden => "Hidden:",
        }
    }

    fn index(self) -> usize {
        Status::ALL.iter().position(|status| *status == self).unwrap_or(0)
    }
}

/// The `[indicators]` table in the config: a mark for each status, or the default.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct IndicatorConfig {
    pub focused: Option<Indicator>,
    pub open: Option<Indicator>,
    pub minimized: Option<Indicator>,
    pub other_desktops: Option<Indicator>,
    pub no_windows: Option<Indicator>,
    pub hidden: Option<Indicator>,
}

impl IndicatorConfig {
    fn slot(&mut self, status: Status) -> &mut Option<Indicator> {
        match status {
            Status::Focused => &mut self.focused,
            Status::Open => &mut self.open,
            Status::Minimized => &mut self.minimized,
            Status::OtherDesktops => &mut self.other_desktops,
            Status::NoWindows => &mut self.no_windows,
            Status::Hidden => &mut self.hidden,
        }
    }

    pub fn get(&self, status: Status) -> Option<Indicator> {
        match status {
            Status::Focused => self.focused,
            Status::Open => self.open,
            Status::Minimized => self.minimized,
            Status::OtherDesktops => self.other_desktops,
            Status::NoWindows => self.no_windows,
            Status::Hidden => self.hidden,
        }
    }

    pub fn set(&mut self, status: Status, indicator: Option<Indicator>) {
        *self.slot(status) = indicator;
    }
}

/// The mark for each status.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Indicators([Indicator; 6]);

impl Default for Indicators {
    fn default() -> Self {
        use Shape::*;
        use Tone::*;
        Self([
            Indicator::new(Bar, Bright, false),
            Indicator::new(Dot, Bright, false),
            Indicator::new(TwoDots, Dim, false),
            Indicator::new(Dot, Dim, false),
            Indicator::new(Dot, Dim, false),
            Indicator::new(Dot, Dim, false),
        ])
    }
}

impl Indicators {
    pub fn from_config(config: &IndicatorConfig) -> Self {
        let mut indicators = Self::default();
        for status in Status::ALL {
            if let Some(indicator) = config.get(status) {
                indicators.0[status.index()] = indicator;
            }
        }
        indicators
    }

    pub fn get(&self, status: Status) -> Indicator {
        self.0[status.index()]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn every_mark() -> impl Iterator<Item = Indicator> {
        Shape::ALL.into_iter().flat_map(|shape| {
            Tone::ALL.into_iter().flat_map(move |tone| [false, true].map(|ring| Indicator::new(shape, tone, ring)))
        })
    }

    #[test]
    fn marks_are_written_as_names() {
        for mark in every_mark().filter(|mark| mark.is_shown()) {
            assert_eq!(mark.to_string().parse::<Indicator>(), Ok(mark));
        }
        assert_eq!("dim-two-dots".parse(), Ok(Indicator::new(Shape::TwoDots, Tone::Dim, false)));
        assert_eq!("accent-ring-bar".parse(), Ok(Indicator::new(Shape::Bar, Tone::Accent, true)));
        assert_eq!("ring-accent-window-dots".parse(), Ok(Indicator::new(Shape::WindowDots, Tone::Accent, true)));
        assert_eq!("bright-dot".parse(), Ok(Indicator::new(Shape::Dot, Tone::Bright, false)));
        assert_eq!("dim-ring-none".parse(), Ok(Indicator::NONE));
        assert!("squiggle".parse::<Indicator>().is_err());
        assert!("dim-".parse::<Indicator>().is_err());
    }

    #[test]
    fn the_config_reads_marks_by_status() {
        let config: IndicatorConfig = toml::from_str("minimized = \"bar\"\nhidden = \"none\"").unwrap();
        let indicators = Indicators::from_config(&config);
        assert_eq!(indicators.get(Status::Minimized), Indicator::new(Shape::Bar, Tone::Bright, false));
        assert!(!indicators.get(Status::Hidden).is_shown());
        assert_eq!(indicators.get(Status::Focused), Indicators::default().get(Status::Focused));
        assert!(toml::from_str::<IndicatorConfig>("open = \"blob\"").is_err());
    }

    #[test]
    fn hidden_then_focused_then_where_the_windows_are() {
        let everywhere = Presence { open: 1, minimized: 1, elsewhere: 1 };
        assert_eq!(Status::of(everywhere, true, true), Status::Hidden);
        assert_eq!(Status::of(everywhere, true, false), Status::Focused);
        assert_eq!(Status::of(everywhere, false, false), Status::Open);
        let minimized = Presence { minimized: 2, elsewhere: 1, ..Default::default() };
        assert_eq!(Status::of(minimized, false, false), Status::Minimized);
        let elsewhere = Presence { elsewhere: 4, ..Default::default() };
        assert_eq!(Status::of(elsewhere, false, false), Status::OtherDesktops);
        assert_eq!(Status::of(Presence::default(), false, false), Status::NoWindows);
    }

    #[test]
    fn a_dot_per_window_counts_up_to_three() {
        let mark = Indicator::new(Shape::WindowDots, Tone::Accent, true);
        let shapes = [0, 1, 2, 3, 7].map(|windows| mark.for_windows(windows).shape);
        assert_eq!(shapes, [Shape::Dot, Shape::Dot, Shape::TwoDots, Shape::ThreeDots, Shape::ThreeDots]);
        assert_eq!(mark.for_windows(2).tone, Tone::Accent);
        let presence = Presence { open: 2, minimized: 1, elsewhere: 5 };
        assert_eq!(presence.windows(Status::Open), 3);
        assert_eq!(presence.windows(Status::OtherDesktops), 5);
    }

    #[test]
    fn every_mark_is_three_pills_side_by_side() {
        for mark in every_mark() {
            let pills = mark.pills();
            assert!(pills.windows(2).all(|pair| pair[0].0 <= pair[1].0), "{mark}");
            assert!(pills.iter().all(|(_, width)| *width >= 1.0), "{mark}");
        }
    }
}
