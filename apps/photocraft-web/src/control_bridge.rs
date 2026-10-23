//! The page-script bridge: lets scripts on the page (the assistant panel) drive the app through
//! the same control methods as the desktop control channel (`docs/control-protocol.md`).
//!
//! The app exposes one function, `window.__photocraftNative.call(method, paramsJson)`, which
//! returns a Promise of the reply as a JSON string. The assistant's `photocraft-bridge.js` wraps
//! it as `window.photocraft.control(method, params)` with parsed objects.
//!
//! Requests travel the same channel as the desktop TCP transport ([`ControlRequest`]), so the
//! reply is exactly what `ui.inspect`, `engine.execute` and the rest reply there. The UI thread
//! handles requests between frames, so each call asks for a repaint and then polls its reply.

use std::sync::mpsc::{Sender, TryRecvError};

use photocraft_ui_egui::ControlRequest;
use wasm_bindgen::{JsValue, closure::Closure};

/// How often a pending call checks for its reply.
const POLL_MS: i32 = 16;
/// The same deadline as the desktop transport (`control::REPLY_DEADLINE`).
const TIMEOUT_MS: f64 = 60_000.0;
/// The global the bridge installs.
const GLOBAL: &str = "__photocraftNative";

/// Installs `window.__photocraftNative` and marks the app as ready for page scripts.
pub fn install(tx: Sender<ControlRequest>, ctx: egui::Context) {
    let Some(window) = web_sys::window() else { return };
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
    let api = js_sys::Object::new();
    let ok = js_sys::Reflect::set(&api, &JsValue::from_str("call"), call.as_ref()).is_ok()
        && js_sys::Reflect::set(&api, &JsValue::from_str("version"), &JsValue::from_str(env!("CARGO_PKG_VERSION"))).is_ok()
        && js_sys::Reflect::set(&window, &JsValue::from_str(GLOBAL), &api).is_ok();
    if !ok {
        log::error!("photocraft-web: could not install window.{GLOBAL}");
    }
    // The page keeps the function for the app's whole lifetime.
    call.forget();
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
