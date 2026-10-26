#![allow(dead_code)] // the shared source files carry server-only code the browser build never calls
//! pdf-engine in the browser: the same renderer as the native server, compiled to WebAssembly.
//!
//! The pure parts of the engine are the native source files, included by path, so there is one
//! implementation of templates, page sizes, error codes, image handling and encryption:
//! `error`, `template`, `fulgur_engine`, `image_util` and `pdf_encrypt` live in `../../../src`.
//! Only the glue below is specific to the browser.
//!
//! There is no operating system here: fonts must be supplied by the caller (`add_font`). Images
//! in `data:` URIs load directly; any other image (a URL, a `blob:`, a File) is fetched by the
//! JavaScript wrapper between `prepare` and `render_prepared`.

use serde::Deserialize;
use std::collections::HashMap;
use std::sync::Arc;
use wasm_bindgen::prelude::*;

#[path = "../../../../src/barcode.rs"]
mod barcode;
#[path = "../../../../src/error.rs"]
mod error;
#[path = "../../../../src/font_face.rs"]
mod font_face;
#[path = "../../../../src/fulgur_engine.rs"]
mod fulgur_engine;
#[path = "../../../../src/image_util.rs"]
mod image_util;
#[path = "../../../../src/markdown.rs"]
mod markdown;
#[path = "../../../../src/pdf_encrypt.rs"]
mod pdf_encrypt;
#[path = "../../../../src/template.rs"]
mod template;

use error::AppError;
use fulgur_engine::{FulgurEngine, InProcessRenderer, PageMm, RenderJob, extract_title};

#[wasm_bindgen(start)]
pub fn init() {
    console_error_panic_hook::set_once();
    // Fast by default: the browser has one thread, so skipping compression saves time (about 15%).
    krilla::stream::set_compression_level(0);
}

/// zlib level for PDF streams: 0 = none (fastest, largest), 1 = fast, 6 = small. Applies to the page's engines.
#[wasm_bindgen]
pub fn set_compression(level: u32) {
    krilla::stream::set_compression_level(level);
}

/// Engine version (the same as the native engine's).
#[wasm_bindgen]
pub fn version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

#[derive(Deserialize)]
struct Request {
    template: Option<String>,
    #[serde(default)]
    data: serde_json::Value,
    width: Option<String>,
    height: Option<String>,
    orientation: Option<String>,
    password: Option<String>,
    /// Named templates the template can `{% include %}` or `{% extends %}`.
    #[serde(default)]
    partials: std::collections::BTreeMap<String, String>,
}

/// Largest `data:` image accepted, in bytes (the native default).
const MAX_IMAGE_BYTES: usize = 5 * 1024 * 1024;
/// Largest font file accepted for `@font-face`.
const MAX_FONT_BYTES: usize = 16 * 1024 * 1024;

#[wasm_bindgen]
pub struct Engine {
    fonts: Vec<Vec<u8>>,
    renderer: FulgurEngine,
    prepared: HashMap<u32, Prepared>,
    next_id: u32,
}

/// A request whose template is rendered and which waits for its external images.
struct Prepared {
    html: String,
    page: PageMm,
    password: Option<String>,
    /// The document's `@font-face` rules, for the fonts the page fetches.
    font_decls: Vec<font_face::FontFaceDecl>,
}

#[wasm_bindgen]
impl Engine {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Engine {
        Engine {
            fonts: Vec::new(),
            renderer: FulgurEngine::default(),
            prepared: HashMap::new(),
            next_id: 1,
        }
    }

    /// Queue a font file (TTF, OTF or TTC). Call `apply_fonts` after the last one.
    pub fn add_font(&mut self, bytes: &[u8]) {
        self.fonts.push(bytes.to_vec());
    }

    /// Make the queued fonts the set used for rendering, replacing any earlier set.
    pub fn apply_fonts(&mut self) {
        self.renderer.set_extra_fonts(self.fonts.clone());
    }

