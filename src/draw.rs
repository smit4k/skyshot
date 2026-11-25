use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    io::Cursor,
};

use cosmic_text::{
    Attrs, Buffer, Color, Ellipsize, EllipsizeHeightLimit, Fallback, Family, FeatureTag,
    FontFeatures, FontSystem, Metrics, Shaping, SwashCache, Weight, Wrap, fontdb,
};
use image::{
    ImageDecoder, ImageEncoder, ImageFormat, ImageReader,
    codecs::png::{CompressionType, FilterType, PngEncoder},
};
use tiny_skia::{
    Color as Ink, FillRule, Mask, Paint, Path, PathBuilder, Pixmap, PremultipliedColorU8, Rect,
    Stroke, Transform,
};
use unicode_script::Script;

use crate::{
    RenderError, RenderOptions, Warning,
    model::{MAX_ASSETS, MAX_BYTES},
};

pub(crate) struct TextEngine {
    fonts: FontSystem,
    glyphs: SwashCache,
    primary: String,
    profile_fonts: bool,
    images: ImageCache,
}

struct NotoFallback;
impl Fallback for NotoFallback {
    fn common_fallback(&self) -> &[&'static str] {
        &[
            "Noto Color Emoji",
            "Noto Sans",
            "Noto Sans Symbols 2",
            "Noto Sans SC",
        ]
    }
    fn forbidden_fallback(&self) -> &[&'static str] {
        &[]
    }
    fn script_fallback(&self, script: Script, _: &str) -> &[&'static str] {
        match script {
            Script::Arabic => &["Noto Sans Arabic"],
            Script::Hebrew => &["Noto Sans Hebrew"],
            Script::Devanagari => &["Noto Sans Devanagari"],
            Script::Thai => &["Noto Sans Thai"],
            Script::Hangul => &["Noto Sans KR"],
            Script::Hiragana | Script::Katakana => &["Noto Sans JP"],
            Script::Han => &["Noto Sans SC", "Noto Sans JP", "Noto Sans KR"],
            _ => &[],
        }
    }
}

#[derive(Clone, Copy)]
pub(crate) struct TextStyle {
    pub size: f32,
    pub spacing: f32,
    pub color: u32,
    pub weight: u16,
    pub lines: Option<usize>,
}

pub(crate) struct TextBlock {
    buffer: Buffer,
    pub height: f32,
    pub width: f32,
}
pub(crate) type Span = (String, Option<u32>);

impl TextEngine {
    pub fn add_font(&mut self, font: Vec<u8>) -> Result<(), RenderError> {
        let db = self.fonts.db_mut();
        let before = db.len();
        db.load_font_data(font);
        if db.len() == before {
            return Err(RenderError::Font);
        }
        Ok(())
    }

    pub fn new(fonts: impl IntoIterator<Item = Vec<u8>>) -> Result<Self, RenderError> {
        let mut db = fontdb::Database::new();
        db.load_font_data(include_bytes!("../fonts/InterVariable.ttf").to_vec());
        let primary = db
            .faces()
            .next()
            .and_then(|face| face.families.first())
            .map(|(name, _)| name.clone())
            .ok_or(RenderError::Font)?;
        db.set_sans_serif_family(primary.clone());
        for font in fonts {
            let before = db.len();
            db.load_font_data(font);
            if db.len() == before {
                return Err(RenderError::Font);
            }
        }
        Ok(Self {
            fonts: FontSystem::new_with_locale_and_db_and_fallback(
                "en-US".to_owned(),
                db,
                NotoFallback,
            ),
            glyphs: SwashCache::new(),
            primary,
            profile_fonts: false,
            images: ImageCache::default(),
        })
    }

    pub fn measure(&mut self, spans: &[Span], width: f32, style: TextStyle) -> TextBlock {
        self.measure_with_family(spans, width, style, None)
    }

