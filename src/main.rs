#![cfg_attr(windows, windows_subsystem = "windows")]

#[cfg(windows)]
mod crypto;
#[cfg(windows)]
mod model;
#[cfg(windows)]
mod mstsc;
#[cfg(windows)]
mod storage;
#[cfg(windows)]
mod ui;

#[cfg(windows)]
fn main() {
    if let Err(error) = ui::run() {
        let message = format!("mstsc-mgr external failed to start:\n{error:#}");
        unsafe {
            use windows::{
                Win32::UI::WindowsAndMessaging::{MB_ICONERROR, MB_OK, MessageBoxW},
                core::{HSTRING, w},
            };
            let _ = MessageBoxW(
                None,
                &HSTRING::from(message),
                w!("mstsc-mgr external"),
                MB_OK | MB_ICONERROR,
            );
        }
    }
}

#[cfg(not(windows))]
fn main() {
    eprintln!("mstsc-mgr external is Windows-only");
}
