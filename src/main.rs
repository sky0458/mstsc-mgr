#![cfg_attr(windows, windows_subsystem = "windows")]

#[cfg(windows)]
mod crypto;
#[cfg(windows)]
mod model;
#[cfg(windows)]
mod platform;
#[cfg(windows)]
mod storage;
#[cfg(windows)]
mod ui;

#[cfg(windows)]
fn main() {
    if let Err(error) = ui::run() {
        ui::show_fatal_error(&format!("{error:#}"));
    }
}

#[cfg(not(windows))]
fn main() {
    eprintln!("mstsc-mgr-external only runs on Windows");
}
