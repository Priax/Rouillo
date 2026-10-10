use std::ffi::{OsStr, OsString};
use std::fs::{self, File};
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::os::windows::io::IntoRawHandle;
use std::path::PathBuf;
use std::ptr;

use windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE;
use windows_sys::Win32::System::Console::{GetStdHandle, SetStdHandle, STD_ERROR_HANDLE};
use windows_sys::Win32::System::Registry::{RegGetValueW, RegSetKeyValueW, HKEY_CURRENT_USER, REG_SZ, RRF_RT_REG_SZ};

/// Where the installer registers the game, under its AppId
/// (assets/windows/rouillo.iss).
const UNINSTALL_KEY: &str =
    r"Software\Microsoft\Windows\CurrentVersion\Uninstall\{89402259-6417-4E94-9C00-13FA4F6030E6}_is1";

/// Without a console, error messages and panics would go nowhere: they go to
/// rouillo.log, the previous run's kept as rouillo.old.log. The game started
/// by an update inherits the log and carries on in it.
pub fn log_to_file() {
    let stderr = unsafe { GetStdHandle(STD_ERROR_HANDLE) };
    if stderr.is_null() || stderr == INVALID_HANDLE_VALUE {
        open_log();
    }
    eprintln!("Rouillo {}", env!("CARGO_PKG_VERSION"));
}

fn open_log() {
    let Some(dir) = crate::storage::data_dir() else { return };
    let log = dir.join("rouillo.log");
    let _ = fs::create_dir_all(&dir);
    let _ = fs::rename(&log, dir.join("rouillo.old.log"));
    let Ok(file) = File::create(&log) else { return };
    unsafe { SetStdHandle(STD_ERROR_HANDLE, file.into_raw_handle()) };
}

/// After the game replaced itself, shows the new version in the installed
/// apps, if this copy is the one the installer put there.
pub fn record_installed_version(version: &str) {
    let installed = installed_location().and_then(|dir| fs::canonicalize(dir).ok());
    let here = std::env::current_exe()
        .ok()
        .and_then(|exe| fs::canonicalize(exe.parent()?).ok());
    if installed.is_none() || installed != here {
        return;
    }
    let value = wide(version);
    unsafe {
        RegSetKeyValueW(
            HKEY_CURRENT_USER,
            wide(UNINSTALL_KEY).as_ptr(),
            wide("DisplayVersion").as_ptr(),
            REG_SZ,
            value.as_ptr().cast(),
            (value.len() * 2) as u32,
        )
    };
}

fn installed_location() -> Option<PathBuf> {
    let mut buf = [0u16; 1024];
    let mut size = (buf.len() * 2) as u32;
    let status = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            wide(UNINSTALL_KEY).as_ptr(),
            wide("InstallLocation").as_ptr(),
            RRF_RT_REG_SZ,
            ptr::null_mut(),
            buf.as_mut_ptr().cast(),
            &mut size,
        )
    };
    if status != 0 {
        return None;
    }
    let len = (size as usize / 2).saturating_sub(1);
    Some(PathBuf::from(OsString::from_wide(&buf[..len])))
}

fn wide(text: &str) -> Vec<u16> {
    OsStr::new(text).encode_wide().chain([0]).collect()
}
