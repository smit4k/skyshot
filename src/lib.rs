//! Render Bluesky posts and profile headers to PNG using supplied data and fonts.
#![forbid(unsafe_code)]

mod draw;
mod icons;
mod layout;
mod model;
mod profile;
mod profile_layout;
mod resolver;
#[cfg(all(target_arch = "wasm32", feature = "wasm"))]
mod wasm;

pub use model::{RenderInput, RenderOptions, Theme, Warning};
pub use profile::{
    ModerationAction, ProfileContext, ProfileInput, ProfileLayout, ProfileModeration,
    ProfileRenderOptions,
};
pub use resolver::{PostLookup, post_lookup, profile_url, trusted_media_url};

use std::collections::BTreeMap;

/// A render failure. Recoverable problems with optional content are warnings instead.
#[derive(Debug, thiserror::Error)]
pub enum RenderError {
    #[error("invalid input: {0}")]
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
    /// Source URLs in the measured header at these options, including only visible link icons.
    pub fn profile_asset_urls(
        &mut self,
        input: &ProfileInput,
        options: ProfileRenderOptions,
    ) -> Result<Vec<String>, RenderError> {
        Ok(self
            .profile_scene(input, options, &mut Vec::new())?
            .asset_urls())
    }

    fn profile_scene(
        &mut self,
        input: &ProfileInput,
        options: ProfileRenderOptions,
        warnings: &mut Vec<Warning>,
    ) -> Result<draw::Scene, RenderError> {
        options.validate()?;
        input.validate_snapshot()?;
        Ok(profile_layout::profile(
            &mut self.0,
            input,
            options,
            warnings,
        ))
    }

    /// Render a profile header using the same font, glyph, and image caches as posts.
    pub fn render_profile(
        &mut self,
        input: &ProfileInput,
        options: ProfileRenderOptions,
        assets: &BTreeMap<String, Vec<u8>>,
    ) -> Result<RenderResult, RenderError> {
        let mut warnings = Vec::new();
        let scene = self.profile_scene(input, options, &mut warnings)?;
        let (png, width, height) = draw::paint(
            &mut self.0,
            scene,
            options.paint_options(),
            assets,
            &mut warnings,
        )?;
        Ok(RenderResult {
            png,
            width,
            height,
            warnings,
        })
    }

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
