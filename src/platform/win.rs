//! Windows: SendInput keys, per-user install that starts with Windows and shows up
//! in Settings > Apps.

use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::ptr::null_mut;
use std::time::Duration;
use std::{env, fs, thread};

use windows_sys::Win32::Foundation::{
    ERROR_ALREADY_EXISTS, GetLastError, HANDLE_FLAG_INHERIT, SetHandleInformation,
};
use windows_sys::Win32::System::Console::{
    ATTACH_PARENT_PROCESS, AttachConsole, GetStdHandle, STD_ERROR_HANDLE, STD_INPUT_HANDLE,
    STD_OUTPUT_HANDLE,
};
use windows_sys::Win32::System::Threading::CreateMutexW;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_EXTENDEDKEY, KEYEVENTF_KEYUP,
    MAPVK_VK_TO_VSC, MapVirtualKeyW, SendInput,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    IDYES, MB_ICONERROR, MB_ICONINFORMATION, MB_ICONQUESTION, MB_YESNO, MessageBoxW,
};
use winreg::RegKey;
use winreg::enums::{HKEY_CURRENT_USER, KEY_SET_VALUE};

use crate::keys::Key;

const APP: &str = "G502 Buttons";
const EXE: &str = "g502-buttons.exe";
const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const UNINSTALL_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Uninstall\g502-buttons";
const CREATE_NO_WINDOW: u32 = 0x0800_0000;
const DETACHED_PROCESS: u32 = 0x0000_0008;

// Keys whose scan code needs the E0 prefix.
const EXTENDED: [u16; 19] = [
    0x21, 0x22, 0x23, 0x24, 0x25, 0x26, 0x27, 0x28, 0x2D, 0x2E, 0x5B, // nav, arrows, win
    0xAD, 0xAE, 0xAF, 0xB0, 0xB1, 0xB2, 0xB3, 0xA3, // media, right ctrl
];

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn message_box(text: &str, style: u32) -> i32 {
    unsafe { MessageBoxW(null_mut(), wide(text).as_ptr(), wide(APP).as_ptr(), style) }
}

/// The exe has no console of its own; borrow the terminal's when started from one.
pub fn attach_console() -> bool {
    unsafe { AttachConsole(ATTACH_PARENT_PROCESS) != 0 }
}

pub fn notify(msg: &str, console: bool) {
    if console {
        println!("{msg}");
    } else {
        message_box(msg, MB_ICONINFORMATION);
    }
}

pub fn report_error(msg: &str, console: bool) {
    if console {
        eprintln!("Error: {msg}");
    } else {
        message_box(msg, MB_ICONERROR);
    }
}

pub fn confirm(msg: &str) -> bool {
    message_box(msg, MB_YESNO | MB_ICONQUESTION) == IDYES
}

pub fn config_dir() -> PathBuf {
    PathBuf::from(env::var_os("APPDATA").unwrap_or_default()).join("g502-buttons")
}

fn install_dir() -> PathBuf {
    PathBuf::from(env::var_os("LOCALAPPDATA").unwrap_or_default()).join("Programs").join("g502-buttons")
}

fn same_file(a: &Path, b: &Path) -> bool {
    matches!((a.canonicalize(), b.canonicalize()), (Ok(a), Ok(b)) if a == b)
}

/// Double-clicked in Explorer (no console) and not the installed copy: act as installer.
pub fn launched_by_double_click(console: bool) -> bool {
    !console && !env::current_exe().is_ok_and(|me| same_file(&me, &install_dir().join(EXE)))
}

pub fn single_instance() -> bool {
    // The mutex is released by the OS when the process exits.
    unsafe {
        CreateMutexW(null_mut(), 0, wide(r"Local\g502-buttons").as_ptr());
        GetLastError() != ERROR_ALREADY_EXISTS
    }
}

pub fn open_in_editor(path: &Path) {
    let _ = Command::new("notepad.exe").arg(path).spawn();
}

pub struct Injector;

impl Injector {
    pub fn new() -> Result<Self, String> {
        Ok(Injector)
    }

