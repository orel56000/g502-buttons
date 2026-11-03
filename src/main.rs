//! Turns Logitech G502 Lightspeed buttons into key presses any app can see, using
//! HID++ Mouse Button Spy directly (no Logitech software). Windows and Linux.

#![cfg_attr(windows, windows_subsystem = "windows")]

mod hidpp;
mod keys;
mod platform;

use std::io::Write;
use std::panic::{self, AssertUnwindSafe};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime};
use std::{env, fs, thread};

use hidapi::HidApi;
use hidpp::{Error, FEATURE_BUTTON_SPY, LONG, Receiver, SHORT};
use keys::Combo;
use platform::Injector;

/// Button Spy bit for each special button, verified on a G502 Lightspeed.
/// g9 = bit 8 is the only unused bit but is still unconfirmed.
const BUTTONS: [(&str, u8); 8] = [
    ("back", 3),
    ("forward", 4),
    ("sniper", 5),
    ("dpi_down", 6),
    ("dpi_up", 7),
    ("g9", 8),
    ("tilt_right", 9),
    ("tilt_left", 10),
];
/// Left, right and middle click (bits 0-2) are left entirely to the OS.
const IGNORED_BITS: u16 = 0b111;

const DEFAULT_CONFIG: &str = r#"# G502 button -> key mapping. The key is held down while the button is held.
# Save the file and changes apply within a second.
# Combos work: "ctrl+alt+f1". Keys: a-z 0-9 f1-f24 ctrl alt shift win space enter esc tab
#   backspace insert delete home end pageup pagedown left up right down
#   mute volume_up volume_down play_pause next_track prev_track stop
#
# Buttons: back forward sniper dpi_down dpi_up g9 tilt_left tilt_right
# back/forward already reach apps; mapping them sends the key as well.

sniper = "f13"
dpi_down = "f14"
dpi_up = "f15"
g9 = "f16"
# tilt_left = "f17"
# tilt_right = "f18"
"#;

const USAGE: &str = "usage: g502-buttons [--install | --uninstall]

  (no option)   run in the background
  --install     install for this user and start automatically at login
  --uninstall   remove the app, its settings and autostart";

const DEVICE_INDEX: u8 = 1; // receiver slot of the mouse
const HEALTH_CHECK: Duration = Duration::from_secs(10);

static RUNNING: AtomicBool = AtomicBool::new(true);
static LOG: OnceLock<Mutex<fs::File>> = OnceLock::new();
static START: OnceLock<Instant> = OnceLock::new();

fn running() -> bool {
    RUNNING.load(Ordering::SeqCst)
}

/// Prints, and on Windows (no visible console) also writes to the log file.
fn log(msg: &str) {
    let secs = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).map_or(0, |d| d.as_secs());
    let (h, m, s) = (secs / 3600 % 24, secs / 60 % 60, secs % 60);
    let line = format!("{h:02}:{m:02}:{s:02} UTC  {msg}");
    println!("{line}");
    if let Some(file) = LOG.get() {
        let _ = writeln!(file.lock().unwrap(), "{line}");
    }
}

fn main() {
    let console = platform::attach_console();
    let installer = |console| platform::install().map(|msg| platform::notify(&msg, console));
    let result = match env::args().nth(1).as_deref() {
        Some("--install") => installer(console),
        Some("--uninstall") => platform::uninstall().map(|msg| platform::notify(&msg, console)),
        Some("-h" | "--help") => {
            println!("{USAGE}");
            Ok(())
        }
        Some(other) => Err(format!("unknown option '{other}'\n\n{USAGE}")),
        None if platform::launched_by_double_click(console) => {
            if platform::confirm(
                "Install G502 Buttons?\n\nIt runs in the background, starts with Windows, \
                 and can be removed from Settings > Apps.",
            ) {
                installer(console)
            } else {
                Ok(())
            }
        }
        None => run(console),
    };
    if let Err(e) = result {
        platform::report_error(&e, console);
        std::process::exit(1);
    }
}

fn run(console: bool) -> Result<(), String> {
    let config_path = platform::config_dir().join("buttons.toml");
    if !platform::single_instance() {
        // Launching it again while it runs opens the settings.
        platform::open_in_editor(&config_path);
        if console {
            println!("Already running; settings: {}", config_path.display());
        }
        return Ok(());
    }
    let mut config = Config::load(config_path)?;
    if cfg!(windows) {
        if let Ok(file) = fs::File::create(platform::config_dir().join("g502-buttons.log")) {
            let _ = LOG.set(Mutex::new(file));
        }
    }
    log(&format!("Config: {}", config.path.display()));
    let _ = ctrlc::set_handler(|| RUNNING.store(false, Ordering::SeqCst));
    let mut injector = Injector::new()?;
    let mut api = HidApi::new().map_err(|e| format!("cannot init HID: {e}"))?;

    let mut last_err = String::new();
    while running() {
        // A background app must never die: errors and even panics just mean reconnect.
        let outcome = panic::catch_unwind(AssertUnwindSafe(|| {
            session(&mut api, &mut config, &mut injector, &mut last_err)
        }));
        let msg = match outcome {
            Ok(Ok(())) => continue,
            Ok(Err(e)) => e.to_string(),
            Err(_) => "internal error".to_string(),
        };
        if msg != last_err {
            log(&format!("{msg}; retrying..."));
            last_err = msg;
        }
        for _ in 0..20 {
            if !running() {
                break;
            }
            thread::sleep(Duration::from_millis(100));
        }
    }
    log("Stopped.");
    Ok(())
}

