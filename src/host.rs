//! The OpenAEC SuperCloud around the web app: `window.openaecHost`, which the workplace page sets
//! before the app starts (see `docs/supercloud-koppeling.md` of the OpenAEC Blueprint).
//!
//! With it the model comes from Docs and goes back there: the file the page was opened for is
//! opened at start, and "Save as" stores the model next to it instead of downloading it. The
//! environment can also offer Blender (`openaecHost.blender`), which renders the file as it is
//! stored in Docs and puts the picture next to it. Without the object the web app works on its own.

use crate::loaders::Source;
use js_sys::{Function, Object, Promise, Reflect, Uint8Array};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_futures::JsFuture;

/// Whether a file from Docs is open: what Blender can work with.
static OPENED: AtomicBool = AtomicBool::new(false);

/// The size of a picture asked from Blender (pixels).
const RENDER_SIZE: (u32, u32) = (1920, 1080);

fn host() -> Option<JsValue> {
    Reflect::get(&js_sys::global(), &"openaecHost".into()).ok().filter(JsValue::is_object)
}

/// A member of `of` that is there.
fn member(of: &JsValue, name: &str) -> Option<JsValue> {
    Reflect::get(of, &name.into()).ok().filter(|v| !v.is_undefined() && !v.is_null())
}

fn blender() -> Option<JsValue> {
    member(&host()?, "blender").filter(JsValue::is_object)
}

/// Whether the app runs in an environment that stores its files.
pub fn present() -> bool {
    host().is_some()
}

/// Whether the environment offers Blender.
pub fn renders() -> bool {
    blender().is_some()
}

/// Whether Blender can render now: it works with the file opened from Docs.
pub fn can_render() -> bool {
    renders() && OPENED.load(Ordering::Relaxed)
}

/// What an error from the environment says.
fn said(e: JsValue) -> String {
    member(&e, "message").and_then(|m| m.as_string()).or_else(|| e.as_string()).unwrap_or_else(|| format!("{e:?}"))
}

/// Calls `of.name(args)` and waits for its answer.
async fn call(of: &JsValue, name: &str, args: &[JsValue]) -> Result<JsValue, String> {
    let function: Function = member(of, name).and_then(|f| f.dyn_into().ok()).ok_or_else(|| format!("The environment has no {name}"))?;
    let answer = function.apply(of, &args.iter().collect::<js_sys::Array>()).map_err(said)?;
    match answer.dyn_into::<Promise>() {
        Ok(promise) => JsFuture::from(promise).await.map_err(said),
        Err(value) => Ok(value),
    }
}

/// The file the page was opened for (`initialFileId`), if any: read from Docs.
pub async fn open_start() -> Option<Result<Source, String>> {
    let host = host()?;
    let id = member(&host, "initialFileId")?.as_string().filter(|id| !id.is_empty())?;
    Some(open(&host, &id).await)
}

async fn open(host: &JsValue, id: &str) -> Result<Source, String> {
    let file = call(host, "open", &[id.into()]).await?;
    let name = member(&file, "name").and_then(|n| n.as_string()).unwrap_or_else(|| "model".to_string());
    let bytes = member(&file, "bytes").ok_or_else(|| format!("Cannot open {name}: the environment gave no file"))?;
    OPENED.store(true, Ordering::Relaxed);
    Ok(Source::Bytes { name, data: Arc::new(Uint8Array::new(&bytes).to_vec()) })
}

/// Stores `bytes` as `name` in Docs: a new version of the file with that name, or a new file next
/// to the one opened (without one, the environment asks where).
pub async fn save(name: &str, bytes: &[u8]) -> Result<(), String> {
    let host = host().ok_or("No environment to save in")?;
    let options = Object::new();
    Reflect::set(&options, &"name".into(), &name.into()).map_err(said)?;
    call(&host, "save", &[Uint8Array::from(bytes).into(), options.into()]).await.map(|_| ())
}

/// Lets Blender render the file opened from Docs, as it is stored there; the name of the picture,
/// which stands next to the model in Docs.
pub async fn render() -> Result<String, String> {
    let blender = blender().ok_or("This environment has no Blender")?;
    let options = Object::new();
    Reflect::set(&options, &"width".into(), &RENDER_SIZE.0.into()).map_err(said)?;
    Reflect::set(&options, &"height".into(), &RENDER_SIZE.1.into()).map_err(said)?;
    let task = call(&blender, "run", &["render".into(), options.into()]).await?;
    Ok(member(&task, "name").and_then(|n| n.as_string()).unwrap_or_default())
}
