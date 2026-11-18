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
RESYNC = b"\x18\x1b[0m\x1b[48;2;0;0;0m\x1b[2J"
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

    def partial_next_frame(self, bottom):
        """Read a prefix of a large diff, but not its closing cursor move."""
        self.until(bottom)
        start = self.raw.index(bottom) + len(bottom)
        self.until(lambda raw: len(raw) >= start + 1024)
        assert bottom not in self.raw[start:], "already received the next full frame"

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


def assert_partial_wire_is_recoverable(packet):
    """Check every cut in the actual packet's promised VTE wire subset.

    This models only ESC/CSI, CAN and the UTF-8 bullet. It does not replace
    a real emulator check: VTE's CAN action and decoder recovery are the
    protocol assumptions verified independently on the pilot profile.
    """
    def step(state, b):
        esc, utf8 = state
        if b == 0x18:  # CAN aborts a partial escape/control sequence.
            return (b"", 0), None
        if utf8:
            assert 0x80 <= b < 0xC0, state
            return (esc, utf8 - 1), None
        if esc == b"\x1b":
            assert b == ord("["), (esc, b)
            return (esc + bytes([b]), 0), None
        if esc:
            assert esc.startswith(b"\x1b[")
            extended = esc + bytes([b])
            if 0x40 <= b <= 0x7E:
                return (b"", 0), extended
            assert 0x20 <= b <= 0x3F
            return (extended, 0), None
        if b == 0x1B:
            return (b"\x1b", 0), None
        if b == 0xE2:  # All non-ASCII in the frame is the UTF-8 bullet.
            return (b"", 2), None
        assert 0x20 <= b <= 0x7E
        return (b"", 0), None

    def commands(state, data):
        result = []
        for b in data:
            state, cmd = step(state, b)
            if cmd is not None:
                result.append(cmd)
        assert state == (b"", 0)
        return result

    state = (b"", 0)
    cuts = set()
    # Test the empty prefix, each byte boundary, and the complete packet.
    for b in [None, *packet]:
        if b is not None:
            state, _ = step(state, b)
        # The CAN action is independent of accumulated CSI parameters.
        kind = (len(state[0]), state[1])
        if kind not in cuts:
            assert commands(state, RESYNC) == [b"\x1b[0m", b"\x1b[48;2;0;0;0m", b"\x1b[2J"]
            assert commands(state, LEAVE) == [b"\x1b[0m", b"\x1b[?1049l"]
            cuts.add(kind)
    assert state == (b"", 0)
    assert {(1, 0), (2, 0), (0, 1), (0, 2)} <= cuts, cuts


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

    # Fill the kernel tty queue after a valid frame. A temporary reader
    # stall exceeding the packet budget must not end the 30-second run.
    # Repeat during full repaint as well; that interrupted repaint must
    # not become the diff baseline.
    with Session([binary, "--measure"], cols=290, rows=97) as s:
        bottom = b"\x1b[97;1H"
        s.partial_next_frame(bottom)
        time.sleep(0.48)
        assert s.proc.poll() is None, "a temporary stall ended playback"
        s.until(lambda raw: RESYNC in raw and len(raw.rsplit(RESYNC, 1)[1]) >= 1024)
        assert bottom not in s.raw.rsplit(RESYNC, 1)[1], "already received the full repaint"
        time.sleep(0.48)
        assert s.proc.poll() is None, "a second stall ended playback"
        s.until(lambda raw: raw.count(RESYNC) >= 2 and bottom in raw.rsplit(RESYNC, 1)[1])
        os.write(s.master, b"q")
        raw, err, _ = s.end(0)
        assert_scope(raw)
        ticks = [int(n) for n in TICK.findall(raw)]
        assert ticks == sorted(set(ticks)) and ticks[-1] >= 16, ticks
        assert max(b - a for a, b in zip(ticks, ticks[1:])) >= 7, ticks
        report = re.search(rb"dropped_write=(\d+) partial_write=(\d+)", err)
        assert report and int(report[2]) >= 2 and int(report[1]) >= int(report[2]), err
        assert b"timed out" not in err
        first = raw[:raw.index(bottom) + len(bottom)]
        recovered = raw.rsplit(RESYNC, 1)[1]
        recovered = recovered[:recovered.index(bottom) + len(bottom)]
        assert_partial_wire_is_recoverable(first)
        assert_partial_wire_is_recoverable(RESYNC + recovered)
    print("PASS two temporary output stalls recover via fresh frames, never a stale suffix")

    # Exercise line-discipline flow control as well as a full kernel
    # queue. Depending on scheduling, TCOOFF may start between frame
    # packets or when the next packet has already sent a prefix.
    with Session([binary, "--measure"], cols=165, rows=55) as s:
        s.until(b"\x1b[55;1H")
        termios.tcflow(s.slave, termios.TCOOFF)
        try:
            time.sleep(0.42)
            assert s.proc.poll() is None
        finally:
            termios.tcflow(s.slave, termios.TCOON)
        s.until(lambda raw: any(int(n) >= 8 for n in TICK.findall(raw)))
        os.write(s.master, b"q")
        raw, err, _ = s.end(0)
        assert_scope(raw)
        report = re.search(rb"dropped_write=(\d+) partial_write=(\d+)", err)
        assert report and int(report[1]) >= 1 and b"timed out" not in err, err
        assert (RESYNC in raw) == (int(report[2]) != 0)
    print("PASS terminal flow control resumes without repainting an untouched frame")

    # Resume a recovery-waiting player with a key, not writable output.
    # CAN must make even a partially submitted escape harmless before exit.
    with Session([binary], cols=290, rows=97) as s:
        s.partial_next_frame(b"\x1b[97;1H")
        time.sleep(0.48)
        assert s.proc.poll() is None
        os.write(s.master, b"q")
        raw, err, _ = s.end(0, timeout=1)
        assert_scope(raw)
        assert RESYNC not in raw and not err, err
    print("PASS q during the recovery wait restores without forcing a new frame")

    # Change the viewport while there is an undrained partial packet. The
    # old coordinates must not be resumed, and resize within bounds is not fatal.
    with Session([binary], cols=290, rows=97) as s:
        s.partial_next_frame(b"\x1b[97;1H")
        s.geometry(213, 85)
        s.until(lambda raw: RESYNC in raw and b"\x1b[85;1H" in raw.rsplit(RESYNC, 1)[1])
        os.write(s.master, b"q")
        raw, err, _ = s.end(0)
        assert_scope(raw)
        redrawn = raw.split(RESYNC, 1)[1]
        positions = re.findall(rb"\x1b\[(\d+);(\d+)H", redrawn)
        assert positions and all(1 <= int(y) <= 85 and 1 <= int(x) <= 213 for y, x in positions)
        assert not err
    print("PASS in-flight usable resize clears the old partial frame and redraws within the new size")

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
        if not strict:
            with Session([binary, "--measure"], cols=290, rows=97) as s:
                s.until(lambda raw: any(int(n) >= 710 for n in TICK.findall(raw[-9000:])),
                        timeout=32)
                # Keep the tty congested across the absolute run deadline,
                # then drain promptly enough to receive the bounded cleanup.
                time.sleep(max(0.0, 30.08 - (time.monotonic() - s.start)))
                raw, err, elapsed = s.end(0, timeout=1)
                assert_scope(raw)
                assert 29.9 <= elapsed < 31.0, (elapsed, err)
                assert b"timed out" not in err and b"restoration was incomplete" not in err
            print("PASS absolute run deadline restores after a partially stalled final turn")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("binary", type=lambda value: str(Path(value).resolve()))
    parser.add_argument("--full", action="store_true", help="also run all 30 seconds (allows missed ticks)")
    parser.add_argument("--strict", action="store_true", help="require every tick on a quiet host")
    args = parser.parse_args()
    tests(args.binary, full=args.full or args.strict, strict=args.strict)


if __name__ == "__main__":
    main()
