use std::io::{IsTerminal, Read, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use clap::Parser;
use mdlook::config::{Overrides, Settings};
use mdlook::files::Tree;
use mdlook::render::tui::DEFAULT_MAX_WIDTH;
use mdlook::ui::App;
use mdlook::{parse, render, Config, Content, Theme, ThemeKind};

#[derive(Parser)]
#[command(
    name = "mdlook",
    version,
    about = "A terminal markdown reader with reflow you can trust and a search index you can navigate"
)]
struct Args {
    /// File or directory to view. Markdown is rendered; anything else that is
    /// text is shown with syntax highlighting; a binary is identified rather
    /// than dumped. A directory opens the file browser. Reads standard input
    /// when omitted or given as `-`.
    file: Option<String>,

    /// Write rendered ANSI to standard output instead of opening the viewer.
    /// Implied when standard output is not a terminal.
    #[arg(short, long)]
    plain: bool,

    /// Wrap width. Defaults to the terminal width, capped at 100 columns.
    #[arg(short, long)]
    width: Option<usize>,

    /// Colour scheme: dark, light, or mono.
    #[arg(short = 't', long)]
    theme: Option<String>,

    /// Disable colour. Also honoured via the NO_COLOR environment variable.
    #[arg(long)]
    no_color: bool,

    /// Open the file browser alongside the viewer.
    #[arg(long, overrides_with = "no_browse")]
    browse: bool,

    /// Go straight to the file, even if the config asks for the browser.
    #[arg(long, overrides_with = "browse")]
    no_browse: bool,

    /// Read this config file instead of the one in the default location.
    #[arg(long, value_name = "PATH", conflicts_with = "no_config")]
    config: Option<PathBuf>,

    /// Ignore the config file entirely and use the built-in defaults.
    #[arg(long)]
    no_config: bool,
}

impl Args {
    /// What the command line said about settings a config can also supply.
    ///
    /// `--browse` and `--no-browse` override each other, so at most one is set;
    /// neither means the config decides.
    fn overrides(&self) -> Overrides {
        Overrides {
            browse: self.browse.then_some(true).or(self.no_browse.then_some(false)),
            theme: self.theme.clone(),
            width: self.width,
        }
    }
}

fn main() -> Result<()> {
    let args = Args::parse();

    let config = match (&args.config, args.no_config) {
        (_, true) => Config::default(),
        (Some(path), _) => Config::load(path)?,
        (None, _) => Config::load_default()?,
    };
    let settings = config.resolve(args.overrides());

    let browse_here = browse_here(
        args.file.as_deref(),
        settings.browse,
        args.plain,
        std::io::stdout().is_terminal(),
        std::io::stdin().is_terminal(),
    );

    let stdin_is_source = !browse_here && matches!(args.file.as_deref(), None | Some("-"));

    let color = !args.no_color && std::env::var_os("NO_COLOR").is_none();
    let kind = if color {
        ThemeKind::parse(&settings.theme).unwrap_or(ThemeKind::Dark)
    } else {
        ThemeKind::Mono
    };
    let theme = Theme::new(kind);

    // The viewer needs a terminal to draw on and a keyboard to drive it. Taking
    // the document from stdin consumes the keyboard, so that case falls back to
    // dumping rather than opening a viewer nobody could control.
    let interactive = !args.plain && std::io::stdout().is_terminal() && !stdin_is_source;

    let here = std::env::current_dir().context("finding the working directory")?;
    let target = match args.file.as_deref() {
        Some(path) => Some(PathBuf::from(path)),
        None if browse_here => Some(here),
        None => None,
    };
    let probe = settings.probe_command.as_deref();
    let browse = interactive && wants_browser(target.as_deref(), &settings);

    let (content, title) = match args.file.as_deref() {
        // The browser is rooted here and nothing is selected yet, so the pane
        // says so until the reader moves the cursor onto a file.
        None if browse_here => {
            let root = target.clone().unwrap_or_default();
            (Content::preview(&root, probe), ".".to_string())
        }
        // Piped input has no name to classify by, and the documented contract
        // for a pipe is markdown, so it is not sniffed.
        None | Some("-") => {
            let mut buffer = String::new();
            std::io::stdin().read_to_string(&mut buffer).context("reading standard input")?;
            (Content::Markdown(parse(&buffer)), "(stdin)".to_string())
        }
        Some(path) if browse => match Path::new(path).is_dir() {
            // Nothing is selected yet, so there is nothing to show beside the
            // tree until the reader moves the cursor onto a file.
            true => (Content::preview(Path::new(path), probe), path.to_string()),
            false => (Content::read(Path::new(path), probe)?, path.to_string()),
        },
        Some(path) => (Content::read(Path::new(path), probe)?, path.to_string()),
    };

    if interactive {
        let width = settings.width.unwrap_or(DEFAULT_MAX_WIDTH);
        let mut app = App::new(content, title, theme, width);
        if browse {
            let (root, reveal) = browser_root(target.as_deref())?;
            let mut tree = Tree::new(&root, settings.hidden);
            if let Some(path) = reveal {
                tree.reveal(&path);
            }
            app = app.with_sidebar(tree, settings.sidebar_width, settings.probe_command.clone());
        }
        return render::tui::run(app, settings.width);
    }

    let width = settings.width.unwrap_or_else(|| {
        if std::io::stdout().is_terminal() {
            crossterm::terminal::size()
                .map(|(w, _)| w as usize)
                .unwrap_or(80)
                .clamp(20, DEFAULT_MAX_WIDTH)
        } else {
            80
        }
    });

    let rendered = content.layout(width, &theme);
    let mut out = std::io::stdout().lock();
    out.write_all(render::to_ansi(&rendered, color).as_bytes())?;
    Ok(())
}

