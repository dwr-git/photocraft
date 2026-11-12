//! The page-script bridge: lets scripts on the page (the assistant panel) drive the app through
//! the same control methods as the desktop control channel (`docs/control-protocol.md`).
//!
//! The app exposes one function, `window.__photocraftNative.call(method, paramsJson)`, which
//! returns a Promise of the reply as a JSON string. The assistant panel (`studio/web/studio.js`) wraps
//! it as `window.photocraft.control(method, params)` with parsed objects.
//!
//! Requests travel the same channel as the desktop TCP transport ([`ControlRequest`]), so the
//! reply is exactly what `ui.inspect`, `engine.execute` and the rest reply there. The UI thread
//! handles requests between frames, so each call asks for a repaint and then polls its reply.
//!
//! The bridge also carries the system clipboard's images, which the browser only hands to page
//! scripts (in a `paste` event): `setClipboardImage(width, height, rgba)` and
//! `clearClipboardImage()` fill the slot that `Services::clipboard_get_image` reads, so Edit ›
//! Paste and ⌘V paste screenshots and images copied in other apps, as on the desktop. Copies go
//! the other way through the page's `window.__studioClipboardWrite(width, height, rgba)`.
//! `wantsKeyboard()` tells the page whether a text field has the keyboard (its ⌘V pastes text).
//! `setBackdropFrame(left, top, width, height, pageWidth, pageHeight)` lays the Brand theme's
//! backdrop out over the whole page rather than the canvas alone. `currentTheme()` is the id of
//! the theme on screen, which the panel follows.
//!
//! The page's own files («Мои файлы» on the server): `open(name, bytes)` opens a file as a
//! dropped one is (`Services::inbox`). While the page has set `takesSaves = true`, saves of
//! documents reach it as a `photocraft:save` event (`detail: {name, path, token, saveAs, bytes}`)
//! instead of downloading, and `savedToServer(token)` marks the document saved once the server
//! has it (`crate::server_saves`).

use std::sync::mpsc::{Sender, TryRecvError};
use std::sync::{Arc, Mutex, PoisonError};

use photocraft_ui_egui::ControlRequest;
use wasm_bindgen::{JsCast as _, JsValue, closure::Closure};

use crate::server_saves::{self, Ledger};

/// How often a pending call checks for its reply.
const POLL_MS: i32 = 16;
/// The same deadline as the desktop transport (`control::REPLY_DEADLINE`).
const TIMEOUT_MS: f64 = 60_000.0;
/// The global the bridge installs.
const GLOBAL: &str = "__photocraftNative";
/// The page function that puts a copied image on the system clipboard.
const CLIPBOARD_WRITE: &str = "__studioClipboardWrite";
/// The largest clipboard image the page may hand over: 16k × 16k RGBA (1 GiB).
const MAX_CLIP_BYTES: usize = 16_384 * 16_384 * 4;
/// The largest file the page may open at once (`open`).
const MAX_OPEN_BYTES: u32 = 1 << 30;
/// The event a document save reaches the page as, while it takes saves.
const SAVE_EVENT: &str = "photocraft:save";

/// Saves handed to the page, waiting for the server (`crate::server_saves`).
pub type Saves = Arc<Mutex<Ledger<photocraft_doc::DocId>>>;
/// Files to open as documents (`Services::inbox`).
pub type Inbox = Arc<Mutex<Vec<(String, Vec<u8>)>>>;

/// The system clipboard's image as the page last saw it: `(width, height, RGBA8)`.
pub type ClipSlot = Arc<Mutex<Option<(u32, u32, Vec<u8>)>>>;

/// `Services::clipboard_get_image` and `clipboard_set_image` for the browser, over `slot`.
pub fn clipboard_services(slot: &ClipSlot) -> (photocraft_ui_egui::ClipboardGetFn, photocraft_ui_egui::ClipboardSetFn) {
    let read = slot.clone();
    let get: photocraft_ui_egui::ClipboardGetFn = Box::new(move || Ok(read.lock().unwrap_or_else(PoisonError::into_inner).clone()));
    let write = slot.clone();
    let set: photocraft_ui_egui::ClipboardSetFn = Box::new(move |w, h, rgba| {
        // Remember what we copied, so a later paste of the same image keeps the editable copy.
        *write.lock().unwrap_or_else(PoisonError::into_inner) = Some((w, h, rgba.to_vec()));
        let Some(window) = web_sys::window() else { return Ok(()) };
        let hook = js_sys::Reflect::get(&window, &JsValue::from_str(CLIPBOARD_WRITE)).unwrap_or(JsValue::UNDEFINED);
        if let Some(f) = wasm_bindgen::JsCast::dyn_ref::<js_sys::Function>(&hook) {
            // The page writes asynchronously; a refusal (no HTTPS, no permission) leaves the copy
            // inside the app, where Edit › Paste still finds it.
            let _ = f.call3(&JsValue::NULL, &JsValue::from(w), &JsValue::from(h), &js_sys::Uint8Array::from(rgba).into());
        }
        Ok(())
    });
    (get, set)
}

