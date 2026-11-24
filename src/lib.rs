//! Render public Bluesky posts to PNG using explicitly supplied data and fonts.
#![forbid(unsafe_code)]

mod draw;
mod icons;
mod layout;
mod model;
mod resolver;
#[cfg(all(target_arch = "wasm32", feature = "wasm"))]
mod wasm;

pub use model::{RenderInput, RenderOptions, Theme, Warning};
pub use resolver::{PostLookup, post_lookup, trusted_media_url};

use std::collections::BTreeMap;

/// A render failure. Recoverable problems with optional content are warnings instead.
#[derive(Debug, thiserror::Error)]
pub enum RenderError {
    #[error("invalid post: {0}")]
    Input(&'static str),
    #[error("invalid JSON: {0}")]
    Json(#[from] serde_json::Error),
    #[error("resource limit exceeded: {0}")]
    Limit(&'static str),
    #[error("could not load font data")]
    Font,
    #[error("could not encode PNG: {0}")]
    Png(#[from] image::ImageError),
}

/// Encoded PNG, dimensions in physical pixels, and recoverable issues.
#[derive(Debug)]
pub struct RenderResult {
    pub png: Vec<u8>,
    pub width: u32,
    pub height: u32,
    pub warnings: Vec<Warning>,
}

/// A reusable renderer with an explicit font database and glyph cache.
pub struct Renderer(draw::TextEngine);

impl Renderer {
    /// Add a fallback font while retaining the existing font and glyph caches.
    pub fn add_font(&mut self, font: Vec<u8>) -> Result<(), RenderError> {
        self.0.add_font(font)
    }

    /// Create a renderer with bundled Inter. Pass open fallback font bytes to
    /// [Self::with_fonts] for color emoji and additional scripts.
    pub fn new() -> Result<Self, RenderError> {
        Self::with_fonts(std::iter::empty())
    }

    /// Load additional fonts into the renderer’s fixed script fallback order.
    pub fn with_fonts(fonts: impl IntoIterator<Item = Vec<u8>>) -> Result<Self, RenderError> {
        Ok(Self(draw::TextEngine::new(fonts)?))
    }

    /// Render a parsed post. Assets are encoded raster images keyed by exact source URL.
    pub fn render(
        &mut self,
        input: &RenderInput,
        options: RenderOptions,
        assets: &BTreeMap<String, Vec<u8>>,
    ) -> Result<RenderResult, RenderError> {
        options.validate()?;
        let mut warnings = Vec::new();
        let scene = layout::post(&mut self.0, input, options, &mut warnings);
        let (png, width, height) = draw::paint(&mut self.0, scene, options, assets, &mut warnings)?;
        Ok(RenderResult {
            png,
            width,
            height,
            warnings,
        })
    }
}
