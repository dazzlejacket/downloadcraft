//! Placement and focus behaviour for the standalone download dialogs.
//!
//! Several download dialogs can be open at once (the browser hands over one
//! capture at a time). Windows stacks those windows down and to the right, so
//! they do not hide each other. This module reproduces that: every new dialog
//! takes the next free slot of the cascade, on the monitor the pointer is on,
//! inside that monitor's work area and at the right scale.
//!
//! A cascade dialog must not pull the keyboard away from whatever the user is
//! typing in. The first dialog of a cascade behaves normally (it is the one
//! the user just asked for); every later one is created without activation
//! and only takes focus when the user clicks it.

use std::path::PathBuf;

/// How far each new dialog steps down and to the right.
const CASCADE_STEP: f32 = 26.0;
/// Gap kept between the work area edge and the first dialog.
const CASCADE_MARGIN: f32 = 24.0;
/// A dialog that has not reported itself for this long is considered gone.
const STALE_AFTER_SECS: i64 = 900;

/// The slot a new dialog takes in the cascade.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CascadeSlot {
    /// 0 for the first dialog of a cascade. Only that one may take focus.
    pub index: usize,
}

impl CascadeSlot {
    /// True when this dialog may steal the keyboard focus on appearance.
    pub fn may_activate(&self) -> bool {
        self.index == 0
    }
}

fn dialog_dir() -> Option<PathBuf> {
    crate::model::config_dir().map(|d| d.join("dialogs"))
}

/// Live dialogs are tracked as one small file per dialog so the cascade
/// resets by itself once the dialogs are closed — no counter to leak.
fn live_dialog_count() -> usize {
    let Some(dir) = dialog_dir() else {
        return 0;
    };
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return 0;
    };
    let now = chrono::Local::now();
    let mut live = 0usize;
    for entry in entries.flatten() {
        let path = entry.path();
        let stale = std::fs::read_to_string(&path)
            .ok()
            .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
            .and_then(|v| v.get("started").and_then(|x| x.as_str()).map(str::to_string))
            .and_then(|s| chrono::DateTime::parse_from_rfc3339(&s).ok())
            .map(|dt| now.timestamp() - dt.timestamp() > STALE_AFTER_SECS)
            .unwrap_or(true);
        if stale {
            let _ = std::fs::remove_file(&path);
            continue;
        }
        live += 1;
    }
    live
}

/// Register this dialog as live and return the cascade slot it should take.
pub fn take_cascade_slot() -> CascadeSlot {
    let index = live_dialog_count();
    if let Some(dir) = dialog_dir() {
        if std::fs::create_dir_all(&dir).is_ok() {
            let file = dir.join(format!("dialog-{}.json", std::process::id()));
            let payload = serde_json::json!({ "started": chrono::Local::now().to_rfc3339() });
            let _ = crate::model::atomic_write(
                &file,
                serde_json::to_string_pretty(&payload)
                    .unwrap_or_default()
                    .as_bytes(),
            );
        }
    }
    CascadeSlot { index }
}

/// Drop this dialog's registration (call it when the dialog closes).
pub fn release_cascade_slot() {
    if let Some(dir) = dialog_dir() {
        let _ = std::fs::remove_file(dir.join(format!("dialog-{}.json", std::process::id())));
    }
}

/// The work area of the monitor under the pointer, in logical points.
///
/// Returns `(min_x, min_y, max_x, max_y)`. On platforms without a native
/// implementation the whole screen is assumed.
pub fn work_area_logical() -> (f32, f32, f32, f32) {
    #[cfg(windows)]
    {
        if let Some(area) = windows_work_area() {
            return area;
        }
    }
    (0.0, 0.0, 1920.0, 1080.0)
}

/// Where a dialog of `size` belongs in the cascade.
pub fn cascade_position(size: (f32, f32), index: usize) -> (f32, f32) {
    let (min_x, min_y, max_x, max_y) = work_area_logical();
    let step = index as f32 * CASCADE_STEP;
    let x = min_x + CASCADE_MARGIN + step;
    let y = min_y + CASCADE_MARGIN + step;
    // Keep the whole dialog inside the work area: once the cascade runs out
    // of room it stacks back up instead of sliding off the screen.
    let max_x = (max_x - size.0 - CASCADE_MARGIN).max(min_x);
    let max_y = (max_y - size.1 - CASCADE_MARGIN).max(min_y);
    (x.min(max_x), y.min(max_y))
}