/// Hands a save of `bytes` to `path` to the page when it takes saves and `path` is a document
/// (`server_saves::is_document`) → whether it did; otherwise the caller downloads the file.
/// `save_as`: the save came through a save dialog (Save As, or the first Save of a new
/// document), so the page asks for the name on the server.
pub fn hand_save(saves: &Saves, path: &str, bytes: &[u8], save_as: bool) -> bool {
    if !server_saves::is_document(path) || !takes_saves() {
        return false;
    }
    let token = saves.lock().unwrap_or_else(PoisonError::into_inner).hand(path);
    match dispatch_save(path, bytes, token, save_as) {
        Ok(()) => true,
        Err(e) => {
            // The page never got it: download it as before, so nothing is lost.
            log::warn!("photocraft-web: could not hand the save of {path} to the page: {e}");
            false
        }
    }
}

/// Whether the page said it keeps saves (`window.__photocraftNative.takesSaves = true`).
fn takes_saves() -> bool {
    web_sys::window()
        .and_then(|w| js_sys::Reflect::get(&w, &JsValue::from_str(GLOBAL)).ok())
        .filter(JsValue::is_object)
        .and_then(|api| js_sys::Reflect::get(&api, &JsValue::from_str("takesSaves")).ok())
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
}

fn dispatch_save(path: &str, bytes: &[u8], token: u64, save_as: bool) -> Result<(), String> {
    let js = |e: JsValue| format!("{e:?}");
    let window = web_sys::window().ok_or("no window")?;
    let detail = js_sys::Object::new();
    let put = |k: &str, v: &JsValue| js_sys::Reflect::set(&detail, &JsValue::from_str(k), v).map(|_| ()).map_err(js);
    put("name", &JsValue::from_str(server_saves::file_name(path)))?;
    put("path", &JsValue::from_str(path))?;
    // Below 2^53 (`Ledger::hand`): exact as a JavaScript number.
    put("token", &JsValue::from_f64(token as f64))?;
    put("saveAs", &JsValue::from_bool(save_as))?;
    put("bytes", &js_sys::Uint8Array::from(bytes).into())?;
    let init = web_sys::CustomEventInit::new();
    init.set_detail(&detail);
    let event = web_sys::CustomEvent::new_with_event_init_dict(SAVE_EVENT, &init).map_err(js)?;
    window.dispatch_event(&event).map(|_| ()).map_err(js)
}

