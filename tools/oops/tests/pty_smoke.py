#!/usr/bin/env python3
"""Exercises the real executable on a Linux kernel PTY, not a display probe."""
import argparse
import fcntl
import os
import pty
import re
import select
import signal
import struct
import subprocess
import termios
import time
from pathlib import Path

ENTER = b"\x1b[?1049h"
LEAVE = b"\x18\x1b[0m\x1b[?1049l"
TICK = re.compile(rb" / tick (\d{3})")
FOREIGN = (b"\x1b[?25", b"\x1b[?7", b"\x1b]")

ENV = os.environ | {
    "TERM": "xterm-256color",
    "COLORTERM": "truecolor",
    "VTE_VERSION": "7000",
    "LC_ALL": "C.UTF-8",
    "PS1": "OOPS_TEST_SHELL> ",
    "PROMPT_COMMAND": "",
}
for name in ("NO_COLOR", "TMUX", "STY", "SSH_TTY", "BASH_ENV", "ENV"):
    ENV.pop(name, None)


class Session:
    def __init__(self, command, *, cols=183, rows=86, env=None, stderr_in_tty=False):
        self.master, self.slave = pty.openpty()
        self.geometry(cols, rows)
        self.attrs = termios.tcgetattr(self.slave)
        self.flags = fcntl.fcntl(self.slave, fcntl.F_GETFL)
        self.raw = bytearray()
        self.start = time.monotonic()
        self.proc = subprocess.Popen(
            command,
            stdin=self.slave,
            stdout=self.slave,
            stderr=self.slave if stderr_in_tty else subprocess.PIPE,
            env=ENV | (env or {}),
            preexec_fn=self.controlling_tty,
        )

    @staticmethod
    def controlling_tty():
        os.setsid()
        fcntl.ioctl(0, termios.TIOCSCTTY, 0)

    def geometry(self, cols, rows):
        fcntl.ioctl(self.slave, termios.TIOCSWINSZ, struct.pack("HHHH", rows, cols, 0, 0))

    def pump(self, timeout=0.025):
        if select.select([self.master], [], [], timeout)[0]:
            self.raw.extend(os.read(self.master, 65536))

    def until(self, needle, timeout=3):
        """Wait for a byte string or predicate; drain continuously while waiting."""
        predicate = needle if callable(needle) else lambda raw: needle in raw
        expires = time.monotonic() + timeout
        while not predicate(self.raw):
            assert time.monotonic() < expires, (needle, bytes(self.raw[-500:]))
            assert self.proc.poll() is None, (self.proc.returncode, bytes(self.raw[-500:]))
            self.pump()

    def keys_active(self):
        current = termios.tcgetattr(self.slave)
        assert not current[3] & (termios.ECHO | termios.ICANON)
        assert (current[3] & termios.ISIG) == (self.attrs[3] & termios.ISIG)
        assert fcntl.fcntl(self.slave, fcntl.F_GETFL) == self.flags

    def state_unchanged(self):
        assert termios.tcgetattr(self.slave) == self.attrs, "termios changed"
        assert fcntl.fcntl(self.slave, fcntl.F_GETFL) == self.flags, "inherited OFD flags changed"

    def end(self, code, timeout=4):
        expires = time.monotonic() + timeout
        while self.proc.poll() is None:
            assert time.monotonic() < expires, "playback did not exit"
            self.pump()
        elapsed = time.monotonic() - self.start
        while select.select([self.master], [], [], 0)[0]:
            self.pump(0)
        stderr = self.proc.stderr.read() if self.proc.stderr else b""
        assert self.proc.returncode == code, (self.proc.returncode, stderr)
        self.state_unchanged()
        return bytes(self.raw), stderr, elapsed

    def __enter__(self):
        return self

    def __exit__(self, *_):
        if self.proc.poll() is None:
            self.proc.kill()
            self.proc.wait(timeout=2)
        if self.proc.stderr:
            self.proc.stderr.close()
        os.close(self.master)
        os.close(self.slave)


def assert_scope(raw):
    assert raw.count(ENTER) == 1, "missing or duplicate alternate screen entry"
    assert raw.count(LEAVE) == 1, "missing or duplicate alternate screen exit"
    assert raw.startswith(ENTER) and raw.endswith(LEAVE), "escaped alternate screen ownership"
    assert not any(token in raw for token in FOREIGN), "unowned mode changes"
    assert raw.count(b"\x1b[?1049l") == 1
    assert b"\n" not in raw and b"\r" not in raw, "might scroll the screen"