    pub(crate) fn measure_profile(
        &mut self,
        spans: &[Span],
        width: f32,
        style: TextStyle,
    ) -> TextBlock {
        let family = match style.size {
            15.0 => "Skyshot Profile Text",
            24.3 => "Skyshot Profile Compact",
            30.0 => "Skyshot Profile Display",
            _ => return self.measure(spans, width, style),
        };
        if !self.profile_fonts {
            let db = self.fonts.db_mut();
            for bytes in [
                include_bytes!("../fonts/ProfileText.ttf").as_slice(),
                include_bytes!("../fonts/ProfileCompact.ttf").as_slice(),
                include_bytes!("../fonts/ProfileDisplay.ttf").as_slice(),
            ] {
                db.load_font_data(bytes.to_vec());
            }
            self.profile_fonts = true;
        }
        self.measure_with_family(spans, width, style, Some(family))
    }

    fn measure_with_family(
        &mut self,
        spans: &[Span],
        width: f32,
        style: TextStyle,
        family: Option<&str>,
    ) -> TextBlock {
        let mut buffer = Buffer::new_empty(Metrics::new(style.size, style.spacing));
        let mut features = FontFeatures::new();
        features.disable(FeatureTag::new(b"calt"));
        let attrs = Attrs::new()
            .family(Family::Name(family.unwrap_or(&self.primary)))
            .font_features(features)
            .weight(Weight(style.weight))
            .color(text_color(style.color));
        buffer.set_size(Some(width.max(1.0)), None);
        buffer.set_wrap(Wrap::WordOrGlyph);
        if let Some(lines) = style.lines {
            buffer.set_ellipsize(Ellipsize::End(EllipsizeHeightLimit::Lines(lines)));
        }
        buffer.set_rich_text(
            spans.iter().map(|(text, color)| {
                let attrs = attrs.clone();
                (
                    text.as_str(),
                    color.map_or_else(
                        || attrs.clone(),
                        |color| attrs.clone().color(text_color(color)),
                    ),
                )
            }),
            &attrs,
            Shaping::Advanced,
            None,
        );
        buffer.shape_until_scroll(&mut self.fonts, false);
        let mut height: f32 = 0.0;
        let mut measured_width: f32 = 0.0;
        for line in buffer.layout_runs().take(style.lines.unwrap_or(usize::MAX)) {
            height = line.line_top + line.line_height;
            measured_width = measured_width.max(line.line_w);
        }
        TextBlock {
            buffer,
            height,
            width: measured_width,
        }
    }
}

impl TextBlock {
    pub(crate) fn last_line(&self) -> Option<(f32, f32, bool)> {
        self.buffer
            .layout_runs()
            .take_while(|line| line.line_top < self.height)
            .last()
            .map(|line| (line.line_w, line.line_y, line.rtl))
    }
}

#[cfg(all(feature = "verify", feature = "wasm", target_arch = "wasm32"))]
impl TextBlock {
    pub(crate) fn report(&self) -> serde_json::Value {
        serde_json::json!({
            "height": self.height,
            "lines": self.buffer.layout_runs().map(|line| {
                let start = line.glyphs.iter().map(|g| g.start).min().unwrap_or(0);
                let end = line.glyphs.iter().map(|g| g.end).max().unwrap_or(0);
                serde_json::json!({
                    "text": line.text.get(start..end).unwrap_or_default(),
                    "width": line.line_w,
                    "rtl": line.rtl,
                })
            }).collect::<Vec<_>>(),
        })
    }
}

pub(crate) struct Scene {
    pub background: u32,
    pub height: f32,
    pub items: Vec<Item>,
}

impl Scene {
    pub(crate) fn asset_urls(&self) -> Vec<String> {
        self.items
            .iter()
            .filter_map(|item| match item {
                Item::Image { url, .. } => Some(url),
                _ => None,
            })
            .collect::<BTreeSet<_>>()
            .into_iter()
            .cloned()
            .collect()
    }
}

pub(crate) enum Item {
    Text {
        block: TextBlock,
        x: f32,
        y: f32,
    },
    Box {
        rect: Rect,
        radius: f32,
        fill: Option<u32>,
        stroke: Option<u32>,
    },
    Image {
        rect: Rect,
        radius: f32,
        url: String,
        cover: bool,
        blur: f32,
        fallback: Option<(Path, u32)>,
    },
    Overlay {
        rect: Rect,
        radius: f32,
        color: u32,
        alpha: u8,
    },
    EmbeddedImage {
        rect: Rect,
        radius: f32,
        bytes: &'static [u8],
    },
    Path {
        path: Path,
        color: u32,
    },
}