// ---------------------------------------------------------------------------
// Windows specifics
// ---------------------------------------------------------------------------

#[cfg(windows)]
mod win {
    use windows_sys::Win32::Foundation::{HWND, LPARAM, POINT, RECT, WPARAM};
    use windows_sys::Win32::Graphics::Gdi::{
        GetMonitorInfoW, MonitorFromPoint, HMONITOR, MONITORINFO, MONITOR_DEFAULTTONEAREST,
    };
    use windows_sys::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GetCursorPos, GetWindowLongPtrW, GetWindowRect, GetWindowThreadProcessId,
        GetWindowTextLengthW, GetWindowTextW, SetForegroundWindow, SetWindowLongPtrW, SetWindowPos,
        ShowWindow, GWL_EXSTYLE, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SW_SHOWNOACTIVATE,
        WS_EX_NOACTIVATE,
    };

    /// Work area of the monitor under the pointer, in logical points.
    pub fn work_area() -> Option<(f32, f32, f32, f32)> {
        unsafe {
            let mut pt = POINT { x: 0, y: 0 };
            if GetCursorPos(&mut pt) == 0 {
                return None;
            }
            let monitor: HMONITOR = MonitorFromPoint(pt, MONITOR_DEFAULTTONEAREST);
            if monitor.is_null() {
                return None;
            }
            let mut info = MONITORINFO {
                cbSize: std::mem::size_of::<MONITORINFO>() as u32,
                ..std::mem::zeroed()
            };
            if GetMonitorInfoW(monitor, &mut info) == 0 {
                return None;
            }
            let mut dpi: u32 = 96;
            // `GetDpiForMonitor` is documented as not DPI-aware itself, but it
            // is the only way to learn a monitor's scale before a window
            // exists on it. A failure just means we assume 100%.
            let _ = GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut dpi, &mut dpi);
            let scale = if dpi == 0 { 1.0 } else { dpi as f32 / 96.0 };
            let r: RECT = info.rcWork;
            let to_logical = |v: i32| v as f32 / scale;
            Some((
                to_logical(r.left),
                to_logical(r.top),
                to_logical(r.right),
                to_logical(r.bottom),
            ))
        }
    }

    struct FindState {
        pid: u32,
        title: Vec<u16>,
        found: HWND,
    }

    unsafe extern "system" fn enum_proc(hwnd: HWND, lparam: LPARAM) -> i32 {
        let state = &mut *(lparam as *mut FindState);
        let mut pid: u32 = 0;
        GetWindowThreadProcessId(hwnd, &mut pid);
        if pid != state.pid {
            return 1;
        }
        let len = GetWindowTextLengthW(hwnd);
        if len <= 0 {
            return 1;
        }
        let mut buf = vec![0u16; (len + 1) as usize];
        let n = GetWindowTextW(hwnd, buf.as_mut_ptr(), buf.len() as i32);
        if n <= 0 {
            return 1;
        }
        buf.truncate(n as usize);
        if buf == state.title {
            state.found = hwnd;
            return 0;
        }
        1
    }

    /// Find one of this process's top level windows by its title.
    pub fn find_own_window(title: &str) -> Option<HWND> {
        let mut wide: Vec<u16> = title.encode_utf16().collect();
        wide.push(0);
        let mut state = FindState {
            pid: std::process::id(),
            title: wide,
            found: std::ptr::null_mut(),
        };
        unsafe {
            EnumWindows(
                Some(enum_proc),
                &mut state as *mut FindState as LPARAM,
            );
        }
        if state.found.is_null() {
            None
        } else {
            Some(state.found)
        }
    }

    /// Mark the window so it does not take focus when it is shown.
    pub fn set_no_activate(hwnd: HWND) {
        unsafe {
            let style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
            SetWindowLongPtrW(hwnd, GWL_EXSTYLE, style | WS_EX_NOACTIVATE as isize);
            // Show it without activating it: the cascade must not steal the
            // keyboard from whatever the user is typing in.
            SetWindowPos(
                hwnd,
                std::ptr::null_mut(),
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
            );
            ShowWindow(hwnd, SW_SHOWNOACTIVATE);
        }
    }

    /// Give the window the keyboard, because the user clicked it.
    pub fn activate(hwnd: HWND) {
        unsafe {
            let style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
            if style & WS_EX_NOACTIVATE as isize != 0 {
                SetWindowLongPtrW(hwnd, GWL_EXSTYLE, style & !(WS_EX_NOACTIVATE as isize));
            }
            SetForegroundWindow(hwnd);
        }
    }

    /// Is the pointer inside this window?
    pub fn pointer_inside(hwnd: HWND) -> bool {
        unsafe {
            let mut rect: RECT = std::mem::zeroed();
            if GetWindowRect(hwnd, &mut rect) == 0 {
                return false;
            }
            let mut pt = POINT { x: 0, y: 0 };
            if GetCursorPos(&mut pt) == 0 {
                return false;
            }
            pt.x >= rect.left && pt.x < rect.right && pt.y >= rect.top && pt.y < rect.bottom
        }
    }

    /// True when the window currently refuses activation.
    pub fn is_no_activate(hwnd: HWND) -> bool {
        unsafe { GetWindowLongPtrW(hwnd, GWL_EXSTYLE) & WS_EX_NOACTIVATE as isize != 0 }
    }

    #[allow(dead_code)]
    pub fn unused(_: WPARAM) {}
}

