//! Putting selected text on the system clipboard.
//!
//! There is no single way to do this that works everywhere, so two are tried
//! and neither is trusted alone:
//!
//! - **OSC 52** asks the terminal to set the clipboard. It is the only route
//!   that works over SSH and through tmux, and kitty, WezTerm, iTerm2, Ghostty,
//!   Alacritty, foot and Windows Terminal honour it. macOS Terminal.app ignores
//!   it, and terminals that do support it may have it switched off.
//! - **The platform's clipboard tool** — `pbcopy`, `wl-copy`, `xclip`, `xsel` —
//!   covers the local terminals OSC 52 does not, and is simply absent on a
//!   headless box, which is where OSC 52 takes over.
//!
//! Writing the same text by both routes is harmless, so both always run.

use std::io::{self, Write};
use std::process::{Command, Stdio};

use crossterm::clipboard::CopyToClipboard;

/// Copy `text` by every route available. Returns whether a clipboard tool
/// confirmed the copy; OSC 52 is fire-and-forget and can never confirm.
pub fn copy(text: &str) -> bool {
    let _ = crossterm::execute!(io::stdout(), CopyToClipboard::to_clipboard_from(text));
    native(text)
}

/// The clipboard tools worth trying here, in order of preference.
fn tools() -> Vec<&'static [&'static str]> {
    if cfg!(target_os = "macos") {
        return vec![&["pbcopy"]];
    }
    if cfg!(windows) {
        // `clip.exe` mangles anything outside the ANSI code page; Windows
        // Terminal handles OSC 52, so leave it to that.
        return vec![];
    }
    let mut tools: Vec<&'static [&'static str]> = Vec::new();
    if std::env::var_os("WAYLAND_DISPLAY").is_some() {
        tools.push(&["wl-copy"]);
    }
    if std::env::var_os("DISPLAY").is_some() {
        tools.push(&["xclip", "-selection", "clipboard"]);
        tools.push(&["xsel", "--clipboard", "--input"]);
    }
    tools
}

fn native(text: &str) -> bool {
    tools().into_iter().any(|argv| run(argv, text).unwrap_or(false))
}

fn run(argv: &[&str], text: &str) -> io::Result<bool> {
    // Output goes nowhere rather than inheriting ours: the terminal is in raw
    // mode on the alternate screen, and `xclip` forks a child that keeps any
    // inherited pipe open for as long as it owns the selection.
    let mut child = Command::new(argv[0])
        .args(&argv[1..])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    // Reap the child even if the write fails, then report both.
    let wrote =
        child.stdin.take().is_some_and(|mut stdin| stdin.write_all(text.as_bytes()).is_ok());
    Ok(child.wait()?.success() && wrote)
}
