use chrono::{DateTime, Utc};
use serde_json::Value;
use tiny_skia::{Rect, Transform};
use unicode_properties::UnicodeEmoji;
use unicode_segmentation::UnicodeSegmentation;

use crate::{
    RenderInput, RenderOptions, Theme, Warning,
    draw::{Item, Scene, Span, TextEngine, TextStyle},
    icons::Icon,
    model::{Entry, Post, Record, Visibility, aspect, hidden_record, kind, media_images, string},
};

const PAD: f32 = 16.0;
const AVATAR: f32 = 42.0;
const SMALL: f32 = 13.1;
const MEDIUM: f32 = 15.0;
const LARGE: f32 = 16.9;

#[derive(Clone, Copy)]
struct Colors {
    background: u32,
    text: u32,
    muted: u32,
    border: u32,
    panel: u32,
    link: u32,
    control: u32,
    follow: u32,
}
impl Colors {
    fn from_theme(theme: Theme) -> Self {
        let [background, text, muted, border, panel, link] = match theme {
            Theme::Light => [0xffffff, 0x000000, 0x405168, 0xdce2ea, 0xf9fafb, 0x006aff],
            Theme::Dark => [0x000000, 0xffffff, 0xa5b2c5, 0x232e3e, 0x111822, 0x4291ff],
            Theme::Dim => [0x151d28, 0xffffff, 0xabb8c9, 0x2c3a4e, 0x1c2736, 0x4f98ff],
        };
        Self {
            background,
            text,
            muted,
            border,
            panel,
            link,
            follow: match theme {
                Theme::Light => 0x232e3e,
                Theme::Dark => 0xdce2ea,
                Theme::Dim => 0xe2e7ee,
            },
            control: match theme {
                Theme::Light | Theme::Dark => 0x667b99,
                Theme::Dim => 0x6f839f,
            },
        }
    }
}

pub(crate) fn post(
    text: &mut TextEngine,
    input: &RenderInput,
    options: RenderOptions,
    warnings: &mut Vec<Warning>,
) -> Scene {
    let colors = Colors::from_theme(options.theme);
    let mut layout = Layout {
        text,
        colors,
        time_zone: options.time_zone,
        hour12: options.hour12,
        items: Vec::new(),
        warnings,
    };
    let width = options.width as f32 - 2.0 * PAD;
    let selected_visible =
        matches!(&input.selected, Entry::Post(post) if post.visibility() != Visibility::HidePost);
    let parents = input.parents.len().min(options.ancestors);
    let mut y = PAD;
    for parent in input.parents[..parents].iter().rev() {
        y = layout.entry(parent, width, y, true);
    }
    if let Entry::Post(post) = &input.selected
        && selected_visible
        && post.record.reply.is_some()
        && parents == 0
    {
        let parent_handle = match input.parents.first() {
            Some(Entry::Post(post)) if post.visibility() != Visibility::HidePost => {
                Some(post.author.handle.as_str())
            }
            _ => None,
        };
        let label = parent_handle.map_or_else(
            || "Replying to a post".into(),
            |handle| format!("Replying to @{handle}"),
        );
        y += layout
            .label(&label, PAD + 1.0, y, width, SMALL, colors.muted, Some(1))
            .0
            + 8.0;
    }
    y = layout.entry(&input.selected, width, y, false);
    Scene {
        background: colors.background,
        height: y + PAD,
        items: layout.items,
    }
}

struct Layout<'a> {
    text: &'a mut TextEngine,
    colors: Colors,
    time_zone: chrono_tz::Tz,
    hour12: bool,
    items: Vec<Item>,
    warnings: &'a mut Vec<Warning>,
}