    pub fn send(&mut self, key: Key, down: bool) {
        let mut flags = if down { 0 } else { KEYEVENTF_KEYUP };
        if EXTENDED.contains(&key.win) {
            flags |= KEYEVENTF_EXTENDEDKEY;
        }
        let input = INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: key.win,
                    // Fill the scan code too: games reading raw input/DirectInput use it.
                    wScan: unsafe { MapVirtualKeyW(key.win as u32, MAPVK_VK_TO_VSC) } as u16,
                    dwFlags: flags,
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        };
        unsafe { SendInput(1, &input, size_of::<INPUT>() as i32) };
    }
}

fn stop_other_instances() {
    let _ = Command::new("taskkill")
        .args(["/F", "/IM", EXE, "/FI", &format!("PID ne {}", std::process::id())])
        .creation_flags(CREATE_NO_WINDOW)
        .output();
}

fn reg_err(e: std::io::Error) -> String {
    format!("registry: {e}")
}

pub fn install() -> Result<String, String> {
    let dir = install_dir();
    let target = dir.join(EXE);
    let me = env::current_exe().map_err(|e| e.to_string())?;
    fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;

    stop_other_instances();
    if !same_file(&me, &target) {
        // A just-killed instance can hold the file for a moment.
        let mut copied = fs::copy(&me, &target);
        for _ in 0..10 {
            if copied.is_ok() {
                break;
            }
            thread::sleep(Duration::from_millis(300));
            copied = fs::copy(&me, &target);
        }
        copied.map_err(|e| format!("copy to {}: {e}", target.display()))?;
    }

    let quoted = format!("\"{}\"", target.display());
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let (run, _) = hkcu.create_subkey(RUN_KEY).map_err(reg_err)?;
    run.set_value(APP, &quoted).map_err(reg_err)?;

    let (u, _) = hkcu.create_subkey(UNINSTALL_KEY).map_err(reg_err)?;
    let size_kb = fs::metadata(&target).map(|m| m.len() / 1024).unwrap_or(0) as u32;
    let strings = [
        ("DisplayName", APP.to_string()),
        ("DisplayVersion", env!("CARGO_PKG_VERSION").to_string()),
        ("Publisher", "g502-buttons".to_string()),
        ("DisplayIcon", quoted.clone()),
        ("InstallLocation", dir.display().to_string()),
        ("UninstallString", format!("{quoted} --uninstall")),
        ("QuietUninstallString", format!("{quoted} --uninstall")),
    ];
    for (name, value) in &strings {
        u.set_value(name, value).map_err(reg_err)?;
    }
    for (name, value) in [("NoModify", 1u32), ("NoRepair", 1), ("EstimatedSize", size_kb)] {
        u.set_value(name, &value).map_err(reg_err)?;
    }

    // Detached, without our stdio, so a terminal that ran --install isn't held open.
    // Child processes inherit every inheritable handle, so un-mark ours first.
    for std in [STD_INPUT_HANDLE, STD_OUTPUT_HANDLE, STD_ERROR_HANDLE] {
        unsafe { SetHandleInformation(GetStdHandle(std), HANDLE_FLAG_INHERIT, 0) };
    }
    Command::new(&target)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .creation_flags(DETACHED_PROCESS)
        .spawn()
        .map_err(|e| format!("start {}: {e}", target.display()))?;
    Ok(format!(
        "{APP} is installed and running. It starts with Windows.\n\n\
         Settings: {}\n(open it again from the installed exe, or edit the file; changes apply right away)\n\n\
         Uninstall from Settings > Apps.",
        config_dir().join("buttons.toml").display()
    ))
}

pub fn uninstall() -> Result<String, String> {
    stop_other_instances();
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    if let Ok(run) = hkcu.open_subkey_with_flags(RUN_KEY, KEY_SET_VALUE) {
        let _ = run.delete_value(APP);
    }
    let _ = hkcu.delete_subkey_all(UNINSTALL_KEY);
    let _ = fs::remove_dir_all(config_dir());

    // A running exe can't delete itself; let cmd remove the folder once we've exited.
    let dir = install_dir();
    if dir.exists() {
        Command::new("cmd")
            .raw_arg(format!("/C ping 127.0.0.1 -n 3 >nul & rmdir /s /q \"{}\"", dir.display()))
            .current_dir(env::temp_dir())
            .creation_flags(CREATE_NO_WINDOW)
            .spawn()
            .map_err(|e| format!("remove {}: {e}", dir.display()))?;
    }
    Ok(format!("{APP} was uninstalled."))
}
