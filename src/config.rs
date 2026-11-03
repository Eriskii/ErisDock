use std::path::PathBuf;
use std::time::SystemTime;
use std::{fs, io};

use serde::{Deserialize, Serialize};
use toml_edit::{DocumentMut, value};

use crate::animation::Style;
use crate::indicators::{IndicatorConfig, Status};

/// The config file, `~/.config/erisdock/config.toml`. Anything left out takes its default; the
/// sizes' defaults scale with the icon size.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub animation: Style,
    pub icon_size: Option<f64>,
    pub spacing: Option<f64>,
    pub padding: Option<f64>,
    pub top_padding: Option<f64>,
    pub bottom_padding: Option<f64>,
    pub dot_size: Option<f64>,
    pub corner_radius: Option<f64>,
    pub flare_radius: Option<f64>,
    pub smoothing: Option<f64>,
    pub reveal_delay: Option<f64>,
    pub hide_delay: Option<f64>,
    pub trigger_margin: Option<f64>,
    pub pinning: Option<bool>,
    pub block_self_pinning: Option<bool>,
    pub indicators: IndicatorConfig,
}

/// What a new config file says: every setting, the ones left at their defaults commented out.
const TEMPLATE: &str = r#"# ErisDock's settings. ErisDock picks up changes to this file as soon as it's saved, and its
# Settings window (right-click the dock between icons) writes the common ones here too.

# How the dock moves: "squishy" (the default), "expressive" (Material 3 Expressive) or
# "standard" (Material 3, no bounce).
animation = "squishy"

# Icon size in points. Left out, it's the native Dock's.
# icon_size = 48

# Sizes in points. Left out, each is a fraction of the icon size.
# spacing = 9          # between icons (0.18 × icon)
# padding = 12         # between the outer icons and the dock's sides (0.26 × icon)
# top_padding = 10     # above the icons (0.2 × icon)
# bottom_padding = 16  # below the icons, holding the dots (top_padding + dot_size + 2)
# dot_size = 4         # (0.08 × icon, at least 3)
# corner_radius = 22   # the top corners (0.46 × icon)
# flare_radius = 12    # the inner curves where the dock meets the screen's edge (0.26 × icon)

# Figma-style corner smoothing, from 0 (circular corners) to 1.
# smoothing = 0.6

# Seconds the pointer rests on the bottom edge before the dock rises, and stays away before
# it tucks back in.
# reveal_delay = 0.06
# hide_delay = 0.35

# How far beside the dock, in points, the bottom edge still raises it.
# trigger_margin = 48

# Pinned apps stay in the dock whether they're running or not; they're kept in pinned.toml,
# beside this file. Turned off, the dock shows only running apps.
# pinning = true

# Apps add themselves to the native Dock (installers, "Keep in Dock?" prompts). ErisDock never
# follows them unless this is turned off; only you pin apps to it.
# block_self_pinning = true

# The marks under running apps' icons, by what their windows are doing: "dot", "two-dots",
# "three-dots", "bar" or "none", with "dim-" in front for a dim one, as in "dim-dot". "Here" is
# the desktop on screen.
# [indicators]
# focused = "bar"               # the app in front
# open = "dot"                  # a window open here
# minimized = "dim-two-dots"    # windows here, all minimized
# other_desktops = "dim-dot"    # windows only on other desktops
# no_windows = "dim-dot"        # running without windows
# hidden = "dim-dot"            # hidden with ⌘H
"#;

pub fn path() -> PathBuf {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".config"));
    base.join("erisdock").join("config.toml")
}

pub fn modified() -> Option<SystemTime> {
    fs::metadata(path()).and_then(|metadata| metadata.modified()).ok()
}

/// Reads the config file, writing the template first if there isn't one.
pub fn load() -> Result<Config, String> {
    let path = path();
    let text = match fs::read_to_string(&path) {
        Ok(text) => text,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            if let Some(dir) = path.parent() {
                let _ = fs::create_dir_all(dir);
            }
            let _ = fs::write(&path, TEMPLATE);
            TEMPLATE.to_owned()
        }
        Err(error) => return Err(format!("{}: {error}", path.display())),
    };
    parse(&text).map_err(|error| format!("{}: {error}", path.display()))
}

pub fn parse(text: &str) -> Result<Config, String> {
    toml::from_str(text).map_err(|error| error.to_string())
}