impl Layout<'_> {
    fn entry(&mut self, entry: &Entry, w: f32, y: f32, ancestor: bool) -> f32 {
        let label = match entry {
            Entry::Post(post) if post.visibility() != Visibility::HidePost => {
                return if ancestor {
                    self.ancestor(post, w, y)
                } else {
                    self.selected(post, w, y)
                };
            }
            Entry::Post(_) => "This post is unavailable to logged-out viewers",
            Entry::Unavailable(message) => message,
        };
        self.notice(label, PAD, y, w) + if ancestor { 12.0 } else { 0.0 }
    }

    fn selected(&mut self, post: &Post, w: f32, mut y: f32) -> f32 {
        self.avatar(post.author.avatar.as_deref(), PAD, y, AVATAR);
        let header_x = PAD + AVATAR + 12.0;
        let wide = w >= 568.0;
        let follow_width = if wide { 86.0 } else { 68.0 };
        let header_w = w - AVATAR - 24.0 - follow_width;
        let badge = if post.author.verified() { 20.0 } else { 0.0 };
        let (_, name_width) = self.label(
            post.author.name(),
            header_x,
            y,
            header_w - badge,
            LARGE,
            self.colors.text,
            Some(1),
        );
        if badge > 0.0 {
            let x = header_x + name_width + 4.0;
            let y = y + 4.0;
            if post.author.trusted_verifier() {
                self.icon(Icon::Verifier, x, y, 14.0, 0x006aff);
                self.icon(Icon::VerifierCheck, x, y, 14.0, 0xffffff);
            } else {
                self.rect([x, y, 14.0, 14.0], 7.0, Some(0x006aff), None);
                self.icon(Icon::Check, x, y, 14.0, 0xffffff);
            }
        }
        self.label(
            &format!("@{}", post.author.handle),
            header_x,
            y + 22.0,
            header_w,
            MEDIUM,
            self.colors.muted,
            Some(1),
        );
        let follow_x = PAD + w - follow_width;
        let (fill, text) = if wide {
            (self.colors.follow, self.colors.background)
        } else {
            (0xe2e7ee, 0x151d28)
        };
        self.rect(
            [follow_x, y + 4.5, follow_width, 33.0],
            17.0,
            Some(fill),
            None,
        );
        if wide {
            self.icon(Icon::Plus, follow_x + 11.0, y + 13.0, 16.0, text);
        }
        self.text_at(
            &[("Follow".into(), None)],
            [follow_x + if wide { 32.0 } else { 14.0 }, y + 12.5, 49.0],
            TextStyle {
                size: SMALL,
                spacing: 17.0,
                color: text,
                strong: false,
                lines: Some(1),
            },
        );
        y += AVATAR + 12.0;
        y = self.body(post, PAD, y, w, false);
        let indexed = date(if post.indexed_at.is_empty() {
            &post.record.created_at
        } else {
            &post.indexed_at
        });
        let created = date(&post.record.created_at);
        if let (Some(indexed), Some(created)) = (indexed, created)
            && (indexed - created).num_hours() > 24
        {
            y += 4.0;
            y += self
                .label(
                    &format!(
                        "Archived from {}",
                        created.with_timezone(&self.time_zone).format("%b %-d, %Y")
                    ),
                    PAD,
                    y,
                    w,
                    SMALL,
                    self.colors.muted,
                    Some(1),
                )
                .0;
        }
        if let Some(date) = indexed {
            y += if post.embed.is_null() { 12.0 } else { 16.0 };
            let stamp = post_timestamp(date, self.time_zone, self.hour12);
            y += self
                .label(&stamp, PAD, y, w, SMALL, self.colors.muted, Some(1))
                .0;
        }

        let counts = [
            (post.repost_count, "repost"),
            (
                if post.viewer.embedding_disabled {
                    None
                } else {
                    post.quote_count
                },
                "quote",
            ),
            (post.like_count, "like"),
            (post.bookmark_count, "save"),
        ];
        let mut stats = counts
            .into_iter()
            .filter_map(|(n, noun)| n.filter(|n| *n > 0).map(|n| (n, noun)))
            .peekable();
        if stats.peek().is_some() {
            y += 12.0;
            self.rect([PAD, y, w, 1.0], 0.0, Some(self.colors.border), None);
            y += 9.0;
            let mut x = PAD;
            for (n, noun) in stats {
                let style = TextStyle {
                    size: MEDIUM,
                    spacing: 20.0,
                    color: self.colors.muted,
                    strong: false,
                    lines: Some(1),
                };
                let number = self.text.measure(
                    &[(compact_number(n), None)],
                    w,
                    TextStyle {
                        color: self.colors.text,
                        strong: true,
                        ..style
                    },
                );
                let word = if n == 1 {
                    noun.to_owned()
                } else {
                    format!("{noun}s")
                };
                let label = self.text.measure(&[(word, None)], w, style);
                let word_offset = number.width + 4.0;
                let stat_width = word_offset + label.width;
                if x > PAD && x + stat_width > PAD + w {
                    x = PAD;
                    y += 24.0;
                }
                self.items.push(Item::Text {
                    block: number,
                    x,
                    y,
                });
                self.items.push(Item::Text {
                    block: label,
                    x: x + word_offset,
                    y,
                });
                x += stat_width + 16.0;
            }
            y += 34.0;
        } else {
            y += 12.0;
        }
        self.controls(post, PAD, y, w, false);
        y + 20.0
    }

    fn ancestor(&mut self, post: &Post, w: f32, y: f32) -> f32 {
        let x = PAD + AVATAR + 12.0;
        let inner = w - AVATAR - 12.0;
        self.avatar(post.author.avatar.as_deref(), PAD, y, AVATAR);
        let meta = format!("{}  @{}", post.author.name(), post.author.handle);
        let top = y
            + self
                .label(&meta, x, y, inner, MEDIUM, self.colors.text, Some(1))
                .0
            + 4.0;
        let bottom = self.body(post, x, top, inner, true) + 4.0;
        self.controls(post, x, bottom, inner, true);
        let end = (bottom + 34.0).max(y + AVATAR + 18.0);
        self.rect(
            [
                PAD + AVATAR / 2.0 - 1.0,
                y + AVATAR + 4.0,
                2.0,
                end - y - AVATAR - 4.0,
            ],
            0.0,
            Some(self.colors.border),
            None,
        );
        end
    }

    fn body(&mut self, post: &Post, x: f32, mut y: f32, w: f32, compact: bool) -> f32 {
        let visibility = post.visibility();
        if visibility == Visibility::HideMedia
            || post.labels.iter().any(|l| !l.val.is_empty() && !l.neg)
        {
            y = self.notice("Content warning", x, y, w) + 8.0;
        }
        if !post.record.text.is_empty() {
            let spans = spans(&post.record, self.colors.link, self.warnings);
            let size = if compact { MEDIUM } else { LARGE };
            let size = if post.record.facets.is_empty() && emoji_only(&post.record.text) {
                size * 1.85
            } else {
                size
            };
            y += self
                .text_at(
                    &spans,
                    [x, y, w],
                    TextStyle {
                        size,
                        spacing: (size * 1.3).round(),
                        color: self.colors.text,
                        strong: false,
                        lines: compact.then_some(10),
                    },
                )
                .0;
        }
        if !post.embed.is_null() {
            if visibility == Visibility::Show {
                let gap = if compact
                    || post.record.text.is_empty()
                    || kind(&post.embed) == "app.bsky.embed.video#view"
                {
                    8.0
                } else {
                    12.0
                };
                y = self.embed(&post.embed, x, y + gap, w, 0, compact);
            } else {
                y = self.notice("Hidden media", x, y + 8.0, w);
            }
        }
        y
    }

    fn embed(
        &mut self,
        embed: &Value,
        x: f32,
        y: f32,
        w: f32,
        quotes: usize,
        compact: bool,
    ) -> f32 {
        match kind(embed) {
            "app.bsky.embed.images#view" | "app.bsky.embed.gallery#view" => {
                self.images(embed, x, y, w, compact)
            }
            "app.bsky.embed.video#view" => {
                let h = (w / aspect(embed).unwrap_or(1.0).clamp(0.5, 5.0)).min(w);
                self.rect([x, y, w, h], 12.0, Some(0x232e3e), None);
                self.image(as_url(embed, "thumbnail"), [x, y, w, h], 12.0, false);
                if !string(embed, "alt").is_empty() {
                    self.alt_badge(x + w - 27.0, y + 4.0);
                }
                let k = if string(embed, "presentation") == "gif" {
                    Icon::Gif
                } else {
                    Icon::Play
                };
                self.media_overlay(k, x, y, w, h);
                y + h
            }
            "app.bsky.embed.external#view" => self.external(&embed["external"], x, y, w),
            "app.bsky.embed.record#view" if quotes == 0 => self.quote(&embed["record"], x, y, w),
            "app.bsky.embed.record#view" => y - 8.0,
            "app.bsky.embed.recordWithMedia#view" => {
                let y = self.embed(&embed["media"], x, y, w, quotes, compact);
                if quotes != 0 {
                    return y;
                }
                self.embed(&embed["record"], x, y + 8.0, w, quotes, compact)
            }
            _ => {
                self.warnings
                    .push(Warning::UnsupportedEmbed(kind(embed).to_owned()));
                self.notice("Unsupported attachment", x, y, w)
            }
        }
    }

    fn images(&mut self, embed: &Value, x: f32, y: f32, w: f32, compact: bool) -> f32 {
        let images = media_images(embed);
        let gap = 4.0;
        let half = (w - gap) / 2.0;
        let mut pixels = |i: usize, frame: [f32; 4], radius, cover| {
            let Some((meta, url)) = images.get(i) else {
                return;
            };
            self.media((!url.is_empty()).then_some(*url), frame, radius, cover);
            if !string(meta, "alt").is_empty() {
                let [x, y, w, h] = frame;
                self.alt_badge(x + w - 27.0, y + h - 22.0);
            }
        };
        match images.len() {
            0 => self.notice("Image unavailable", x, y, w),
            1 => {
                let a = aspect(images[0].0)
                    .unwrap_or(1.0)
                    .clamp(if compact { 0.5 } else { 0.25 }, 5.0);
                let h = if compact { (w / a).min(w) } else { w / a };
                let full = if compact { w.min(h * a) } else { w };
                pixels(0, [x, y, full, h], 12.0, compact);
                y + h
            }
            2 | 3 => {
                pixels(0, [x, y, half, half], 8.0, true);
                if images.len() == 2 {
                    pixels(1, [x + half + gap, y, half, half], 8.0, true);
                } else {
                    let h = (half - gap) / 2.0;
                    pixels(1, [x + half + gap, y, half, h], 8.0, true);
                    pixels(2, [x + half + gap, y + h + gap, half, h], 8.0, true);
                }
                y + half
            }
            4 => {
                let h = half / 1.5;
                for i in 0..4 {
                    pixels(
                        i,
                        [
                            x + (i % 2) as f32 * (half + gap),
                            y + (i / 2) as f32 * (h + gap),
                            half,
                            h,
                        ],
                        8.0,
                        true,
                    );
                }
                y + h * 2.0 + gap
            }
            count => {
                let h = (w * 0.47).min(320.0);
                let first = (h * aspect(images[0].0).unwrap_or(1.0).clamp(2.0 / 3.0, 1.5)).floor();
                pixels(0, [x, y, first, h], 12.0, true);
                pixels(1, [x + first + 8.0, y, w - first - 8.0, h], 12.0, true);
                let dots_x = x + (w - (count * 10) as f32) / 2.0;
                for i in 0..count {
                    self.rect(
                        [dots_x + i as f32 * 10.0, y + h + 8.0, 6.0, 6.0],
                        3.0,
                        Some(if i == 0 {
                            self.colors.muted
                        } else {
                            self.colors.border
                        }),
                        None,
                    );
                }
                y + h + 18.0
            }
        }
    }

    fn external(&mut self, card: &Value, x: f32, y: f32, w: f32) -> f32 {
        if hidden_record(card) {
            return self.notice("Link preview hidden", x, y, w);
        }
        let uri = string(card, "uri");
        let host = url::Url::parse(uri)
            .ok()
            .and_then(|u| u.host_str().map(str::to_owned))
            .unwrap_or_default();
        let thumbnail = as_url(card, "thumb");
        let gif = matches!(
            host.as_str(),
            "tenor.com"
                | "www.tenor.com"
                | "media.tenor.com"
                | "giphy.com"
                | "www.giphy.com"
                | "klipy.com"
                | "www.klipy.com"
        ) || host.ends_with(".giphy.com")
            || uri.split('?').next().is_some_and(|s| s.ends_with(".gif"));
        if gif {
            let h = w * 0.65;
            self.media(thumbnail, [x, y, w, h], 12.0, false);
            self.media_overlay(Icon::Gif, x, y, w, h);
            return y + h;
        }
        let mut cursor = y;
        if let Some(url) = thumbnail {
            let h = w / 1.91;
            self.media(Some(url), [x, y, w, h], 12.0, true);
            if matches!(
                host.as_str(),
                "youtube.com"
                    | "www.youtube.com"
                    | "m.youtube.com"
                    | "music.youtube.com"
                    | "youtu.be"
                    | "vimeo.com"
                    | "www.twitch.tv"
                    | "twitch.tv"
                    | "clips.twitch.tv"
                    | "open.spotify.com"
                    | "soundcloud.com"
                    | "music.apple.com"
            ) {
                self.media_overlay(Icon::Play, x, y, w, h);
            }
            cursor += h;
        }
        cursor += 8.0;
        let title = string(card, "title");
        cursor += self
            .label(
                if title.is_empty() { uri } else { title },
                x + 12.0,
                cursor,
                w - 24.0,
                MEDIUM,
                self.colors.text,
                Some(2),
            )
            .0;
        let description = string(card, "description");
        if !description.is_empty() {
            cursor += 2.0
                + self
                    .label(
                        description,
                        x + 12.0,
                        cursor + 2.0,
                        w - 24.0,
                        SMALL,
                        self.colors.text,
                        Some(2),
                    )
                    .0;
        }
        if !host.is_empty() {
            cursor += 4.0
                + self
                    .label(
                        &host,
                        x + 12.0,
                        cursor + 4.0,
                        w - 24.0,
                        SMALL,
                        self.colors.muted,
                        Some(1),
                    )
                    .0;
        }
        cursor += 8.0;
        self.rect([x, y, w, cursor - y], 12.0, None, Some(self.colors.border));
        cursor
    }

    fn quote(&mut self, record: &Value, x: f32, y: f32, w: f32) -> f32 {
        let message = if hidden_record(record) {
            Some("Quoted post hidden")
        } else {
            match kind(record) {
                "app.bsky.embed.record#viewNotFound" => Some("Deleted"),
                "app.bsky.embed.record#viewBlocked" => Some("Blocked"),
                "app.bsky.embed.record#viewDetached" => Some("Removed by the author"),
                _ => None,
            }
        };
        if let Some(message) = message {
            return self.notice(message, x, y, w);
        }
        let left = x + 12.0;
        let mut top = y + 10.0;
        let inner = w - 24.0;
        match kind(record) {
            "app.bsky.embed.record#viewRecord" => {
                let author = &record["author"];
                self.avatar(as_url(author, "avatar"), left, top, 20.0);
                let handle = string(author, "handle");
                let name = string(author, "displayName");
                let author_line = if name.is_empty() {
                    format!("@{handle}")
                } else {
                    format!("{name}  @{handle}")
                };
                self.label(
                    &author_line,
                    left + 26.0,
                    top + 1.0,
                    inner - 26.0,
                    SMALL,
                    self.colors.text,
                    Some(1),
                );
                top += 25.0;
                if let Ok(quoted) = serde_json::from_value::<Record>(record["value"].clone())
                    && !quoted.text.is_empty()
                    && quoted.text.len() <= 3000
                {
                    let spans = spans(&quoted, self.colors.link, self.warnings);
                    top += self
                        .text_at(
                            &spans,
                            [left, top, inner],
                            TextStyle {
                                size: MEDIUM,
                                spacing: 19.5,
                                color: self.colors.text,
                                strong: false,
                                lines: Some(6),
                            },
                        )
                        .0;
                }
                if let Some(media) = record["embeds"].as_array().and_then(|a| a.first()) {
                    top = self.embed(media, left, top + 6.0, inner, 1, true);
                }
            }
            "app.bsky.feed.defs#generatorView"
            | "app.bsky.graph.defs#listView"
            | "app.bsky.graph.defs#starterPackViewBasic" => {
                let is_feed = kind(record) == "app.bsky.feed.defs#generatorView";
                let is_list = kind(record) == "app.bsky.graph.defs#listView";
                let category = if is_feed {
                    "Feed"
                } else if is_list {
                    "List"
                } else {
                    "Starter pack"
                };
                let title = if is_list {
                    string(record, "name")
                } else if is_feed {
                    string(record, "displayName")
                } else {
                    string(&record["record"], "name")
                };
                let by = string(&record["creator"], "handle");
                self.avatar(
                    if is_feed || is_list {
                        as_url(record, "avatar")
                    } else {
                        None
                    },
                    left,
                    top,
                    34.0,
                );
                self.label(
                    if title.is_empty() { category } else { title },
                    left + 42.0,
                    top,
                    inner - 42.0,
                    MEDIUM,
                    self.colors.text,
                    Some(1),
                );
                self.label(
                    &format!("{category} by @{by}"),
                    left + 42.0,
                    top + 19.0,
                    inner - 42.0,
                    SMALL,
                    self.colors.muted,
                    Some(1),
                );
                top += 39.0;
                if is_feed || is_list {
                    let desc = string(record, "description");
                    if !desc.is_empty() {
                        top += 4.0
                            + self
                                .label(
                                    desc,
                                    left,
                                    top + 4.0,
                                    inner,
                                    SMALL,
                                    self.colors.text,
                                    Some(2),
                                )
                                .0;
                    }
                }
            }
            "app.bsky.labeler.defs#labelerView" => return y - 8.0,
            _ => {
                self.warnings
                    .push(Warning::UnsupportedEmbed(kind(record).to_owned()));
                return self.notice("Unsupported quoted record", x, y, w);
            }
        }
        top += 10.0;
        self.rect([x, y, w, top - y], 12.0, None, Some(self.colors.border));
        top
    }

    fn controls(&mut self, post: &Post, x: f32, y: f32, w: f32, compact: bool) {
        let icons = [
            (Icon::Reply, post.reply_count),
            (
                Icon::Repost,
                Some(
                    post.repost_count
                        .unwrap_or(0)
                        .saturating_add(post.quote_count.unwrap_or(0)),
                ),
            ),
            (Icon::Heart, post.like_count),
            (Icon::Bookmark, None),
            (Icon::Share, None),
            (Icon::More, None),
        ];
        let size = if compact { 18.0 } else { 22.0 };
        let step = if compact {
            w / 6.0
        } else {
            ((w - 36.0) / 4.0).min(107.33)
        };
        for (i, (icon, count)) in icons.into_iter().enumerate() {
            let left = if compact || i < 3 {
                x + i as f32 * step
            } else {
                x + w - 28.0 - (5 - i) as f32 * 40.0
            };
            self.icon(icon, left, y, size, self.colors.control);
            if let Some(n) = count.filter(|n| *n > 0) {
                let label = compact_number(n);
                self.label(
                    &label,
                    left + size + if compact { 3.0 } else { 2.0 },
                    y,
                    step - size - 4.0,
                    if compact { SMALL } else { MEDIUM },
                    self.colors.control,
                    Some(1),
                );
            }
        }
    }

    fn alt_badge(&mut self, x: f32, y: f32) {
        self.rect([x, y, 23.0, 18.0], 7.0, Some(0x232e3e), None);
        self.label("ALT", x + 4.0, y + 4.0, 18.0, 8.0, 0xffffff, Some(1));
    }

    fn media_overlay(&mut self, icon: Icon, x: f32, y: f32, w: f32, h: f32) {
        if matches!(icon, Icon::Gif) {
            self.rect(
                [x + 8.0, y + h - 33.0, 36.0, 25.0],
                5.0,
                Some(0x232e3e),
                None,
            );
            self.label(
                "GIF",
                x + 14.0,
                y + h - 29.0,
                27.0,
                SMALL,
                0xffffff,
                Some(1),
            );
        } else {
            let (cx, cy) = (x + w / 2.0, y + h / 2.0);
            self.rect(
                [cx - 21.0, cy - 21.0, 42.0, 42.0],
                21.0,
                Some(0x232e3e),
                None,
            );
            self.icon(Icon::Play, cx - 11.0, cy - 11.0, 22.0, 0xffffff);
        }
    }

    fn avatar(&mut self, url: Option<&str>, x: f32, y: f32, size: f32) {
        self.rect(
            [x, y, size, size],
            size / 2.0,
            Some(self.colors.panel),
            Some(self.colors.border),
        );
        self.icon(
            Icon::Person,
            x + size * 0.22,
            y + size * 0.22,
            size * 0.56,
            self.colors.muted,
        );
        self.image(url, [x, y, size, size], size / 2.0, true);
    }

    fn notice(&mut self, label: &str, x: f32, y: f32, w: f32) -> f32 {
        let at = self.items.len();
        let h =
            self.label(
                label,
                x + 10.0,
                y + 8.0,
                w - 20.0,
                MEDIUM,
                self.colors.muted,
                Some(2),
            )
            .0 + 16.0;
        if let Some(rect) = Rect::from_xywh(x, y, w, h) {
            self.items.insert(
                at,
                Item::Box {
                    rect,
                    radius: 8.0,
                    fill: Some(self.colors.panel),
                    stroke: Some(self.colors.border),
                },
            );
        }
        y + h
    }

    fn media(&mut self, url: Option<&str>, frame: [f32; 4], radius: f32, cover: bool) {
        self.rect(frame, radius, Some(self.colors.panel), None);
        self.image(url, frame, radius, cover);
    }

    fn image(&mut self, url: Option<&str>, [x, y, w, h]: [f32; 4], radius: f32, cover: bool) {
        if let Some(url) = url
            && let Some(rect) = Rect::from_xywh(x, y, w, h)
        {
            self.items.push(Item::Image {
                rect,
                radius,
                url: url.to_owned(),
                cover,
            });
        }
    }

    fn rect(
        &mut self,
        [x, y, w, h]: [f32; 4],
        radius: f32,
        fill: Option<u32>,
        stroke: Option<u32>,
    ) {
        if let Some(rect) = Rect::from_xywh(x, y, w, h) {
            self.items.push(Item::Box {
                rect,
                radius,
                fill,
                stroke,
            });
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn label(
        &mut self,
        text: &str,
        x: f32,
        y: f32,
        w: f32,
        size: f32,
        color: u32,
        lines: Option<usize>,
    ) -> (f32, f32) {
        let text = text
            .chars()
            .take(640)
            .collect::<String>()
            .replace(['\n', '\r'], " ");
        self.text_at(
            &[(text, None)],
            [x, y, w],
            TextStyle {
                size,
                spacing: (size * 1.3).round(),
                color,
                strong: size == LARGE,
                lines,
            },
        )
    }

    fn text_at(&mut self, spans: &[Span], [x, y, w]: [f32; 3], style: TextStyle) -> (f32, f32) {
        let block = self.text.measure(spans, w, style);
        let metrics = (block.height, block.width);
        self.items.push(Item::Text { block, x, y });
        metrics
    }

    fn icon(&mut self, icon: Icon, x: f32, y: f32, size: f32, color: u32) {
        let transform = Transform::from_scale(size / 24.0, size / 24.0).post_translate(x, y);
        if let Some(path) = icon.path().and_then(|path| path.transform(transform)) {
            self.items.push(Item::Path { path, color });
        }
    }
}

fn date(value: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(value)
        .ok()
        .map(|time| time.to_utc())
}

fn post_timestamp(date: DateTime<Utc>, zone: chrono_tz::Tz, hour12: bool) -> String {
    let pattern = if hour12 {
        "%-I:%M %p · %b %-d, %Y"
    } else {
        "%H:%M · %b %-d, %Y"
    };
    date.with_timezone(&zone).format(pattern).to_string()
}

fn as_url<'a>(value: &'a Value, field: &str) -> Option<&'a str> {
    value[field].as_str().filter(|s| !s.is_empty())
}

