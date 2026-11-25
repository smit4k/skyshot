use std::collections::BTreeMap;

use js_sys::{Array, Map, Uint8Array};
use wasm_bindgen::prelude::*;

use crate::{
    ProfileInput, RenderInput, RenderOptions, RenderResult, Renderer,
    model::{MAX_ASSETS, MAX_BYTES},
    post_lookup, profile_url, trusted_media_url,
};

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_namespace = performance)]
    fn now() -> f64;
}

fn js_error(error: impl std::fmt::Display) -> JsValue {
    js_sys::Error::new(&error.to_string()).into()
}

fn options(json: &str) -> Result<RenderOptions, JsValue> {
    serde_json::from_str(json).map_err(js_error)
}

#[wasm_bindgen(js_name = Renderer)]
pub struct WasmRenderer {
    renderer: Renderer,
    font_bytes: usize,
}

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
        Ok(Self {
            renderer: Renderer::with_fonts(data).map_err(js_error)?,
            font_bytes: total,
        })
    }

    #[wasm_bindgen]
    pub fn add_font(&mut self, font: JsValue) -> Result<(), JsValue> {
        let font: Uint8Array = font
            .dyn_into()
            .map_err(|_| js_error("font must be a Uint8Array"))?;
        let total = self.font_bytes.saturating_add(font.length() as usize);
        if total > 80 * 1024 * 1024 {
            return Err(js_error("font byte limit exceeded"));
        }
        self.renderer.add_font(font.to_vec()).map_err(js_error)?;
        self.font_bytes = total;
        Ok(())
    }

    #[wasm_bindgen]
    pub fn render(&mut self, json: &str, config: &str, assets: Map) -> Result<WasmResult, JsValue> {
        let options = options(config)?;
        let input = RenderInput::from_json(json).map_err(js_error)?;
        let images = copy_images(input.asset_urls(options).map_err(js_error)?, assets)?;
        let result = self
            .renderer
            .render(&input, options, &images)
            .map_err(js_error)?;
        WasmResult::from_render(result, 0.0)
    }

    pub fn render_profile(
        &mut self,
        json: &str,
        config: &str,
        assets: Map,
    ) -> Result<WasmResult, JsValue> {
        let options = serde_json::from_str(config).map_err(js_error)?;
        let input = ProfileInput::from_json(json).map_err(js_error)?;
        let mut warnings = Vec::new();
        let scene = self
            .renderer
            .profile_scene(&input, options, &mut warnings)
            .map_err(js_error)?;
        let images = copy_images(scene.asset_urls(), assets)?;
        let (png, width, height) = crate::draw::paint(
            &mut self.renderer.0,
            scene,
            options.paint_options(),
            &images,
            &mut warnings,
        )
        .map_err(js_error)?;
        WasmResult::from_render(
            RenderResult {
                png,
                width,
                height,
                warnings,
            },
            0.0,
        )
    }

    pub fn profile_media_urls(&mut self, json: &str, config: &str) -> Result<Vec<String>, JsValue> {
        let options = serde_json::from_str(config).map_err(js_error)?;
        let input = ProfileInput::from_json(json).map_err(js_error)?;
        self.renderer
            .profile_asset_urls(&input, options)
            .map_err(js_error)
    }

    pub fn render_many(
        &mut self,
        json: &str,
        configs: &str,
        assets: Map,
    ) -> Result<Vec<WasmResult>, JsValue> {
        let options: Vec<RenderOptions> = serde_json::from_str(configs).map_err(js_error)?;
        if options.len() > 8 {
            return Err(js_error(
                "at most eight layouts can be rendered in one batch",
            ));
        }
        for config in &options {
            config.validate().map_err(js_error)?;
        }
        let Some(widest_context) = options.iter().copied().max_by_key(|o| o.ancestors) else {
            return Ok(Vec::new());
        };
        let input = RenderInput::from_json(json).map_err(js_error)?;
        // Only ancestor count affects asset discovery. Its maximum covers the batch.
        let images = copy_images(input.asset_urls(widest_context).map_err(js_error)?, assets)?;
        let mut results = Vec::with_capacity(options.len());
        let mut total_bytes = 0;
        for config in options {
            let start = now();
            let result = self
                .renderer
                .render(&input, config, &images)
                .map_err(js_error)?;
            let render_ms = now() - start;
            total_bytes += result.png.len();
            if total_bytes > 96 * 1024 * 1024 {
                return Err(js_error("total output byte limit exceeded"));
            }
            results.push(WasmResult::from_render(result, render_ms)?);
        }
        Ok(results)
    }
}