fn text_color(c: u32) -> Color {
    Color::rgb((c >> 16) as u8, (c >> 8) as u8, c as u8)
}

fn ink(c: u32) -> Ink {
    Ink::from_rgba8((c >> 16) as u8, (c >> 8) as u8, c as u8, 255)
}

fn paint_color(c: u32) -> Paint<'static> {
    let mut paint = Paint::default();
    paint.set_color(ink(c));
    paint
}

fn rounded(rect: Rect, radius: f32) -> Option<Path> {
    let (x, y, r, b) = (rect.left(), rect.top(), rect.right(), rect.bottom());
    let d = radius.min(rect.width() / 2.0).min(rect.height() / 2.0);
    if d <= 0.0 {
        return Some(PathBuilder::from_rect(rect));
    }
    let k = d * 0.552_284_8;
    let mut p = PathBuilder::new();
    p.move_to(x + d, y);
    p.line_to(r - d, y);
    p.cubic_to(r - d + k, y, r, y + d - k, r, y + d);
    p.line_to(r, b - d);
    p.cubic_to(r, b - d + k, r - d + k, b, r - d, b);
    p.line_to(x + d, b);
    p.cubic_to(x + d - k, b, x, b - d + k, x, b - d);
    p.line_to(x, y + d);
    p.cubic_to(x, y + d - k, x + d - k, y, x + d, y);
    p.close();
    p.finish()
}

fn stroke(width: f32) -> Stroke {
    Stroke {
        width,
        line_cap: tiny_skia::LineCap::Round,
        line_join: tiny_skia::LineJoin::Round,
        ..Stroke::default()
    }
}