fn emoji_only(text: &str) -> bool {
    let mut any = false;
    for cluster in text.graphemes(true).filter(|g| !g.trim().is_empty()) {
        let emoji = cluster
            .chars()
            .any(|c| c.is_emoji_char() && !c.is_ascii() && !matches!(c, '™' | '©' | '®'))
            || cluster.contains('\u{20e3}')
            || cluster.contains('\u{fe0f}');
        if !emoji {
            return false;
        }
        any = true;
    }
    any
}

fn spans(record: &Record, link: u32, warnings: &mut Vec<Warning>) -> Vec<Span> {
    if record.facets.len() > 300 {
        warnings.push(Warning::InvalidFacet);
    }
    let mut facets = record.facets.iter().take(300).collect::<Vec<_>>();
    facets.sort_by_key(|facet| facet["index"]["byteStart"].as_u64());
    let mut spans = Vec::new();
    let mut start = 0;
    for facet in facets {
        let Some((a, b)) = facet["index"]["byteStart"]
            .as_u64()
            .zip(facet["index"]["byteEnd"].as_u64())
            .and_then(|(a, b)| Some((a.try_into().ok()?, b.try_into().ok()?)))
            .filter(|(a, b)| a < b && *a >= start && record.text.get(*a..*b).is_some())
        else {
            warnings.push(Warning::InvalidFacet);
            continue;
        };
        let feature = facet["features"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|f| {
                matches!(
                    kind(f),
                    "app.bsky.richtext.facet#mention"
                        | "app.bsky.richtext.facet#link"
                        | "app.bsky.richtext.facet#tag"
                )
            });
        let Some(feature) = feature else {
            continue;
        };
        if a > start {
            spans.push((record.text[start..a].to_owned(), None));
        }
        let text = &record.text[a..b];
        let (display, color) = match kind(feature) {
            "app.bsky.richtext.facet#link" => (
                short_url(text),
                url::Url::parse(string(feature, "uri"))
                    .ok()
                    .filter(|u| matches!(u.scheme(), "http" | "https"))
                    .map(|_| link),
            ),
            "app.bsky.richtext.facet#mention" => (
                text.to_owned(),
                string(feature, "did").starts_with("did:").then_some(link),
            ),
            _ => (text.to_owned(), Some(link)),
        };
        spans.push((display, color));
        start = b;
    }
    if start < record.text.len() {
        spans.push((record.text[start..].to_owned(), None));
    }
    spans
}