/// Whether a bare invocation should browse the working directory instead of
/// waiting on standard input.
///
/// Asking for the browser is asking for a session, and with no path given the
/// working directory is the thing being asked about. Reading standard input
/// there would block on a keyboard that is also the terminal we are drawing on,
/// which just hangs.
///
/// Everything that names an input still means what it says: an explicit `-`, a
/// real pipe on standard input, and `--plain` all keep the old behaviour.
fn browse_here(
    file: Option<&str>,
    browse: bool,
    plain: bool,
    stdout_tty: bool,
    stdin_tty: bool,
) -> bool {
    file.is_none() && browse && !plain && stdout_tty && stdin_tty
}

/// Whether to open the file browser.
///
/// A directory argument implies it whatever the settings say, because there is
/// nothing else a directory could mean. Otherwise it is the `--browse` flag or
/// the config, already merged into `settings`.
fn wants_browser(target: Option<&Path>, settings: &Settings) -> bool {
    if target.is_some_and(Path::is_dir) {
        return true;
    }
    settings.browse
}

/// The directory the browser is rooted at, and the file to start on.
///
/// Rooting at a file's *parent* rather than at the working directory is what
/// makes `mdlook --browse docs/guide.md` show the file's neighbours, which is
/// the reason to have asked for a browser at all.
fn browser_root(target: Option<&Path>) -> Result<(PathBuf, Option<PathBuf>)> {
    let Some(path) = target else {
        return Ok((std::env::current_dir().context("finding the working directory")?, None));
    };

    // Canonicalising is what lets `reveal` match: the tree stores the paths it
    // read from disk, and `docs/../docs/guide.md` is not one of them.
    let path = path.canonicalize().with_context(|| format!("reading {}", path.display()))?;
    if path.is_dir() {
        return Ok((path, None));
    }
    let parent = path.parent().map(Path::to_path_buf).unwrap_or_else(|| PathBuf::from("."));
    Ok((parent, Some(path)))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A terminal on both ends, which is the interactive case.
    fn tty(file: Option<&str>, browse: bool, plain: bool) -> bool {
        browse_here(file, browse, plain, true, true)
    }

    #[test]
    fn asking_for_the_browser_with_no_path_browses_here() {
        assert!(tty(None, true, false));
    }

    #[test]
    fn without_the_browser_a_bare_invocation_still_reads_stdin() {
        assert!(!tty(None, false, false), "this is the documented pipe contract");
    }

    #[test]
    fn naming_an_input_always_wins() {
        assert!(!tty(Some("-"), true, false), "an explicit dash means stdin");
        assert!(!tty(Some("README.md"), true, false), "a named file is the file");
    }

    #[test]
    fn a_real_pipe_is_still_the_input() {
        // `cat notes.md | mdlook --browse`: the pipe is an explicit input, and
        // browsing would throw it away.
        assert!(!browse_here(None, true, false, true, false));
    }

    #[test]
    fn dumping_never_browses() {
        assert!(!tty(None, true, true), "--plain has nothing to browse with");
        assert!(!browse_here(None, true, false, false, true), "piped out, so dump");
    }

    #[test]
    fn a_directory_argument_implies_the_browser_whatever_the_settings_say() {
        let settings = Settings { browse: false, ..Settings::default() };
        assert!(wants_browser(Some(Path::new(".")), &settings));
        assert!(!wants_browser(Some(Path::new("Cargo.toml")), &settings));
        assert!(!wants_browser(None, &settings));
    }

    #[test]
    fn a_file_roots_the_browser_at_its_parent_and_reveals_it() {
        let (root, reveal) = browser_root(Some(Path::new("Cargo.toml"))).unwrap();
        assert!(root.is_dir());
        assert_eq!(reveal.unwrap().file_name().unwrap(), "Cargo.toml");
        assert_eq!(root, std::env::current_dir().unwrap().canonicalize().unwrap());
    }

    #[test]
    fn a_directory_roots_the_browser_at_itself_with_nothing_revealed() {
        let (root, reveal) = browser_root(Some(Path::new("src"))).unwrap();
        assert!(root.ends_with("src"));
        assert_eq!(reveal, None);
    }

    #[test]
    fn no_target_roots_the_browser_at_the_working_directory() {
        let (root, reveal) = browser_root(None).unwrap();
        assert_eq!(root, std::env::current_dir().unwrap());
        assert_eq!(reveal, None);
    }
}