pub(crate) fn paint(
    text: &mut TextEngine,
    scene: Scene,
    options: RenderOptions,
    assets: &BTreeMap<String, Vec<u8>>,
    warnings: &mut Vec<Warning>,
) -> Result<(Vec<u8>, u32, u32), RenderError> {
    let scale = options.scale as f32;
    let width = options.width * options.scale;
    let height = (scene.height * scale).ceil() as u32;
    if u64::from(width) * u64::from(height) > 16_000_000 {
        return Err(RenderError::Limit("output pixels"));
    }
    let mut pix = Pixmap::new(width, height).ok_or(RenderError::Limit("output pixels"))?;
    pix.fill(ink(scene.background));
    let transform = Transform::from_scale(scale, scale);
    if text.glyphs.image_cache.len() > 4096 {
        text.glyphs = SwashCache::new();
    }
    let mut asset_bytes = 0usize;
    let mut counted = BTreeSet::new();
    for item in scene.items {
        match item {
            Item::Text { block, x, y } => {
                for line in block
                    .buffer
                    .layout_runs()
                    .take_while(|line| line.line_top < block.height)
                {
                    for glyph in line.glyphs {
                        let g = glyph.physical((x * scale, (y + line.line_y) * scale), scale);
                        text.glyphs.with_pixels(
                            &mut text.fonts,
                            g.cache_key,
                            glyph.color_opt.unwrap_or(Color::rgb(0, 0, 0)),
                            |dx, dy, c| {
                                let (px, py) = (g.x + dx, g.y + dy);
                                if c.a() == 0
                                    || px < 0
                                    || py < 0
                                    || px >= width as i32
                                    || py >= height as i32
                                {
                                    return;
                                }
                                let dst = &mut pix.pixels_mut()
                                    [py as usize * width as usize + px as usize];
                                let a = u32::from(c.a());
                                let channel = |src, dst| {
                                    ((u32::from(src) * a + u32::from(dst) * (255 - a) + 127) / 255)
                                        as u8
                                };
                                if let Some(color) = PremultipliedColorU8::from_rgba(
                                    channel(c.r(), dst.red()),
                                    channel(c.g(), dst.green()),
                                    channel(c.b(), dst.blue()),
                                    255,
                                ) {
                                    *dst = color;
                                }
                            },
                        );
                    }
                }
            }
            Item::Box {
                rect,
                radius,
                fill,
                stroke: outline,
            } => {
                if let Some(path) = rounded(rect, radius) {
                    if let Some(color) = fill {
                        pix.fill_path(
                            &path,
                            &paint_color(color),
                            FillRule::Winding,
                            transform,
                            None,
                        );
                    }
                    if let Some(color) = outline {
                        pix.stroke_path(&path, &paint_color(color), &stroke(1.0), transform, None);
                    }
                }
            }
            Item::Path { path, color } => {
                pix.fill_path(
                    &path,
                    &paint_color(color),
                    FillRule::EvenOdd,
                    transform,
                    None,
                );
            }
            Item::Image {
                rect,
                radius,
                url,
                cover,
                blur,
                fallback,
            } => {
                let drawn = if let Some(bytes) = assets.get(&url) {
                    if counted.insert(url.clone()) {
                        asset_bytes = asset_bytes.saturating_add(bytes.len());
                    }
                    if asset_bytes > MAX_ASSETS {
                        return Err(RenderError::Limit("total image bytes"));
                    }
                    let drawn = text.images.draw(
                        &mut pix,
                        rect,
                        radius,
                        scale,
                        bytes,
                        cover,
                        blur,
                        fallback.is_some(),
                    );
                    if !drawn {
                        warnings.push(Warning::InvalidAsset(url));
                    }
                    drawn
                } else {
                    warnings.push(Warning::MissingAsset(url));
                    false
                };
                if !drawn && let Some((path, color)) = fallback {
                    pix.fill_path(
                        &path,
                        &paint_color(color),
                        FillRule::EvenOdd,
                        transform,
                        None,
                    );
                }
            }
            Item::Overlay {
                rect,
                radius,
                color,
                alpha,
            } => {
                if let Some(path) = rounded(rect, radius) {
                    let mut paint = Paint::default();
                    paint.set_color_rgba8(
                        (color >> 16) as u8,
                        (color >> 8) as u8,
                        color as u8,
                        alpha,
                    );
                    pix.fill_path(&path, &paint, FillRule::Winding, transform, None);
                }
            }
            Item::EmbeddedImage {
                rect,
                radius,
                bytes,
            } => {
                text.images
                    .draw(&mut pix, rect, radius, scale, bytes, true, 0.0, false);
            }
        }
    }
    let mut png = Vec::new();
    // A fixed vertical filter avoids trying five filters on every scanline.
    PngEncoder::new_with_quality(&mut png, CompressionType::Fast, FilterType::Up).write_image(
        pix.data(),
        width,
        height,
        image::ExtendedColorType::Rgba8,
    )?;
    Ok((png, width, height))
}

struct DecodedImage {
    jpeg_size: Option<(u32, u32)>,
    reduction: u32,
    pixels: image::RgbaImage,
}

fn decode(bytes: &[u8], size: TileSize) -> Option<DecodedImage> {
    if bytes.len() > MAX_BYTES {
        return None;
    }
    let mut reader = ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .ok()?;
    let jpeg = reader.format() == Some(ImageFormat::Jpeg);
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(8192);
    limits.max_image_height = Some(8192);
    limits.max_alloc = Some(96 * 1024 * 1024);
    reader.limits(limits);
    let mut decoder = reader.into_decoder().ok()?;
    let (w, h) = decoder.dimensions();
    if w == 0 || h == 0 || u64::from(w) * u64::from(h) > 16_000_000 {
        return None;
    }
    let orientation = decoder.orientation().ok()?;
    let (oriented_w, oriented_h) = match orientation {
        image::metadata::Orientation::Rotate90
        | image::metadata::Orientation::Rotate270
        | image::metadata::Orientation::Rotate90FlipH
        | image::metadata::Orientation::Rotate270FlipH => (h, w),
        _ => (w, h),
    };
    let jpeg_size = jpeg.then_some((oriented_w, oriented_h));
    let reduction = size.jpeg_reduction(jpeg_size);
    let reduced = if reduction > 1 {
        decode_small_jpeg(bytes, w.div_ceil(reduction), h.div_ceil(reduction))
    } else {
        None
    };
    let mut raster = match reduced {
        Some(raster) => raster,
        None => image::DynamicImage::from_decoder(decoder).ok()?,
    };
    raster.apply_orientation(orientation);
    let mut pixels = raster.into_rgba8();
    for pixel in pixels.pixels_mut() {
        pixel.0 = premultiply(pixel.0);
    }
    Some(DecodedImage {
        jpeg_size,
        reduction,
        pixels,
    })
}

