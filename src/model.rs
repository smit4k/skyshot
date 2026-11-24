use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::RenderError;

pub(crate) const MAX_JSON: usize = 2 * 1024 * 1024;
pub(crate) const MAX_BYTES: usize = 8 * 1024 * 1024;
pub(crate) const MAX_ASSETS: usize = 32 * 1024 * 1024;

/// Bluesky web color mode.
#[derive(Clone, Copy, Debug, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Theme {
    #[default]
    Light,
    Dark,
    Dim,
}

/// Size, thread context, and date display. Width is in CSS pixels.
#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct RenderOptions {
    pub width: u32,
    pub scale: u32,
    pub theme: Theme,
    pub ancestors: usize,
    /// Time zone for the post timestamp. The Rust library defaults to UTC.
    pub time_zone: chrono_tz::Tz,
    /// Use a 12-hour clock.
    pub hour12: bool,
}
impl Default for RenderOptions {
    fn default() -> Self {
        Self {
            width: 600,
            scale: 2,
            theme: Theme::Light,
            ancestors: 0,
            time_zone: chrono_tz::UTC,
            hour12: true,
        }
    }
}
impl RenderOptions {
    pub(crate) fn validate(self) -> Result<(), RenderError> {
        if !(280..=1000).contains(&self.width)
            || !(1..=3).contains(&self.scale)
            || self.ancestors > 3
        {
            return Err(RenderError::Input(
                "width must be 280–1000, scale 1–3, and ancestors 0–3",
            ));
        }
        Ok(())
    }
}

/// An issue that did not prevent rendering.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "code", content = "detail", rename_all = "camelCase")]
pub enum Warning {
    InvalidFacet,
    MissingAsset(String),
    InvalidAsset(String),
    UnsupportedEmbed(String),
}

pub(crate) enum Entry {
    Post(Box<Post>),
    Unavailable(&'static str),
}

/// A selected post and up to three available ancestors, parsed from AppView JSON.
pub struct RenderInput {
    pub(crate) selected: Entry,
    pub(crate) parents: Vec<Entry>,
}
impl RenderInput {
    /// Accept a PostView or a getPostThread response.
    pub fn from_json(json: &str) -> Result<Self, RenderError> {
        if json.len() > MAX_JSON {
            return Err(RenderError::Limit("post JSON"));
        }
        let raw: Value = serde_json::from_str(json)?;
        let Some(thread) = raw.get("thread") else {
            return Ok(Self {
                selected: Entry::Post(Box::new(parse_post(&raw)?)),
                parents: vec![],
            });
        };
        let mut cursor = thread;
        let mut parents = Vec::new();
        while let Some(parent) = cursor.get("parent").filter(|v| !v.is_null()) {
            parents.push(parse_entry(parent)?);
            if parents.len() == 3 {
                break;
            }
            cursor = parent;
        }
        Ok(Self {
            selected: parse_entry(thread)?,
            parents,
        })
    }

