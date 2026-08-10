use std::io::{IsTerminal, Read, Write};

use anyhow::{Context, Result};
use clap::Parser;
use mdlook::render::tui::DEFAULT_MAX_WIDTH;
use mdlook::ui::App;
use mdlook::{layout, parse, render, Theme, ThemeKind};

#[derive(Parser)]
#[command(
    name = "mdlook",
    version,
    about = "A terminal markdown reader with reflow you can trust and a search index you can navigate"
)]
struct Args {
    /// Markdown file to view. Reads standard input when omitted or given as `-`.
    file: Option<String>,

    /// Write rendered ANSI to standard output instead of opening the viewer.
    /// Implied when standard output is not a terminal.
    #[arg(short, long)]
    plain: bool,

    /// Wrap width. Defaults to the terminal width, capped at 100 columns.
    #[arg(short, long)]
    width: Option<usize>,

    /// Colour scheme: dark, light, or mono.
    #[arg(short = 't', long, default_value = "dark")]
    theme: String,

    /// Disable colour. Also honoured via the NO_COLOR environment variable.
    #[arg(long)]
    no_color: bool,
}

fn main() -> Result<()> {
    let args = Args::parse();

    let stdin_is_source = matches!(args.file.as_deref(), None | Some("-"));
    let (source, title) = match args.file.as_deref() {
        None | Some("-") => {
            let mut buffer = String::new();
            std::io::stdin().read_to_string(&mut buffer).context("reading standard input")?;
            (buffer, "(stdin)".to_string())
        }
        Some(path) => {
            let source =
                std::fs::read_to_string(path).with_context(|| format!("reading {path}"))?;
            (source, path.to_string())
        }
    };

    let color = !args.no_color && std::env::var_os("NO_COLOR").is_none();
    let kind = if color {
        ThemeKind::parse(&args.theme).unwrap_or(ThemeKind::Dark)
    } else {
        ThemeKind::Mono
    };
    let theme = Theme::new(kind);
    let document = parse(&source);

    // The viewer needs a terminal to draw on and a keyboard to drive it. Taking
    // the document from stdin consumes the keyboard, so that case falls back to
    // dumping rather than opening a viewer nobody could control.
    let interactive = !args.plain && std::io::stdout().is_terminal() && !stdin_is_source;

    if interactive {
        let width = args.width.unwrap_or(DEFAULT_MAX_WIDTH);
        let app = App::new(document, title, theme, width);
        return render::tui::run(app, args.width);
    }

    let width = args.width.unwrap_or_else(|| {
        if std::io::stdout().is_terminal() {
            crossterm::terminal::size()
                .map(|(w, _)| w as usize)
                .unwrap_or(80)
                .clamp(20, DEFAULT_MAX_WIDTH)
        } else {
            80
        }
    });

    let rendered = layout(&document, width, &theme);
    let mut out = std::io::stdout().lock();
    out.write_all(render::to_ansi(&rendered, color).as_bytes())?;
    Ok(())
}