    /// Render a request, given as JSON: `{ template, data, width, height, orientation, password }`.
    ///
    /// Returns the PDF. On failure throws a JSON string `{ error, code, status, hint, line }`
    /// (the same error object the native engine sends).
    pub fn generate(&self, request_json: &str) -> Result<Vec<u8>, JsValue> {
        self.run(request_json)
            .map_err(|e| JsValue::from_str(&e.to_json().to_string()))
    }

    /// First half of a render that has external images: render the template and keep the HTML.
    ///
    /// Returns `{ id, sources, fontSources }`: the `<img src>` values and the `@font-face` `src` values that are
    /// not `data:` URIs. Fetch them, then call `render_prepared` (or `discard` to give up).
    pub fn prepare(&mut self, request_json: &str) -> Result<String, JsValue> {
        self.prepare_inner(request_json)
            .map_err(|e| JsValue::from_str(&e.to_json().to_string()))
    }

    /// Second half: render the prepared request with the fetched images. `names[i]` is the `src`
    /// value that `bytes[i]` answers; unreadable images are skipped, never fatal.
    pub fn render_prepared(
        &mut self,
        id: u32,
        names: js_sys::Array,
        bytes: js_sys::Array,
        font_names: js_sys::Array,
        font_bytes: js_sys::Array,
    ) -> Result<Vec<u8>, JsValue> {
        let Some(p) = self.prepared.remove(&id) else {
            return Err(JsValue::from_str(
                &AppError::InvalidField("unknown prepared request".into())
                    .to_json()
                    .to_string(),
            ));
        };
        let mut images = data_uri_images(&p.html);
        for (name, data) in names.iter().zip(bytes.iter()) {
            let (Some(name), Some(data)) =
                (name.as_string(), data.dyn_into::<js_sys::Uint8Array>().ok())
            else {
                continue;
            };
            let data = data.to_vec();
            if data.len() <= MAX_IMAGE_BYTES && image_util::is_supported_image(&data) {
                images.push((name, Arc::new(data)));
            }
        }
        let mut fetched: Vec<(String, Arc<Vec<u8>>)> = Vec::new();
        for (name, data) in font_names.iter().zip(font_bytes.iter()) {
            let (Some(name), Some(data)) =
                (name.as_string(), data.dyn_into::<js_sys::Uint8Array>().ok())
            else {
                continue;
            };
            let data = data.to_vec();
            if data.len() <= MAX_FONT_BYTES && font_face::is_font(&data) {
                fetched.push((name, Arc::new(data)));
            }
        }
        let faces = font_faces(&p.font_decls, &fetched);
        self.finish(p, images, faces)
            .map_err(|e| JsValue::from_str(&e.to_json().to_string()))
    }

    /// Forget a prepared request that will not be rendered.
    pub fn discard(&mut self, id: u32) {
        self.prepared.remove(&id);
    }
}

impl Default for Engine {
    fn default() -> Self {
        Self::new()
    }
}

impl Engine {
    fn run(&self, request_json: &str) -> Result<Vec<u8>, AppError> {
        let req: Request = serde_json::from_str(request_json)
            .map_err(|e| AppError::InvalidField(format!("request is not valid JSON: {e}")))?;
        let template = req
            .template
            .ok_or_else(|| AppError::MissingField("template".into()))?;
        let page = PageMm::parse_request(
            req.width.as_deref().unwrap_or("8.5in"),
            req.height.as_deref().unwrap_or("11in"),
            req.orientation.as_deref().unwrap_or("portrait"),
        )?;

        let partials: Vec<(String, String)> = req.partials.into_iter().collect();
        let html = template::render_with(&template, req.data, &partials)?;
        let images = data_uri_images(&html);
        let font_decls = font_face::font_face_decls(&html);
        let faces = font_faces(&font_decls, &[]);
        self.finish(
            Prepared {
                html,
                page,
                password: req.password,
                font_decls,
            },
            images,
            faces,
        )
    }