fn decode_small_jpeg(bytes: &[u8], width: u32, height: u32) -> Option<image::DynamicImage> {
    let mut jpeg = jpeg_decoder::Decoder::new(Cursor::new(bytes));
    jpeg.set_max_decoding_buffer_size(96 * 1024 * 1024);
    let (w, h) = jpeg
        .scale(width.try_into().ok()?, height.try_into().ok()?)
        .ok()?;
    let pixels = jpeg.decode().ok()?;
    match jpeg.info()?.pixel_format {
        jpeg_decoder::PixelFormat::RGB24 => image::RgbImage::from_raw(w.into(), h.into(), pixels)
            .map(image::DynamicImage::ImageRgb8),
        jpeg_decoder::PixelFormat::L8 => image::GrayImage::from_raw(w.into(), h.into(), pixels)
            .map(image::DynamicImage::ImageLuma8),
        _ => None,
    }
}

#[derive(Clone, Copy, PartialEq)]
struct TileSize {
    width: u32,
    height: u32,
    radius: f32,
    cover: bool,
    blur: f32,
    smooth: bool,
}

impl TileSize {
    fn jpeg_reduction(self, source: Option<(u32, u32)>) -> u32 {
        let Some((width, height)) = source else {
            return 1;
        };
        let (x, y) = (width / self.width, height / self.height);
        match if self.cover { x.min(y) } else { x.max(y) } {
            0..=1 => 1,
            2..=3 => 2,
            4..=7 => 4,
            _ => 8,
        }
    }
}

enum CachedPixels {
    Decoded(DecodedImage),
    Tile(TileSize, Pixmap),
}

impl CachedPixels {
    fn len(&self) -> usize {
        match self {
            Self::Decoded(image) => image.pixels.len(),
            Self::Tile(_, tile) => tile.data().len(),
        }
    }
}

#[derive(Default)]
struct ImageCache {
    entries: VecDeque<(Vec<u8>, CachedPixels)>,
    bytes: usize,
}

impl ImageCache {
    fn insert(&mut self, bytes: &[u8], pixels: CachedPixels) {
        const CACHE_LIMIT: usize = 32 * 1024 * 1024;
        let cost = bytes.len() + pixels.len();
        if cost > CACHE_LIMIT {
            return;
        }
        while self.bytes + cost > CACHE_LIMIT || self.entries.len() >= 32 {
            let Some((bytes, pixels)) = self.entries.pop_front() else {
                break;
            };
            self.bytes -= bytes.len() + pixels.len();
        }
        self.bytes += cost;
        self.entries.push_back((bytes.to_vec(), pixels));
    }

    fn tile(&mut self, bytes: &[u8], size: TileSize) -> Option<Pixmap> {
        if let Some(decoded) = self
            .entries
            .iter()
            .rev()
            .find_map(|(source, entry)| match entry {
                CachedPixels::Decoded(decoded)
                    if decoded.reduction == size.jpeg_reduction(decoded.jpeg_size)
                        && source == bytes =>
                {
                    Some(decoded)
                }
                _ => None,
            })
        {
            return image_tile(&decoded.pixels, size);
        }
        let decoded = decode(bytes, size)?;
        let tile = image_tile(&decoded.pixels, size)?;
        self.insert(bytes, CachedPixels::Decoded(decoded));
        Some(tile)
    }

