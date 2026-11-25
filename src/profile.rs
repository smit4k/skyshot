use std::collections::{BTreeMap, BTreeSet};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use unicode_segmentation::UnicodeSegmentation;

use crate::{
    RenderError, RenderOptions, Theme,
    model::{Author, MAX_JSON, string},
    resolver::{valid_did, valid_handle},
};

/// Web header layout. Auto uses desktop at 600 CSS pixels and above.
#[derive(Clone, Copy, Debug, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProfileLayout {
    #[default]
    Auto,
    Mobile,
    Desktop,
}

/// Profile image dimensions and color mode. Scale does not change layout breakpoints.
#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct ProfileRenderOptions {
    pub width: u32,
    pub scale: u32,
    pub theme: Theme,
    pub layout: ProfileLayout,
}
impl Default for ProfileRenderOptions {
    fn default() -> Self {
        Self {
            width: 600,
            scale: 2,
            theme: Theme::Light,
            layout: ProfileLayout::Auto,
        }
    }
}
impl ProfileRenderOptions {
    pub(crate) fn paint_options(self) -> RenderOptions {
        RenderOptions {
            width: self.width,
            scale: self.scale,
            theme: self.theme,
            ..RenderOptions::default()
        }
    }
    pub(crate) fn validate(self) -> Result<(), RenderError> {
        self.paint_options().validate()
    }
    pub(crate) fn desktop(self) -> bool {
        match self.layout {
            ProfileLayout::Auto => self.width >= 600,
            ProfileLayout::Mobile => false,
            ProfileLayout::Desktop => true,
        }
    }
}

/// A settled moderation decision, evaluated by the host before rendering.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ModerationAction {
    #[default]
    Show,
    Blur,
    Hide,
}

/// Optional host moderation decisions. Link and follower maps are keyed by URI and DID respectively.
#[derive(Debug, Default, Deserialize, Serialize)]
#[serde(default, rename_all = "camelCase")]
pub struct ProfileModeration {
    pub display_name: Option<ModerationAction>,
    pub profile_view: Option<ModerationAction>,
    pub avatar: Option<ModerationAction>,
    pub banner: Option<ModerationAction>,
    pub status: Option<ModerationAction>,
    pub links: BTreeMap<String, ModerationAction>,
    pub known_followers: BTreeMap<String, ModerationAction>,
    pub alerts: Vec<String>,
}

/// Viewer-specific data that is absent from a public profile response.
#[derive(Debug, Default, Deserialize, Serialize)]
#[serde(default, rename_all = "camelCase")]
pub struct ProfileContext {
    pub viewer_did: Option<String>,
    /// RFC 3339 time used for live expiry and new-account indicators.
    pub snapshot_at: Option<String>,
    pub labeler_subscribed: bool,
    /// Whether to show the viewer's own beta badge.
    pub beta_user: bool,
    /// Settled chat availability; when absent, use the profile's chat declaration.
    pub can_chat: Option<bool>,
    /// Resolved UTF-8 byte facets. None performs deterministic syntax detection offline.
    pub description_facets: Option<Vec<Value>>,
    pub moderation: ProfileModeration,
    /// Override the pinned Live Now host policy when reproducing another client configuration.
    pub live_allowed: Option<bool>,
}

