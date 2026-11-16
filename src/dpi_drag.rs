//! Dragging the window between displays with a different scale, on Windows.
//!
//! When a window crosses to a display with another scale, Windows sends `WM_DPICHANGED` with the
//! rectangle the window should take: the same logical size, placed so that it stays on the new
//! display. winit 0.30 computes its own rectangle and then pushes it towards the display it takes
//! for the new one, which it reads from where the window is *now*. During a drag that is still the
//! old display, so the window lands back on it, Windows changes the scale back at once, and the
//! window ends up larger on every crossing (1600 px wide, then 2000, then 2500). Displays above
//! each other show it; side by side the window is mostly on the new display already.
//!
//! [`install`] puts a subclass before winit's window procedure. winit still handles the message,
//! so its scale factor and the events to iced stay right, but the position it asks for while doing
//! so is replaced by the rectangle Windows suggested. Its size is the one winit asks for too.
use std::cell::Cell;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::UI::Shell::{DefSubclassProc, SetWindowSubclass};
use windows::Win32::UI::WindowsAndMessaging::{IsZoomed, SWP_NOMOVE, SWP_NOSIZE, WINDOWPOS, WM_DPICHANGED, WM_WINDOWPOSCHANGING};

thread_local! {
    /// The rectangle of the `WM_DPICHANGED` under way: left, top, width, height.
    static SUGGESTED: Cell<Option<(i32, i32, i32, i32)>> = const { Cell::new(None) };
}

/// Makes the window with this raw id (its `HWND`, from `iced::window::raw_id`) keep the rectangle
/// Windows suggests when its scale changes. Call it on the thread that owns the window.
pub fn install(raw_id: u64) {
    let hwnd = HWND(raw_id as usize as *mut core::ffi::c_void);
    // SAFETY: `subclass` matches SUBCLASSPROC and forwards every message; a failure (not our
    // window, or the wrong thread) only leaves the window as it was
    let _ = unsafe { SetWindowSubclass(hwnd, Some(subclass), 1, 0) };
}

unsafe extern "system" fn subclass(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM, _id: usize, _data: usize) -> LRESULT {
    match msg {
        // a maximized window keeps winit's handling: it has no size of its own to preserve
        WM_DPICHANGED if lparam.0 != 0 && !IsZoomed(hwnd).as_bool() => {
            // SAFETY: for WM_DPICHANGED, lparam points to the suggested RECT
            let r = *(lparam.0 as *const RECT);
            SUGGESTED.with(|s| s.set(Some((r.left, r.top, r.right - r.left, r.bottom - r.top))));
            let result = DefSubclassProc(hwnd, msg, wparam, lparam);
            SUGGESTED.with(|s| s.set(None));
            return result;
        }
        WM_WINDOWPOSCHANGING if lparam.0 != 0 => {
            if let Some((x, y, cx, cy)) = SUGGESTED.with(Cell::get) {
                // SAFETY: for WM_WINDOWPOSCHANGING, lparam points to a WINDOWPOS we may change
                let pos = &mut *(lparam.0 as *mut WINDOWPOS);
                if !pos.flags.contains(SWP_NOMOVE) && !pos.flags.contains(SWP_NOSIZE) {
                    (pos.x, pos.y, pos.cx, pos.cy) = (x, y, cx, cy);
                }
            }
        }
        _ => {}
    }
    DefSubclassProc(hwnd, msg, wparam, lparam)
}