struct Config {
    path: PathBuf,
    modified: Option<SystemTime>,
    combos: Vec<Option<Combo>>,
}

impl Config {
    fn load(path: PathBuf) -> Result<Self, String> {
        let io_err = |e: std::io::Error| format!("{}: {e}", path.display());
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir).map_err(io_err)?;
        }
        if !path.exists() {
            fs::write(&path, DEFAULT_CONFIG).map_err(io_err)?;
        }
        let mut config = Config { path, modified: None, combos: Vec::new() };
        config.reload()?;
        Ok(config)
    }

    fn modified_time(&self) -> Option<SystemTime> {
        fs::metadata(&self.path).and_then(|m| m.modified()).ok()
    }

    fn changed(&self) -> bool {
        self.modified_time() != self.modified
    }

    fn reload(&mut self) -> Result<(), String> {
        self.modified = self.modified_time();
        let text = fs::read_to_string(&self.path).map_err(|e| format!("{}: {e}", self.path.display()))?;
        self.combos = parse_config(&text)?;
        Ok(())
    }
}

fn parse_config(text: &str) -> Result<Vec<Option<Combo>>, String> {
    let mut combos: Vec<Option<Combo>> = BUTTONS.iter().map(|_| None).collect();
    for (n, line) in text.lines().enumerate() {
        let line = line.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        let err = |msg: String| format!("line {}: {msg}", n + 1);
        let (button, key) = line
            .split_once('=')
            .ok_or_else(|| err("expected: button = \"key\"".into()))?;
        let button = button.trim();
        let i = BUTTONS
            .iter()
            .position(|(name, _)| *name == button)
            .ok_or_else(|| err(format!("unknown button '{button}'")))?;
        combos[i] = Some(Combo::parse(key.trim().trim_matches('"')).map_err(err)?);
    }
    Ok(combos)
}

fn session(
    api: &mut HidApi,
    config: &mut Config,
    inj: &mut Injector,
    last_err: &mut String,
) -> Result<(), Error> {
    let rx = Receiver::open(api, DEVICE_INDEX)?;
    rx.ensure_wireless_notifications()?;
    rx.ping()?;
    let spy = rx.feature_index(FEATURE_BUTTON_SPY)?;
    rx.start_spy(spy)?;
    last_err.clear();
    log("Connected to mouse; listening for buttons.");

    let mut held = 0u16;
    let result = listen(&rx, spy, config, inj, &mut held);
    update(&config.combos, inj, held, 0); // never leave a key stuck down
    let _ = rx.stop_spy(spy);
    result
}

fn listen(
    rx: &Receiver,
    spy: u8,
    config: &mut Config,
    inj: &mut Injector,
    held: &mut u16,
) -> Result<(), Error> {
    let mut next_config_check = Instant::now();
    let mut next_health_check = Instant::now() + HEALTH_CHECK;
    while running() {
        if Instant::now() >= next_health_check {
            // Receiver-only query: catches a reset receiver or a handle gone stale after
            // PC sleep, without waking the mouse.
            next_health_check = Instant::now() + HEALTH_CHECK;
            rx.ensure_wireless_notifications()?;
        }
        if Instant::now() >= next_config_check {
            next_config_check = Instant::now() + Duration::from_secs(1);
            if config.changed() {
                update(&config.combos, inj, *held, 0);
                *held = 0;
                match config.reload() {
                    Ok(()) => log("Config reloaded."),
                    Err(e) => log(&format!("Config error, keeping previous mapping: {e}")),
                }
            }
        }

        let Some(r) = rx.wait(10)? else {
            continue;
        };
        if r.len() < 6 || r[1] != rx.dev {
            continue;
        }
        if r[0] == LONG && r[2] == spy && r[3] & 0x0F == 0 {
            let now = u16::from_be_bytes([r[4], r[5]]) & !IGNORED_BITS;
            update(&config.combos, inj, *held, now);
            *held = now;
        } else if r[0] == SHORT && r[2] == 0x41 {
            // Receiver link notification; the mouse forgets spy mode when it reconnects.
            update(&config.combos, inj, *held, 0);
            *held = 0;
            if r[4] & 0x40 == 0 {
                log("Mouse reconnected.");
                // A just-woken mouse can need a moment before it accepts requests.
                let mut started = rx.start_spy(spy);
                for _ in 0..10 {
                    if started.is_ok() {
                        break;
                    }
                    thread::sleep(Duration::from_millis(200));
                    started = rx.start_spy(spy);
                }
                started?;
            } else {
                log("Mouse disconnected.");
            }
        }
    }
    Ok(())
}

fn update(combos: &[Option<Combo>], inj: &mut Injector, old: u16, new: u16) {
    let changed = old ^ new;
    for bit in (0..16u8).filter(|b| (changed >> b) & 1 == 1) {
        let down = (new >> bit) & 1 == 1;
        let state = if down { "down" } else { "up" };
        match BUTTONS.iter().position(|&(_, b)| b == bit) {
            Some(i) => {
                let start = Instant::now();
                if let Some(combo) = &combos[i] {
                    if down { combo.press(inj) } else { combo.release(inj) }
                }
                // Timestamp + time from receiving the report to the key being sent.
                println!(
                    "{:>9.3}s {:>10} {state:<4}  key sent in {} µs",
                    START.get_or_init(Instant::now).elapsed().as_secs_f64(),
                    BUTTONS[i].0,
                    start.elapsed().as_micros()
                );
            }
            None => println!("unknown bit {bit} {state}"),
        }
    }
}
