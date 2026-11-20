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
import tempfile
import termios
import time
from pathlib import Path

ENTER = b"\x1b[?1049h"
LEAVE = b"\x18\x1b[0m\x1b[?1049l"
RESYNC = b"\x18\x1b[0m\x1b[48;2;0;0;0m\x1b[2J"
CURSOR_QUERY = b"\x1b[6n"
CURSOR_HIDE = b"\x1b[?25s\x1b[?25l"
CURSOR_RESTORE = b"\x18\x1b[?25r"
BRAILLE = re.compile(rb"\xe2(?:\xa0[\x81-\xbf]|[\xa1-\xa3][\x80-\xbf])")
BULLET = "•".encode()
FOREIGN = (b"\x1b[?7", b"\x1b]")

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
    def __init__(self, command, *, cols=183, rows=86, pixels=(0, 0), env=None, stderr_in_tty=False):
        self.master, self.slave = pty.openpty()
        self.geometry(cols, rows, pixels)
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

    def geometry(self, cols, rows, pixels=(0, 0)):
        self.rows = rows
        fcntl.ioctl(self.slave, termios.TIOCSWINSZ, struct.pack("HHHH", rows, cols, *pixels))

    def cursor_reply(self, number, row, col=1):
        self.until(lambda raw: raw.count(CURSOR_QUERY) >= number)
        data = f"\x1b[{row};{col}R".encode()
        # Split the report so assembly across nonblocking reads is exercised.
        os.write(self.master, data[:3])
        time.sleep(0.006)
        os.write(self.master, data[3:])

    @property
    def bottom(self):
        return f"\x1b[{self.rows};1H".encode()

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

    def partial_next_frame(self, bottom, *, force_repaint=False):
        """Wait for a substantial open packet, skipping completed holds."""
        self.until(bottom)
        after = self.raw.rindex(bottom) + len(bottom)
        if force_repaint:
            # A sparse archived diff may fit the queue. A pixel-only
            # viewport change keeps the same aspect and drawing dimensions
            # but provokes a whole-picture packet, giving a real stall.
            self.geometry(290, 97, (2900, 1940))

        def pending_prefix(raw):
            # Do not count short complete status packets on held poses.
            start = raw.rindex(bottom) + len(bottom)
            if force_repaint:
                clear = raw.rfind(b"\x1b[2J", after)
                if clear < start:
                    return False
                start = clear + len(b"\x1b[2J")
            return len(raw) >= start + 1024

        self.until(pending_prefix)

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


def assert_cursor_scope(raw):
    assert raw.count(CURSOR_HIDE) == raw.count(CURSOR_RESTORE) == 1
    assert raw.count(b"\x1b[?25") == 3, "unexpected cursor toggle"
    assert raw.index(CURSOR_HIDE) < raw.index(CURSOR_RESTORE)
    art = raw.find(b"\x1b[38;2;")
    if art != -1:
        assert raw.index(CURSOR_HIDE) < art < raw.index(CURSOR_RESTORE)


def assert_scope(raw):
    assert raw.count(ENTER) == 1, "missing or duplicate alternate screen entry"
    assert raw.count(LEAVE) == 1, "missing or duplicate alternate screen exit"
    assert raw.startswith(ENTER) and raw.endswith(LEAVE + CURSOR_RESTORE), "escaped alternate screen ownership"
    assert_cursor_scope(raw)
    assert not any(token in raw for token in FOREIGN), "unowned mode changes"
    assert raw.count(b"\x1b[?1049l") == 1
    assert not any(text in raw for text in (b"oops /", b" / tick ", b"q or Ctrl-C"))
    assert b"\n" not in raw and b"\r" not in raw, "might scroll the screen"


