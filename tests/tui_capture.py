#!/usr/bin/env python3
"""Drive the viewer in a real pty and print what the screen would show.

The Rust tests in `tests/viewer.rs` cover the state machine; this covers the
drawing layer, which needs an actual terminal. Alt-screen output is invisible to
`script`/`tee` because the app restores the screen on exit, so this captures the
byte stream mid-run and replays it into a grid.

Usage:
    tests/tui_capture.py -- ./target/debug/mdlook README.md
    tests/tui_capture.py / u s e r -- ./target/debug/mdlook README.md
    tests/tui_capture.py --rows 30 --cols 80 t -- ./target/debug/mdlook README.md

Each bare argument before `--` is one keystroke, sent in order with a pause
between them so the app has time to redraw.
"""

import argparse
import fcntl
import os
import pty
import re
import select
import struct
import sys
import termios
import time
import unicodedata


def _width(char):
    """Columns a character occupies, so wide glyphs do not skew the grid."""
    if unicodedata.combining(char):
        return 0
    return 2 if unicodedata.east_asian_width(char) in ("W", "F") else 1


def capture(argv, keys=(), rows=45, cols=100, settle=1.2, per_key=0.5):
    """Run argv in a pty, send keys, and return the raw output stream."""
    pid, fd = pty.fork()
    if pid == 0:
        os.environ["TERM"] = "xterm-256color"
        os.environ["COLORTERM"] = "truecolor"
        os.execvp(argv[0], argv)

    fcntl.ioctl(fd, termios.TIOCSWINSZ, struct.pack("HHHH", rows, cols, 0, 0))
    buffer = b""

    def drain(seconds):
        nonlocal buffer
        deadline = time.time() + seconds
        while time.time() < deadline:
            ready, _, _ = select.select([fd], [], [], 0.05)
            if not ready:
                continue
            try:
                chunk = os.read(fd, 65536)
            except OSError:
                return
            if not chunk:
                return
            buffer += chunk
            # Some TUIs ask the terminal for its background colour and block
            # until they get an answer; without a reply they never draw.
            if b"\x1b]11;?" in chunk:
                os.write(fd, b"\x1b]11;rgb:0000/0000/0000\x1b\\")

    drain(settle)
    for key in keys:
        os.write(fd, key.encode())
        drain(per_key)

    os.write(fd, b"q")
    time.sleep(0.2)
    try:
        os.close(fd)
    except OSError:
        pass
    os.waitpid(pid, os.WNOHANG)
    return buffer


CSI = re.compile(r"\x1b\[([0-9;?]*)([a-zA-Z])")
OTHER_ESC = re.compile(r"\x1b[()][B0]|\x1b[=><]|\x1b\][^\x07\x1b]*(\x07|\x1b\\)")


def render(data, rows, cols):
    """Replay the stream into a character grid.

    Only the escapes ratatui actually emits are interpreted: absolute cursor
    positioning, erase-display and erase-line. Colour is dropped on purpose so
    the output diffs cleanly.
    """
    grid = [[" "] * cols for _ in range(rows)]
    cy = cx = 0
    text = data.decode("utf8", "replace")
    i = 0

    while i < len(text):
        char = text[i]
        if char == "\x1b":
            match = CSI.match(text, i)
            if match:
                params, command = match.group(1), match.group(2)
                numbers = [int(p) for p in params.split(";") if p.isdigit()]
                if command == "H":
                    cy = (numbers[0] - 1) if numbers else 0
                    cx = (numbers[1] - 1) if len(numbers) > 1 else 0
                elif command == "J":
                    grid = [[" "] * cols for _ in range(rows)]
                elif command == "K":
                    for x in range(cx, cols):
                        grid[cy][x] = " "
                i = match.end()
                continue
            match = OTHER_ESC.match(text, i)
            i = match.end() if match else i + 1
            continue

        if char == "\n":
            cy, cx = cy + 1, 0
        elif char == "\r":
            cx = 0
        elif char == "\t":
            cx = (cx // 8 + 1) * 8
        elif char == "\ufe0f":
            # Emoji presentation selector: promotes the character already placed
            # from one column to two. Keep it attached to its base rather than
            # dropping it, and claim the extra cell as a continuation.
            if 0 <= cy < rows and 0 < cx <= cols:
                if grid[cy][cx - 1]:
                    grid[cy][cx - 1] += char
                if cx < cols:
                    grid[cy][cx] = ""
            cx += 1
        elif ord(char) >= 32:
            width = _width(char)
            if 0 <= cy < rows and 0 <= cx < cols:
                grid[cy][cx] = char
                # A double-width glyph owns the following cell too. Marking it as
                # an empty continuation rather than leaving a space keeps the
                # joined row the same display width the terminal actually shows.
                if width == 2 and cx + 1 < cols:
                    grid[cy][cx + 1] = ""
            cx += width
        i += 1

    return "\n".join("".join(row).rstrip() for row in grid)


def main():
    argv = sys.argv[1:]
    if "--" not in argv:
        print(__doc__, file=sys.stderr)
        return 2
    split = argv.index("--")
    parser = argparse.ArgumentParser(add_help=False)
    parser.add_argument("--rows", type=int, default=45)
    parser.add_argument("--cols", type=int, default=100)
    parser.add_argument("keys", nargs="*")
    options = parser.parse_args(argv[:split])
    command = argv[split + 1:]
    if not command:
        print("no command given after --", file=sys.stderr)
        return 2

    stream = capture(command, options.keys, options.rows, options.cols)
    print(render(stream, options.rows, options.cols))
    return 0


if __name__ == "__main__":
    sys.exit(main())