fn short_url(text: &str) -> String {
    let Ok(url) = url::Url::parse(text) else {
        return text.to_owned();
    };
    let Some(host) = url
        .host_str()
        .filter(|_| matches!(url.scheme(), "http" | "https"))
    else {
        return text.to_owned();
    };
    let host = url
        .port()
        .map_or_else(|| host.to_owned(), |port| format!("{host}:{port}"));
    let mut path = if url.path() == "/" {
        String::new()
    } else {
        url.path().to_owned()
    };
    if let Some(query) = url.query() {
        path += &format!("?{query}");
    }
    if let Some(hash) = url.fragment() {
        path += &format!("#{hash}");
    }
    if path.encode_utf16().count() > 15 {
        path = path
            .chars()
            .scan(0, |used, c| {
                *used += c.len_utf16();
                (*used <= 13).then_some(c)
            })
            .collect::<String>()
            + "...";
    }
    host + &path
}

fn compact_number(n: u64) -> String {
    let (step, suffix) = match n {
        0..=999 => return n.to_string(),
        1_000..=999_999 => (1_000, "K"),
        1_000_000..=999_999_999 => (1_000_000, "M"),
        1_000_000_000..=999_999_999_999 => (1_000_000_000, "B"),
        _ => (1_000_000_000_000, "T"),
    };
    let whole = n / step;
    let tenths = n % step / (step / 10);
    if n < 10_000 && tenths != 0 {
        format!("{whole}.{tenths}{suffix}")
    } else {
        format!("{whole}{suffix}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timestamp_uses_the_posts_daylight_offset_and_hour_cycle() {
        let zone = chrono_tz::America::Detroit;
        let before = date("2025-03-09T06:30:00Z").unwrap();
        let after = date("2025-03-09T07:30:00Z").unwrap();
        assert_eq!(post_timestamp(before, zone, false), "01:30 · Mar 9, 2025");
        assert_eq!(post_timestamp(after, zone, true), "3:30 AM · Mar 9, 2025");
    }
}
