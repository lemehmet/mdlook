//! The optional config file, and how it merges with the command line.
//!
//! Three levels of precedence, in this order: an explicit command-line flag, a
//! value from the config file, then the built-in default. That is why almost
//! every field here is an `Option` — "the user did not say" has to stay
//! distinguishable from "the user said the default", or a config setting could
//! never take effect against a flag that always has a value.
//!
//! mdlook reads this file and never writes it. There is no `mdlook init`, no
//! migration on upgrade, and no cache beside it.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::Deserialize;

use crate::layout::picture::BlockMode;

/// Width of the file browser's sidebar, in columns.
pub const DEFAULT_SIDEBAR_WIDTH: usize = 30;

/// Bounds on the sidebar width.
///
/// Narrower than the lower bound and a file name is all ellipsis; wider than the
/// upper bound and the sidebar is taking space from the thing you are reading.
pub const SIDEBAR_WIDTH_RANGE: std::ops::RangeInclusive<usize> = 12..=60;

/// A parsed config file. Every field is optional; an empty file is valid and
/// means "no opinions".
#[derive(Debug, Default, Deserialize, PartialEq, Eq)]
// Strict rather than forgiving: a mistyped key that silently does nothing is a
// worse experience than one that says so. The cost is that a config written for
// a newer mdlook is rejected by an older one, which is the right way round.
#[serde(deny_unknown_fields)]
pub struct Config {
    /// Open the file browser instead of going straight to a single file.
    pub browse: Option<bool>,
    /// Colour scheme: `dark`, `light` or `mono`.
    pub theme: Option<String>,
    /// Wrap width. Omitted means "follow the terminal".
    pub width: Option<usize>,
    #[serde(default)]
    pub browser: Browser,
    #[serde(default)]
    pub images: Images,
}

#[derive(Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Browser {
    /// Show dotfiles in the tree.
    pub hidden: Option<bool>,
    pub sidebar_width: Option<usize>,
    /// External command used to identify binary files, e.g. `"file"`.
    ///
    /// Empty or absent means the built-in identifier is used and no process is
    /// ever started. See [`Settings::probe_command`].
    pub probe_command: Option<String>,
}

#[derive(Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Images {
    /// Render images as coloured block characters. Off means an image is
    /// identified like any other binary — the setting for directories that are
    /// mostly photographs.
    pub enabled: Option<bool>,
    /// Starting subpixel grid: `half`, `quadrant`, `sextant` or `octant`.
    ///
    /// Typed rather than a string so a misspelling is an error at load, the
    /// same deal `deny_unknown_fields` gives a mistyped key. The finer grids
    /// need font support no terminal will report having, which is why this is
    /// only a starting point — the viewer cycles through them with a key.
    pub block_mode: Option<BlockMode>,
}

impl Config {
    /// Where the config lives when the user has not said otherwise.
    ///
    /// XDG on Unix, `%APPDATA%` on Windows. Returns `None` when neither the
    /// relevant variable nor a home directory is set, which is normal in a
    /// container and simply means there is no config.
    pub fn default_path() -> Option<PathBuf> {
        let base = if cfg!(windows) {
            std::env::var_os("APPDATA").map(PathBuf::from)
        } else {
            std::env::var_os("XDG_CONFIG_HOME")
                .map(PathBuf::from)
                .filter(|p| p.is_absolute())
                .or_else(|| {
                    std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config"))
                })
        };
        Some(base?.join("mdlook").join("config.toml"))
    }

    /// Load from an explicit path. A missing file here is an error, because the
    /// user named it.
    pub fn load(path: &Path) -> Result<Self> {
        let text = std::fs::read_to_string(path)
            .with_context(|| format!("reading config {}", path.display()))?;
        Self::parse(&text).with_context(|| format!("in config {}", path.display()))
    }

    /// Load from the default location, where absence is the normal case and
    /// means "no config". A file that exists but does not parse is still an
    /// error: silently ignoring it would leave the reader wondering why their
    /// settings do nothing.
    pub fn load_default() -> Result<Self> {
        match Self::default_path() {
            Some(path) if path.is_file() => Self::load(&path),
            _ => Ok(Self::default()),
        }
    }

    pub fn parse(text: &str) -> Result<Self> {
        toml::from_str(text).context("parsing TOML")
    }

    /// Merge under the command line and fill in the defaults.
    pub fn resolve(self, cli: Overrides) -> Settings {
        let sidebar_width = self
            .browser
            .sidebar_width
            .unwrap_or(DEFAULT_SIDEBAR_WIDTH)
            .clamp(*SIDEBAR_WIDTH_RANGE.start(), *SIDEBAR_WIDTH_RANGE.end());

        Settings {
            browse: cli.browse.or(self.browse).unwrap_or(false),
            theme: cli.theme.or(self.theme).unwrap_or_else(|| "dark".to_string()),
            width: cli.width.or(self.width),
            hidden: self.browser.hidden.unwrap_or(false),
            sidebar_width,
            // An empty string is how a config disables a command it previously
            // set, so it means the same as absent rather than "run ``".
            probe_command: self.browser.probe_command.filter(|c| !c.trim().is_empty()),
            images: cli.images.or(self.images.enabled).unwrap_or(true),
            block_mode: self.images.block_mode.unwrap_or_default(),
        }
    }
}

/// What the command line had to say. `None` means the flag was not given.
#[derive(Debug, Default)]
pub struct Overrides {
    pub browse: Option<bool>,
    pub theme: Option<String>,
    pub width: Option<usize>,
    pub images: Option<bool>,
}