#[cfg(windows)]
fn windows_work_area() -> Option<(f32, f32, f32, f32)> {
    win::work_area()
}

/// The window handle of one of this process's windows, by title.
#[cfg(windows)]
pub fn own_window_handle(title: &str) -> Option<isize> {
    win::find_own_window(title).map(|h| h as isize)
}

/// Stop the window from taking focus when it appears.
#[cfg(windows)]
pub fn suppress_activation(handle: isize) {
    let hwnd = handle as windows_sys::Win32::Foundation::HWND;
    if !hwnd.is_null() {
        win::set_no_activate(hwnd);
    }
}

/// Let the window take focus again (the user clicked it).
#[cfg(windows)]
pub fn allow_activation(handle: isize) {
    let hwnd = handle as windows_sys::Win32::Foundation::HWND;
    if !hwnd.is_null() {
        win::activate(hwnd);
    }
}

/// Is the pointer inside this window?
#[cfg(windows)]
pub fn pointer_inside(handle: isize) -> bool {
    let hwnd = handle as windows_sys::Win32::Foundation::HWND;
    if hwnd.is_null() {
        false
    } else {
        win::pointer_inside(hwnd)
    }
}

/// True while the window refuses activation.
#[cfg(windows)]
pub fn activation_suppressed(handle: isize) -> bool {
    let hwnd = handle as windows_sys::Win32::Foundation::HWND;
    if hwnd.is_null() {
        false
    } else {
        win::is_no_activate(hwnd)
    }
}

/// Non-Windows builds have nothing to do: there is no always-on-top window
/// style to undo and the toolkit handles activation.
#[cfg(not(windows))]
pub fn own_window_handle(_title: &str) -> Option<isize> {
    None
}
#[cfg(not(windows))]
pub fn suppress_activation(_handle: isize) {}
#[cfg(not(windows))]
pub fn allow_activation(_handle: isize) {}
#[cfg(not(windows))]
pub fn activation_suppressed(_handle: isize) -> bool {
    false
}
#[cfg(not(windows))]
pub fn pointer_inside(_handle: isize) -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_first_dialog_of_a_cascade_may_take_the_focus() {
        assert!(CascadeSlot { index: 0 }.may_activate());
        assert!(!CascadeSlot { index: 1 }.may_activate());
        assert!(!CascadeSlot { index: 7 }.may_activate());
    }

    #[test]
    fn the_cascade_steps_down_and_to_the_right() {
        let first = cascade_position((400.0, 300.0), 0);
        let second = cascade_position((400.0, 300.0), 1);
        assert!(second.0 > first.0);
        assert!(second.1 > first.1);
        assert!((second.0 - first.0 - CASCADE_STEP).abs() < 0.01);
    }

    #[test]
    fn a_dialog_never_leaves_the_work_area() {
        let (min_x, min_y, max_x, max_y) = work_area_logical();
        let (x, y) = cascade_position((400.0, 300.0), 50);
        assert!(x >= min_x);
        assert!(y >= min_y);
        assert!(x + 400.0 <= max_x.max(min_x + 400.0));
        assert!(y + 300.0 <= max_y.max(min_y + 300.0));
    }
}
