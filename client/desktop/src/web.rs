//! The browser side of the app (wasm32 only): `localStorage` for the prefs, picked files, saving
//! files, and in-page passkeys. The DOM calls live in `js/web.js`; this module is their typed
//! face, plus the registry that lets the path-based file code (attachments, avatars, emoji)
//! work on picked `File`s.
//!
//! The JS handles are `!Send`; every future that holds one is wrapped in [`send`], and
//! [`SendWrapper`] is sound because wasm32-unknown-unknown has a single thread.

use std::cell::RefCell;
use std::collections::HashMap;
use std::future::Future;
use std::path::{Path, PathBuf};

use rune::prefs::Prefs;
use send_wrapper::SendWrapper;
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;

use crate::app::App;
use thehideout_design::components::ToastKind;

#[wasm_bindgen(module = "/js/web.js")]
extern "C" {
    #[wasm_bindgen(js_name = origin)]
    fn js_origin() -> String;
    #[wasm_bindgen(js_name = storageGet)]
    fn storage_get(key: &str) -> Option<String>;
    #[wasm_bindgen(js_name = storageSet)]
    fn storage_set(key: &str, value: &str);
    #[wasm_bindgen(js_name = storageRemove)]
    fn storage_remove(key: &str);
    #[wasm_bindgen(js_name = userAgent)]
    fn user_agent() -> String;
    #[wasm_bindgen(js_name = pickFiles)]
    fn js_pick_files(accept: &str, multiple: bool) -> js_sys::Promise;
    #[wasm_bindgen(js_name = readBlob)]
    fn js_read_blob(blob: &web_sys::Blob) -> js_sys::Promise;
    #[wasm_bindgen(js_name = downloadText)]
    fn js_download_text(name: &str, text: &str);
    #[wasm_bindgen(js_name = downloadUrl)]
    fn js_download_url(url: &str, name: &str);
    #[wasm_bindgen(js_name = passkeyCreate, catch)]
    fn js_passkey_create(options_json: &str) -> Result<js_sys::Promise, JsValue>;
    #[wasm_bindgen(js_name = passkeyGet, catch)]
    fn js_passkey_get(options_json: &str) -> Result<js_sys::Promise, JsValue>;
}

/// Makes a future holding JS handles `Send`.
pub fn send<F: Future>(fut: F) -> SendWrapper<F> {
    SendWrapper::new(fut)
}

fn describe(e: JsValue) -> String {
    e.as_string()
        .or_else(|| e.dyn_ref::<js_sys::Error>().map(|e| String::from(e.message())))
        .unwrap_or_else(|| format!("{e:?}"))
}

/// The page's origin: the server this client talks to.
pub fn origin() -> String {
    js_origin()
}

// ---- the app handle ------------------------------------------------------------------------

thread_local! {
    static HANDLE: RefCell<Option<rune::AppHandle<App>>> = const { RefCell::new(None) };
}

/// Lets async browser work (a finished file chooser) reach the app.
pub fn set_handle(handle: rune::AppHandle<App>) {
    HANDLE.with(|h| *h.borrow_mut() = Some(handle));
}

fn with_app(f: impl FnOnce(&mut App) + Send + 'static) {
    HANDLE.with(|h| {
        if let Some(handle) = h.borrow().as_ref() {
            handle.update(move |a, _tx| f(a));
        }
    });
}

// ---- prefs in localStorage -----------------------------------------------------------------

/// Every prefs key the app uses; the store has no way to list its keys.
const PREF_KEYS: &[&str] = &["session", "appearance", "voice", "collapsed_sections", "collapsed_embeds", "recent_emojis"];

fn storage_key(key: &str) -> String {
    format!("thehideout.{key}")
}

/// The prefs as last saved in this browser.
pub fn load_prefs() -> Prefs {
    let mut prefs = Prefs::memory();
    for key in PREF_KEYS {
        if let Some(json) = storage_get(&storage_key(key))
            && let Ok(value) = serde_json::from_str::<serde_json::Value>(&json)
        {
            let _ = prefs.set(key, &value);
        }
    }
    prefs
}

/// Writes the prefs to `localStorage` (they are a few hundred bytes; the call is synchronous).
pub fn save_prefs(prefs: &Prefs) {
    for key in PREF_KEYS {
        match prefs.get::<serde_json::Value>(key) {
            Some(value) => storage_set(&storage_key(key), &value.to_string()),
            None => storage_remove(&storage_key(key)),
        }
    }
}

// ---- picked files --------------------------------------------------------------------------