/// A parsed AppView profile and optional labeler/viewer context. No network I/O occurs here.
pub struct ProfileInput {
    pub(crate) profile: Profile,
    pub(crate) context: ProfileContext,
    pub(crate) labeler: Value,
}
impl ProfileInput {
    /// Accept ProfileViewDetailed or { profile, context?, labeler? } JSON.
    pub fn from_json(json: &str) -> Result<Self, RenderError> {
        if json.len() > MAX_JSON {
            return Err(RenderError::Limit("profile JSON"));
        }
        let mut raw: Value = serde_json::from_str(json)?;
        let envelope = raw.get("profile").is_some();
        let data = if envelope {
            raw["profile"].take()
        } else {
            std::mem::take(&mut raw)
        };
        let viewer_present = data.get("viewer").is_some_and(|v| v.is_object());
        let mut profile: Profile = serde_json::from_value(data)?;
        profile.viewer_present = viewer_present;
        if !valid_did(&profile.did)
            || profile.author.handle.is_empty()
            || profile.author.handle.len() > 253
            || profile.author.display_name.len() > 640
            || profile.description.len() > 2560
            || profile.pronouns.len() > 200
            || profile.links.len() > 10
            || profile.viewer.known_followers.followers.len() > 5
        {
            return Err(RenderError::Input("invalid or oversized profile fields"));
        }
        let context: ProfileContext = if envelope && !raw["context"].is_null() {
            serde_json::from_value(raw["context"].take())?
        } else {
            ProfileContext::default()
        };
        if context
            .viewer_did
            .as_deref()
            .is_some_and(|did| !valid_did(did))
            || context
                .snapshot_at
                .as_deref()
                .is_some_and(|time| timestamp(time).is_none())
            || context
                .description_facets
                .as_ref()
                .is_some_and(|f| f.len() > 300)
            || context.moderation.alerts.len() > 32
            || context.moderation.alerts.iter().any(|s| s.len() > 640)
        {
            return Err(RenderError::Input("invalid profile context"));
        }
        for link in &profile.links {
            if link.url.len() > 4096
                || link.title.len() > 320
                || link.icon.as_ref().is_some_and(|s| s.len() > 4096)
            {
                return Err(RenderError::Input("oversized profile link"));
            }
        }
        Ok(Self {
            profile,
            context,
            labeler: if envelope {
                raw["labeler"].take()
            } else {
                Value::Null
            },
        })
    }

    /// Viewer context after parsing or resolving biography mentions.
    pub fn context(&self) -> &ProfileContext {
        &self.context
    }

    /// Candidate image URLs, excluding moderated content. Use Renderer::profile_asset_urls
    /// to also exclude icons folded out of the measured header.
    pub fn asset_urls(&self, options: ProfileRenderOptions) -> Result<Vec<String>, RenderError> {
        options.validate()?;
        let mut urls = self.primary_asset_urls();
        for link in self.visible_links() {
            if support_provider(&link.url).is_none() {
                urls.extend(link.icon.iter().cloned());
            }
        }
        Ok(urls.into_iter().collect())
    }

    pub(crate) fn primary_asset_urls(&self) -> BTreeSet<String> {
        let mut urls = BTreeSet::new();
        if !self.unavailable() {
            if self.avatar_action() != ModerationAction::Hide {
                urls.extend(self.profile.author.avatar.iter().cloned());
            }
            if self.banner_action() != ModerationAction::Hide {
                urls.extend(self.profile.banner.iter().cloned());
            }
            for follower in self.known_followers() {
                if self.follower_action(follower) != ModerationAction::Hide {
                    urls.extend(follower.author.avatar.iter().cloned());
                }
            }
        }
        urls.retain(|url| !url.is_empty());
        urls
    }

    /// Public AppView request for a labeler header when one has not been supplied.
    pub fn labeler_request(&self) -> Option<url::Url> {
        (self.profile.associated.labeler && self.labeler.is_null() && !self.unavailable())
            .then(|| crate::resolver::labeler_url(&self.profile.did))
    }

    /// Batched AppView requests for handles detected in the visible biography.
    pub fn bio_mention_requests(&self) -> Vec<url::Url> {
        if self.context.description_facets.is_some() || !self.body_visible() || self.blocked() {
            return Vec::new();
        }
        let facets = bio_facets(&self.profile.description);
        let handles: Vec<_> = facets
            .iter()
            .filter_map(|f| f["features"][0]["did"].as_str())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        handles
            .chunks(25)
            .map(crate::resolver::profiles_url)
            .collect()
    }