/// Installs `window.__photocraftNative` and marks the app as ready for page scripts.
pub fn install(tx: Sender<ControlRequest>, ctx: egui::Context, clip: ClipSlot, inbox: Inbox, saves: Saves) {
    let Some(window) = web_sys::window() else { return };
    let open_ctx = ctx.clone();
    let saved_ctx = ctx.clone();
    let keyboard_ctx = ctx.clone();
    let call = Closure::<dyn FnMut(String, String) -> js_sys::Promise>::new(move |method: String, params: String| {
        let params: serde_json::Value = if params.trim().is_empty() {
            serde_json::Value::Null
        } else {
            match serde_json::from_str(&params) {
                Ok(v) => v,
                Err(e) => return js_sys::Promise::reject(&JsValue::from_str(&format!("params are not valid JSON: {e}"))),
            }
        };
        let (req, rx) = ControlRequest::new(method, params);
        if tx.send(req).is_err() {
            return js_sys::Promise::reject(&JsValue::from_str("PhotoCraft is not running"));
        }
        ctx.request_repaint();
        let ctx = ctx.clone();
        wasm_bindgen_futures::future_to_promise(async move {
            let started = js_sys::Date::now();
            loop {
                match rx.try_recv() {
                    Ok(reply) => return Ok(JsValue::from_str(&reply.to_string())),
                    Err(TryRecvError::Disconnected) => return Err(JsValue::from_str("PhotoCraft dropped the request")),
                    Err(TryRecvError::Empty) => {}
                }
                if js_sys::Date::now() - started > TIMEOUT_MS {
                    return Err(JsValue::from_str(&photocraft_ui_egui::control::timeout_error()));
                }
                ctx.request_repaint();
                sleep(POLL_MS).await;
            }
        })
    });
    let slot = clip.clone();
    let set_clip = Closure::<dyn FnMut(u32, u32, js_sys::Uint8Array) -> bool>::new(move |w: u32, h: u32, rgba: js_sys::Uint8Array| {
        let expected = (w as usize).checked_mul(h as usize).and_then(|n| n.checked_mul(4));
        let ok = w > 0 && h > 0 && expected.is_some_and(|n| n <= MAX_CLIP_BYTES && n == rgba.length() as usize);
        if ok {
            *slot.lock().unwrap_or_else(PoisonError::into_inner) = Some((w, h, rgba.to_vec()));
        }
        ok
    });
    let clear_clip = Closure::<dyn FnMut()>::new(move || {
        *clip.lock().unwrap_or_else(PoisonError::into_inner) = None;
    });
    let frame_ctx = keyboard_ctx.clone();
    let theme_ctx = keyboard_ctx.clone();
    // The theme on screen, however it was chosen (Window › Theme, Preferences, the appearance
    // button, the system appearance): its id, as `window.theme.<id>` names it.
    let current_theme = Closure::<dyn FnMut() -> String>::new(move || photocraft_ui_egui::theme::Tokens::get(&theme_ctx).kind.id().to_string());
    let wants_keyboard = Closure::<dyn FnMut() -> bool>::new(move || keyboard_ctx.text_edit_focused());
    // The page's viewport around the canvas (CSS px: the canvas's left and top in the page, its
    // size, the page's size), so the Brand backdrop lines up with the page's own CSS gradient.
    let set_frame = Closure::<dyn FnMut(f32, f32, f32, f32, f32, f32)>::new(move |left: f32, top: f32, cw: f32, ch: f32, vw: f32, vh: f32| {
        let screen = frame_ctx.content_rect();
        let finite = [left, top, cw, ch, vw, vh].iter().all(|v| v.is_finite());
        if !finite || cw < 1.0 || ch < 1.0 || vw < 1.0 || vh < 1.0 || !screen.is_positive() {
            return;
        }
        let (sx, sy) = (screen.width() / cw, screen.height() / ch);
        let frame = egui::Rect::from_min_size(
            egui::pos2(screen.left() - left * sx, screen.top() - top * sy),
            egui::vec2(vw * sx, vh * sy),
        );
        frame_ctx.data_mut(|d| d.insert_temp(egui::Id::new(photocraft_ui_egui::theme::BACKDROP_FRAME), frame));
        frame_ctx.request_repaint();
    });
    // A file from the page (the user's files on the server): opened as a dropped file is.
    let open = Closure::<dyn FnMut(String, JsValue) -> bool>::new(move |name: String, bytes: JsValue| {
        let Some(bytes) = bytes.dyn_ref::<js_sys::Uint8Array>() else { return false };
        if name.trim().is_empty() || bytes.length() == 0 || bytes.length() > MAX_OPEN_BYTES {
            return false;
        }
        inbox.lock().unwrap_or_else(PoisonError::into_inner).push((name, bytes.to_vec()));
        open_ctx.request_repaint();
        true
    });
    // The server has the save the page was handed with `token`: its document is saved.
    let saved_to_server = Closure::<dyn FnMut(f64)>::new(move |token: f64| {
        if token.is_finite() && token >= 1.0 && token < 9_007_199_254_740_992.0 && token.fract() == 0.0 {
            saves.lock().unwrap_or_else(PoisonError::into_inner).confirm(token as u64);
            saved_ctx.request_repaint();
        }
    });
    let api = js_sys::Object::new();
    let ok = js_sys::Reflect::set(&api, &JsValue::from_str("call"), call.as_ref()).is_ok()
        && js_sys::Reflect::set(&api, &JsValue::from_str("open"), open.as_ref()).is_ok()
        && js_sys::Reflect::set(&api, &JsValue::from_str("savedToServer"), saved_to_server.as_ref()).is_ok()
        && js_sys::Reflect::set(&api, &JsValue::from_str("takesSaves"), &JsValue::FALSE).is_ok()
        && js_sys::Reflect::set(&api, &JsValue::from_str("setClipboardImage"), set_clip.as_ref()).is_ok()
        && js_sys::Reflect::set(&api, &JsValue::from_str("clearClipboardImage"), clear_clip.as_ref()).is_ok()
        && js_sys::Reflect::set(&api, &JsValue::from_str("wantsKeyboard"), wants_keyboard.as_ref()).is_ok()
        && js_sys::Reflect::set(&api, &JsValue::from_str("setBackdropFrame"), set_frame.as_ref()).is_ok()
        && js_sys::Reflect::set(&api, &JsValue::from_str("currentTheme"), current_theme.as_ref()).is_ok()
        && js_sys::Reflect::set(&api, &JsValue::from_str("version"), &JsValue::from_str(env!("CARGO_PKG_VERSION"))).is_ok()
        && js_sys::Reflect::set(&window, &JsValue::from_str(GLOBAL), &api).is_ok();
    if !ok {
        log::error!("photocraft-web: could not install window.{GLOBAL}");
    }
    // The page keeps the functions for the app's whole lifetime.
    call.forget();
    set_clip.forget();
    clear_clip.forget();
    wants_keyboard.forget();
    set_frame.forget();
    current_theme.forget();
    open.forget();
    saved_to_server.forget();
}

/// Resolves after `ms` milliseconds (a `setTimeout` promise).
async fn sleep(ms: i32) {
    let promise = js_sys::Promise::new(&mut |resolve, _reject| {
        if let Some(window) = web_sys::window() {
            let _ = window.set_timeout_with_callback_and_timeout_and_arguments_0(&resolve, ms);
        }
    });
    let _ = wasm_bindgen_futures::JsFuture::from(promise).await;
}
