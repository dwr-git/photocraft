//! Starts Photocraft in the browser from compressed parts of its WebAssembly.
//!
//! Static hosts cap the size of one file (Cloudflare Pages: 25 MiB), and the app's wasm is at
//! that size. `packaging/web/package.sh` gzips it and cuts it into parts listed in
//! `app/parts.txt`. This small module is what the packaged page loads: it fetches the parts in
//! order, joins and inflates them, and hands the WebAssembly to the app's own generated loader.
//! No file on the host is larger than one part, however large the app grows.
//!
//! The browser inflates the parts (`DecompressionStream`: Safari 16.4, Firefox 113, Chrome 80).
//! In one that can't, this module inflates them itself, a little slower. Some servers recognise
//! the gzip in a part and send it with `Content-Encoding: gzip` (Apache's `mod_mime_magic`
//! labels it `x-gzip`), so the browser has inflated it already: the joined download is then
//! taken as it is, by its first bytes.
//!
//! A download that fails is tried again, the later tries past the browser's cache (a bad copy
//! there would otherwise fail forever). When every try fails the page says so and offers a reload.
//!
//! `trunk build` in `apps/photocraft-web` still gives the app as one `.wasm`, for development and
//! for hosts without a per-file cap; only the packaged site starts through this module.

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

/// First line of `parts.txt`. Some hosts answer a missing file with the home page and HTTP 200;
/// without this an HTML page would be read as a list of parts.
#[cfg(any(target_arch = "wasm32", test))]
const MARKER: &str = "photocraft-parts 1";

/// The part names in a `parts.txt`, in order, or `None` when the text isn't one. A name is a
/// file next to `parts.txt`: anything that could point elsewhere is refused.
#[cfg(any(target_arch = "wasm32", test))]
fn parts(list: &str) -> Option<Vec<String>> {
    let mut lines = list.lines().map(str::trim).filter(|l| !l.is_empty());
    if lines.next() != Some(MARKER) {
        return None;
    }
    // Lines starting with `#` are notes (packaging writes the build number there, so the file's
    // size changes with every build and upload tools that skip same-size files still replace it).
    let parts: Vec<String> = lines.filter(|l| !l.starts_with('#')).map(str::to_owned).collect();
    let plain = |p: &String| !p.starts_with('.') && !p.contains(['/', '\\', '?', '#', ':', '%']);
    (!parts.is_empty() && parts.iter().all(plain)).then_some(parts)
}

/// What the joined parts hold, told by their first bytes.
#[cfg(any(target_arch = "wasm32", test))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Payload {
    /// The gzip packaging made, still to be inflated.
    Gzip,
    /// The app's WebAssembly: the server sent the gzip with a `Content-Encoding` the browser
    /// already undid.
    Wasm,
}

/// Enough leading bytes to tell the payloads apart.
#[cfg(any(target_arch = "wasm32", test))]
const HEAD: usize = 4;

/// What a download starting with `head` (at least `HEAD` bytes, or all there were) holds, or
/// `None` when it is neither, such as a web page or nothing at all.
#[cfg(any(target_arch = "wasm32", test))]
fn payload(head: &[u8]) -> Option<Payload> {
    if head.starts_with(&[0x1f, 0x8b]) {
        Some(Payload::Gzip)
    } else if head.starts_with(b"\0asm") {
        Some(Payload::Wasm)
    } else {
        None
    }
}

/// Gzip inflated a chunk at a time, for browsers that can't do it themselves.
#[cfg(any(target_arch = "wasm32", test))]
struct Inflater(flate2::write::GzDecoder<Vec<u8>>);

#[cfg(any(target_arch = "wasm32", test))]
impl Inflater {
    fn new() -> Self {
        Inflater(flate2::write::GzDecoder::new(Vec::new()))
    }

    /// What the next `chunk` of the gzip stream inflates to (often nothing: the decoder holds on
    /// to input until it has a whole block).
    fn push(&mut self, chunk: &[u8]) -> std::io::Result<Vec<u8>> {
        std::io::Write::write_all(&mut self.0, chunk)?;
        Ok(std::mem::take(self.0.get_mut()))
    }

    /// The rest, once the stream has ended. An error when it ended early or its checksum is
    /// wrong, the same as the browser's inflater gives.
    fn finish(&mut self) -> std::io::Result<Vec<u8>> {
        self.0.try_finish()?;
        Ok(std::mem::take(self.0.get_mut()))
    }
}

#[cfg(target_arch = "wasm32")]
mod boot {
    use wasm_bindgen::prelude::*;
    use wasm_bindgen_futures::{JsFuture, spawn_local};
    use web_sys::{ReadableWritablePair, RequestCache, RequestInit, Response};