    /// Source URLs for visible assets. Hosts decide whether to fetch them.
    pub fn asset_urls(&self, options: RenderOptions) -> Result<Vec<String>, RenderError> {
        options.validate()?;
        let mut urls = BTreeSet::new();
        for entry in
            std::iter::once(&self.selected).chain(self.parents.iter().take(options.ancestors))
        {
            if let Entry::Post(post) = entry {
                post.assets(&mut urls);
            }
        }
        Ok(urls.into_iter().map(str::to_owned).collect())
    }
}

fn parse_entry(raw: &Value) -> Result<Entry, RenderError> {
    match kind(raw) {
        "app.bsky.feed.defs#notFoundPost" => Ok(Entry::Unavailable("Post not found")),
        "app.bsky.feed.defs#blockedPost" => Ok(Entry::Unavailable("Post blocked")),
        "app.bsky.feed.defs#threadViewPost" | "" => {
            Ok(Entry::Post(Box::new(parse_post(&raw["post"])?)))
        }
        _ => Err(RenderError::Input("unsupported thread item")),
    }
}

fn parse_post(raw: &Value) -> Result<Post, RenderError> {
    let post: Post = deserialize(raw)?;
    if post.author.handle.is_empty() || post.record.text.len() > 3000 {
        return Err(RenderError::Input(
            "missing author handle or oversized post text",
        ));
    }
    if !matches!(
        post.record.kind.as_deref(),
        None | Some("app.bsky.feed.post")
    ) {
        return Err(RenderError::Input("record is not a Bluesky post"));
    }
    Ok(post)
}

fn deserialize<T: serde::de::DeserializeOwned>(value: &Value) -> Result<T, serde_json::Error> {
    T::deserialize(value)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Post {
    pub author: Author,
    pub record: Record,
    #[serde(default)]
    pub indexed_at: String,
    #[serde(default)]
    pub labels: Vec<Label>,
    #[serde(default)]
    pub embed: Value,
    #[serde(default)]
    pub reply_count: Option<u64>,
    #[serde(default)]
    pub repost_count: Option<u64>,
    #[serde(default)]
    pub like_count: Option<u64>,
    #[serde(default)]
    pub quote_count: Option<u64>,
    #[serde(default)]
    pub bookmark_count: Option<u64>,
    #[serde(default)]
    pub viewer: Viewer,
}

#[derive(Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
pub(crate) struct Viewer {
    pub embedding_disabled: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Author {
    pub handle: String,
    #[serde(default)]
    pub display_name: String,
    pub avatar: Option<String>,
    #[serde(default)]
    pub labels: Vec<Label>,
    #[serde(default)]
    pub verification: Value,
}
impl Author {
    pub fn name(&self) -> &str {
        if self.display_name.trim().is_empty() {
            &self.handle
        } else {
            self.display_name.trim()
        }
    }
    pub fn verified(&self) -> bool {
        matches!(self.verification["verifiedStatus"].as_str(), Some("valid"))
            || self.trusted_verifier()
    }
    pub fn trusted_verifier(&self) -> bool {
        self.verification["trustedVerifierStatus"].as_str() == Some("valid")
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Record {
    #[serde(rename = "$type")]
    kind: Option<String>,
    pub text: String,
    pub created_at: String,
    #[serde(default)]
    pub facets: Vec<Value>,
    #[serde(default)]
    pub reply: Option<Value>,
    #[serde(default)]
    labels: SelfLabels,
}

#[derive(Default, Deserialize)]
struct SelfLabels {
    #[serde(default)]
    values: Vec<Label>,
}

#[derive(Deserialize)]
pub(crate) struct Label {
    pub val: String,
    #[serde(default)]
    pub neg: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Visibility {
    Show,
    HideMedia,
    HidePost,
}

fn visibility<'a>(labels: impl Iterator<Item = &'a Label>) -> Visibility {
    let mut result = Visibility::Show;
    for label in labels.filter(|l| !l.neg) {
        match label.val.as_str() {
            "!hide"
            | "!warn"
            | "!takedown"
            | "!no-unauthenticated"
            | "porn"
            | "sexual"
            | "nudity"
            | "graphic-media" => return Visibility::HidePost,
            "gore" | "sexual-figurative" | "self-harm" | "sensitive" => {
                result = Visibility::HideMedia
            }
            _ => {}
        }
    }
    result
}

impl Post {
    pub fn visibility(&self) -> Visibility {
        visibility(
            self.author
                .labels
                .iter()
                .chain(&self.labels)
                .chain(&self.record.labels.values),
        )
    }
    fn assets<'a>(&'a self, urls: &mut BTreeSet<&'a str>) {
        if self.visibility() == Visibility::HidePost {
            return;
        }
        urls.extend(self.author.avatar.as_deref());
        if self.visibility() == Visibility::Show {
            embed_assets(&self.embed, urls, 0);
        }
    }
}

pub(crate) fn kind(value: &Value) -> &str {
    string(value, "$type")
}

pub(crate) fn string<'a>(value: &'a Value, key: &str) -> &'a str {
    value[key].as_str().unwrap_or_default()
}

pub(crate) fn aspect(value: &Value) -> Option<f32> {
    let w = value["aspectRatio"]["width"].as_u64()?;
    let h = value["aspectRatio"]["height"].as_u64()?;
    (w > 0 && h > 0).then(|| w as f32 / h as f32)
}

pub(crate) fn hidden_record(raw: &Value) -> bool {
    // A malformed moderation field must not expose the record.
    [
        &raw["labels"],
        &raw["author"]["labels"],
        &raw["value"]["labels"]["values"],
    ]
    .into_iter()
    .any(|field| {
        !field.is_null()
            && deserialize::<Vec<Label>>(field)
                .map_or(true, |labels| visibility(labels.iter()) != Visibility::Show)
    })
}

pub(crate) fn media_images(embed: &Value) -> Vec<(&Value, &str)> {
    let gallery = kind(embed) == "app.bsky.embed.gallery#view";
    let list = if gallery {
        &embed["items"]
    } else {
        &embed["images"]
    };
    list.as_array()
        .into_iter()
        .flatten()
        .take(if gallery { 20 } else { 4 })
        .filter(|item| !gallery || kind(item) == "app.bsky.embed.gallery#viewImage")
        .map(|item| {
            (
                item,
                string(item, if gallery { "thumbnail" } else { "thumb" }),
            )
        })
        .collect()
}

fn embed_assets<'a>(embed: &'a Value, urls: &mut BTreeSet<&'a str>, quotes: usize) {
    let mut add = |value: &'a Value, key| {
        let url = string(value, key);
        if !url.is_empty() {
            urls.insert(url);
        }
    };
    match kind(embed) {
        "app.bsky.embed.images#view" | "app.bsky.embed.gallery#view" => {
            let images = media_images(embed);
            for (_, url) in images.iter().take(if images.len() > 4 { 2 } else { 4 }) {
                if !url.is_empty() {
                    urls.insert(url);
                }
            }
        }
        "app.bsky.embed.external#view" if !hidden_record(&embed["external"]) => {
            add(&embed["external"], "thumb")
        }
        "app.bsky.embed.video#view" => add(embed, "thumbnail"),
        "app.bsky.embed.record#view" if quotes == 0 => {
            let record = &embed["record"];
            if hidden_record(record) {
                return;
            }
            match kind(record) {
                "app.bsky.embed.record#viewRecord" => {
                    add(&record["author"], "avatar");
                    if let Some(media) = record["embeds"].as_array().and_then(|a| a.first()) {
                        embed_assets(media, urls, quotes + 1);
                    }
                }
                "app.bsky.feed.defs#generatorView" | "app.bsky.graph.defs#listView" => {
                    add(record, "avatar")
                }
                _ => {}
            }
        }
        "app.bsky.embed.recordWithMedia#view" => {
            embed_assets(&embed["media"], urls, quotes);
            if quotes == 0 {
                embed_assets(&embed["record"], urls, quotes);
            }
        }
        _ => {}
    }
}
