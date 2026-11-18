//! The web frame of the Planner: the web version of Open Planner Studio in the system's web view,
//! a child of the app's own window that lies over the workspace.
//!
//! A trial. The web view is made the first time the Planner is shown and is kept on the thread
//! that made it (the window's own); it is hidden, not closed, when another product is shown, so
//! the page keeps its state. What the app draws itself in the workspace (a tooltip that hangs
//! below the ribbon, a dialog) lies under the frame: the shell hides the frame for a dialog.

use std::cell::RefCell;

use wry::dpi::{LogicalPosition, LogicalSize};
use wry::raw_window_handle::HasWindowHandle;
use wry::{Rect, WebContext, WebView, WebViewBuilder};


use crate::shell::PLANNER_URL as URL;

/// Where the frame lies in the window: left, top, width and height, in pixels before the display's
/// scale.
pub type Frame = [f32; 4];

fn rect([x, y, width, height]: Frame) -> Rect {
    Rect { position: LogicalPosition::new(x, y).into(), size: LogicalSize::new(width, height).into() }
}

thread_local! {
    /// The web view of this thread's window, with the context that holds its data folder.
    static VIEW: RefCell<Option<(WebView, WebContext)>> = const { RefCell::new(None) };
}

/// Lays the frame at `frame` in `window` and shows it, or hides it (`None`). The first frame
/// makes the web view and loads the page. To be called on the thread of the window.
pub fn place(window: &impl HasWindowHandle, frame: Option<Frame>) -> Result<(), String> {
    VIEW.with_borrow_mut(|view| {
        match (view.as_ref(), frame) {
            (None, None) => {}
            (Some((view, _)), None) => view.set_visible(false).map_err(|e| e.to_string())?,
            (Some((view, _)), Some(frame)) => {
                view.set_bounds(rect(frame)).map_err(|e| e.to_string())?;
                view.set_visible(true).map_err(|e| e.to_string())?;
            }
            (None, Some(frame)) => {
                // the web view's own files (cache, sign-in) go beside the app's other data, not
                // beside the program, where an installed app may not write
                let mut context = WebContext::new(data_folder());
                let made = WebViewBuilder::new_with_web_context(&mut context).with_url(URL).with_bounds(rect(frame)).build_as_child(window).map_err(|e| format!("Web frame: {e}"))?;
                *view = Some((made, context));
            }
        }
        Ok(())
    })
}

/// Loads the page again, when the frame is there.
pub fn reload() -> Result<(), String> {
    VIEW.with_borrow(|view| match view {
        Some((view, _)) => view.reload().map_err(|e| e.to_string()),
        None => Ok(()),
    })
}

fn data_folder() -> Option<std::path::PathBuf> {
    std::env::var_os("LOCALAPPDATA").map(|base| std::path::PathBuf::from(base).join("OpenAEC").join("Open BIM Suite").join("webview"))
}
