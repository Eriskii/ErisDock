use serde::{Deserialize, Serialize};

use crate::spring::Spring;

/// A spring's response (period in seconds) and damping fraction.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SpringSpec {
    pub response: f64,
    pub damping: f64,
}

impl SpringSpec {
    const fn new(response: f64, damping: f64) -> Self {
        Self { response, damping }
    }

    /// From Material's stiffness, for a unit mass.
    fn stiffness(stiffness: f64, damping: f64) -> Self {
        Self { response: std::f64::consts::TAU / stiffness.sqrt(), damping }
    }

    pub fn spring(self, value: f64) -> Spring {
        Spring::new(value, self.response, self.damping)
    }
}

impl Spring {
    pub fn animate_spec(&mut self, target: f64, spec: SpringSpec) {
        self.animate_with(target, spec.response, spec.damping);
    }

    /// Moves on with `spec` from wherever it is now.
    pub fn retune(&mut self, spec: SpringSpec) {
        self.response = spec.response;
        self.damping = spec.damping;
    }
}

/// A set of animations, picked as a whole in the settings.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Style {
    #[default]
    Squishy,
    Expressive,
    Standard,
}

impl Style {
    pub const ALL: [Style; 3] = [Style::Squishy, Style::Expressive, Style::Standard];

    pub fn title(self) -> &'static str {
        match self {
            Style::Expressive => "Expressive",
            Style::Squishy => "Squishy",
            Style::Standard => "Standard",
        }
    }

    pub fn summary(self) -> &'static str {
        match self {
            Style::Expressive => "Material 3 Expressive springs: quick and direct, with a little give at the end.",
            Style::Squishy => "Rises before it widens, overshooting tall and then wide, like jelly.",
            Style::Standard => "Material 3 standard springs: straight up and out, no bounce.",
        }
    }

    pub fn animation(self) -> Animation {
        // Material 3's motion tokens, as stiffness and damping.
        let expressive_fast = SpringSpec::stiffness(800.0, 0.6);
        let expressive_default = SpringSpec::stiffness(380.0, 0.8);
        let standard_fast = SpringSpec::stiffness(1400.0, 0.9);
        let standard_default = SpringSpec::stiffness(700.0, 0.9);
        let effects = SpringSpec::stiffness(1600.0, 1.0);
        match self {
            Style::Expressive => Animation {
                rise: expressive_default,
                spread: expressive_default,
                collapsed_width: 0.8,
                tuck: standard_fast,
                tuck_spread: SpringSpec { damping: 1.0, ..standard_fast },
                appear: expressive_fast,
                leave: effects,
                hover: expressive_fast,
                press: SpringSpec { damping: 0.7, ..standard_fast },
                active: expressive_default,
                label: effects,
                label_move: expressive_default,
                slide: expressive_default,
                bounce: SpringSpec::stiffness(380.0, 0.35),
                hover_scale: 0.08,
                hover_lift: 3.0,
                press_scale: (-0.08, -0.08),
            },
            Style::Squishy => Animation {
                rise: SpringSpec::new(0.42, 0.58),
                spread: SpringSpec::new(0.5, 0.62),
                collapsed_width: 0.45,
                tuck: SpringSpec::new(0.3, 0.92),
                tuck_spread: SpringSpec::new(0.34, 1.0),
                appear: SpringSpec::new(0.42, 0.6),
                leave: SpringSpec::new(0.3, 1.0),
                hover: SpringSpec::new(0.3, 0.55),
                press: SpringSpec::new(0.16, 0.5),
                active: SpringSpec::new(0.3, 0.75),
                label: SpringSpec::new(0.24, 0.8),
                label_move: SpringSpec::new(0.32, 0.82),
                slide: SpringSpec::new(0.38, 0.7),
                bounce: SpringSpec::new(0.42, 0.3),
                hover_scale: 0.12,
                hover_lift: 4.0,
                press_scale: (0.1, -0.16),
            },
            Style::Standard => Animation {
                rise: standard_default,
                spread: standard_default,
                collapsed_width: 1.0,
                tuck: SpringSpec { damping: 0.95, ..standard_fast },
                tuck_spread: SpringSpec { damping: 1.0, ..standard_fast },
                appear: standard_default,
                leave: effects,
                hover: standard_fast,
                press: standard_fast,
                active: standard_default,
                label: effects,
                label_move: standard_default,
                slide: standard_default,
                bounce: SpringSpec::stiffness(380.0, 0.5),
                hover_scale: 0.06,
                hover_lift: 2.0,
                press_scale: (-0.05, -0.05),
            },
        }
    }
}

/// How each part of the dock moves.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Animation {
    /// The body rising out of the edge, and widening as it does.
    pub rise: SpringSpec,
    pub spread: SpringSpec,
    /// The body's width, as a fraction of its full width, while it's tucked into the edge.
    pub collapsed_width: f64,
    /// The body tucking back in.
    pub tuck: SpringSpec,
    pub tuck_spread: SpringSpec,
    /// An app's icon arriving and leaving.
    pub appear: SpringSpec,
    pub leave: SpringSpec,
    pub hover: SpringSpec,
    pub press: SpringSpec,
    /// The frontmost app's dot stretching into a bar.
    pub active: SpringSpec,
    /// The name above the dock showing, and sliding from icon to icon.
    pub label: SpringSpec,
    pub label_move: SpringSpec,
    /// An icon sliding into its place after the row is reordered or it's dropped.
    pub slide: SpringSpec,
    /// A launching app's icon bouncing.
    pub bounce: SpringSpec,
    /// How much a hovered icon grows, and how far it lifts.
    pub hover_scale: f64,
    pub hover_lift: f64,
    /// How much a pressed icon's width and height change.
    pub press_scale: (f64, f64),
}

#[cfg(test)]
mod tests {
    use super::*;

    fn peak(spec: SpringSpec) -> f64 {
        let mut spring = spec.spring(0.0);
        spring.animate(1.0);
        (0..240).fold(0.0, |peak: f64, _| {
            spring.step(1.0 / 120.0);
            peak.max(spring.value)
        })
    }

    #[test]
    fn material_stiffness_becomes_a_response() {
        let spec = SpringSpec::stiffness(380.0, 0.8);
        assert!((spec.response - 0.3223).abs() < 1e-3, "{}", spec.response);
    }

    #[test]
    fn the_styles_rise_from_most_to_least_bounce() {
        let [squishy, expressive, standard] = Style::ALL.map(|style| peak(style.animation().rise));
        assert!(squishy > expressive && expressive > standard, "{squishy} {expressive} {standard}");
        assert!(standard < 1.005);
    }

    #[test]
    fn styles_are_written_in_lowercase() {
        #[derive(Serialize, Deserialize)]
        struct Wrapper {
            style: Style,
        }
        let text = toml::to_string(&Wrapper { style: Style::Squishy }).unwrap();
        assert_eq!(text.trim(), "style = \"squishy\"");
        assert_eq!(toml::from_str::<Wrapper>(&text).unwrap().style, Style::Squishy);
    }
}
