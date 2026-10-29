//! Best-effort labels for the MSTSC process started by this application. Discovery remains global.
use anyhow::{Context as _, Result};
use std::{process::Child, thread, time::Duration};
use windows::{
    Win32::{
        Foundation::{HWND, LPARAM, WPARAM},
        UI::WindowsAndMessaging::{
            EnumWindows, GetClassNameW, GetWindowTextW, GetWindowThreadProcessId, SMTO_ABORTIFHUNG,
            SMTO_BLOCK, SendMessageTimeoutW, WM_SETTEXT,
        },
    },
    core::BOOL,
};

struct Label {
    pid: u32,
    title: Vec<u16>,
    hwnd: Option<isize>,
    found: bool,
}

pub fn track(mut child: Child, title: String) -> Result<()> {
    thread::Builder::new()
        .name("mstsc-session-title".into())
        .spawn(move || {
            let mut label = Label {
                pid: child.id(),
                title: title.encode_utf16().chain(Some(0)).collect(),
                hwnd: None,
                found: false,
            };
            loop {
                // Keep the child handle alive and stop on exit; never match a later process reusing its PID.
                if !matches!(child.try_wait(), Ok(None)) {
                    break;
                }
                label.found = false;
                // SAFETY: the callback borrows label only during synchronous EnumWindows. It targets
                // this live child's main RDP container only, never login dialogs or other MSTSC sessions.
                let enumerated = unsafe {
                    EnumWindows(
                        Some(label_window),
                        LPARAM((&mut label as *mut Label) as isize),
                    )
                };
                if enumerated.is_ok() && label.hwnd.is_some() && !label.found {
                    // Do not transfer a closed container's label to another session in the process.
                    break;
                }
                thread::sleep(Duration::from_millis(1000));
            }
        })
        .context("MSTSC launched, but its session title monitor could not start")?;
    Ok(())
}

unsafe extern "system" fn label_window(hwnd: HWND, data: LPARAM) -> BOOL {
    // SAFETY: data points to the Label owned by track's synchronous EnumWindows call.
    let label = unsafe { &mut *(data.0 as *mut Label) };
    let mut pid = 0;
    let mut class = [0u16; 128];
    // SAFETY: hwnd is supplied by EnumWindows and all output buffers are valid writable storage.
    let class_len = unsafe {
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        GetClassNameW(hwnd, &mut class)
    };
    if pid != label.pid
        || String::from_utf16_lossy(&class[..class_len.max(0) as usize]) != "TscShellContainerClass"
        || label.hwnd.is_some_and(|target| target != hwnd.0 as isize)
    {
        return BOOL(1);
    }
    label.hwnd = Some(hwnd.0 as isize);
    label.found = true;
    let mut current = vec![0u16; label.title.len().saturating_add(1)];
    // SAFETY: GetWindowTextW copies at most current.len() units into the provided buffer.
    let count = unsafe { GetWindowTextW(hwnd, &mut current) };
    if current[..count.max(0) as usize] != label.title[..label.title.len() - 1] {
        // SAFETY: label.title is NUL-terminated and remains alive until this synchronous call
        // completes. WM_SETTEXT is system-marshalled across processes. Bound the wait for hung RDP
        // clients; no hooks, injection, subclassing, focus, or ownership changes are involved.
        unsafe {
            let _ = SendMessageTimeoutW(
                hwnd,
                WM_SETTEXT,
                WPARAM(0),
                LPARAM(label.title.as_ptr() as isize),
                SMTO_ABORTIFHUNG | SMTO_BLOCK,
                100,
                None,
            );
        }
    }
    BOOL(1)
}