    /// Apply batched handle lookups to detected biography facets.
    pub fn resolve_bio_mentions(&mut self, profiles: &[Value]) {
        if self.context.description_facets.is_some() || !self.body_visible() || self.blocked() {
            return;
        }
        let resolved: BTreeMap<_, _> = profiles
            .iter()
            .filter_map(|p| {
                let handle = p["handle"].as_str()?.to_ascii_lowercase();
                let did = p["did"].as_str().filter(|did| valid_did(did))?;
                Some((handle, did))
            })
            .collect();
        let mut facets = bio_facets(&self.profile.description);
        for facet in &mut facets {
            let feature = &mut facet["features"][0];
            if let Some(did) = feature["did"]
                .as_str()
                .and_then(|h| resolved.get(&h.to_ascii_lowercase()))
            {
                feature["did"] = Value::String((*did).to_owned());
            }
        }
        self.context.description_facets = Some(facets);
    }

    pub(crate) fn authenticated(&self) -> bool {
        self.context.viewer_did.is_some() || self.profile.viewer_present
    }
    pub(crate) fn can_chat(&self) -> bool {
        self.authenticated()
            && self.context.can_chat.unwrap_or_else(|| {
                match string(&self.profile.associated.chat, "allowIncoming") {
                    "all" => true,
                    "following" | "" => self.profile.viewer.followed_by.is_some(),
                    _ => false,
                }
            })
    }
    pub(crate) fn is_self(&self) -> bool {
        self.context.viewer_did.as_deref() == Some(self.profile.did.as_str())
    }
    pub(crate) fn blocked(&self) -> bool {
        let viewer = &self.profile.viewer;
        viewer.blocked_by || viewer.blocking.is_some() || !viewer.blocking_by_list.is_null()
    }
    pub(crate) fn unavailable(&self) -> bool {
        labels_action(&self.profile.author.labels, self.authenticated()) == ModerationAction::Hide
    }
    pub(crate) fn body_visible(&self) -> bool {
        !self.unavailable()
            && self
                .context
                .moderation
                .profile_view
                .unwrap_or_else(|| labels_action(&self.profile.author.labels, self.authenticated()))
                == ModerationAction::Show
    }
    pub(crate) fn avatar_action(&self) -> ModerationAction {
        self.context
            .moderation
            .avatar
            .unwrap_or_else(|| labels_action(&self.profile.author.labels, self.authenticated()))
    }
    pub(crate) fn banner_action(&self) -> ModerationAction {
        self.context
            .moderation
            .banner
            .unwrap_or_else(|| labels_action(&self.profile.author.labels, self.authenticated()))
    }
    pub(crate) fn follower_action(&self, f: &Follower) -> ModerationAction {
        self.context
            .moderation
            .known_followers
            .get(&f.did)
            .copied()
            .unwrap_or_else(|| labels_action(&f.author.labels, self.authenticated()))
    }
    pub(crate) fn known_followers(&self) -> Vec<&Follower> {
        if !self.authenticated()
            || self.is_self()
            || self.blocked()
            || !self.body_visible()
            || self.profile.associated.labeler
        {
            return Vec::new();
        }
        self.profile
            .viewer
            .known_followers
            .followers
            .iter()
            .take(3)
            .collect()
    }
    pub(crate) fn visible_links(&self) -> Vec<&ProfileLink> {
        if !self.body_visible() || self.blocked() {
            return Vec::new();
        }
        let mut seen = BTreeSet::new();
        self.profile
            .links
            .iter()
            .filter(|link| {
                let action = self
                    .context
                    .moderation
                    .links
                    .get(&link.uri)
                    .copied()
                    .unwrap_or_else(|| labels_action(&link.labels, self.authenticated()));
                action == ModerationAction::Show
                    && showable_link(&link.url)
                    && seen.insert(link.url.trim_end_matches('/'))
            })
            .collect()
    }
    pub(crate) fn snapshot(&self) -> Option<DateTime<Utc>> {
        self.context.snapshot_at.as_deref().and_then(timestamp)
    }
    pub(crate) fn validate_snapshot(&self) -> Result<(), RenderError> {
        if !self.profile.status.is_null() && self.snapshot().is_none() {
            return Err(RenderError::Input("snapshotAt is required for live status"));
        }
        Ok(())
    }
    pub(crate) fn live(&self) -> bool {
        let status = &self.profile.status;
        let allowed = self.context.live_allowed.unwrap_or_else(|| {
            url::Url::parse(string(&status["embed"]["external"], "uri"))
                .ok()
                .and_then(|u| u.host_str().map(str::to_owned))
                .is_some_and(|host| {
                    [
                        "twitch.tv",
                        "youtube.com",
                        "stream.place",
                        "bluecast.app",
                        "substack.com",
                        "beehiiv.com",
                    ]
                    .iter()
                    .any(|domain| host == *domain || host.ends_with(&format!(".{domain}")))
                })
        });
        allowed
            && !self.blocked()
            && self.body_visible()
            && self.context.moderation.status.unwrap_or_default() == ModerationAction::Show
            && string(status, "status") == "app.bsky.actor.status#live"
            && status["isDisabled"].as_bool() != Some(true)
            && timestamp(string(status, "expiresAt"))
                .zip(self.snapshot())
                .is_some_and(|(expires, now)| expires > now)
    }
    pub(crate) fn germ(&self) -> bool {
        let g = &self.profile.associated.germ;
        self.body_visible()
            && !self.blocked()
            && (string(g, "showButtonTo") == "everyone"
                || (string(g, "showButtonTo") == "usersIFollow"
                    && (self.is_self() || self.profile.viewer.followed_by.is_some())))
            && string(g, "messageMeUrl").starts_with("https://")
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Profile {
    pub did: String,
    #[serde(flatten)]
    pub author: Author,
    #[serde(default)]
    pub banner: Option<String>,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub pronouns: String,
    #[serde(default)]
    pub followers_count: u64,
    #[serde(default)]
    pub follows_count: u64,
    #[serde(default)]
    pub posts_count: u64,
    #[serde(default)]
    pub links: Vec<ProfileLink>,
    #[serde(default)]
    pub associated: Associated,
    #[serde(default)]
    pub viewer: ProfileViewer,
    #[serde(skip)]
    pub viewer_present: bool,
    #[serde(default)]
    pub created_at: String,
    #[serde(default)]
    pub status: Value,
}

#[derive(Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub(crate) struct Associated {
    pub labeler: bool,
    pub chat: Value,
    pub activity_subscription: Value,
    pub germ: Value,
}
#[derive(Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub(crate) struct ProfileViewer {
    pub following: Option<String>,
    pub followed_by: Option<String>,
    pub blocking: Option<String>,
    pub blocking_by_list: Value,
    pub blocked_by: bool,
    pub muted: bool,
    pub muted_only_reposts: bool,
    pub muted_only_quoteposts: bool,
    pub known_followers: KnownFollowers,
    pub activity_subscription: Value,
}
#[derive(Default, Deserialize)]
#[serde(default)]
pub(crate) struct KnownFollowers {
    pub count: u64,
    pub followers: Vec<Follower>,
}
#[derive(Deserialize)]
pub(crate) struct Follower {
    pub did: String,
    #[serde(flatten)]
    pub author: Author,
    #[serde(default)]
    pub associated: Associated,
}
#[derive(Default, Deserialize)]
#[serde(default)]
pub(crate) struct ProfileLink {
    pub uri: String,
    pub url: String,
    pub title: String,
    pub icon: Option<String>,
    pub labels: Vec<crate::model::Label>,
}

