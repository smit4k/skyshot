use std::{
    collections::{BTreeMap, BTreeSet},
    io::Cursor,
};

use cosmic_text::{
    Attrs, Buffer, Color, Ellipsize, EllipsizeHeightLimit, Fallback, Family, FeatureTag,
    FontFeatures, FontSystem, Metrics, Shaping, SwashCache, Weight, Wrap, fontdb,
};
use image::{ImageDecoder, ImageEncoder, ImageReader, codecs::png::PngEncoder};
use tiny_skia::{
    Color as Ink, FillRule, FilterQuality, Mask, Paint, Path, PathBuilder, Pixmap, PixmapPaint,
    PremultipliedColorU8, Rect, Stroke, Transform,
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
    pub strong: bool,
    pub lines: Option<usize>,
}

pub(crate) struct TextBlock {
    buffer: Buffer,
    pub height: f32,
    pub width: f32,
}
pub(crate) type Span = (String, Option<u32>);

impl TextEngine {
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
        })
    }

    pub fn measure(&mut self, spans: &[Span], width: f32, style: TextStyle) -> TextBlock {
        let mut buffer = Buffer::new_empty(Metrics::new(style.size, style.spacing));
        let mut features = FontFeatures::new();
        features.disable(FeatureTag::new(b"calt"));
        let attrs = Attrs::new()
            .family(Family::Name(&self.primary))
            .font_features(features)
            .weight(if style.strong {
                Weight::SEMIBOLD
            } else {
                Weight::NORMAL
            })
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

pub(crate) struct Scene {
    pub background: u32,
    pub height: f32,
    pub items: Vec<Item>,
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
            } => {
                let Some(bytes) = assets.get(&url) else {
                    warnings.push(Warning::MissingAsset(url));
                    continue;
                };
                if counted.insert(url.clone()) {
                    asset_bytes = asset_bytes.saturating_add(bytes.len());
                }
                if asset_bytes > MAX_ASSETS {
                    return Err(RenderError::Limit("total image bytes"));
                }
                if !image(&mut pix, rect, radius, scale, bytes, cover) {
                    warnings.push(Warning::InvalidAsset(url));
                }
            }
        }
    }
    let mut png = Vec::new();
    PngEncoder::new(&mut png).write_image(
        pix.data(),
        width,
        height,
        image::ExtendedColorType::Rgba8,
    )?;
    Ok((png, width, height))
}

fn decode(bytes: &[u8]) -> Option<image::RgbaImage> {
    if bytes.len() > MAX_BYTES {
        return None;
    }
    let mut reader = ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .ok()?;
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
    let mut raster = image::DynamicImage::from_decoder(decoder).ok()?;
    raster.apply_orientation(orientation);
    Some(raster.into_rgba8())
}

fn image(pix: &mut Pixmap, rect: Rect, radius: f32, scale: f32, bytes: &[u8], cover: bool) -> bool {
    let Some(raster) = decode(bytes) else {
        return false;
    };
    let (w, h) = raster.dimensions();
    let mut data = raster.into_raw();
    for rgba in data.chunks_exact_mut(4) {
        let a = u32::from(rgba[3]);
        for c in &mut rgba[..3] {
            *c = ((u32::from(*c) * a + 127) / 255) as u8;
        }
    }
    let Some(source) = tiny_skia::PixmapRef::from_bytes(&data, w, h) else {
        return false;
    };
    let x = (rect.x() * scale).round() as i32;
    let y = (rect.y() * scale).round() as i32;
    let dw = ((rect.right() * scale).round() as i32 - x).max(1) as u32;
    let dh = ((rect.bottom() * scale).round() as i32 - y).max(1) as u32;
    let (sx, sy) = (dw as f32 / w as f32, dh as f32 / h as f32);
    let k = if cover { sx.max(sy) } else { sx.min(sy) };
    let Some(mut tile) = Pixmap::new(dw, dh) else {
        return false;
    };
    let t = Transform::from_scale(k, k).post_translate(
        (dw as f32 - w as f32 * k) / 2.0,
        (dh as f32 - h as f32 * k) / 2.0,
    );
    tile.draw_pixmap(
        0,
        0,
        source,
        &PixmapPaint {
            quality: FilterQuality::Bilinear,
            ..PixmapPaint::default()
        },
        t,
        None,
    );
    if radius > 0.0
        && let Some(mut mask) = Mask::new(dw, dh)
        && let Some(rect) = Rect::from_xywh(0.0, 0.0, dw as f32, dh as f32)
        && let Some(path) = rounded(rect, radius * scale)
    {
        mask.fill_path(&path, FillRule::Winding, true, Transform::identity());
        tile.apply_mask(&mask);
    }
    pix.draw_pixmap(
        x,
        y,
        tile.as_ref(),
        &PixmapPaint::default(),
        Transform::identity(),
        None,
    );
    true
}