def tests(binary, *, full, strict):
    for args, code, out in [
        (["--help"], 0, b"Usage: oops"),
        (["--version"], 0, b"oops 0.0.1"),
        (["--bad-argument"], 2, b"expected"),
        ([], 2, b"not a terminal"),
        (["--check"], 2, b"not a terminal"),
    ]:
        result = subprocess.run(
            [binary, *args],
            stdin=subprocess.DEVNULL,
            capture_output=True,
            env=ENV,
            timeout=2,
            check=False,
        )
        assert result.returncode == code
        plain = result.stdout + result.stderr
        assert out in plain
        assert b"\x1b" not in plain
        if code:
            assert not result.stdout
    print("PASS plain help, version, args and redirected stdout")

    for cols, rows in [(91, 45), (92, 44), (0, 0)]:
        with Session([binary], cols=cols, rows=rows) as s:
            out, err, _ = s.end(2)
            assert not out and b"need at least" in err and b"\x1b" not in err
    for env, diagnostic in [
        ({"TERM": "dumb"}, b"Xfce/VTE"),
        ({"COLORTERM": "unknown"}, b"true color"),
        ({"LC_ALL": "C"}, b"UTF-8"),
        ({"TMUX": "fake,10,0"}, b"nested terminal"),
        ({"NO_COLOR": "1"}, b"NO_COLOR"),
    ]:
        with Session([binary], env=env) as s:
            out, err, _ = s.end(2)
            assert not out and diagnostic in err and b"\x1b" not in err
    with Session([binary, "--check"], cols=92, rows=45) as s:
        out, err, _ = s.end(0)
        assert not err and b"92 columns by 45 rows" in out
        assert b"\x1b" not in out
    print("PASS exact minimum, one-column/row undersize, unknown size/profile and read-only preflight")

    with Session(["bash", "-c", 'exec "$1" </dev/null', "_", binary]) as s:
        out, err, _ = s.end(2)
        assert not out and b"stdin is not a terminal" in err and b"\x1b" not in err
    print("PASS redirected input does not touch the terminal")

    for cols, rows in [(92, 45), (365, 169)]:
        with Session([binary], cols=cols, rows=rows) as s:
            s.until(f"\x1b[{rows};1H".encode())
            os.write(s.master, b"q")
            raw, err, _ = s.end(0)
            assert_scope(raw)
            assert not err and "•".encode() in raw
            positions = re.findall(rb"\x1b\[(\d+);(\d+)H", raw)
            assert all(1 <= int(y) <= rows and 1 <= int(x) <= cols for y, x in positions)
    print("PASS minimum and oversized terminal output stays inside its viewport")

    for sig, code in [
        (signal.SIGINT, 130),
        (signal.SIGTERM, 143),
        (signal.SIGHUP, 129),
        (signal.SIGQUIT, 131),
        (signal.SIGTSTP, 148),
    ]:
        with Session([binary]) as s:
            s.until(b" / tick 000")
            s.keys_active()
            os.kill(s.proc.pid, sig)
            raw, err, elapsed = s.end(code, timeout=1)
            assert_scope(raw)
            assert not err and elapsed < 1
    print("PASS SIGINT/TERM/HUP/QUIT/TSTP exit, owned cleanup, inherited tty state preserved")

    with Session([binary]) as s:
        s.until(b" / tick 000")
        s.keys_active()
        os.write(s.master, b"q")
        raw, err, elapsed = s.end(0, timeout=1)
        assert_scope(raw)
        assert not err and elapsed < 1
    print("PASS q exits without Enter and restores termios")

    with Session([binary]) as s:
        s.until(b"\x1b[86;1H")
        s.geometry(200, 90)
        s.until(lambda raw: raw.count(b"\x1b[2J") >= 2)
        s.geometry(91, 90)
        raw, err, elapsed = s.end(1, timeout=1)
        assert_scope(raw)
        assert b"need at least" in err and elapsed < 1
    print("PASS wider resize recenters and below-minimum resize exits with cleanup")

    with Session([binary]) as s:
        s.until(ENTER)
        time.sleep(0.16)  # deliberately let the kernel output queue back up
        os.kill(s.proc.pid, signal.SIGINT)
        raw, err, elapsed = s.end(130, timeout=1.5)
        assert_scope(raw)
        assert not err and elapsed < 1.5
    print("PASS SIGINT while output backs up, cleanup once PTY resumes draining")

    # If the terminal never consumes its input, cleanup cannot be promised.
    # Even diagnostics on that same tty must not block shutdown indefinitely.
    with Session([binary], stderr_in_tty=True) as s:
        s.until(ENTER)
        time.sleep(0.16)
        os.kill(s.proc.pid, signal.SIGINT)
        assert s.proc.wait(timeout=1.5) == 130
        _, _, elapsed = s.end(130)
        assert elapsed < 1.5
    print("PASS stopped PTY reader cannot block signal exit and diagnostics indefinitely")

    with Session([binary]) as s:
        s.until(b"\x1b[86;1H")
        os.kill(s.proc.pid, signal.SIGSTOP)
        time.sleep(0.65)
        os.kill(s.proc.pid, signal.SIGCONT)
        s.until(lambda raw: len(TICK.findall(raw)) >= 3)
        os.kill(s.proc.pid, signal.SIGTERM)
        raw, err, _ = s.end(143)
        ticks = [int(n) for n in TICK.findall(raw)]
        assert_scope(raw)
        assert not err and max(b - a for a, b in zip(ticks, ticks[1:])) >= 4, ticks
        assert ticks == sorted(set(ticks)), ticks
    print("PASS blocked schedule skips stale ticks instead of replaying a FIFO")

    # Actual foreground shell process, real keyboard VINTR, then a fresh
    # shell command. ANSI is captured, not interpreted by a GUI terminal.
    with Session(["bash", "--noprofile", "--norc", "-i"], stderr_in_tty=True) as s:
        s.until(b"OOPS_TEST_SHELL> ")
        # Make shell echo unable to masquerade as the sentinel.
        quoted_path = "'" + binary.replace("'", "'\"'\"'") + "'"
        os.write(s.master, (
            "stty_before=$(stty -g); " + quoted_path
            + "; r=$?; if [[ $(stty -g) = $stty_before ]]; then "
            + "printf 'CHECK_%s_OK\\n' \"$r\"; fi\n"
        ).encode())
        s.until(b" / tick 000")
        os.write(s.master, b"\x03")
        s.until(b"CHECK_130_OK")
        os.write(s.master, (
            "stty_before=$(stty -g); " + quoted_path
            + "; r=$?; if [[ $(stty -g) = $stty_before ]]; then "
            + "printf 'CHECK_%s_OK\n' \"$r\"; fi\n"
        ).encode())
        s.until(lambda raw: raw.count(ENTER) == 2)
        s.until(lambda raw: raw.count(b" / tick 000") == 2)
        os.write(s.master, b"q")
        s.until(b"CHECK_0_OK")
        os.write(s.master, b"exit\n")
        raw, err, _ = s.end(0)
        assert raw.count(ENTER) == raw.count(LEAVE) == 2, bytes(raw[-500:])
        assert raw.index(LEAVE) < raw.index(b"CHECK_130_OK")
        assert raw.rindex(LEAVE) < raw.index(b"CHECK_0_OK")
        assert b"failed" not in err
    print("PASS interactive Bash returns after q and keyboard Ctrl-C with original stty")

    if full:
        with Session([binary, "--measure"]) as s:
            raw, err, elapsed = s.end(0, timeout=34)
            assert_scope(raw)
            ticks = [int(n) for n in TICK.findall(raw)]
            assert ticks == sorted(set(ticks)), "duplicates or catch-up ordering"
            assert ticks[0] <= 3 and ticks[-1] >= 716, (ticks[:5], ticks[-5:])
            assert b"grid=" in err and b"trace_p95/max_us=" in err
            print(err.decode().strip())
            assert 29.9 <= elapsed < 34, elapsed
            if strict:
                assert ticks == list(range(720)), f"only {len(ticks)} distinct ticks"
        print(f"PASS natural completion: {elapsed:.6f} s, {len(ticks)}/720 ticks, {len(raw)} bytes")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("binary", type=lambda value: str(Path(value).resolve()))
    parser.add_argument("--full", action="store_true", help="also run all 30 seconds (allows missed ticks)")
    parser.add_argument("--strict", action="store_true", help="require every tick on a quiet host")
    args = parser.parse_args()
    tests(args.binary, full=args.full or args.strict, strict=args.strict)


if __name__ == "__main__":
    main()