pub(crate) fn timestamp(value: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(value).ok().map(|d| d.to_utc())
}
pub(crate) fn labels_action(
    labels: &[crate::model::Label],
    authenticated: bool,
) -> ModerationAction {
    let mut action = ModerationAction::Show;
    for label in labels.iter().filter(|l| !l.neg) {
        match label.val.as_str() {
            "!hide" | "!takedown" => return ModerationAction::Hide,
            "!no-unauthenticated" if !authenticated => return ModerationAction::Hide,
            "!warn" | "porn" | "sexual" | "nudity" | "graphic-media" | "gore"
            | "sexual-figurative" | "self-harm" | "sensitive" => action = ModerationAction::Blur,
            _ => {}
        }
    }
    action
}

pub(crate) fn showable_link(raw: &str) -> bool {
    url::Url::parse(raw).ok().is_some_and(|url| {
        url.scheme() == "https"
            && url.host_str().is_some_and(|host| {
                !profile_data()["shorteners"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .any(|value| {
                        value.as_str().is_some_and(|domain| {
                            host == domain || host.ends_with(&format!(".{domain}"))
                        })
                    })
            })
            && url.username().is_empty()
            && url.password().is_none()
    })
}
pub(crate) fn profile_data() -> &'static Value {
    static DATA: std::sync::OnceLock<Value> = std::sync::OnceLock::new();
    DATA.get_or_init(|| {
        serde_json::from_str(include_str!("profile-data.json"))
            .unwrap_or_else(|_| unreachable!("bundled profile data"))
    })
}
pub(crate) fn support_provider(raw: &str) -> Option<&'static Value> {
    let url = url::Url::parse(raw).ok()?;
    let host = url.host_str()?;
    profile_data()["providers"].as_array()?.iter().find(|p| {
        let domain = string(p, "domain");
        let path = url.path().trim_end_matches('/');
        (host == domain || host.ends_with(&format!(".{domain}")))
            && if let Some(prefix) = p["pathPrefix"].as_str() {
                path.starts_with(prefix) && path.len() > prefix.len()
            } else {
                !path.is_empty() || (host != domain && host != format!("www.{domain}"))
            }
    })
}