    use super::{HEAD, Inflater, Payload};

    /// Where packaging puts the app, next to this module's own files.
    const PARTS: &str = "./app/parts.txt";
    /// Seconds to wait before each further try; one more than this many tries are made.
    const RETRY_AFTER: [u32; 2] = [1, 3];

    // The app's `wasm-bindgen` loader. Its default export instantiates the module it is given and
    // runs the app's `main`. The path is fixed at compile time, so packaging gives the file this
    // name (Trunk's own name for it carries a content hash).
    #[wasm_bindgen(raw_module = "./app/ai-photoshop.js")]
    extern "C" {
        #[wasm_bindgen(js_name = default, catch)]
        async fn start_app(module: JsValue) -> Result<JsValue, JsValue>;
    }

    // The browser's own inflater. `web-sys` has it only behind its unstable-API flag, which would
    // have to be set for the whole build, so the three members used are declared here.
    #[wasm_bindgen]
    extern "C" {
        type DecompressionStream;
        #[wasm_bindgen(constructor, catch)]
        fn new(format: &str) -> Result<DecompressionStream, JsValue>;
        #[wasm_bindgen(method, getter)]
        fn readable(this: &DecompressionStream) -> web_sys::ReadableStream;
        #[wasm_bindgen(method, getter)]
        fn writable(this: &DecompressionStream) -> web_sys::WritableStream;
    }

    fn window() -> Result<web_sys::Window, JsValue> {
        web_sys::window().ok_or_else(|| JsValue::from_str("no window"))
    }

    /// `fresh` skips the browser's cache.
    async fn fetch(url: &str, fresh: bool) -> Result<Response, JsValue> {
        let init = RequestInit::new();
        if fresh {
            init.set_cache(RequestCache::Reload);
        }
        let response: Response = JsFuture::from(window()?.fetch_with_str_and_init(url, &init)).await?.dyn_into()?;
        if !response.ok() {
            return Err(JsValue::from_str(&format!("{url}: HTTP {}", response.status())));
        }
        if response.headers().get("Content-Type")?.is_some_and(|t| t.starts_with("text/html")) {
            return Err(JsValue::from_str(&format!("{url}: the server sent a web page, not the file")));
        }
        Ok(response)
    }

    /// Every part, downloaded whole and in order, as one `Blob`.
    ///
    /// The earlier version piped the parts through a chain of streams into the compiler while
    /// they downloaded. On a slow connection that chain could stop pulling and the page waited
    /// forever; whole downloads cost a second of overlap at most and cannot stall that way.
    async fn download(parts: &[String], fresh: bool) -> Result<web_sys::Blob, JsValue> {
        let buffers = js_sys::Array::new();
        for part in parts {
            let response = fetch(&format!("./app/{part}"), fresh).await?;
            let buffer = JsFuture::from(response.array_buffer()?).await?;
            buffers.push(&buffer);
        }
        web_sys::Blob::new_with_buffer_source_sequence(&buffers)
    }

    async fn bytes_of(blob: &web_sys::Blob) -> Result<js_sys::Uint8Array, JsValue> {
        Ok(js_sys::Uint8Array::new(&JsFuture::from(blob.array_buffer()).await?))
    }

    /// The app's WebAssembly from the joined parts: inflated, unless its first bytes show the
    /// server sent it already inflated (a `Content-Encoding` the browser undid).
    async fn unpack(joined: web_sys::Blob) -> Result<js_sys::Uint8Array, JsValue> {
        let head = bytes_of(&joined.slice_with_i32_and_i32(0, i32::try_from(HEAD).unwrap_or(4))?).await?.to_vec();
        match super::payload(&head) {
            Some(Payload::Wasm) => bytes_of(&joined).await,
            Some(Payload::Gzip) => {
                if js_sys::Reflect::has(&js_sys::global(), &JsValue::from_str("DecompressionStream"))? {
                    let inflater = DecompressionStream::new("gzip")?;
                    let inflated = joined.stream().pipe_through(&ReadableWritablePair::new(&inflater.readable(), &inflater.writable()));
                    let response = Response::new_with_opt_readable_stream(Some(&inflated))?;
                    let buffer = JsFuture::from(response.array_buffer()?).await.map_err(|e| JsValue::from_str(&format!("the download is damaged ({e:?})")))?;
                    Ok(js_sys::Uint8Array::new(&buffer))
                } else {
                    let mut inflater = Inflater::new();
                    let damaged = |e: std::io::Error| JsValue::from_str(&format!("the download is damaged ({e})"));
                    let mut wasm = inflater.push(&bytes_of(&joined).await?.to_vec()).map_err(damaged)?;
                    wasm.extend(inflater.finish().map_err(damaged)?);
                    Ok(js_sys::Uint8Array::from(wasm.as_slice()))
                }
            }
            None => Err(JsValue::from_str("the download is not the app's WebAssembly")),
        }
    }