/// Writes `config` into the file, keeping its comments and whatever else it holds.
pub fn save(config: &Config) -> io::Result<()> {
    let path = path();
    let text = fs::read_to_string(&path).unwrap_or_else(|_| TEMPLATE.to_owned());
    let updated = update(&text, config).map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    fs::write(path, updated)
}

/// `text` with `config`'s values written into it.
pub fn update(text: &str, config: &Config) -> Result<String, String> {
    let mut document: DocumentMut = text.parse().map_err(|error: toml_edit::TomlError| error.to_string())?;
    document["animation"] = value(match config.animation {
        Style::Expressive => "expressive",
        Style::Squishy => "squishy",
        Style::Standard => "standard",
    });
    let numbers = [
        ("icon_size", config.icon_size),
        ("spacing", config.spacing),
        ("padding", config.padding),
        ("top_padding", config.top_padding),
        ("bottom_padding", config.bottom_padding),
        ("dot_size", config.dot_size),
        ("corner_radius", config.corner_radius),
        ("flare_radius", config.flare_radius),
        ("smoothing", config.smoothing),
        ("reveal_delay", config.reveal_delay),
        ("hide_delay", config.hide_delay),
        ("trigger_margin", config.trigger_margin),
    ];
    for (key, flag) in [("pinning", config.pinning), ("block_self_pinning", config.block_self_pinning)] {
        match flag {
            Some(flag) => document[key] = value(flag),
            None => {
                document.remove(key);
            }
        }
    }
    let marks: Vec<_> = Status::ALL.iter().map(|status| (status.key(), config.indicators.get(*status))).collect();
    if document.contains_key("indicators") || marks.iter().any(|(_, mark)| mark.is_some()) {
        let table = document.entry("indicators").or_insert(toml_edit::table());
        let table = table.as_table_mut().ok_or("indicators should be a table")?;
        for (key, mark) in marks {
            match mark {
                Some(mark) => table[key] = value(mark.to_string()),
                None => {
                    table.remove(key);
                }
            }
        }
        if table.is_empty() {
            document.remove("indicators");
        }
    }
    for (key, number) in numbers {
        match number {
            // Whole numbers are written as integers, which read back as floats all the same.
            Some(number) if number.fract() == 0.0 && number.abs() < 1e9 => document[key] = value(number as i64),
            Some(number) => document[key] = value((number * 1000.0).round() / 1000.0),
            None => {
                document.remove(key);
            }
        }
    }
    Ok(document.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_template_reads_as_the_defaults() {
        assert_eq!(parse(TEMPLATE).unwrap(), Config::default());
    }

    #[test]
    fn integers_and_floats_both_read_as_sizes() {
        let config = parse("icon_size = 40\nsmoothing = 0.5\nanimation = \"standard\"").unwrap();
        assert_eq!(config.icon_size, Some(40.0));
        assert_eq!(config.smoothing, Some(0.5));
        assert_eq!(config.animation, Style::Standard);
    }

    #[test]
    fn bad_values_are_reported() {
        assert!(parse("animation = \"wobbly\"").is_err());
        assert!(parse("icon_size = \"big\"").is_err());
    }

    #[test]
    fn saving_keeps_comments_and_round_trips() {
        let config = Config {
            animation: Style::Standard,
            icon_size: Some(40.0),
            hide_delay: Some(0.5),
            block_self_pinning: Some(false),
            indicators: IndicatorConfig { minimized: Some("bar".parse().unwrap()), ..Default::default() },
            ..Config::default()
        };
        let text = update(TEMPLATE, &config).unwrap();
        assert!(text.contains("# ErisDock's settings."));
        assert!(text.contains("icon_size = 40\n"));
        assert!(text.contains("[indicators]\nminimized = \"bar\"\n"));
        assert_eq!(parse(&text).unwrap(), config);

        let cleared = update(&text, &Config::default()).unwrap();
        assert!(!cleared.contains("\n[indicators]"));
        assert!(!cleared.contains("icon_size = 40"));
        assert_eq!(parse(&cleared).unwrap(), Config::default());
    }

    #[test]
    fn settings_added_after_the_indicators_stay_out_of_their_table() {
        let config = Config {
            indicators: IndicatorConfig { hidden: Some("none".parse().unwrap()), ..Default::default() },
            ..Config::default()
        };
        let text = update("animation = \"squishy\"\n", &config).unwrap();
        let config = Config { icon_size: Some(30.0), ..config };
        let text = update(&text, &config).unwrap();
        assert_eq!(parse(&text).unwrap(), config, "{text}");
    }
}
