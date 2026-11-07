# G502 Buttons

[![Release](https://img.shields.io/github/v/release/orel56000/g502-buttons)](https://github.com/orel56000/g502-buttons/releases/latest)
[![Build](https://github.com/orel56000/g502-buttons/actions/workflows/release.yml/badge.svg)](https://github.com/orel56000/g502-buttons/actions/workflows/release.yml)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)
![Platforms](https://img.shields.io/badge/platform-Windows%20%7C%20Linux-lightgrey)

Use **every button** on your Logitech G502 Lightspeed in any app (Discord push-to-talk, games, OBS, anything with keybinds) **without G Hub or any Logitech software**.

The sniper button, DPI up/down and G9 normally never reach your apps: the mouse handles them internally. G502 Buttons turns them into regular key presses (F13–F16 by default), so any app can bind them.

## Features

- **Install and forget.** One small file (under 1 MB) that installs itself, runs hidden in the background and survives restarts, sleep and receiver replugs.
- **No Logitech software.** It talks directly to the mouse; nothing else to install.
- **Hold-aware.** The key stays down while you hold the button, so push-to-talk works.
- **Instant.** Button to key press in microseconds.
- **Easy to remove.** Uninstall from Settings > Apps on Windows, or one command on Linux.
- **Windows and Linux.**

## Install

Grab the latest file from **[Releases](https://github.com/orel56000/g502-buttons/releases/latest)**.

### Windows

1. Download `g502-buttons-windows-x64.exe`.
2. Double-click it and choose **Yes**.

That's it. It installs just for your user (no admin needed), runs hidden and starts with Windows.

> The exe isn't code-signed, so Windows SmartScreen may say "Windows protected your PC". Click **More info > Run anyway**.

**Uninstall:** Settings > Apps > **G502 Buttons** > Uninstall. This removes the app, its settings and the autostart entry.

### Linux

```sh
chmod +x g502-buttons-linux-x64
./g502-buttons-linux-x64 --install
```

This copies the app to `~/.local/bin`, enables a systemd user service that starts at login, and installs a udev rule so your user can reach the receiver and create a virtual keyboard. Installing the udev rule asks for sudo once. The binary is statically linked and runs on any x86-64 distro.

**Uninstall:** `~/.local/bin/g502-buttons --uninstall`

## Settings

Default mapping:

| Button | Key |
|---|---|
| Sniper (red thumb button, G6) | F13 |
| DPI down (G7) | F14 |
| DPI up (G8) | F15 |
| G9 | F16 |

To change it, edit `buttons.toml`. The change applies as soon as you save.

- **Windows:** `%APPDATA%\g502-buttons\buttons.toml`. Running the exe again while it's running opens this file.
- **Linux:** `~/.config/g502-buttons/buttons.toml`

```toml
sniper = "f13"
dpi_down = "f14"
dpi_up = "f15"
g9 = "f16"
# tilt_left = "f17"
# tilt_right = "f18"
# back = "ctrl+alt+b"
```

- **Buttons:** `back forward sniper dpi_down dpi_up g9 tilt_left tilt_right`.
- **Keys:** `a`–`z`, `0`–`9`, `f1`–`f24`, `ctrl alt shift win space enter esc tab backspace insert delete home end pageup pagedown left up right down mute volume_up volume_down play_pause next_track prev_track stop`.
- **Combos:** join keys with `+`, e.g. `"ctrl+shift+m"`.

Left, right and middle click are never touched. Back and forward already work in apps, so they're unmapped by default.

## Troubleshooting

- **A key doesn't work in one app.** Windows blocks key presses from normal apps to apps running as administrator. Run that app without admin, or bind a different key.
- **Discord feels laggy.** Check Settings > Voice & Video > *Push to Talk Release Delay*, then re-record the keybind by pressing the mouse button itself.
- **Nothing happens.** Look at the log: `%APPDATA%\g502-buttons\g502-buttons.log` on Windows, `journalctl --user -u g502-buttons -f` on Linux. On Linux, unplug and replug the receiver once after installing.

## How it works

The app finds the Lightspeed receiver's vendor HID interface and speaks Logitech's HID++ 2.0 protocol to the mouse. It turns on the **Mouse Button Spy** feature (`0x8110`), which reports the state of all 11 buttons. Buttons with a mapping become key presses: `SendInput` on Windows, a `uinput` virtual keyboard on Linux. Spy mode is temporary: it ends when the app exits or the mouse power-cycles, and the app turns it back on whenever the mouse reconnects. Nothing is written to the mouse's memory.

| Spy bit | Button | Spy bit | Button |
|---|---|---|---|
| 0–2 | left / right / middle (ignored) | 7 | DPI up (G8) |
| 3 | back (G5) | 8 | G9 (unconfirmed) |
| 4 | forward (G4) | 9 | tilt right |
| 5 | sniper (G6) | 10 | tilt left |
| 6 | DPI down (G7) | | |

`probe/g502_probe.py` is the Python tool used to discover this map. Run it on another HID++ mouse that has `0x8110` to map its buttons.

**Tested with:** G502 Lightspeed (receiver `046d:c539`) on Windows 10. Other Lightspeed receivers (`c53f`, `c547`) are recognized, but their button bits may differ.

## Build from source

```sh
cargo build --release
target/release/g502-buttons --install   # .exe on Windows
```

You don't need any system libraries. On Linux it uses hidapi's pure-Rust hidraw backend and evdev. Releases are built by [`.github/workflows/release.yml`](.github/workflows/release.yml): pushing a `v*` tag publishes one.

## Support

If this saved you from installing G Hub, you can [buy me a coffee](https://www.buymeacoffee.com/orel56000). Bug reports and pull requests are welcome.

## License

[MIT](LICENSE). Not affiliated with or endorsed by Logitech. "Logitech", "G502" and "Lightspeed" are trademarks of Logitech.