    fn prepare_inner(&mut self, request_json: &str) -> Result<String, AppError> {
        let req: Request = serde_json::from_str(request_json)
            .map_err(|e| AppError::InvalidField(format!("request is not valid JSON: {e}")))?;
        let template = req
            .template
            .ok_or_else(|| AppError::MissingField("template".into()))?;
        let page = PageMm::parse_request(
            req.width.as_deref().unwrap_or("8.5in"),
            req.height.as_deref().unwrap_or("11in"),
            req.orientation.as_deref().unwrap_or("portrait"),
        )?;
        let partials: Vec<(String, String)> = req.partials.into_iter().collect();
        let html = template::render_with(&template, req.data, &partials)?;
        let sources: Vec<String> = image_util::image_sources(&html)
            .into_iter()
            .filter(|s| !s.starts_with("data:"))
            .collect();
        let font_decls = font_face::font_face_decls(&html);
        let font_sources: Vec<&str> = font_decls
            .iter()
            .map(|d| d.src.as_str())
            .filter(|s| !s.starts_with("data:"))
            .collect();
        let font_sources = serde_json::json!(font_sources);
        let id = self.next_id;
        self.next_id = self.next_id.wrapping_add(1).max(1);
        self.prepared.insert(
            id,
            Prepared {
                html,
                page,
                password: req.password,
                font_decls,
            },
        );
        Ok(serde_json::json!({ "id": id, "sources": sources, "fontSources": font_sources }).to_string())
    }

    fn finish(
        &self,
        p: Prepared,
        images: Vec<(String, Arc<Vec<u8>>)>,
        font_faces: Vec<font_face::LoadedFontFace>,
    ) -> Result<Vec<u8>, AppError> {
        let html = image_util::size_images(&p.html, &images).unwrap_or(p.html);
        let title = extract_title(&html);
        let mut job = RenderJob {
            html,
            page: p.page,
            title,
            images,
            font_faces,
        };

        // A corrupt image or font fails the whole render; retry once without them, as the native
        // engine does, so a bad one never costs the document.
        let pdf = match self.renderer.render(&job) {
            Err(_) if !job.images.is_empty() || !job.font_faces.is_empty() => {
                job.images.clear();
                job.font_faces.clear();
                self.renderer.render(&job)
            }
            other => other,
        }
        .map_err(AppError::Render)?;

        match p.password.as_deref().filter(|p| !p.is_empty()) {
            Some(password) => pdf_encrypt::encrypt(&pdf, password).map_err(AppError::PdfProcessing),
            None => Ok(pdf),
        }
    }
}

/// The `<img src="data:...">` images in `html`, decoded. Other sources cannot be loaded here.
fn data_uri_images(html: &str) -> Vec<(String, Arc<Vec<u8>>)> {
    image_util::image_sources(html)
        .into_iter()
        .filter_map(|src| {
            let rest = src.strip_prefix("data:")?;
            let bytes = image_util::decode_data_uri(rest, MAX_IMAGE_BYTES).ok()?;
            image_util::is_supported_image(&bytes).then(|| (src, Arc::new(bytes)))
        })
        .collect()
}

/// The fonts of the `@font-face` rules: `data:` fonts decoded here, the others from what the page fetched (keyed by `src`).
fn font_faces(
    decls: &[font_face::FontFaceDecl],
    fetched: &[(String, Arc<Vec<u8>>)],
) -> Vec<font_face::LoadedFontFace> {
    decls
        .iter()
        .filter_map(|d| {
            let data = match d.src.strip_prefix("data:") {
                Some(rest) => Arc::new(image_util::decode_data_uri_as(rest, MAX_FONT_BYTES, false).ok()?),
                None => Arc::clone(&fetched.iter().find(|(name, _)| *name == d.src)?.1),
            };
            font_face::is_font(&data).then(|| font_face::LoadedFontFace {
                family: d.family.clone(),
                data,
                weight: d.weight,
                italic: d.italic,
            })
        })
        .collect()
}
