//! Linux: uinput virtual keyboard, systemd user service, udev rule for device access.

use std::os::fd::AsRawFd;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::{env, fs};

use evdev::uinput::{VirtualDevice, VirtualDeviceBuilder};
use evdev::{AttributeSet, EventType, InputEvent, Key as EvKey};

use crate::keys::Key;

const SERVICE: &str = "g502-buttons.service";
const UDEV_RULES: &str = include_str!("../../linux/70-g502-buttons.rules");
const UDEV_PATH: &str = "/etc/udev/rules.d/70-g502-buttons.rules";

pub fn attach_console() -> bool {
    true // stdout goes to the terminal or the journal
}

pub fn notify(msg: &str, _console: bool) {
    println!("{msg}");
}

pub fn report_error(msg: &str, _console: bool) {
    eprintln!("Error: {msg}");
}

pub fn confirm(_msg: &str) -> bool {
    false
}

pub fn launched_by_double_click(_console: bool) -> bool {
    false
}

fn home() -> PathBuf {
    env::var_os("HOME").map(PathBuf::from).unwrap_or_default()
}

fn xdg_config() -> PathBuf {
    env::var_os("XDG_CONFIG_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| home().join(".config"))
}

pub fn config_dir() -> PathBuf {
    xdg_config().join("g502-buttons")
}

fn bin_path() -> PathBuf {
    home().join(".local/bin/g502-buttons")
}

fn unit_path() -> PathBuf {
    xdg_config().join("systemd/user").join(SERVICE)
}

pub fn single_instance() -> bool {
    let dir = env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from).unwrap_or_else(env::temp_dir);
    let Ok(file) = fs::File::create(dir.join("g502-buttons.lock")) else {
        return true;
    };
    let locked = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0;
    if locked {
        std::mem::forget(file); // hold the lock until the process exits
    }
    locked
}

pub fn open_in_editor(path: &Path) {
    let _ = Command::new("xdg-open").arg(path).spawn();
}

pub struct Injector(VirtualDevice);

impl Injector {
    pub fn new() -> Result<Self, String> {
        let err = |e: std::io::Error| {
            format!("cannot create virtual keyboard via /dev/uinput ({e}); run `g502-buttons --install`")
        };
        let mut keys = AttributeSet::<EvKey>::new();
        for code in 1..256 {
            keys.insert(EvKey::new(code));
        }
        let dev = VirtualDeviceBuilder::new()
            .map_err(err)?
            .name("g502-buttons")
            .with_keys(&keys)
            .map_err(err)?
            .build()
            .map_err(err)?;
        Ok(Injector(dev))
    }

    pub fn send(&mut self, key: Key, down: bool) {
        let _ = self.0.emit(&[InputEvent::new(EventType::KEY, key.linux, down as i32)]);
    }
}

fn run(program: &str, args: &[&str]) -> Result<(), String> {
    let status = Command::new(program)
        .args(args)
        .status()
        .map_err(|e| format!("{program}: {e}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("`{program} {}` failed", args.join(" ")))
    }
}

fn systemctl(args: &[&str]) -> Result<(), String> {
    run("systemctl", &[&["--user"], args].concat())
}

pub fn install() -> Result<String, String> {
    let me = env::current_exe().map_err(|e| e.to_string())?;
    let bin = bin_path();
    let _ = systemctl(&["stop", SERVICE]);

    if me != bin {
        fs::create_dir_all(bin.parent().unwrap()).map_err(|e| e.to_string())?;
        // Copy then rename, so replacing a binary that is still running works.
        let tmp = bin.with_extension("new");
        fs::copy(&me, &tmp).map_err(|e| format!("copy to {}: {e}", tmp.display()))?;
        fs::rename(&tmp, &bin).map_err(|e| format!("{}: {e}", bin.display()))?;
    }

    if fs::read_to_string(UDEV_PATH).ok().as_deref() != Some(UDEV_RULES) {
        println!("Installing udev rule {UDEV_PATH} (needs sudo) for receiver and uinput access...");
        let tmp = env::temp_dir().join("70-g502-buttons.rules");
        fs::write(&tmp, UDEV_RULES).map_err(|e| e.to_string())?;
        run("sudo", &["install", "-m", "644", &tmp.to_string_lossy(), UDEV_PATH])?;
        run("sudo", &["udevadm", "control", "--reload-rules"])?;
        run("sudo", &["udevadm", "trigger"])?;
    }

    let unit = unit_path();
    fs::create_dir_all(unit.parent().unwrap()).map_err(|e| e.to_string())?;
    fs::write(
        &unit,
        format!(
            "[Unit]\nDescription=G502 Buttons (Logitech G502 button mapper)\n\n\
             [Service]\nExecStart={}\nRestart=on-failure\nRestartSec=3\n\n\
             [Install]\nWantedBy=default.target\n",
            bin.display()
        ),
    )
    .map_err(|e| format!("{}: {e}", unit.display()))?;
    systemctl(&["daemon-reload"])?;
    systemctl(&["enable", "--now", SERVICE])?;

    Ok(format!(
        "G502 Buttons is installed and running; it starts when you log in.\n\
         Settings: {} (changes apply right away)\n\
         Logs:     journalctl --user -u g502-buttons -f\n\
         Remove:   {} --uninstall\n\
         If buttons don't map yet, unplug and replug the receiver once.",
        config_dir().join("buttons.toml").display(),
        bin.display()
    ))
}

pub fn uninstall() -> Result<String, String> {
    let _ = systemctl(&["disable", "--now", SERVICE]);
    let _ = fs::remove_file(unit_path());
    let _ = systemctl(&["daemon-reload"]);
    let _ = fs::remove_file(bin_path());
    let _ = fs::remove_dir_all(config_dir());
    if Path::new(UDEV_PATH).exists() {
        println!("Removing udev rule {UDEV_PATH} (needs sudo)...");
        run("sudo", &["rm", "-f", UDEV_PATH])?;
        let _ = run("sudo", &["udevadm", "control", "--reload-rules"]);
    }
    Ok("G502 Buttons was uninstalled.".into())
}
