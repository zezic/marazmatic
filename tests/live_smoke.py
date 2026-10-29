#!/usr/bin/env python3
"""Exercise the real Unix TUI in a pseudo-terminal, with no Python dependencies.

Build first, then run: python3 tests/live_smoke.py [path/to/marazmatic]
"""

import errno
import fcntl
import os
from pathlib import Path
import pty
import re
import select
import signal
import struct
import sys
import tempfile
import termios
import time


EXECUTABLE = str(Path(sys.argv[1] if len(sys.argv) > 1 else "target/debug/marazmatic").resolve())


class Session:
    def __init__(self, path, *args):
        self.pid, self.fd = pty.fork()
        if self.pid == 0:
            os.environ["TERM"] = "xterm-256color"
            os.execv(EXECUTABLE, [EXECUTABLE, "live", str(path), "--plain", *args])
        self.resize(120, 34)
        self.pending = b""
        self.output = b""
        self.finished = False

    def resize(self, columns, rows):
        fcntl.ioctl(self.fd, termios.TIOCSWINSZ, struct.pack("HHHH", rows, columns, 0, 0))

    def read(self, seconds=0.15):
        end = time.monotonic() + seconds
        chunk = b""
        while time.monotonic() < end:
            if not select.select([self.fd], [], [], max(0, end - time.monotonic()))[0]:
                break
            try:
                part = os.read(self.fd, 65536)
                if not part:
                    break
                chunk += part
            except OSError as error:
                if error.errno == errno.EIO:
                    break
                raise
        self.pending += chunk
        self.output += chunk
        return chunk

    def expect(self, text, timeout=5):
        needle = text.encode()
        end = time.monotonic() + timeout
        while needle not in self.pending and time.monotonic() < end:
            self.read(0.1)
        assert needle in self.pending, f"Missing {text!r}: {self.pending[-10000:].decode(errors='replace')}"
        result = self.pending
        self.pending = b""
        return result

    def send(self, keys):
        self.read(0.05)
        self.pending = b""
        os.write(self.fd, keys.encode())

    def finish(self, key="q"):
        self.send(key)
        end = time.monotonic() + 5
        while time.monotonic() < end:
            self.read(0.05)
            pid, status = os.waitpid(self.pid, os.WNOHANG)
            if pid:
                self.finished = True
                assert os.waitstatus_to_exitcode(status) == 0, status
                after = termios.tcgetattr(self.fd)
                mask = termios.ECHO | termios.ICANON
                assert after[3] & mask == mask, "terminal raw mode was not restored"
                assert b"\x1b[?1049l" in self.output, "alternate screen was not restored"
                assert b"\x1b[?25h" in self.output, "cursor was not restored"
                os.close(self.fd)
                return
        raise AssertionError("TUI did not exit promptly")

    def cleanup(self):
        if not self.finished:
            os.kill(self.pid, signal.SIGKILL)
            os.waitpid(self.pid, 0)
            os.close(self.fd)


def main():
    with tempfile.TemporaryDirectory(prefix="marazmatic-live-") as directory:
        path = Path(directory) / "diagram.mmd"
        path.write_text("flowchart LR\nA[Alpha] -->|request| B[Bravo]\n")
        session = Session(path, "--gap", "4")
        try:
            first = session.expect("revision 1")
            assert b"Alpha" in first and b"Bravo" in first
            assert not re.search(rb"\x1b\[[0-9;]*m", first), "--plain emitted SGR styles"

            assert b"Center      on" in first, "centering was not enabled by default"
            for report in ["35;15;10M", "0;15;10M", "32;16;10M", "0;16;10m", "64;15;10M", "65;15;10M", "66;15;10M", "67;15;10M"]:
                session.send(f"\x1b[<{report}")
                assert session.read(0.2) == b"", f"ignored mouse report {report!r} redrew an unchanged screen"

            for key, text in [("m", "Center      off"), ("m", "Center      on"), ("b", "square"), ("t", "mono"), ("t", "ansi"), ("a", "ascii"), ("a", "rounded"), ("d", "TB"), ("+", "Horiz gap   5"), ("x", "Horiz gap   4"), ("X", "Horiz gap   5"), ("y", "Vert gap    4"), ("Y", "Vert gap    5"), ("]", "Label width 28"), ("p", "Padding     2"), ("v", "Labels      off")]:
                session.send(key)
                session.expect(text)

            for key, text in [("p", "Padding     3"), ("p", "Padding     0"), ("xxxxx", "Horiz gap   0"), ("yyyyy", "Vert gap    0")]:
                session.send(key)
                frame = session.expect(text)
                assert b"ERROR /" not in frame, "zero padding or spacing failed to render"

            session.send("c")
            colored = session.expect("Colors")
            assert re.search(rb"\x1b\[(?:[0-9]+;)*(?:3[0-7]|39|9[0-7])(?:;[0-9]+)*m", colored), "colors did not use the terminal ANSI palette"
            assert b"38;5;" not in colored and b"38;2;" not in colored, "colors used a custom palette"
            session.send("c")
            session.expect("Colors")
            session.send("s")
            assert re.search(rb"\x1b\[[0-9;]*1m", session.expect("Accents"))
            session.send("s")
            session.expect("Accents")

            session.send("?")
            session.expect("live controls")
            session.send("\x1b[<35;15;10M")
            assert session.read(0.2) == b"", "mouse movement redrew help"
            session.send("\x1b")
            session.expect("CONTROLS")

            path.write_text("flowchart LR\nA -->\n")
            failed = session.expect("ERROR / line 2")
            assert b"Alpha" in failed, "invalid edit did not preserve the last good render"

            replacement = path.with_suffix(".tmp")
            replacement.write_text("flowchart LR\nA[Changed] --> B[Reloaded]\n")
            replacement.replace(path)
            changed = session.expect("revision 2")
            assert b"Changed" in changed and b"Reloaded" in changed

            path.unlink()
            session.expect("No such file")
            path.write_text("flowchart LR\nA[Changed] --> B[Reloaded]\n")
            session.expect("revision 3")

            session.send("w")
            session.expect("watching paused")
            path.write_text("flowchart LR\nA[Manual] --> B[Reload]\n")
            assert b"Manual" not in session.read(0.6), "paused watcher still reloaded the file"
            session.send("r")
            session.expect("revision 4")
            session.send("w")
            session.expect("Watching file")

            session.send("0")
            session.expect("Restored startup settings")
            for suffix in ["", "-1"]:
                session.send("e")
                session.expect("Exported")
                export = Path(directory) / f"diagram.marazmatic{suffix}.txt"
                assert "Manual" in export.read_text()
                assert "\x1b" not in export.read_text()

            session.resize(40, 10)
            session.expect("Resize to at least")
            session.resize(80, 24)
            session.expect("MARAZMATIC")
            session.finish()
        finally:
            session.cleanup()

        path.write_text("flowchart LR\nA -->\n")
        session = Session(path, "--no-center", "--padding", "0", "--gap", "0", "--horizontal-gap", "7", "--vertical-gap", "3")
        try:
            initial = session.expect("ERROR / line 2")
            assert b"Center      off" in initial and b"Horiz gap   7" in initial and b"Vert gap    3" in initial
            path.write_text("flowchart LR\nA[Recovered] --> B[Initially invalid]\n")
            session.expect("revision 1")
            session.finish("\x03")
        finally:
            session.cleanup()
    print("Live smoke checks passed: centering, independent spacing, idle mouse, settings, styling, help, reload/recovery, watch pause, export, resize, quit/Ctrl+C, terminal restoration.")


if __name__ == "__main__":
    main()