thread_local! {
    static FILES: RefCell<HashMap<PathBuf, web_sys::File>> = RefCell::new(HashMap::new());
    static NEXT_FILE: RefCell<u64> = const { RefCell::new(0) };
}

/// A picked file as a path the path-based code can carry (`/web-files/<n>/<name>`); the file
/// itself stays in the browser and is looked up by that path.
fn register(file: web_sys::File) -> PathBuf {
    let n = NEXT_FILE.with(|n| {
        *n.borrow_mut() += 1;
        *n.borrow()
    });
    let path = Path::new("/web-files").join(n.to_string()).join(file.name());
    FILES.with(|f| f.borrow_mut().insert(path.clone(), file));
    path
}

/// The size of a picked file; `None` for a path that isn't one.
pub fn file_size(path: &Path) -> Option<u64> {
    FILES.with(|f| f.borrow().get(path).map(|f| f.size() as u64))
}

/// The picked file behind `path`, as a blob that can go to `fetch` or `XMLHttpRequest` as is.
pub fn file_blob(path: &Path) -> Option<SendWrapper<web_sys::Blob>> {
    FILES.with(|f| f.borrow().get(path).map(|f| SendWrapper::new(web_sys::Blob::from(JsValue::from(f.clone())))))
}

/// The bytes of a blob.
pub async fn read_blob(blob: &SendWrapper<web_sys::Blob>) -> Result<Vec<u8>, String> {
    send(async {
        let array = JsFuture::from(js_read_blob(blob)).await.map_err(describe)?;
        Ok(js_sys::Uint8Array::new(&array).to_vec())
    })
    .await
}

/// Opens the browser's file chooser (call from a press handler: it needs the user's gesture)
/// and hands the picked files, as registered paths, to `then`.
pub fn pick_files(accept: &'static str, multiple: bool, then: impl FnOnce(&mut App, Vec<PathBuf>) + Send + 'static) {
    let picking = js_pick_files(accept, multiple);
    wasm_bindgen_futures::spawn_local(async move {
        let picked = match JsFuture::from(picking).await {
            Ok(list) => js_sys::Array::from(&list),
            Err(e) => {
                let why = describe(e);
                return with_app(move |a| a.state.toast(ToastKind::Danger, format!("Can't open the file chooser. {why}")));
            }
        };
        let paths: Vec<PathBuf> = picked.iter().filter_map(|f| f.dyn_into::<web_sys::File>().ok()).map(register).collect();
        if !paths.is_empty() {
            with_app(move |a| then(a, paths));
        }
    });
}

// ---- saving ----------------------------------------------------------------------------------

/// Saves `text` as a file through the browser's download.
pub fn save_text(name: &str, text: &str) {
    js_download_text(name, text);
}

/// Downloads `url` (same origin) as `name`; the server's Content-Disposition names it too.
pub fn download_url(url: &str, name: &str) {
    js_download_url(url, name);
}

// ---- passkeys --------------------------------------------------------------------------------

/// Runs a WebAuthn ceremony in this page for the server's `options_json`: `create` to register a
/// passkey, else `get` to sign in. Resolves to the credential as JSON for `finish_*`.
pub async fn passkey(options_json: String, create: bool) -> Result<String, String> {
    send(async move {
        let promise = if create { js_passkey_create(&options_json) } else { js_passkey_get(&options_json) }.map_err(describe)?;
        let json = JsFuture::from(promise).await.map_err(describe)?;
        json.as_string().ok_or_else(|| "the browser returned no credential".to_owned())
    })
    .await
}

// ---- the device -------------------------------------------------------------------------------

/// "Chrome on macOS": what this browser is called in the account's session list.
pub fn device_label() -> String {
    let ua = user_agent();
    let browser = ["Edg/", "OPR/", "Firefox/", "Chrome/", "CriOS/", "Safari/"]
        .iter()
        .zip(["Edge", "Opera", "Firefox", "Chrome", "Chrome", "Safari"])
        .find(|(m, _)| ua.contains(**m))
        .map_or("a browser", |(_, name)| name);
    let os = [("iPhone", "iOS"), ("iPad", "iPadOS"), ("Android", "Android"), ("Windows", "Windows"), ("Mac OS", "macOS"), ("CrOS", "ChromeOS"), ("Linux", "Linux")]
        .iter()
        .find(|(m, _)| ua.contains(m))
        .map(|(_, name)| *name);
    match os {
        Some(os) => format!("thehideout web ({browser} on {os})"),
        None => format!("thehideout web ({browser})"),
    }
}
