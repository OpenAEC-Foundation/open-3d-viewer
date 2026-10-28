//! The web app: the same viewer in the browser (wasm32, WebGPU), built with `trunk build`.
//!
//! What differs from the desktop: files come from the browser's file picker, from drag and drop
//! onto the page or from a `?file=URL` link, as bytes; "Save as" becomes a download; the
//! preferences are not kept; there is no remote control.

use crate::app::{App, Message};
use crate::loaders::Source;
use crate::shell;
use iced::futures::channel::mpsc;
use iced::futures::{SinkExt, Stream};
use std::sync::Arc;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::JsFuture;

/// Starts the app in the page.
pub fn main() {
    console_error_panic_hook::set_once();
    if let Err(e) = openaec_ui::run(shell::Studio { app: App::default() }, None) {
        web_sys::console::error_1(&format!("Open 3D Viewer: {e}").into());
    }
}

/// Waits until the browser has drawn the next frame, so a status change shows before a long,
/// blocking step (the browser has no worker threads here).
pub async fn next_frame() {
    let promise = js_sys::Promise::new(&mut |resolve, _| {
        if let Some(window) = web_sys::window() {
            // two animation frames: the first runs before paint, the second after it
            let inner = Closure::once_into_js(move || {
                let _ = web_sys::window().map(|w| w.request_animation_frame(resolve.unchecked_ref()));
            });
            let _ = window.request_animation_frame(inner.unchecked_ref());
        }
    });
    let _ = JsFuture::from(promise).await;
}

/// Hands `bytes` to the browser as a download named `name`.
pub fn download(name: &str, bytes: &[u8], mime: &str) -> Result<(), String> {
    let js = |e: JsValue| format!("Download {name}: {e:?}");
    let parts = js_sys::Array::of1(&js_sys::Uint8Array::from(bytes));
    let options = web_sys::BlobPropertyBag::new();
    options.set_type(mime);
    let blob = web_sys::Blob::new_with_u8_array_sequence_and_options(&parts, &options).map_err(js)?;
    let url = web_sys::Url::create_object_url_with_blob(&blob).map_err(js)?;
    let document = web_sys::window().and_then(|w| w.document()).ok_or("no document")?;
    let link: web_sys::HtmlAnchorElement = document.create_element("a").map_err(js)?.unchecked_into();
    link.set_href(&url);
    link.set_download(name);
    link.click();
    web_sys::Url::revoke_object_url(&url).map_err(js)
}

/// Files that arrive from the page: the one in `?file=URL` at start, then every file dropped
/// onto the page.
pub fn files() -> impl Stream<Item = Message> {
    let (tx, rx) = mpsc::unbounded::<Message>();
    if let Some(url) = start_url() {
        let mut tx = tx.clone();
        wasm_bindgen_futures::spawn_local(async move {
            let message = match fetch(&url).await {
                Ok(source) => Message::Picked(Some(source)),
                Err(e) => Message::Loaded(Err(e)),
            };
            let _ = tx.send(message).await;
        });
    }
    listen_for_drops(tx);
    rx
}

/// The `file` parameter of the page address, e.g. `?file=samples/house.skp`.
fn start_url() -> Option<String> {
    let search = web_sys::window()?.location().search().ok()?;
    let params = web_sys::UrlSearchParams::new_with_str(&search).ok()?;
    params.get("file").filter(|f| !f.is_empty())
}

/// Downloads a model file.
async fn fetch(url: &str) -> Result<Source, String> {
    let fail = |e: JsValue| format!("Cannot open {url}: {}", e.as_string().unwrap_or_else(|| format!("{e:?}")));
    let window = web_sys::window().ok_or("no window")?;
    let response: web_sys::Response = JsFuture::from(window.fetch_with_str(url)).await.map_err(fail)?.unchecked_into();
    if !response.ok() {
        return Err(format!("Cannot open {url}: HTTP {}", response.status()));
    }
    let buffer = JsFuture::from(response.array_buffer().map_err(fail)?).await.map_err(fail)?;
    let name = url.rsplit('/').next().unwrap_or(url).split(['?', '#']).next().unwrap_or_default().to_string();
    Ok(Source::Bytes { name, data: Arc::new(js_sys::Uint8Array::new(&buffer).to_vec()) })
}

/// Opens files dropped onto the page (the browser gives the window no file drop events).
fn listen_for_drops(tx: mpsc::UnboundedSender<Message>) {
    let Some(window) = web_sys::window() else { return };
    // without this the browser opens the file itself
    let over = Closure::<dyn FnMut(web_sys::DragEvent)>::new(|e: web_sys::DragEvent| e.prevent_default());
    let drop = Closure::<dyn FnMut(web_sys::DragEvent)>::new(move |e: web_sys::DragEvent| {
        e.prevent_default();
        let Some(file) = e.data_transfer().and_then(|d| d.files()).and_then(|f| f.get(0)) else { return };
        let mut tx = tx.clone();
        wasm_bindgen_futures::spawn_local(async move {
            let name = file.name();
            let message = match JsFuture::from(file.array_buffer()).await {
                Ok(buffer) => Message::Picked(Some(Source::Bytes { name, data: Arc::new(js_sys::Uint8Array::new(&buffer).to_vec()) })),
                Err(e) => Message::Loaded(Err(format!("Cannot read {name}: {e:?}"))),
            };
            let _ = tx.send(message).await;
        });
    });
    let _ = window.add_event_listener_with_callback("dragover", over.as_ref().unchecked_ref());
    let _ = window.add_event_listener_with_callback("drop", drop.as_ref().unchecked_ref());
    // the listeners live as long as the page
    over.forget();
    drop.forget();
}

/// The subscription of the web app (see [`files`]).
pub fn subscription() -> iced::Subscription<Message> {
    iced::Subscription::run(files)
}