/// The config file merged under the command line, with defaults applied.
#[derive(Debug, PartialEq, Eq)]
pub struct Settings {
    pub browse: bool,
    pub theme: String,
    /// `None` means "follow the terminal".
    pub width: Option<usize>,
    pub hidden: bool,
    pub sidebar_width: usize,
    /// External identifier for binary files, if the user asked for one.
    ///
    /// Absent by default, and absent is the interesting case: with no command
    /// configured mdlook never starts a process, which is the posture the rest
    /// of the tool is built around. Setting this is an explicit trade of that
    /// guarantee for `file(1)`'s much larger database.
    pub probe_command: Option<String>,
    /// Render images as block characters, on by default.
    pub images: bool,
    /// The subpixel grid the image renderer starts in.
    pub block_mode: BlockMode,
}

impl Default for Settings {
    fn default() -> Self {
        Config::default().resolve(Overrides::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_absent_or_empty_config_is_all_defaults() {
        let settings = Settings::default();
        assert!(!settings.browse);
        assert_eq!(settings.theme, "dark");
        assert_eq!(settings.width, None);
        assert_eq!(settings.sidebar_width, DEFAULT_SIDEBAR_WIDTH);
        assert_eq!(settings.probe_command, None);
        assert_eq!(Config::parse("").unwrap(), Config::default());
    }

    #[test]
    fn a_full_config_round_trips() {
        let config = Config::parse(
            r#"
            browse = true
            theme  = "light"
            width  = 72

            [browser]
            hidden        = true
            sidebar_width = 24
            probe_command = "file"
            "#,
        )
        .unwrap();
        let settings = config.resolve(Overrides::default());
        assert!(settings.browse);
        assert_eq!(settings.theme, "light");
        assert_eq!(settings.width, Some(72));
        assert!(settings.hidden);
        assert_eq!(settings.sidebar_width, 24);
        assert_eq!(settings.probe_command.as_deref(), Some("file"));
    }

    #[test]
    fn the_command_line_beats_the_config_file() {
        let config = Config::parse("browse = true\ntheme = \"light\"\nwidth = 72").unwrap();
        let settings = config.resolve(Overrides {
            browse: Some(false),
            theme: Some("mono".into()),
            width: Some(40),
            images: None,
        });
        assert!(!settings.browse, "--no-browse must override browse = true");
        assert_eq!(settings.theme, "mono");
        assert_eq!(settings.width, Some(40));
    }

    #[test]
    fn a_flag_that_was_not_given_does_not_override_anything() {
        // The whole reason `Overrides` is optional per field: `--width 40` must
        // not quietly reset the theme to its default.
        let config = Config::parse("theme = \"light\"\nbrowse = true").unwrap();
        let settings = config.resolve(Overrides { width: Some(40), ..Default::default() });
        assert_eq!(settings.theme, "light");
        assert!(settings.browse);
    }

    #[test]
    fn a_mistyped_key_is_reported_rather_than_ignored() {
        let error = Config::parse("brows = true").unwrap_err();
        let text = format!("{error:#}");
        assert!(text.contains("brows"), "the error should name the key: {text}");
    }

    #[test]
    fn a_wrong_type_is_reported() {
        assert!(Config::parse("browse = \"yes\"").is_err());
        assert!(Config::parse("width = \"wide\"").is_err());
    }

    #[test]
    fn an_unknown_key_under_browser_is_caught_too() {
        assert!(Config::parse("[browser]\nsidebar_widht = 30").is_err());
    }

    #[test]
    fn an_absurd_sidebar_width_is_clamped_rather_than_rejected() {
        // A number out of range is a preference expressed badly, not a mistake
        // worth refusing to start over.
        let wide = Config::parse("[browser]\nsidebar_width = 5000").unwrap();
        assert_eq!(wide.resolve(Overrides::default()).sidebar_width, *SIDEBAR_WIDTH_RANGE.end());
        let narrow = Config::parse("[browser]\nsidebar_width = 0").unwrap();
        assert_eq!(
            narrow.resolve(Overrides::default()).sidebar_width,
            *SIDEBAR_WIDTH_RANGE.start()
        );
    }

    #[test]
    fn images_default_on_with_half_blocks() {
        let settings = Settings::default();
        assert!(settings.images);
        assert_eq!(settings.block_mode, BlockMode::Half);
    }

    #[test]
    fn the_images_section_parses_and_the_flag_overrides_it() {
        let text = "[images]\nenabled = true\nblock_mode = \"sextant\"";
        let settings = Config::parse(text)
            .unwrap()
            .resolve(Overrides { images: Some(false), ..Default::default() });
        assert!(!settings.images, "--no-images must override enabled = true");
        assert_eq!(settings.block_mode, BlockMode::Sextant);
    }

    #[test]
    fn a_misspelled_block_mode_is_an_error_not_a_silent_default() {
        // The same contract as deny_unknown_fields: a config that does nothing
        // should say so at load, not at the moment an image looks wrong.
        assert!(Config::parse("[images]\nblock_mode = \"sextants\"").is_err());
    }

    #[test]
    fn an_empty_probe_command_means_no_command() {
        let config = Config::parse("[browser]\nprobe_command = \"  \"").unwrap();
        assert_eq!(config.resolve(Overrides::default()).probe_command, None);
    }

    #[test]
    fn a_missing_config_at_the_default_location_is_not_an_error() {
        assert!(Config::load_default().is_ok());
    }

    #[test]
    fn a_config_the_user_named_must_exist() {
        let error = Config::load(Path::new("/nonexistent/mdlook.toml")).unwrap_err();
        assert!(format!("{error:#}").contains("mdlook.toml"));
    }

    #[test]
    fn the_default_path_lands_under_the_expected_directory() {
        let path = Config::default_path().expect("a config path in this environment");
        assert!(path.ends_with("mdlook/config.toml"), "got {}", path.display());
        assert!(path.is_absolute());
    }
}