fn copy_images(urls: Vec<String>, assets: Map) -> Result<BTreeMap<String, Vec<u8>>, JsValue> {
    let mut images = BTreeMap::new();
    let mut total = 0usize;
    for url in urls {
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
    Ok(images)
}

#[cfg(feature = "verify")]
#[wasm_bindgen(js_class = Renderer)]
impl WasmRenderer {
    /// Development-only geometry and line-break oracle for profiles.
    pub fn profile_scene_layout(&mut self, json: &str, config: &str) -> Result<JsValue, JsValue> {
        let options: crate::ProfileRenderOptions =
            serde_json::from_str(config).map_err(js_error)?;
        let input = ProfileInput::from_json(json).map_err(js_error)?;
        let scene = self
            .renderer
            .profile_scene(&input, options, &mut Vec::new())
            .map_err(js_error)?;
        scene_report(scene)
    }

    /// Development-only oracle: inspect the actual post layout before rasterization.
    pub fn scene_layout(&mut self, json: &str, config: &str) -> Result<JsValue, JsValue> {
        let options = options(config)?;
        options.validate().map_err(js_error)?;
        let input = RenderInput::from_json(json).map_err(js_error)?;
        let scene = crate::layout::post(&mut self.renderer.0, &input, options, &mut Vec::new());
        scene_report(scene)
    }

    /// Development-only oracle: exercise the production text engine without painting.
    pub fn text_layout(
        &mut self,
        text: &str,
        width: f32,
        size: f32,
        strong: bool,
    ) -> Result<JsValue, JsValue> {
        if text.len() > crate::model::MAX_JSON
            || !width.is_finite()
            || !(1.0..=1000.0).contains(&width)
            || !size.is_finite()
            || !(1.0..=128.0).contains(&size)
        {
            return Err(js_error("invalid text layout limits"));
        }
        let block = self.renderer.0.measure(
            &[(text.to_owned(), None)],
            width,
            crate::draw::TextStyle {
                size,
                spacing: (size * 1.3).round(),
                color: 0,
                weight: if strong { 600 } else { 400 },
                lines: None,
            },
        );
        serde::Serialize::serialize(
            &block.report(),
            &serde_wasm_bindgen::Serializer::json_compatible(),
        )
        .map_err(js_error)
    }
}

#[cfg(feature = "verify")]
fn scene_report(scene: crate::draw::Scene) -> Result<JsValue, JsValue> {
    let text = scene
        .items
        .iter()
        .filter_map(|item| match item {
            crate::draw::Item::Text { block, x, y } => {
                Some(serde_json::json!({ "x": x, "y": y, "block": block.report() }))
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    let boxes = scene.items.iter().filter_map(|item| {
        let (rect, fill) = match item {
            crate::draw::Item::Box { rect, fill, .. } => (rect, *fill),
            crate::draw::Item::Image { rect, .. } => (rect, None),
            _ => return None,
        };
        Some(serde_json::json!({"x":rect.x(), "y":rect.y(), "w":rect.width(), "h":rect.height(), "fill":fill}))
    }).collect::<Vec<_>>();
    serde::Serialize::serialize(
        &serde_json::json!({ "height": scene.height, "text": text, "boxes": boxes }),
        &serde_wasm_bindgen::Serializer::json_compatible(),
    )
    .map_err(js_error)
}

#[wasm_bindgen]
pub struct WasmResult {
    png: Vec<u8>,
    pub width: u32,
    pub height: u32,
    pub render_ms: f64,
    warnings: JsValue,
}

impl WasmResult {
    fn from_render(result: RenderResult, render_ms: f64) -> Result<Self, JsValue> {
        Ok(Self {
            png: result.png,
            width: result.width,
            height: result.height,
            render_ms,
            warnings: serde_wasm_bindgen::to_value(&result.warnings).map_err(js_error)?,
        })
    }
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
pub fn profile_request(input: &str) -> Result<String, JsValue> {
    profile_url(input).map(Into::into).map_err(js_error)
}

#[wasm_bindgen]
pub fn profile_dependencies(json: &str) -> Result<Vec<String>, JsValue> {
    let input = ProfileInput::from_json(json).map_err(js_error)?;
    Ok(input
        .labeler_request()
        .into_iter()
        .chain(input.bio_mention_requests())
        .map(Into::into)
        .collect())
}

#[wasm_bindgen]
pub fn profile_primary_assets(json: &str, config: &str) -> Result<Vec<String>, JsValue> {
    let options: crate::ProfileRenderOptions = serde_json::from_str(config).map_err(js_error)?;
    options.validate().map_err(js_error)?;
    let input = ProfileInput::from_json(json).map_err(js_error)?;
    Ok(input.primary_asset_urls().into_iter().collect())
}

#[wasm_bindgen]
pub fn profile_bio_facets(json: &str, profiles: &str) -> Result<String, JsValue> {
    let mut input = ProfileInput::from_json(json).map_err(js_error)?;
    let profiles: Vec<serde_json::Value> = serde_json::from_str(profiles).map_err(js_error)?;
    input.resolve_bio_mentions(&profiles);
    serde_json::to_string(&input.context().description_facets).map_err(js_error)
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
