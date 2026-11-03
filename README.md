# g502-buttons

Makes the special buttons on a Logitech G502 Lightspeed visible to any app, on Windows and Linux, without G Hub or any Logitech software.

It talks HID++ directly to the Lightspeed receiver, enables the mouse's **Mouse Button Spy** feature (0x8110), and turns buttons the OS normally never sees (sniper, DPI up/down, G9, wheel tilt) into key presses. Keys are held while the button is held, so push-to-talk works. Left, right and middle click are left alone.

## Install

Download the latest file from [Releases](../../releases). It is a single small file that installs and uninstalls itself. Install it once and forget about it: it runs in the background and survives restarts, sleep and receiver replugs.

### Windows

Download `g502-buttons-windows-x64.exe`, double-click it and choose **Yes**. (The exe is not code-signed, so SmartScreen may warn; click **More info > Run anyway**.)

- Installs per user to `%LOCALAPPDATA%\Programs\g502-buttons`. No admin rights needed.
- Starts hidden at login, with no window or tray icon.
- Uninstall from **Settings > Apps > G502 Buttons**. This removes the app, its settings and the autostart entry.
- Settings: `%APPDATA%\g502-buttons\buttons.toml`. Launching the exe again while it runs opens this file. The log is in the same folder.

### Linux

```
chmod +x g502-buttons-linux-x64
./g502-buttons-linux-x64 --install
```

The Linux binary is statically linked, so it runs on any x86-64 distro.

- Copies the binary to `~/.local/bin` and enables a systemd user service that starts at login.
- Installs a udev rule (asks for sudo) so your user can reach the receiver (hidraw) and `/dev/uinput`.
- Settings: `~/.config/g502-buttons/buttons.toml`. Logs: `journalctl --user -u g502-buttons -f`.
- Uninstall: `~/.local/bin/g502-buttons --uninstall`.

## Build from source

`cargo build --release`, then run the binary with `--install`. Building needs no system libraries: on Linux it uses hidapi's pure-Rust hidraw backend and evdev.

Releases are built by `.github/workflows/release.yml`. Push a `v*` tag to publish one.

## Settings

```toml
sniper = "f13"
dpi_down = "f14"
dpi_up = "f15"
g9 = "f16"
# tilt_left = "f17"
# tilt_right = "f18"
```

Bind F13–F24 in your apps; combos like `"ctrl+alt+f1"` also work. Buttons: `back forward sniper dpi_down dpi_up g9 tilt_left tilt_right`. Changes apply as soon as the file is saved.

Spy mode is volatile: it stops when the app exits or the mouse power-cycles, and the app re-enables it when the mouse reconnects.

## Button bits

| Bit | Button | Bit | Button |
|---|---|---|---|
| 0–2 | left / right / middle (ignored) | 7 | DPI up (G8) |
| 3 | back (G5) | 8 | G9 (unconfirmed) |
| 4 | forward (G4) | 9 | tilt right |
| 5 | sniper (G6) | 10 | tilt left |
| 6 | DPI down (G7) | | |

`probe/g502_probe.py` is the Python tool used to discover these.