def assert_partial_wire_is_recoverable(packet):
    """Check every cut in the actual packet's promised VTE wire subset.

    This models only ESC/CSI, CAN and the three-byte UTF-8 artwork glyphs. It does not replace
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
        if b == 0xE2:  # Braille and the legacy bullet share this lead byte.
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
            assert commands(state, LEAVE + CURSOR_RESTORE) == [
                b"\x1b[0m", b"\x1b[?1049l", b"\x1b[?25r"
            ]
            cuts.add(kind)
    assert state == (b"", 0)
    assert {(1, 0), (2, 0), (0, 1), (0, 2)} <= cuts, cuts


def assert_resize_wire(raw, first, before, after, cols, rows):
    # A completed old packet ends in bottom-row CUP. Anything accepted
    # after the last such marker needs CAN before the first layout clear.
    clear = b"\x1b[2J"
    clear_end = raw.index(clear, first) + len(clear)
    assert clear_end < raw.index(after)
    resynced = raw[:clear_end].endswith(RESYNC)
    control = RESYNC if resynced else clear
    last_old_bytes = raw[first:clear_end - len(control)]
    truncated = bool(last_old_bytes.rsplit(before, 1)[-1])
    assert resynced == truncated, (resynced, last_old_bytes[-80:])
    positions = re.findall(rb"\x1b\[(\d+);(\d+)H", raw[clear_end:])
    assert positions and all(1 <= int(y) <= rows and 1 <= int(x) <= cols for y, x in positions)
    return resynced


def test_resize_wire_expectations():
    before, after = b"\x1b[97;1H", b"\x1b[85;1H"
    old_complete = b"old diff" + before
    partial_ansi = b"old diff\x1b[38;2;"
    clear = b"\x1b[2J"
    redrawn = b"\x1b[80;17Hnew canvas" + after
    for old, control, result in (
        (old_complete, clear, False),
        (partial_ansi, RESYNC, True),
        # Invalid streams: uncancelled partial ANSI, unnecessary cancel
        # after a finished packet, or an old cursor position after clearing.
        (partial_ansi, clear, None),
        (old_complete, RESYNC, None),
        (old_complete, clear + b"\x1b[96;50H", None),
    ):
        raw = before + old + control + redrawn
        try:
            actual = assert_resize_wire(raw, len(before), before, after, 213, 85)
        except AssertionError:
            assert result is None, raw
        else:
            assert actual is result, raw


def model_grid(frame, cols, rows, aspect=0.5, *, inline=False):
    width, height = frame
    ratio = width / (height * aspect)
    usable_cols = min(cols - 1, 220)
    usable_rows = min(rows * 3 // 4 if inline else rows - 3, 80)
    return min(usable_cols, int(usable_rows * ratio)), min(usable_rows, int(usable_cols / ratio))


def test_config(binary, frame):
    def check(args, expected, pixels=(0, 0), env=None):
        with Session([binary, "--check", "--alternate-buffer", *args], cols=165, rows=55,
                     pixels=pixels, env=env) as s:
            out, err, _ = s.end(0)
            assert not err and expected in out, (out, err)
            assert b"\x1b" not in out

    # The example itself should work, with no ambient config loaded.
    example = Path(__file__).resolve().parent.parent / "oops.example.conf"
    def named_grid(aspect):
        cols, rows = model_grid(frame, 165, 55, aspect)
        return f"model grid {cols} by {rows}".encode()

    check([], named_grid(0.5) + b", cell aspect 0.5000 (assumed), zoom 100%")
    check(["--config", example], b"duration=30s direction=reverse offset=0%,0% brightness=100% renderer=braille")
    check([], named_grid(7 / 15) + b", cell aspect 0.4667 (tty pixels), zoom 100%", (1155, 825))
    check([], b"cell aspect 0.5000 (assumed)", (1155, 0))

    with tempfile.TemporaryDirectory() as directory:
        config = Path(directory) / "display settings.conf"
        config.write_text(
            "zoom=125\ncell_aspect=0.60\nduration=50\ndirection=forward\n"
            "offset_x=-10\noffset_y=15\nbrightness=125\nrenderer=braille\n",
            encoding="utf-8",
        )
        default_location = Path(directory) / "oops"
        default_location.mkdir()
        (default_location / "config").write_text("zoom=150\n", encoding="utf-8")
        check([], b"zoom 100%", env={"XDG_CONFIG_HOME": directory})
        check(["--config", config],
              named_grid(0.60) + b", cell aspect 0.6000 (configured), zoom 125%")
        check(
            ["--zoom", "85", "--config", config],
            b"cell aspect 0.6000 (configured), zoom 85%",
        )
        check(
            ["--cell-aspect", "auto", "--config", config],
            b"cell aspect 0.4667 (tty pixels), zoom 125%",
            (1155, 825),
        )
        check(
            ["--config", config, "--cell-aspect", "0.75", "--zoom", "200"],
            b"cell aspect 0.7500 (configured), zoom 200%",
            (1155, 825),
        )
        check(
            ["--config", config],
            b"duration=50s direction=forward offset=-10%,15% brightness=125% renderer=braille",
        )
        check(
            ["--duration", "60", "--direction", "reverse", "--offset-x", "30",
             "--offset-y", "-50", "--brightness", "0", "--dots", "--config", config],
            b"duration=60s direction=reverse offset=30%,-50% brightness=0% renderer=dots",
        )
        config.write_bytes(b"renderer=dots\n")
        check(["--config", config], b"renderer=dots")
        for source, expected in [
            (None, b"No such file"),
            (b"zoom=125\nunknown=1\n", b'line 2: unknown key "unknown"'),
            (b"zoom=125\nzoom=150\n", b"line 2: zoom: specified more than once"),
            (b"zoom=201\n", b"line 1: zoom: expected"),
            (b"cell_aspect=NaN\n", b"line 1: cell_aspect: expected"),
            (b"duration=61\n", b"line 1: duration: expected"),
            (b"direction=left\n", b"line 1: direction: expected"),
            (b"offset_x=-51\n", b"line 1: offset_x: expected"),
            (b"brightness=201\n", b"line 1: brightness: expected"),
            (b"renderer=blocks\n", b"line 1: renderer: expected braille or dots"),
            (b"renderer=braille\nrenderer=dots\n", b"line 2: renderer: specified more than once"),
            (b"\xff", b"file must be UTF-8"),
            (b"#" * 8193, b"file exceeds 8192 bytes"),
        ]:
            if source is not None:
                config.write_bytes(source)
            else:
                config.unlink()
            # A CLI override does not conceal a malformed config file.
            with Session([binary, "--terminal", "--config", config, "--zoom", "100", "--dots"]) as s:
                out, err, _ = s.end(2)
                assert not out and b"display settings.conf" in err
                assert expected in err and b"\x1b" not in err, err

        config.write_bytes(b"#" * 8192)
        symlink = Path(directory) / "config-link"
        symlink.symlink_to(config)
        check(["--config", symlink], b"zoom 100%")

        fifo = Path(directory) / "not-a-config"
        os.mkfifo(fifo)
        symlink.unlink()
        symlink.symlink_to(fifo)
        for path in (fifo, symlink, directory):
            with Session([binary, "--terminal", "--config", path]) as s:
                out, err, _ = s.end(2, timeout=1)
                assert not out and b"expected a regular file" in err, err
    print("PASS config example, per-setting CLI precedence, pitch source, and startup failures")


def test_zoom_resize(binary, framing):
    for zoom in (50, 200):
        with Session([binary, "--alternate-buffer", "--measure", "--zoom", str(zoom)], cols=165, rows=55,
                     pixels=(1155, 825)) as s:
            bottom = b"\x1b[55;1H"
            clear = b"\x1b[2J"
            s.until(bottom)

            # A real TIOCSWINSZ can change the pixel pitch at the same cell
            # count. The auto projection must repaint, even without a new CUP
            # at the bottom of the screen to use as a viewport marker.
            s.geometry(165, 55, (1485, 825))
            s.until(lambda raw: raw.count(clear) >= 2 and bottom in raw.rsplit(clear, 1)[1])

            # Then resize in both directions while the same zoom stays in effect.
            for index, (cols, rows, pixels) in enumerate((
                (220, 93, (1980, 1395)),
                (112, 53, (1008, 795)),
            ), 3):
                s.geometry(cols, rows, pixels)
                marker = f"\x1b[{rows};1H".encode()
                s.until(lambda raw: raw.count(clear) >= index and marker in raw.rsplit(clear, 1)[1])
                frame = bytes(s.raw.rsplit(clear, 1)[1])
                positions = re.findall(rb"\x1b\[(\d+);(\d+)H", frame)
                assert positions and all(1 <= int(y) <= rows and 1 <= int(x) <= cols
                                         for y, x in positions)
                assert BRAILLE.search(frame)
            os.write(s.master, b"q")
            raw, err, _ = s.end(0)
            assert_scope(raw)
            cols, rows = model_grid(framing, 112, 53, 0.60)
            assert f"grid={cols}x{rows} aspect=0.6000 pitch=tty zoom={zoom}%".encode() in err, err
    print("PASS zoom endpoints survive pixel-only, wider, and narrower TTY resizes")


def opening_cells(binary, options):
    """Decode occupied cells in one complete wire frame, not its appearance."""
    marker = b"\x1b[65;1H"
    with Session([binary, "--terminal", "--alternate-buffer", "--zoom", "50", "--cell-aspect", "0.60", *options],
                 cols=121, rows=65) as s:
        s.until(marker)
        os.write(s.master, b"q")
        raw, err, _ = s.end(0)
        assert_scope(raw)
        frame = raw[:raw.index(marker) + len(marker)]
        assert not err, (frame[-80:], err)
    occupied = {}
    dots = "--dots" in options
    row = col = gray = None
    for token in re.split(rb"(\x1b\[[0-9;?]*[A-Za-z])", frame):
        if token.startswith(b"\x1b["):
            if token.endswith(b"H"):
                row, col = map(int, token[2:-1].split(b";"))
            elif token.startswith(b"\x1b[38;2;"):
                red, green, blue = map(int, token[7:-1].split(b";"))
                assert red == green == blue
                gray = red
        else:
            for char in token.decode("utf-8"):
                is_mark = char == "•" if dots else "\u2801" <= char <= "\u28ff"
                if is_mark:
                    occupied[row, col] = (char, gray)
                col += 1
    assert occupied
    return occupied


def test_playback_controls(binary):
    centered = opening_cells(binary, [])
    forward = opening_cells(binary, ["--direction", "forward", "--duration", "12"])
    assert forward == centered, "direction or duration changed the opening pose"

    dark = opening_cells(binary, ["--brightness", "0"])
    assert dark == {pos: (mark, 0) for pos, (mark, _) in centered.items()}, \
        "zero gain changed Braille dot occupancy"

    legacy = opening_cells(binary, ["--dots"])
    shifted = opening_cells(binary, ["--dots", "--brightness", "200", "--offset-x", "10"])
    # Both scenes fit all 120 columns in this viewport. A 10% move adds
    # exactly 12 columns; 50% zoom avoids cropping. Use the uncapped
    # legacy sample grid to exercise exact cell alignment in both scenes.
    assert shifted == {(row, col + 12): (mark, min(255, gray * 2))
                       for (row, col), (mark, gray) in legacy.items()}
    print("PASS playback direction/duration opening parity, gray gain, and artwork-only offset")


def test_renderer(binary):
    for alternate in (False, True):
        surface = ["--alternate-buffer"] if alternate else []
        for dots in (False, True):
            with Session([binary, "--terminal", *surface, *(["--dots"] if dots else [])],
                         cols=116, rows=41) as s:
                if not alternate:
                    s.cursor_reply(1, 41)
                    s.cursor_reply(2, 41)
                s.until(lambda raw: raw.count(s.bottom) >= (1 if alternate else 2))
                # Exercise both exit keys in both surfaces, including legacy.
                os.write(s.master, b"\x03" if dots else b"q")
                raw, err, _ = s.end(130 if dots else 0)
                assert not err
                assert_cursor_scope(raw)
                assert bool(BRAILLE.search(raw)) == (not dots), "wrong Braille renderer"
                assert (BULLET in raw) == dots, "wrong bullet renderer"
                if alternate:
                    assert_scope(raw)
                else:
                    assert raw.endswith(b"\x1b[11;1H" + CURSOR_RESTORE)
    print("PASS default Braille and --dots in inline and alternate screen; q and Ctrl-C restore")


def timing_output(err):
    def field(name):
        return re.search(rb"\b" + name + rb"=([^ \n]+)", err)[1]
    first, last = (tuple(map(int, field(name).split(b":"))) for name in (b"first", b"last"))
    count = int(field(b"written"))
    assert field(b"order_errors") == b"0", err
    return first, last, count, field


def test_default_loop(binary, *, stall=False):
    command = [binary, "--measure", "--duration", "12"]
    with Session(command, cols=121, rows=65) as s:
        s.cursor_reply(1, 65)
        s.cursor_reply(2, 65)
        s.until(lambda raw: raw.count(s.bottom) >= 2)
        s.until(lambda _: time.monotonic() - s.start >= 11.80, timeout=14)
        if stall:
            # Hold output unavailable over the clock join.
            termios.tcflow(s.slave, termios.TCOOFF)
            try:
                time.sleep(0.42)
            finally:
                termios.tcflow(s.slave, termios.TCOON)
        s.until(lambda _: time.monotonic() - s.start >= 12.45, timeout=2)
        assert s.proc.poll() is None and LEAVE not in s.raw
        os.write(s.master, b"q")
        raw, err, elapsed = s.end(0, timeout=2)
        assert not any(token in raw for token in (ENTER, LEAVE, b"\x1b[2J", b" / tick "))
        first, last, count, field = timing_output(err)
        before, after = (tuple(map(int, label.split(b":"))) for label in field(b"join").split(b"-"))
        assert first[0] == 0 and last[0] == 1 and last[1] >= 6, err
        assert before[0] == 0 and after[0] == 1
        if stall:
            assert before[1] >= 280 and 1 <= after[1] <= 24, err
        else:
            assert before[1] >= 285 and after[1] <= 2, err
        assert 11.9 <= elapsed < 14, elapsed
        # One extra lower-anchor positioning was sent before the first draw.
        assert count + 1 == raw.count(s.bottom), err
        assert int(field(b"timing_samples")) <= 288, err
        assert b"playback=loop" in err and b"restoration was incomplete" not in err
    case = " with flow control" if stall else ""
    print(f"PASS inline default loop{case}: phase {before} to {after}; q restores")


def test_custom_duration(binary):
    with Session([binary, "--alternate-buffer", "--measure", "--once", "--duration", "12", "--direction", "forward"],
                 cols=121, rows=65) as s:
        raw, err, elapsed = s.end(0, timeout=15)
        assert_scope(raw)
        first, last, count, field = timing_output(err)
        assert first[0] == last[0] == 0 and first[1] <= 3 and 284 <= last[1] < 288, err
        assert field(b"handled").endswith(b"/288") and count == raw.count(s.bottom), err
        assert b"duration=12s direction=forward" in err and b"playback=once" in err
        assert 11.9 <= elapsed < 15, elapsed
    print(f"PASS 12-second forward completion: {elapsed:.6f} s, {count}/288 ticks")


def test_inline(binary):
    # Synthetic CPR positions model a normal foreground shell and a reflow
    # moving both cursor markers. A real VTE resize is verified separately.
    with Session([binary, "--terminal"], cols=165, rows=55) as s:
        s.cursor_reply(1, 55)
        s.cursor_reply(2, 55)
        s.until(BRAILLE.search)
        s.until(lambda raw: raw.count(s.bottom) >= 3)
        first = len(s.raw)

        s.geometry(145, 50)
        s.cursor_reply(3, 50)
        s.cursor_reply(4, 7)
        s.until(lambda raw: b"\x1b[7;1H\x1b[K" in raw[first:] and
                BRAILLE.search(raw, first))
        s.until(lambda raw: raw[first:].count(s.bottom) >= 3)
        os.write(s.master, b"q")
        raw, err, _ = s.end(0)
        assert not err
        assert raw.count(b"\n") == 41
        assert_cursor_scope(raw)
        assert raw.endswith(b"\x1b[7;1H" + CURSOR_RESTORE)
        for packet, top, below in ((raw[:first], 14, 55), (raw[first:], 7, 50)):
            assert all(top <= int(row) <= below
                       for row in re.findall(rb"\x1b\[(\d+);\d+H", packet))
        for token in (ENTER, LEAVE, b"\x1b[2J", b"\x1b[3J", b"oops /", b" / tick "):
            assert token not in raw, token
    with Session([binary, "--terminal"], cols=165, rows=55) as s:
        s.cursor_reply(1, 55, 3)
        raw, err, _ = s.end(1)
        assert raw == CURSOR_QUERY and b"fresh line" in err
    with Session([binary, "--terminal"], cols=165, rows=55) as s:
        raw, err, _ = s.end(1, timeout=1)
        assert raw == CURSOR_QUERY and b"did not report its cursor position" in err
    with Session([binary, "--terminal"], cols=165, rows=55) as s:
        s.cursor_reply(1, 55)
        s.cursor_reply(2, 55)
        s.until(lambda raw: raw.count(s.bottom) >= 2)
        s.geometry(145, 50)
        s.cursor_reply(3, 49)
        s.cursor_reply(4, 50)  # Reversed markers do not establish ownership.
        raw, err, _ = s.end(1)
        assert b"cannot locate reserved inline rows" in err
        assert_cursor_scope(raw)
        assert raw.endswith(CURSOR_RESTORE)
        assert b"\x1b[K" not in raw and b"\x1b[2J" not in raw
    print("PASS inline only repaints the reserved rows, rejects a mid-line launch, and clears on q")


def test_width_follow(binary):
    with Session([binary, "--terminal"], cols=165, rows=55) as s:
        s.cursor_reply(1, 55)
        s.cursor_reply(2, 55)
        s.until(lambda raw: raw.count(s.bottom) >= 3)
        # Width-only safe expansion. Other shell output can reflow by a
        # row, so absolute old positions are not safe even in this case.
        s.geometry(170, 55)
        s.cursor_reply(3, 54)
        start = s.raw.rfind(CURSOR_QUERY)
        s.until(lambda raw: b"\x1b[13;1H\x1b[K" in raw[start:])
        s.until(lambda raw: raw[start:].count(b"\x1b[54;1H") >= 2)
        assert raw_position_safe(s.raw[start:], bottom=54, top=13)

        # A second resize races another inert probe. It does not change
        # the live cursor or erase against the obsolete geometry.
        s.geometry(172, 55)
        s.until(lambda raw: raw.count(CURSOR_QUERY) >= 4)
        end = s.raw.rfind(CURSOR_QUERY)
        s.geometry(174, 55)
        s.cursor_reply(4, 54)
        s.until(lambda raw: raw.count(CURSOR_QUERY) >= 5)
        assert b"\x1b[K" not in s.raw[end:]
        s.cursor_reply(5, 53)
        s.until(lambda raw: b"\x1b[12;1H\x1b[K" in raw[end:])
        os.write(s.master, b"q")
        raw, err, _ = s.end(0)
        assert not err and b"\x1b8" not in raw
        assert_cursor_scope(raw)
        assert raw.endswith(b"\x1b[12;1H" + CURSOR_RESTORE)
        assert raw_position_safe(raw[end:], bottom=54, top=12)
    print("PASS width follows one inert cursor report; a racing report cannot erase stale rows")


def raw_position_safe(raw, *, bottom, top):
    return all(top <= int(row) <= bottom for row in re.findall(rb"\x1b\[(\d+);\d+H", raw))


def test_inline_row_tails(binary):
    with Session([binary, "--terminal", "--duration", "12"], cols=116, rows=41) as s:
        s.cursor_reply(1, 41)
        s.cursor_reply(2, 41)
        # No full clear on startup. EL here means a live row got shorter.
        s.until(b"\x1b[K")
        assert b"\x1b[2K" not in s.raw
        os.write(s.master, b"q")
        raw, err, _ = s.end(0)
        assert not err and raw_position_safe(raw, bottom=41, top=11)
    print("PASS inline shortening uses row-scoped EL, not historical trailing spaces")


def test_inline_shell(binary):
    with Session(["bash", "--noprofile", "--norc", "-i"], cols=165, rows=55,
                 stderr_in_tty=True) as s:
        s.until(b"OOPS_TEST_SHELL> ")
        path = "'" + binary.replace("'", "'\"'\"'") + "'"
        os.write(s.master, b"printf 'PRIOR_%s\\n' OUTPUT\n")
        s.until(b"PRIOR_OUTPUT")
        for run, key, status in ((0, b"q", 0), (1, b"\x03", 130)):
            pos = len(s.raw)
            os.write(s.master, (
                "previous=$(stty -g); " + path
                + " --terminal; code=$?; if [[ $(stty -g) = $previous ]]; then "
                + "printf 'CHECK_%s_OK\\n' \"$code\"; fi\n"
            ).encode())
            s.cursor_reply(run * 2 + 1, 55)
            s.cursor_reply(run * 2 + 2, 55)
            s.until(lambda raw: BRAILLE.search(raw, pos))
            os.write(s.master, key)
            s.until(f"CHECK_{status}_OK".encode())
        os.write(s.master, b"printf 'AFTER_%s\\n' OK\n")
        s.until(b"AFTER_OK")
        os.write(s.master, b"exit\n")
        raw, err, _ = s.end(0)
        assert not err and raw.count(CURSOR_QUERY) == 4
        assert raw.index(b"PRIOR_OUTPUT") < BRAILLE.search(raw).start() < raw.index(b"AFTER_OK")
        assert not any(token in raw for token in (ENTER, LEAVE, b"\x1b[2J", b"\x1b[3J"))
    print("PASS inline repeated normal-shell runs restore q/Ctrl-C; prior output and next command remain")


def test_standard_sizes(binary, framing):
    for cols, rows in ((80, 24), (116, 41), (120, 30), (132, 43)):
        grid_cols, grid_rows = model_grid(framing, cols, rows, inline=True)
        with Session([binary, "--check"], cols=cols, rows=rows) as s:
            raw, err, _ = s.end(0)
            assert f"model grid {grid_cols} by {grid_rows}".encode() in raw
            assert b"surface=inline" in raw and not err and b"\x1b" not in raw
        for alternate in (False, True):
            opts = ["--alternate-buffer"] if alternate else []
            with Session([binary, "--terminal", *opts], cols=cols, rows=rows) as s:
                top = 1
                if not alternate:
                    top = rows - min(rows * 3 // 4, 80)
                    s.cursor_reply(1, rows)
                    s.cursor_reply(2, rows)
                s.until(BRAILLE.search)
                os.write(s.master, b"q")
                raw, err, _ = s.end(0)
                assert not err
                if alternate:
                    assert_scope(raw)
                else:
                    assert ENTER not in raw and b"\x1b[2J" not in raw
                    assert_cursor_scope(raw)
                    assert raw.endswith(f"\x1b[{top};1H".encode() + CURSOR_RESTORE)
                positions = re.findall(rb"\x1b\[(\d+);(\d+)H", raw)
                assert positions and all(top <= int(y) <= rows and 1 <= int(x) <= cols
                                         for y, x in positions)
                assert len(BRAILLE.findall(raw)) >= 20
    print("PASS 80×24, 116×41, 120×30, and 132×43 draw bounded art in inline and alternate screen modes")


def tests(binary, *, full, strict):
    for args, code, out in [
        (["--help"], 0, b"Usage: oops"),
        (["--version"], 0, b"oops 0.0.3"),
        (["--bad-argument"], 2, b"unknown option"),
        (["--terminal"], 2, b"not a terminal"),
        (["--terminal", "--alternate-buffer"], 2, b"not a terminal"),
        (["--alternate-buffer"], 2, b"terminal display flags require"),
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

    embedded = b"This build plays embedded review." in subprocess.check_output([binary, "--help"])
    with Session([binary, "--check"]) as s:
        out, err, _ = s.end(0)
        assert not err
        report = re.search(rb"frame=([0-9.]+)x([0-9.]+)", out)
        assert report, out
        framing = tuple(float(value) for value in report.groups())
        assert min(framing) > 0
    test_config(binary, framing)
    test_zoom_resize(binary, framing)
    test_playback_controls(binary)
    test_renderer(binary)
    test_standard_sizes(binary, framing)

    for cols, rows in [(79, 24), (80, 23), (0, 0)]:
        with Session([binary, "--terminal"], cols=cols, rows=rows) as s:
            out, err, _ = s.end(2)
            assert not out and b"need at least" in err and b"\x1b" not in err
    for env, diagnostic in [
        ({"TERM": "dumb"}, b"Xfce/VTE"),
        ({"COLORTERM": "unknown"}, b"true color"),
        ({"LC_ALL": "C"}, b"UTF-8"),
        ({"TMUX": "fake,10,0"}, b"nested terminal"),
        ({"NO_COLOR": "1"}, b"NO_COLOR"),
    ]:
        with Session([binary, "--terminal"], env=env) as s:
            out, err, _ = s.end(2)
            assert not out and diagnostic in err and b"\x1b" not in err
    with Session([binary, "--check"], cols=80, rows=24) as s:
        out, err, _ = s.end(0)
        assert not err and b"80 columns by 24 rows" in out
        assert b"\x1b" not in out
    print("PASS exact minimum, one-column/row undersize, unknown size/profile and read-only preflight")

    with Session(["bash", "-c", 'exec "$1" --terminal </dev/null', "_", binary]) as s:
        out, err, _ = s.end(2)
        assert not out and b"stdin is not a terminal" in err and b"\x1b" not in err
    print("PASS redirected input does not touch the terminal")

    for cols, rows in [(80, 24), (365, 169)]:
        with Session([binary, "--terminal", "--alternate-buffer"], cols=cols, rows=rows) as s:
            s.until(f"\x1b[{rows};1H".encode())
            os.write(s.master, b"q")
            raw, err, _ = s.end(0)
            assert_scope(raw)
            assert not err and BRAILLE.search(raw)
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
        with Session([binary, "--terminal", "--alternate-buffer"]) as s:
            s.until(s.bottom)
            s.keys_active()
            os.kill(s.proc.pid, sig)
            raw, err, elapsed = s.end(code, timeout=1)
            assert_scope(raw)
            assert not err and elapsed < 1
    print("PASS SIGINT/TERM/HUP/QUIT/TSTP exit, owned cleanup, inherited tty state preserved")

    with Session([binary, "--terminal", "--alternate-buffer"]) as s:
        s.until(s.bottom)
        s.keys_active()
        os.write(s.master, b"q")
        raw, err, elapsed = s.end(0, timeout=1)
        assert_scope(raw)
        assert not err and elapsed < 1
    print("PASS q exits without Enter and restores termios")

    with Session([binary, "--terminal", "--alternate-buffer"]) as s:
        s.until(b"\x1b[86;1H")
        s.geometry(200, 90)
        s.until(lambda raw: raw.count(b"\x1b[2J") >= 2)
        s.geometry(79, 90)
        raw, err, elapsed = s.end(1, timeout=1)
        assert_scope(raw)
        assert b"need at least" in err and elapsed < 1
    print("PASS wider resize recenters and below-minimum resize exits with cleanup")

    with Session([binary, "--terminal", "--alternate-buffer"]) as s:
        s.until(ENTER)
        time.sleep(0.16)  # deliberately let the kernel output queue back up
        os.kill(s.proc.pid, signal.SIGINT)
        raw, err, elapsed = s.end(130, timeout=1.5)
        assert_scope(raw)
        assert not err and elapsed < 1.5
    print("PASS SIGINT while output backs up, cleanup once PTY resumes draining")

    # If the terminal never consumes its input, cleanup cannot be promised.
    # Even diagnostics on that same tty must not block shutdown indefinitely.
    with Session([binary, "--terminal", "--alternate-buffer"], stderr_in_tty=True) as s:
        s.until(ENTER)
        time.sleep(0.16)
        os.kill(s.proc.pid, signal.SIGINT)
        assert s.proc.wait(timeout=1.5) == 130
        _, _, elapsed = s.end(130)
        assert elapsed < 1.5
    print("PASS stopped PTY reader cannot block signal exit and diagnostics indefinitely")

    with Session([binary, "--alternate-buffer", "--measure"]) as s:
        s.until(b"\x1b[86;1H")
        os.kill(s.proc.pid, signal.SIGSTOP)
        time.sleep(0.65)
        os.kill(s.proc.pid, signal.SIGCONT)
        s.until(lambda raw: raw.count(s.bottom) >= 3)
        os.kill(s.proc.pid, signal.SIGTERM)
        raw, err, _ = s.end(143)
        assert_scope(raw)
        _, _, count, field = timing_output(err)
        assert count >= 3 and int(field(b"max_gap")) >= 4, err
    print("PASS blocked schedule skips stale ticks instead of replaying a FIFO")

    # Fill the kernel tty queue after a valid frame. A temporary reader
    # stall exceeding the packet budget must not end the 30-second run.
    # Repeat during full repaint as well; that interrupted repaint must
    # not become the diff baseline.
    with Session([binary, "--alternate-buffer", "--measure"], cols=290, rows=97) as s:
        bottom = b"\x1b[97;1H"
        s.partial_next_frame(bottom, force_repaint=embedded)
        before_recoveries = s.raw.count(RESYNC)
        time.sleep(0.48)
        assert s.proc.poll() is None, "a temporary stall ended playback"
        s.until(lambda raw: raw.count(RESYNC) > before_recoveries
                and len(raw.rsplit(RESYNC, 1)[1]) >= 1024)
        assert bottom not in s.raw.rsplit(RESYNC, 1)[1], "already received the full repaint"
        time.sleep(0.48)
        assert s.proc.poll() is None, "a second stall ended playback"
        s.until(lambda raw: raw.count(RESYNC) >= before_recoveries + 2
                and bottom in raw.rsplit(RESYNC, 1)[1])
        os.write(s.master, b"q")
        raw, err, _ = s.end(0)
        assert_scope(raw)
        _, last, _, field = timing_output(err)
        assert last[1] >= 16 and int(field(b"max_gap")) >= 7, err
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
    with Session([binary, "--alternate-buffer", "--measure"], cols=165, rows=55) as s:
        s.until(b"\x1b[55;1H")
        termios.tcflow(s.slave, termios.TCOOFF)
        try:
            time.sleep(0.42)
            assert s.proc.poll() is None
        finally:
            termios.tcflow(s.slave, termios.TCOON)
        s.until(lambda raw: raw.count(s.bottom) >= 2)
        os.write(s.master, b"q")
        raw, err, _ = s.end(0)
        assert_scope(raw)
        report = re.search(rb"dropped_write=(\d+) partial_write=(\d+)", err)
        assert report and int(report[1]) >= 1 and b"timed out" not in err, err
        assert (RESYNC in raw) == (int(report[2]) != 0)
    print("PASS terminal flow control resumes without repainting an untouched frame")

    # Resume a recovery-waiting player with a key, not writable output.
    # CAN must make even a partially submitted escape harmless before exit.
    with Session([binary, "--terminal", "--alternate-buffer"], cols=290, rows=97) as s:
        s.partial_next_frame(b"\x1b[97;1H", force_repaint=embedded)
        before_recoveries = s.raw.count(RESYNC)
        time.sleep(0.48)
        assert s.proc.poll() is None
        os.write(s.master, b"q")
        raw, err, _ = s.end(0, timeout=1)
        assert_scope(raw)
        assert raw.count(RESYNC) == before_recoveries and not err, err
    print("PASS q during the recovery wait restores without forcing a new frame")

    test_resize_wire_expectations()
    # Resize right as the next packet is being received. The remaining bytes
    # may reach the tty before ioctl on a faster runner. Inspect the accepted
    # stream rather than assuming that a partial read implies a partial write.
    with Session([binary, "--terminal", "--alternate-buffer"], cols=290, rows=97) as s:
        before = b"\x1b[97;1H"
        after = b"\x1b[85;1H"
        s.until(before)
        first = s.raw.index(before) + len(before)
        s.until(lambda raw: len(raw) >= first + 1024)
        s.geometry(213, 85)
        s.until(after)
        os.write(s.master, b"q")
        raw, err, _ = s.end(0)
        assert_scope(raw)

        outcome = assert_resize_wire(raw, first, before, after, 213, 85)
        assert not err
    mode = "partial old packet" if outcome else "completed old packet"
    print(f"PASS usable resize with {mode}; both legal wire cases and invalid controls checked")

    # Actual foreground shell process, real keyboard VINTR, then a fresh
    # shell command. ANSI is captured, not interpreted by a GUI terminal.
    with Session(["bash", "--noprofile", "--norc", "-i"], stderr_in_tty=True) as s:
        s.until(b"OOPS_TEST_SHELL> ")
        # Make shell echo unable to masquerade as the sentinel.
        quoted_path = "'" + binary.replace("'", "'\"'\"'") + "'"
        os.write(s.master, (
            "stty_before=$(stty -g); " + quoted_path
            + " --terminal --alternate-buffer; r=$?; if [[ $(stty -g) = $stty_before ]]; then "
            + "printf 'CHECK_%s_OK\\n' \"$r\"; fi\n"
        ).encode())
        s.until(s.bottom)
        os.write(s.master, b"\x03")
        s.until(b"CHECK_130_OK")
        os.write(s.master, (
            "stty_before=$(stty -g); " + quoted_path
            + " --terminal --alternate-buffer; r=$?; if [[ $(stty -g) = $stty_before ]]; then "
            + "printf 'CHECK_%s_OK\n' \"$r\"; fi\n"
        ).encode())
        s.until(lambda raw: raw.count(ENTER) == 2)
        s.until(lambda raw: raw.count(s.bottom) >= 2)
        os.write(s.master, b"q")
        s.until(b"CHECK_0_OK")
        os.write(s.master, b"exit\n")
        raw, err, _ = s.end(0)
        assert raw.count(ENTER) == raw.count(LEAVE) == 2, bytes(raw[-500:])
        assert raw.index(LEAVE + CURSOR_RESTORE) < raw.index(b"CHECK_130_OK")
        assert raw.rindex(LEAVE + CURSOR_RESTORE) < raw.index(b"CHECK_0_OK")
        assert b"failed" not in err
    print("PASS interactive Bash returns after q and keyboard Ctrl-C with original stty")

    test_inline(binary)
    test_width_follow(binary)
    test_inline_row_tails(binary)
    test_inline_shell(binary)
    test_default_loop(binary)
    if full:
        test_default_loop(binary, stall=True)
        test_custom_duration(binary)
        with Session([binary, "--alternate-buffer", "--measure", "--once"]) as s:
            raw, err, elapsed = s.end(0, timeout=34)
            assert_scope(raw)
            first, last, count, field = timing_output(err)
            assert first[0] == last[0] == 0 and first[1] <= 3 and last[1] >= 716, err
            assert b"grid=" in err and b"trace_p95/max_us=" in err
            print(err.decode().strip())
            assert 29.9 <= elapsed < 34, elapsed
            handled = int(field(b"handled").split(b"/")[0])
            held = int(field(b"held"))
            assert handled == count + held and count == raw.count(s.bottom)
            if embedded:
                assert 0 < held <= 432 and count <= 288, err
            else:
                assert held == 0, err
            if strict:
                assert handled == 720, f"only {handled} handled ticks"
                assert count == (288 if embedded else 720), err
        print(f"PASS natural completion: {elapsed:.6f} s, {handled}/720 ticks, "
              f"{count} writes, {held} holds, {len(raw)} bytes")
        if not strict:
            with Session([binary, "--alternate-buffer", "--measure", "--once"], cols=290, rows=97) as s:
                s.until(lambda _: time.monotonic() - s.start >= 29.68, timeout=32)
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
    parser.add_argument("--full", action="store_true",
                        help="also test stalled loop and one-turn deadlines (allows missed ticks)")
    parser.add_argument("--strict", action="store_true", help="require every tick on a quiet host")
    args = parser.parse_args()
    tests(args.binary, full=args.full or args.strict, strict=args.strict)


if __name__ == "__main__":
    main()