    async fn run(fresh: bool) -> Result<(), JsValue> {
        let list = JsFuture::from(fetch(PARTS, fresh).await?.text()?).await?.as_string().unwrap_or_default();
        let parts = super::parts(&list).ok_or_else(|| JsValue::from_str("app/parts.txt is not a parts list"))?;
        let wasm = unpack(download(&parts, fresh).await?).await?;
        let arg = js_sys::Object::new();
        js_sys::Reflect::set(&arg, &JsValue::from_str("module_or_path"), &wasm)?;
        start_app(arg.into()).await.map(|_| ())
    }

    async fn sleep(seconds: u32) {
        let wait = js_sys::Promise::new(&mut |done, _| {
            let ms = i32::try_from(seconds.saturating_mul(1000)).unwrap_or(i32::MAX);
            if window().and_then(|w| w.set_timeout_with_callback_and_timeout_and_arguments_0(&done, ms)).is_err() {
                let _ = done.call0(&JsValue::NULL);
            }
        });
        let _ = JsFuture::from(wait).await;
    }

    /// Replaces the page's "Loading" text. `reload` adds a link that reloads the page.
    fn say(text: &str, reload: bool) {
        let Some(document) = web_sys::window().and_then(|w| w.document()) else { return };
        let Some(loading) = document.get_element_by_id("photocraft_loading") else { return };
        loading.set_text_content(None);
        let Ok(line) = document.create_element("p") else { return };
        line.set_text_content(Some(text));
        if reload && let Ok(link) = document.create_element("a") {
            link.set_text_content(Some("Обновить страницу"));
            let _ = link.set_attribute("href", "");
            let _ = line.append_child(&link);
        }
        let _ = loading.append_child(&line);
    }

    pub fn start() {
        spawn_local(async {
            let mut attempt = 0;
            loop {
                let error = match run(attempt > 0).await {
                    Ok(()) => return,
                    Err(e) => e,
                };
                web_sys::console::error_2(&JsValue::from_str("ai-photoshop-boot: could not start AI-Photoshop:"), &error);
                let Some(&wait) = RETRY_AFTER.get(attempt) else {
                    say("AI-Photoshop не скачался с сервера. Проверьте, что папка скопирована целиком, и попробуйте ещё раз: ", true);
                    return;
                };
                say("Загрузка AI-Photoshop\u{2026} (скачать не вышло, пробую ещё раз)", false);
                sleep(wait).await;
                attempt += 1;
            }
        });
    }
}

#[cfg(target_arch = "wasm32")]
fn main() {
    boot::start();
}

#[cfg(not(target_arch = "wasm32"))]
fn main() {
    eprintln!("photocraft-web-boot only runs in the browser: packaging/web/package.sh builds it with `trunk build --release` in apps/photocraft-web-boot");
}

#[cfg(test)]
mod tests {
    use super::{HEAD, Inflater, MARKER, Payload, parts, payload};

    fn gzip(bytes: &[u8]) -> Vec<u8> {
        let mut z = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        std::io::Write::write_all(&mut z, bytes).expect("gzip");
        z.finish().expect("gzip")
    }

    /// Bytes that don't compress to nothing, so the gzip stream spans many chunks.
    fn sample() -> Vec<u8> {
        let mut x = 0x2545_f491_4f6c_dd1d_u64;
        (0..200_000)
            .map(|i| {
                x ^= x << 13;
                x ^= x >> 7;
                x ^= x << 17;
                if i % 3 == 0 { x as u8 } else { (i / 7) as u8 }
            })
            .collect()
    }

    /// The parts arrive in whatever pieces the network gives; the inflater must not care where
    /// they are cut.
    #[test]
    fn the_inflater_gives_back_the_bytes_however_the_stream_is_cut() {
        let (plain, packed) = (sample(), gzip(&sample()));
        for cut in [1, 7, 4096, 65_536, packed.len()] {
            let mut inflater = Inflater::new();
            let mut out = Vec::new();
            for chunk in packed.chunks(cut) {
                out.extend(inflater.push(chunk).expect("push"));
            }
            out.extend(inflater.finish().expect("finish"));
            assert_eq!(out, plain, "chunks of {cut}");
        }
    }