    #[allow(clippy::too_many_arguments)]
    fn draw(
        &mut self,
        dst: &mut Pixmap,
        rect: Rect,
        radius: f32,
        scale: f32,
        bytes: &[u8],
        cover: bool,
        blur: f32,
        smooth: bool,
    ) -> bool {
        let x = (rect.x() * scale).round() as i32;
        let y = (rect.y() * scale).round() as i32;
        let size = TileSize {
            width: ((rect.right() * scale).round() as i32 - x).max(1) as u32,
            height: ((rect.bottom() * scale).round() as i32 - y).max(1) as u32,
            radius: radius * scale,
            cover,
            blur: blur * scale,
            smooth,
        };
        if let Some(tile) = self
            .entries
            .iter()
            .rev()
            .find_map(|(source, entry)| match entry {
                CachedPixels::Tile(key, tile) if *key == size && source == bytes => Some(tile),
                _ => None,
            })
        {
            blit_image(dst, tile, x, y);
            return true;
        }
        let Some(tile) = self.tile(bytes, size) else {
            return false;
        };
        blit_image(dst, &tile, x, y);
        self.insert(bytes, CachedPixels::Tile(size, tile));
        true
    }
}

fn blit_image(dst: &mut Pixmap, tile: &Pixmap, x: i32, y: i32) {
    let left = x.max(0);
    let top = y.max(0);
    let right = x
        .saturating_add(tile.width() as i32)
        .min(dst.width() as i32);
    let bottom = y
        .saturating_add(tile.height() as i32)
        .min(dst.height() as i32);
    if left >= right || top >= bottom {
        return;
    }
    let src_stride = tile.width() as usize * 4;
    let dst_stride = dst.width() as usize * 4;
    let row_bytes = (right - left) as usize * 4;
    let src = tile.data();
    let dst = dst.data_mut();
    for row in top..bottom {
        let from = (row - y) as usize * src_stride + (left - x) as usize * 4;
        let to = row as usize * dst_stride + left as usize * 4;
        for (src, dst) in src[from..from + row_bytes]
            .chunks_exact(4)
            .zip(dst[to..to + row_bytes].chunks_exact_mut(4))
        {
            match src[3] {
                255 => dst.copy_from_slice(src),
                0 => {}
                a => {
                    let keep = u16::from(255 - a);
                    // The destination is opaque and the tile is already premultiplied.
                    for channel in 0..3 {
                        dst[channel] =
                            src[channel] + ((u16::from(dst[channel]) * keep + 127) / 255) as u8;
                    }
                }
            }
        }
    }
}

fn premultiply([r, g, b, a]: [u8; 4]) -> [u8; 4] {
    if a == 255 {
        return [r, g, b, a];
    }
    let channel = |value| ((u16::from(value) * u16::from(a) + 127) / 255) as u8;
    [channel(r), channel(g), channel(b), a]
}

struct Tap {
    first: usize,
    next: usize,
    weight: u32,
}

fn taps(range: std::ops::Range<usize>, offset: f32, scale: f32, extent: usize) -> Vec<Tap> {
    range
        .map(|destination| {
            let source =
                ((destination as f32 + 0.5 - offset) / scale - 0.5).clamp(0.0, (extent - 1) as f32);
            let first = source as usize;
            Tap {
                first,
                next: (first + 1).min(extent - 1),
                weight: ((source - first as f32) * 256.0).round() as u32,
            }
        })
        .collect()
}

