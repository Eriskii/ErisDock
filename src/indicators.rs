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
    Bar,
}

/// A mark: its shape, bright or dim. Written as `"dot"`, `"two-dots"`, `"three-dots"`, `"bar"`
/// or `"none"`, with `"dim-"` in front for a dim one.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct Indicator {
    pub shape: Shape,
    pub dim: bool,
}

impl Indicator {
    pub const fn new(shape: Shape, dim: bool) -> Self {
        Self { shape, dim }
    }

    /// Every mark, in the order the settings list them.
    pub const ALL: [Indicator; 9] = [
        Indicator::new(Shape::Dot, false),
        Indicator::new(Shape::TwoDots, false),
        Indicator::new(Shape::ThreeDots, false),
        Indicator::new(Shape::Bar, false),
        Indicator::new(Shape::Dot, true),
        Indicator::new(Shape::TwoDots, true),
        Indicator::new(Shape::ThreeDots, true),
        Indicator::new(Shape::Bar, true),
        Indicator::new(Shape::None, false),
    ];

    pub fn title(self) -> String {
        let shape = match self.shape {
            Shape::None => return "Nothing".into(),
            Shape::Dot => "dot",
            Shape::TwoDots => "two dots",
            Shape::ThreeDots => "three dots",
            Shape::Bar => "bar",
        };
        format!("{} {shape}", if self.dim { "Dim" } else { "Bright" })
    }

    /// Where its three pills sit and how long they are, in dot sizes from the middle. A mark
    /// with fewer stacks some on top of each other, so one mark turns into another by sliding
    /// its pills over.
    pub fn pills(self) -> [(f64, f64); 3] {
        match self.shape {
            Shape::None | Shape::Dot => [(0.0, 1.0); 3],
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
        let shape = match self.shape {
            Shape::None => return f.write_str("none"),
            Shape::Dot => "dot",
            Shape::TwoDots => "two-dots",
            Shape::ThreeDots => "three-dots",
            Shape::Bar => "bar",
        };
        write!(f, "{}{shape}", if self.dim { "dim-" } else { "" })
    }
}

impl FromStr for Indicator {
    type Err = String;

    fn from_str(text: &str) -> Result<Self, String> {
        let (dim, shape) = match text.strip_prefix("dim-") {
            Some(shape) => (true, shape),
            None => (false, text.strip_prefix("bright-").unwrap_or(text)),
        };
        let shape = match shape {
            "none" => Shape::None,
            "dot" => Shape::Dot,
            "two-dots" => Shape::TwoDots,
            "three-dots" => Shape::ThreeDots,
            "bar" => Shape::Bar,
            _ => {
                return Err(format!(
                    "unknown mark \"{text}\"; use \"dot\", \"two-dots\", \"three-dots\", \"bar\" or \"none\", \
                     with \"dim-\" in front for a dim one"
                ));
            }
        };
        Ok(Self { shape, dim: dim && shape != Shape::None })
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
    /// A window showing on a desktop that's on screen.
    pub open: bool,
    /// A window minimized from a desktop that's on screen.
    pub minimized: bool,
    /// A window on a desktop that isn't on screen, minimized or not.
    pub elsewhere: bool,
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
        } else if presence.open {
            Status::Open
        } else if presence.minimized {
            Status::Minimized
        } else if presence.elsewhere {
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
        Self([
            Indicator::new(Bar, false),
            Indicator::new(Dot, false),
            Indicator::new(TwoDots, true),
            Indicator::new(Dot, true),
            Indicator::new(Dot, true),
            Indicator::new(Dot, true),
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

    #[test]
    fn marks_are_written_as_names() {
        for indicator in Indicator::ALL {
            assert_eq!(indicator.to_string().parse::<Indicator>(), Ok(indicator));
        }
        assert_eq!("dim-two-dots".parse(), Ok(Indicator::new(Shape::TwoDots, true)));
        assert_eq!("bright-bar".parse(), Ok(Indicator::new(Shape::Bar, false)));
        assert_eq!("dim-none".parse(), Ok(Indicator::new(Shape::None, false)));
        assert!("squiggle".parse::<Indicator>().is_err());
    }

    #[test]
    fn the_config_reads_marks_by_status() {
        let config: IndicatorConfig = toml::from_str("minimized = \"bar\"\nhidden = \"none\"").unwrap();
        let indicators = Indicators::from_config(&config);
        assert_eq!(indicators.get(Status::Minimized), Indicator::new(Shape::Bar, false));
        assert!(!indicators.get(Status::Hidden).is_shown());
        assert_eq!(indicators.get(Status::Focused), Indicators::default().get(Status::Focused));
        assert!(toml::from_str::<IndicatorConfig>("open = \"blob\"").is_err());
    }

    #[test]
    fn hidden_then_focused_then_where_the_windows_are() {
        let everywhere = Presence { open: true, minimized: true, elsewhere: true };
        assert_eq!(Status::of(everywhere, true, true), Status::Hidden);
        assert_eq!(Status::of(everywhere, true, false), Status::Focused);
        assert_eq!(Status::of(everywhere, false, false), Status::Open);
        assert_eq!(
            Status::of(Presence { minimized: true, elsewhere: true, ..Default::default() }, false, false),
            Status::Minimized
        );
        assert_eq!(Status::of(Presence { elsewhere: true, ..Default::default() }, false, false), Status::OtherDesktops);
        assert_eq!(Status::of(Presence::default(), false, false), Status::NoWindows);
    }

    #[test]
    fn every_mark_is_three_pills_side_by_side() {
        for indicator in Indicator::ALL {
            let pills = indicator.pills();
            assert!(pills.windows(2).all(|pair| pair[0].0 <= pair[1].0), "{indicator}");
            assert!(pills.iter().all(|(_, width)| *width >= 1.0), "{indicator}");
        }
    }
}
