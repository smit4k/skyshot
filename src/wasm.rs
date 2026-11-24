use std::collections::BTreeMap;

use js_sys::{Array, Map, Uint8Array};
use wasm_bindgen::prelude::*;

use crate::{
    RenderInput, RenderOptions, Renderer,
    model::{MAX_ASSETS, MAX_BYTES},
    post_lookup, trusted_media_url,
};

fn js_error(error: impl std::fmt::Display) -> JsValue {
    js_sys::Error::new(&error.to_string()).into()
}

fn options(json: &str) -> Result<RenderOptions, JsValue> {
    serde_json::from_str(json).map_err(js_error)
}

#[wasm_bindgen(js_name = Renderer)]
pub struct WasmRenderer(Renderer);

#[wasm_bindgen(js_class = Renderer)]
impl WasmRenderer {
    #[wasm_bindgen(constructor)]
    pub fn new(fonts: Array) -> Result<WasmRenderer, JsValue> {
        let mut data = Vec::new();
        let mut total = 0usize;
        for font in fonts.iter() {
            let font: Uint8Array = font
                .dyn_into()
                .map_err(|_| js_error("font must be a Uint8Array"))?;
            total = total.saturating_add(font.length() as usize);
            if total > 80 * 1024 * 1024 {
                return Err(js_error("font byte limit exceeded"));
            }
            data.push(font.to_vec());
        }
        Ok(Self(Renderer::with_fonts(data).map_err(js_error)?))
    }

    #[wasm_bindgen]
    pub fn render(&mut self, json: &str, config: &str, assets: Map) -> Result<WasmResult, JsValue> {
        let options = options(config)?;
        let input = RenderInput::from_json(json).map_err(js_error)?;
        let mut images = BTreeMap::new();
        let mut total = 0usize;
        for url in input.asset_urls(options).map_err(js_error)? {
            let bytes = assets.get(&JsValue::from_str(&url));
            if bytes.is_undefined() {
                continue;
            }
            let bytes: Uint8Array = bytes
                .dyn_into()
                .map_err(|_| js_error("asset must be a Uint8Array"))?;
            let size = bytes.length() as usize;
            total = total.saturating_add(size);
            if total > MAX_ASSETS {
                return Err(js_error("total image byte limit exceeded"));
            }
            // An empty buffer takes the same invalid-image path without copying oversized data.
            images.insert(
                url,
                if size > MAX_BYTES {
                    Vec::new()
                } else {
                    bytes.to_vec()
                },
            );
        }
        let result = self.0.render(&input, options, &images).map_err(js_error)?;
        let warnings = serde_wasm_bindgen::to_value(&result.warnings).map_err(js_error)?;
        Ok(WasmResult {
            png: result.png,
            width: result.width,
            height: result.height,
            warnings,
        })
    }
}

#[wasm_bindgen]
pub struct WasmResult {
    png: Vec<u8>,
    pub width: u32,
    pub height: u32,
    warnings: JsValue,
}

#[wasm_bindgen]
impl WasmResult {
    #[wasm_bindgen(getter)]
    pub fn png(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.png)
    }

    #[wasm_bindgen(getter)]
    pub fn warnings(&self) -> JsValue {
        self.warnings.clone()
    }
}

#[wasm_bindgen]
pub fn media_urls(post: &str, config: &str) -> Result<Vec<String>, JsValue> {
    RenderInput::from_json(post)
        .map_err(js_error)?
        .asset_urls(options(config)?)
        .map_err(js_error)
}

#[wasm_bindgen]
pub fn media_url(url: &str) -> Option<String> {
    trusted_media_url(url).map(Into::into)
}

#[wasm_bindgen]
pub fn handle_url(input: &str) -> Result<Option<String>, JsValue> {
    Ok(post_lookup(input)
        .map_err(js_error)?
        .handle_url()
        .map(Into::into))
}

#[wasm_bindgen]
pub fn thread_url(input: &str, did: Option<String>, ancestors: u32) -> Result<String, JsValue> {
    post_lookup(input)
        .map_err(js_error)?
        .thread_url(did.as_deref(), ancestors as usize)
        .map(Into::into)
        .map_err(js_error)
}