fn image_tile(raster: &image::RgbaImage, size: TileSize) -> Option<Pixmap> {
    let TileSize {
        width: dw,
        height: dh,
        radius,
        cover,
        blur,
        smooth,
    } = size;
    let downscaled;
    let raster =
        if smooth && dw == dh && raster.width() == raster.height() && raster.width() >= dw * 2 {
            // Chromium filters large avatar and favicon reductions before sampling at display size.
            downscaled =
                image::imageops::resize(raster, dw, dh, image::imageops::FilterType::Triangle);
            &downscaled
        } else {
            raster
        };
    let (w, h) = raster.dimensions();
    let (sx, sy) = (dw as f32 / w as f32, dh as f32 / h as f32);
    let k = if cover { sx.max(sy) } else { sx.min(sy) };
    let tx = (dw as f32 - w as f32 * k) / 2.0;
    let ty = (dh as f32 - h as f32 * k) / 2.0;
    let mut tile = Pixmap::new(dw, dh)?;
    let pixels = raster.as_raw().as_chunks::<4>().0;
    let (left, top) = (tx.round().max(0.0) as usize, ty.round().max(0.0) as usize);
    let right = ((dw as f32 - tx).round() as usize).min(dw as usize);
    let bottom = ((dh as f32 - ty).round() as usize).min(dh as usize);
    let columns = taps(left..right, tx, k, w as usize);
    let rows = taps(top..bottom, ty, k, h as usize);
    let dest = tile.data_mut().as_chunks_mut::<4>().0;
    for (row, y) in dest.chunks_exact_mut(dw as usize).skip(top).zip(rows) {
        let above = &pixels[y.first * w as usize..][..w as usize];
        let below = &pixels[y.next * w as usize..][..w as usize];
        for (pixel, x) in row[left..right].iter_mut().zip(&columns) {
            let (a, b) = (above[x.first], above[x.next]);
            let (c, d) = (below[x.first], below[x.next]);
            *pixel = std::array::from_fn(|i| {
                let upper = u32::from(a[i]) * (256 - x.weight) + u32::from(b[i]) * x.weight;
                let lower = u32::from(c[i]) * (256 - x.weight) + u32::from(d[i]) * x.weight;
                ((upper * (256 - y.weight) + lower * y.weight + 32768) >> 16) as u8
            });
        }
    }
    if blur > 0.0 {
        let raster = image::RgbaImage::from_raw(dw, dh, tile.take())?;
        let blurred = image::imageops::fast_blur(&raster, blur);
        tile = Pixmap::from_vec(blurred.into_raw(), tiny_skia::IntSize::from_wh(dw, dh)?)?;
    }
    if radius > 0.0
        && let Some(mut mask) = Mask::new(dw, dh)
        && let Some(rect) = Rect::from_xywh(0.0, 0.0, dw as f32, dh as f32)
        && let Some(path) = rounded(rect, radius)
    {
        mask.fill_path(&path, FillRule::Winding, true, Transform::identity());
        let edge = radius.min(dw.min(dh) as f32 / 2.0).ceil() as usize;
        // Only rows that touch a rounded corner can contain partial coverage.
        let width = dw as usize;
        for (y, row) in tile
            .data_mut()
            .as_chunks_mut::<4>()
            .0
            .chunks_exact_mut(width)
            .enumerate()
        {
            if y < edge || y >= dh as usize - edge {
                for (pixel, &alpha) in row.iter_mut().zip(&mask.data()[y * width..][..width]) {
                    if alpha != 255 {
                        *pixel = pixel.map(|channel| {
                            ((u16::from(channel) * u16::from(alpha) + 127) / 255) as u8
                        });
                    }
                }
            }
        }
    }
    Some(tile)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sampler_interpolates_premultiplied_color_and_handles_fit_and_cover() {
        let pixels = image::RgbaImage::from_fn(2, 1, |x, _| {
            image::Rgba(premultiply(if x == 0 {
                [255, 0, 0, 255]
            } else {
                [0, 0, 255, 0]
            }))
        });
        let size = TileSize {
            width: 4,
            height: 4,
            radius: 0.0,
            cover: false,
            blur: 0.0,
            smooth: false,
        };
        let contained = image_tile(&pixels, size).unwrap();
        assert_eq!(&contained.data()[..16], &[0; 16]);
        assert_eq!(
            &contained.data()[16..32],
            &[255, 0, 0, 255, 191, 0, 0, 191, 64, 0, 0, 64, 0, 0, 0, 0]
        );
        let covered = image_tile(
            &pixels,
            TileSize {
                width: 2,
                height: 2,
                cover: true,
                blur: 0.0,
                ..size
            },
        )
        .unwrap();
        assert_eq!(covered.data(), &[191, 0, 0, 191, 64, 0, 0, 64].repeat(2));
    }

    #[test]
    fn rendering_different_tile_sizes_reuses_the_decode() {
        let mut cache = ImageCache::default();
        let bytes = include_bytes!("../tests/fixtures/tiles.png");
        let size = TileSize {
            width: 20,
            height: 20,
            radius: 5.0,
            cover: true,
            blur: 0.0,
            smooth: false,
        };
        cache.tile(bytes, size).unwrap();
        let larger = cache.tile(bytes, TileSize { width: 40, ..size }).unwrap();
        assert_eq!(larger.width(), 40);
        assert_eq!(cache.entries.len(), 1);
    }
}
