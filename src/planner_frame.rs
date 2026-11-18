//! The web frame of the Planner: the web version of Open Planner Studio in the system's web view,
//! a child of the app's own window that lies over the workspace.
//!
//! The web view is made the first time the Planner is shown and is kept on the thread that made
//! it (the window's own); it is hidden, not closed, when another product is shown, so the page
//! keeps its state. A window of the system lies over all the app draws itself: the shell hides
//! the frame for a dialog and for File, and keeps it out of the way of what hangs into the
//! workspace (see `Studio::planner_area`).

use std::cell::RefCell;
use std::path::PathBuf;

use windows::Win32::Foundation::{HWND, RECT};
use wry::dpi::{PhysicalPosition, PhysicalSize};
use wry::raw_window_handle::{HasWindowHandle, RawWindowHandle};
use wry::{Rect, WebContext, WebView, WebViewBuilder};

use crate::shell::PLANNER_URL as URL;

/// How far the frame stays from the window's left, top, right and bottom, in pixels before the
/// display's scale.
pub type Insets = [f32; 4];

/// The frame's left, top, width and height in the window, in the display's pixels.
type Bounds = [i32; 4];

fn rect([x, y, width, height]: Bounds) -> Rect {
    Rect { position: PhysicalPosition::new(x, y).into(), size: PhysicalSize::new(width, height).into() }
}

fn handle(window: &impl HasWindowHandle) -> Option<HWND> {
    match window.window_handle().ok()?.as_raw() {
        RawWindowHandle::Win32(handle) => Some(HWND(handle.hwnd.get() as *mut core::ffi::c_void)),
        _ => None,
    }
}

/// Where the frame lies in `window` as it is now: the window's own size and display scale are
/// asked of the system here, so the frame is right whatever the app last heard of them.
fn bounds(window: &impl HasWindowHandle, [left, top, right, bottom]: Insets) -> Option<Bounds> {
    use windows::Win32::UI::HiDpi::GetDpiForWindow;
    use windows::Win32::UI::WindowsAndMessaging::GetClientRect;
    let parent = handle(window)?;
    let mut client = RECT::default();
    // SAFETY: the handle is the live window's, and this runs on its thread
    let scale = unsafe {
        GetClientRect(parent, &mut client).ok()?;
        GetDpiForWindow(parent) as f32 / 96.0
    };
    let px = |v: f32| (v * scale).round() as i32;
    Some([px(left), px(top), (client.right - px(left) - px(right)).max(1), (client.bottom - px(top) - px(bottom)).max(1)])
}

thread_local! {
    /// The web view of this thread's window, the context that holds its data folder, and where
    /// the frame was last laid (`None`: hidden).
    static VIEW: RefCell<Option<(WebView, WebContext, Option<Bounds>)>> = const { RefCell::new(None) };
}

/// Lays the frame in `window`, `insets` from its edges, and shows it, or hides it (`None`); a
/// frame that already lies so is left alone. The first frame makes the web view and loads the
/// page; an error then says the web view could not be made (its runtime is missing, or its data
/// folder cannot be written). To be called on the thread of the window.
pub fn place(window: &impl HasWindowHandle, insets: Option<Insets>) -> Result<(), String> {
    let frame = match insets {
        Some(insets) => Some(bounds(window, insets).ok_or("the window has no handle")?),
        None => None,
    };
    VIEW.with_borrow_mut(|view| {
        match (view.as_mut(), frame) {
            (None, None) => {}
            (Some((view, _, laid)), None) => {
                if laid.take().is_some() {
                    // a hidden frame that kept the keyboard would swallow the app's shortcuts
                    let _ = view.focus_parent();
                    view.set_visible(false).map_err(|e| e.to_string())?;
                }
            }
            (Some((view, _, laid)), Some(frame)) => {
                if *laid != Some(frame) {
                    view.set_bounds(rect(frame)).map_err(|e| e.to_string())?;
                    if laid.is_none() {
                        view.set_visible(true).map_err(|e| e.to_string())?;
                    }
                    *laid = Some(frame);
                }
            }
            (None, Some(frame)) => {
                // the web view's own files (cache, sign-in) go with the app's other data, not
                // beside the program, where an installed app may not write
                let mut context = WebContext::new(data_folder());
                let made = WebViewBuilder::new_with_web_context(&mut context)
                    .with_url(URL)
                    .with_bounds(rect(frame))
                    // the app keeps the keyboard until the page is clicked
                    .with_focused(false)
                    .build_as_child(window);
                match made {
                    Ok(made) => *view = Some((made, context, Some(frame))),
                    Err(e) => {
                        remove_leftover(window);
                        return Err(e.to_string());
                    }
                }
            }
        }
        Ok(())
    })
}

/// Loads the page again, when the frame is there.
pub fn reload() -> Result<(), String> {
    VIEW.with_borrow(|view| match view {
        Some((view, _, _)) => view.reload().map_err(|e| e.to_string()),
        None => Ok(()),
    })
}

/// Gives the keyboard back to the app's window: the page keeps it after a click in it, and a
/// click on the app's own controls does not take it away by itself.
pub fn release_keyboard() -> Result<(), String> {
    VIEW.with_borrow(|view| match view {
        Some((view, _, _)) => view.focus_parent().map_err(|e| e.to_string()),
        None => Ok(()),
    })
}

/// The web view's data folder: `webview` in the app's settings folder, or the folder
/// `O3V_WEBVIEW_DATA` names (a test gives one that cannot be made, to see the tab without a web
/// view).
fn data_folder() -> Option<PathBuf> {
    std::env::var_os("O3V_WEBVIEW_DATA").map(PathBuf::from).or_else(|| Some(crate::remote_link::settings_dir()?.join("webview")))
}

/// Takes away the window a web view that could not be made leaves in `window`: the library makes
/// that window first and does not close it when the web view fails, so it would lie, empty, over
/// the page that says the web view is missing. Only called while this thread has no web view.
fn remove_leftover(window: &impl HasWindowHandle) {
    use windows::core::w;
    use windows::Win32::UI::WindowsAndMessaging::{DestroyWindow, FindWindowExW};
    let Some(parent) = handle(window) else {
        return;
    };
    // SAFETY: the handle is the live window's, and this runs on its thread
    unsafe {
        while let Ok(child) = FindWindowExW(parent, HWND::default(), w!("WRY_WEBVIEW"), None) {
            if child.is_invalid() || DestroyWindow(child).is_err() {
                break;
            }
        }
    }
}
