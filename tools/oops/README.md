# oops playback and configuration

## Open the window

Download the Linux or Windows x64 package from
[GitHub Releases](https://github.com/elysia090/Goodhand/releases) when
available, extract it, then open `oops` (Linux) or `oops.exe` (Windows).
Linux requires a GTK 3 desktop runtime and glibc 2.35 or later. No
terminal, video player, network connection or external artwork is
required for native playback.

`--window` and `--island` select the same default native window. It
requests a 595×290 client area, excluding decorations. It plays all
288 original grayscale poses on an opaque near-black background, at
equal intervals over a 45-second turn. It loops by default; press **q**
or **Esc** to close, or use `--once` to stop after one turn. A busy
desktop can miss a displayed pose.

    ./oops --once --duration 22
    ./oops --direction forward
    ./oops --config oops.conf

In PowerShell use `.\oops.exe` in place of `./oops`.
`--duration` accepts 12–60 whole seconds. `--direction reverse` follows
the original order (the default); `forward` starts from the same pose
and rotates the other way without mirroring the picture.

## Configuration

No config is needed or discovered automatically. Copy
[`oops.example.conf`](oops.example.conf), uncomment only settings you
want to change, and pass the file with `--config PATH`.

Use one `key = value` per line in a UTF-8 regular file up to 8 KiB.
Blank lines and `#` comments are OK. Command-line options override the
corresponding config values, then built-in defaults apply. An explicit
unreadable file, unknown or duplicate key, or invalid value is an
error, even if a command-line option would override that key.

| Config key | Applies to | Values |
| --- | --- | --- |
| `duration` | Both | 12–60 whole seconds; default window 45, terminal 30 |
| `direction` | Both | `reverse` (default), `forward` |
| `window_appearance` | Window only | `safe` (opaque black, default), `clear` (transparent), `framed` (translucent) |
| `zoom` | Terminal only | 50–200 percent, default 100; above 100 may crop |
| `cell_aspect` | Terminal only | `auto` (default), or cell width/height from 0.30 to 0.85; fallback 0.50 |
| `offset_x`, `offset_y` | Terminal only | −50 to 50 percent, default 0; positive moves right or down |
| `brightness` | Terminal only | 0–200 percent, default 100; 0 keeps occupied dots black |
| `renderer` | Terminal only | `braille` (default), `dots` (one bullet per cell) |

The window ignores valid terminal-only config values; terminal modes
ignore valid `window_appearance` values. Terminal display CLI flags
require `--terminal`, `--check` or `--measure`.
`window_appearance` has no CLI flag. On a composited Linux desktop
press **t** to cycle appearances. Other platforms support only
`safe`.

## Terminal preview (Linux x64)

Use a foreground Xfce/VTE-compatible terminal with true color, UTF-8,
and at least 80 columns × 24 rows. A larger terminal shows more detail.
The terminal preview can differ from the native artwork.

    oops --terminal
    oops --terminal --alternate-buffer
    oops --terminal --dots
    oops --check

`--terminal` draws below the command; `--alternate-buffer` uses the
same terminal's alternate screen, not a new window. Press **q** or
**Ctrl-C** to return to the shell. `--check` reports settings without
painting; `--measure` reports terminal submission timings after exit.
Use `--help` for the terminal options.

[Build and test the player](../../docs/playback-development.md).