/// Detect profile-bio entities without I/O. Hosts can resolve returned mention handles before rendering.
pub(crate) fn bio_facets(text: &str) -> Vec<Value> {
    let mut facets = Vec::new();
    let mut offset = 0;
    for word in text.split_inclusive(char::is_whitespace) {
        let token = word
            .trim_end()
            .trim_start_matches(['(', '[', '{', '"', '\'']);
        let start = offset + word.find(token).unwrap_or(0);
        let token =
            token.trim_end_matches(['.', ',', '!', '?', ';', ':', ')', ']', '}', '"', '\'']);
        let feature = if let Some(handle) = token
            .strip_prefix('@')
            .filter(|handle| valid_handle(handle))
        {
            Some(serde_json::json!({"$type":"app.bsky.richtext.facet#mention", "did":handle}))
        } else if let Some(tag) = token.strip_prefix('#').filter(|s| {
            !s.is_empty()
                && s.graphemes(true).count() <= 64
                && !s.chars().all(|c| c.is_ascii_digit())
        }) {
            Some(serde_json::json!({"$type":"app.bsky.richtext.facet#tag", "tag":tag}))
        } else {
            let url = if token.starts_with("https://") || token.starts_with("http://") {
                token.to_owned()
            } else {
                format!("https://{token}")
            };
            url::Url::parse(&url)
                .ok()
                .filter(|u| {
                    matches!(u.scheme(), "http" | "https")
                        && u.host_str().is_some_and(valid_handle)
                        && !token.contains('@')
                })
                .map(|_| serde_json::json!({"$type":"app.bsky.richtext.facet#link", "uri":url}))
        };
        if let Some(feature) = feature {
            facets.push(serde_json::json!({"index":{"byteStart":start,"byteEnd":start+token.len()},"features":[feature]}));
        }
        offset += word.len();
    }
    facets
}