    /// A download that stops early, or whose bytes changed on the way, must fail (the boot module
    /// then tries again), never start a damaged app.
    #[test]
    fn the_inflater_refuses_a_short_or_damaged_stream() {
        let packed = gzip(&sample());
        let run = |bytes: &[u8]| -> std::io::Result<Vec<u8>> {
            let mut inflater = Inflater::new();
            let mut out = inflater.push(bytes)?;
            out.extend(inflater.finish()?);
            Ok(out)
        };
        assert!(run(&packed).is_ok());
        assert!(run(&packed[..packed.len() / 2]).is_err(), "cut short");
        assert!(run(&packed[..packed.len() - 4]).is_err(), "trailer cut");
        let mut damaged = packed.clone();
        let middle = damaged.len() / 2;
        damaged[middle] ^= 0x55;
        assert!(run(&damaged).is_err(), "a changed byte");
        assert!(run(b"<!DOCTYPE html><html>not gzip</html>").is_err(), "a web page");
        assert!(run(b"").is_err(), "nothing at all");
    }

    /// A server that recognises the gzip (Apache's `mod_mime_magic`) sends it with
    /// `Content-Encoding: x-gzip`, and the browser inflates it on the way (#7639). The joined parts
    /// are then the app's WebAssembly, which must start as it is, not be inflated a second time.
    #[test]
    fn the_first_bytes_tell_gzip_from_wasm_the_browser_already_inflated() {
        let mut wasm = b"\0asm\x01\0\0\0".to_vec();
        wasm.extend(sample());
        let packed = gzip(&wasm);
        assert_eq!(payload(&packed[..HEAD]), Some(Payload::Gzip), "as packaged");
        assert_eq!(payload(&wasm[..HEAD]), Some(Payload::Wasm), "inflated by the browser");
        assert_eq!(payload(&packed[..2]), Some(Payload::Gzip), "a download shorter than HEAD");
        assert_eq!(payload(b"<!DOCTYPE html>"), None, "a web page");
        assert_eq!(payload(b"\0as"), None, "a cut-off wasm header");
        assert_eq!(payload(b""), None, "nothing at all");
    }

    #[test]
    fn a_parts_list_gives_its_names_in_order() {
        let list = format!("{MARKER}\nphotocraft-web-0123abcd.part-aa\r\n\n  photocraft-web-0123abcd.part-ab  \n");
        assert_eq!(parts(&list), Some(vec!["photocraft-web-0123abcd.part-aa".to_owned(), "photocraft-web-0123abcd.part-ab".to_owned()]));
    }

    /// A host that answers a missing file with its home page must not have that page read as a
    /// list of parts.
    #[test]
    fn a_web_page_is_not_a_parts_list() {
        assert_eq!(parts(include_str!("../index.html")), None);
        assert_eq!(parts(""), None);
        assert_eq!(parts("photocraft-web.part-aa\n"), None, "no marker");
        assert_eq!(parts(&format!("{MARKER}\n")), None, "no parts");
        assert_eq!(parts("photocraft-parts 2\na.part-aa\n"), None, "a later layout this build doesn't know");
    }

    /// Part names are joined onto `./app/`, so one that could leave that folder, or name another
    /// origin, refuses the whole list.
    #[test]
    fn notes_in_the_parts_list_are_skipped() {
        let list = format!("{MARKER}\n# build 21, 2026-10-24 ########\na.part-aa\nb.part-ab\n");
        assert_eq!(parts(&list), Some(vec!["a.part-aa".to_string(), "b.part-ab".to_string()]));
        assert_eq!(parts(&format!("{MARKER}\n# only a note\n")), None, "no parts");
    }

    #[test]
    fn a_part_name_cannot_point_outside_the_app_folder() {
        for bad in ["../index.html", "a/b.part-aa", "a\\b", "https://example.com/x", "//example.com/x", "x?y", "x#y", ".hidden", "%2e%2e"] {
            assert_eq!(parts(&format!("{MARKER}\ngood.part-aa\n{bad}\n")), None, "{bad}");
        }
    }

    /// The packaged site shows this crate's page, the development build the app's own. They are
    /// the same page apart from which program the `<link data-trunk>` line builds.
    #[test]
    fn the_page_is_the_apps_page() {
        let trunk = |html: &str| html.lines().filter(|l| !l.contains("data-trunk")).collect::<Vec<_>>().join("\n");
        let (boot, app) = (include_str!("../index.html"), include_str!("../../photocraft-web/index.html"));
        assert_eq!(trunk(boot), trunk(app));
        assert!(boot.contains(r#"data-bin="ai-photoshop""#) && boot.contains(r#"data-cargo-profile="wasm-release""#), "{boot}");
        assert!(boot.contains(r#"id="photocraft_loading""#), "the element the failure message replaces");
    }
}
