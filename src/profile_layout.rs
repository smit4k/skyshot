use std::borrow::Cow;

use tiny_skia::{Path, Rect, Transform};
use unicode_segmentation::UnicodeSegmentation;

use crate::{
    ProfileInput, ProfileRenderOptions, Theme, Warning,
    draw::{Item, Scene, TextBlock, TextEngine, TextStyle},
    icons::{Icon, svg_path},
    layout::{Colors, Layout, rich_spans},
    model::string,
    profile::{ModerationAction, bio_facets, profile_data, support_provider, timestamp},
    resolver::valid_handle,
};

const PAD: f32 = 16.0;
const BANNER_HEIGHT: f32 = 150.0;
const PRIMARY: u32 = 0x006aff;
const WHITE: u32 = 0xffffff;
const LABELER: u32 = 0x6900ff;

struct Palette {
    secondary: u32,
    primary: u32,
    panel: u32,
    high: u32,
    pronouns: u32,
    green: u32,
    red: u32,
    beta: u32,
}
impl Palette {
    fn new(theme: Theme) -> Self {
        match theme {
            Theme::Light => Self {
                secondary: 0xeff2f6,
                primary: PRIMARY,
                panel: 0xf9fafb,
                high: 0x405168,
                pronouns: 0x526580,
                green: 0x04904a,
                red: 0xe91646,
                beta: 0xe5f0ff,
            },
            Theme::Dark => Self {
                secondary: 0x19222e,
                primary: PRIMARY,
                panel: 0x111822,
                high: 0xa5b2c5,
                pronouns: 0x8798b0,
                green: 0x04904a,
                red: 0xe91646,
                beta: 0x001e47,
            },
            Theme::Dim => Self {
                secondary: 0x222e3f,
                primary: 0x0f73ff,
                panel: 0x1c2736,
                high: 0xabb8c9,
                pronouns: 0x8d9db4,
                green: 0x049f52,
                red: 0xeb2452,
                beta: 0x122949,
            },
        }
    }
}

pub(crate) fn profile(
    text: &mut TextEngine,
    input: &ProfileInput,
    options: ProfileRenderOptions,
    warnings: &mut Vec<Warning>,
) -> Scene {
    let colors = Colors::from_theme(options.theme);
    let palette = Palette::new(options.theme);
    let mut layout = Layout {
        text,
        colors,
        items: Vec::new(),
        warnings,
        time_zone: chrono_tz::UTC,
        hour12: true,
    };
    let width = options.width as f32;
    let height = if input.unavailable() {
        layout.notice("This profile is unavailable", PAD, PAD, width - 2.0 * PAD) + PAD
    } else {
        header(&mut layout, input, &palette, width, options.desktop())
    };
    Scene {
        background: colors.background,
        height: height + 8.0,
        items: layout.items,
    }
}

fn header(
    layout: &mut Layout,
    input: &ProfileInput,
    palette: &Palette,
    width: f32,
    desktop: bool,
) -> f32 {
    let p = &input.profile;
    let colors = layout.colors;
    let inner = width - 2.0 * PAD;
    let labeler = p.associated.labeler;
    let official = p.did == "did:plc:ar7c4by46qjdydhdevvrndac";
    let live = input.live();
    let banner = [0.0, 0.0, width, BANNER_HEIGHT];
    let banner_url = p
        .banner
        .as_deref()
        .filter(|_| input.banner_action() != ModerationAction::Hide);
    layout.rect(
        banner,
        0.0,
        Some(if labeler && banner_url.is_none() {
            LABELER
        } else {
            palette.panel
        }),
        None,
    );
    media(
        layout,
        banner_url,
        banner,
        0.0,
        input.banner_action(),
        100.0,
        None,
    );
    if let Some(rect) = rect([18.0, 10.0, 31.0, 31.0]) {
        layout.items.push(Item::Overlay {
            rect,
            radius: 15.5,
            color: 0,
            alpha: 128,
        });
    }
    vector(layout, "back", 21.5, 13.5, 24.0, WHITE);

    let mut y =
        actions(layout, input, palette, inner, official) + if labeler { 18.0 } else { 10.0 };
    if live {
        y += 6.0;
    }
    let hidden_name = input
        .context
        .moderation
        .display_name
        .is_some_and(|a| a != ModerationAction::Show);
    let name = if hidden_name {
        String::new()
    } else {
        sanitize_name(p.author.name())
    };
    let size = if desktop { 30.0 } else { 24.3 };
    let line = if desktop { 35.0 } else { 28.0 };
    let block = measure(
        layout,
        &name,
        inner,
        style(size, line, colors.text, 700, None),
    );
    let bot = p
        .author
        .labels
        .iter()
        .any(|label| label.val == "bot" && !label.neg && label.src == p.did);
    let beta = input.is_self() && input.context.beta_user;
    if let Some((last_width, last_baseline, rtl)) = block.last_line() {
        let top = y;
        let verified = p.author.verified();
        let badge_width = if verified { 22.0 } else { 0.0 }
            + if beta { 22.0 } else { 0.0 }
            + if bot { 23.0 } else { 0.0 };
        let wrapped = last_width + badge_width > inner;
        let x = if wrapped {
            PAD
        } else if rtl {
            PAD + (inner - last_width - badge_width).max(0.0)
        } else {
            PAD + last_width
        };
        let baseline = y + last_baseline + if wrapped { line } else { 0.0 };
        y += block.height
            + if wrapped && badge_width > 0.0 {
                line
            } else {
                0.0
            };
        add_text(layout, block, PAD, top);
        if verified {
            if p.author.trusted_verifier() {
                layout.icon(Icon::Verifier, x + 4.0, baseline - 17.25, 18.0, PRIMARY);
                layout.icon(Icon::VerifierCheck, x + 4.0, baseline - 17.25, 18.0, WHITE);
            } else {
                layout.rect(
                    [x + 4.0, baseline - 17.25, 18.0, 18.0],
                    9.0,
                    Some(PRIMARY),
                    None,
                );
                layout.icon(Icon::Check, x + 4.0, baseline - 17.25, 18.0, WHITE);
            }
        }
        if beta {
            let x = x + 4.0 + if verified { 22.0 } else { 0.0 };
            layout.rect(
                [x, baseline - 17.25, 18.0, 18.0],
                9.0,
                Some(palette.beta),
                None,
            );
            vector(
                layout,
                "beta",
                x + 4.0,
                baseline - 13.25,
                10.0,
                palette.primary,
            );
        }
        if bot {
            vector(
                layout,
                "bot",
                x + badge_width - 19.0,
                baseline - 18.25,
                19.0,
                colors.muted,
            );
        }
    }
    y += if labeler { 2.0 } else { 4.0 };
    y += handle(layout, input, palette, inner, y);
    y += if labeler { 12.0 } else { 8.0 };
    if !input.blocked() {
        let mut has_row = false;
        if !labeler || input.is_self() {
            stats(layout, p, inner, y);
            y += 20.0;
            has_row = true;
        }
        if input.body_visible() && !p.description.trim().is_empty() {
            if has_row {
                y += 12.0;
            }
            let facets: Cow<'_, [_]> = input
                .context
                .description_facets
                .as_deref()
                .map_or_else(|| Cow::Owned(bio_facets(&p.description)), Cow::Borrowed);
            let spans = rich_spans(&p.description, &facets, colors.link, layout.warnings);
            let block = layout.text.measure_profile(
                &spans,
                inner,
                style(15.0, 20.0, colors.text, 400, Some(15)),
            );
            let height = block.height;
            add_text(layout, block, PAD, y);
            y += height;
            has_row = true;
        }
        let pills = profile_pills(layout, input, palette, inner);
        if !pills.is_empty() {
            if has_row {
                y += 12.0;
            }
            y += draw_pills(
                layout,
                pills,
                inner,
                y,
                Some((palette.secondary, colors.text)),
            );
            has_row = true;
        }
        if labeler && !official && !input.labeler.is_null() {
            if has_row {
                y += 12.0;
            }
            let liked = input.labeler["viewer"]["like"].as_str().is_some();
            layout.rect([PAD, y, 33.0, 33.0], 16.5, Some(palette.secondary), None);
            if liked {
                vector(layout, "heart", PAD + 8.0, y + 8.0, 17.0, palette.red);
            } else {
                layout.icon(Icon::Heart, PAD + 8.0, y + 8.0, 17.0, palette.high);
            }
            let count = input.labeler["likeCount"].as_u64().unwrap_or(0);
            let label = format!(
                "Liked by {count} {}",
                if count == 1 { "user" } else { "users" }
            );
            let block = measure(
                layout,
                &label,
                inner - 43.0,
                style(13.1, 17.0, colors.muted, 600, Some(1)),
            );
            add_text(layout, block, PAD + 43.0, y + 8.0);
            y += 33.0;
        }
        let followers = input.known_followers();
        if !followers.is_empty() {
            if has_row {
                y += 12.0;
            }
            y += known_followers(layout, input, &followers, inner, y);
        }
    }
    y += 8.0;
    let mut alerts: Vec<&str> = input
        .context
        .moderation
        .alerts
        .iter()
        .map(String::as_str)
        .collect();
    if input.blocked() {
        alerts.push(if p.viewer.blocked_by {
            "Account blocks you"
        } else {
            "Account blocked"
        });
    }
    if p.viewer.muted {
        alerts.push("Account muted");
    }
    if p.viewer.muted_only_reposts {
        alerts.push("Reposts muted");
    }
    if p.viewer.muted_only_quoteposts {
        alerts.push("Quote posts muted");
    }
    if !input.body_visible() && alerts.is_empty() {
        alerts.push("Content warning");
    }
    if !alerts.is_empty() {
        let pills = alerts
            .into_iter()
            .map(|alert| Pill::new(layout, alert, inner, palette.secondary, palette.high))
            .collect();
        y += 4.0 + draw_pills(layout, pills, inner, y + 4.0, None) + 8.0;
    }

    let border = if live { 3.0 } else { 2.0 };
    avatar(
        layout,
        p.author.avatar.as_deref(),
        [10.0, 104.0, 94.0],
        labeler,
        input.avatar_action(),
        border,
        if live { palette.red } else { colors.background },
    );
    if live {
        let badge = measure(layout, "LIVE", 40.0, style(12.0, 15.0, WHITE, 600, Some(1)));
        let x = 57.0 - badge.width / 2.0;
        layout.rect(
            [x - 4.0, 183.0, badge.width + 8.0, 19.0],
            5.0,
            Some(palette.red),
            None,
        );
        add_text(layout, badge, x, 185.0);
    }
    y.max(198.0)
}

fn sanitize_name(name: &str) -> String {
    name.chars()
        .filter(|c| !c.is_control() && !matches!(c, '✓' | '✔' | '☑' | '✅'))
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn actions(
    layout: &mut Layout,
    input: &ProfileInput,
    palette: &Palette,
    width: f32,
    official: bool,
) -> f32 {
    let p = &input.profile;
    let v = &p.viewer;
    let secondary = (palette.secondary, palette.high);
    let mut buttons = Vec::new();
    let mut push = |label: &str, icon: Option<&'static str>, colors: (u32, u32), rounded| {
        let block = (!str::is_empty(label)).then(|| {
            measure(
                layout,
                label,
                width - 118.0,
                style(13.1, 17.0, colors.1, 500, Some(1)),
            )
        });
        let w = block.as_ref().map_or(33.0, |b| {
            b.width + 28.0 + if icon.is_some() { 18.0 } else { 0.0 }
        });
        buttons.push((block, icon, colors, w, rounded));
    };
    if input.is_self() {
        push("Edit Profile", None, secondary, true);
    } else if p.associated.labeler {
        if input.can_chat() && !input.blocked() {
            push("", Some("chat"), secondary, true);
        }
        if !official {
            if input.context.labeler_subscribed {
                push("Unsubscribe", None, secondary, false);
            } else {
                push("Subscribe to Labeler", None, (LABELER, WHITE), false);
            }
        }
    } else if !input.blocked() {
        let following = v.following.is_some();
        let mutual = v.followed_by.is_some();
        let allows = match p.associated.activity_subscription["allowSubscriptions"].as_str() {
            Some("mutuals") => mutual,
            Some("followers") | None => true,
            _ => false,
        };
        if input.authenticated() && following && allows {
            let ringing = v.activity_subscription["post"].as_bool().unwrap_or(false)
                || v.activity_subscription["reply"].as_bool().unwrap_or(false);
            push(
                "",
                Some(if ringing { "ringing" } else { "bell" }),
                secondary,
                true,
            );
        }
        if input.can_chat() {
            push("", Some("chat"), secondary, true);
        }
        if following {
            push("Following", None, secondary, true);
        } else {
            push(
                if mutual { "Follow back" } else { "Follow" },
                Some("plus"),
                (palette.primary, WHITE),
                true,
            );
        }
    } else if v.blocking.is_some() && v.blocking_by_list.is_null() {
        push("Unblock", None, secondary, true);
    }
    push("", Some("more"), secondary, true);
    let mut y = 162.0;
    let available = width - if p.associated.labeler { 0.0 } else { 90.0 };
    while !buttons.is_empty() {
        let mut end = 0;
        let mut occupied = 0.0;
        while let Some((_, _, _, w, _)) = buttons.get(end) {
            let gap = if end > 0 { 4.0 } else { 0.0 };
            if end > 0 && occupied + gap + w > available {
                break;
            }
            occupied += gap + w;
            end += 1;
        }
        let mut x = PAD + width - occupied;
        for (text, icon, (bg, fg), w, rounded) in buttons.drain(..end) {
            layout.rect(
                [x, y, w, 33.0],
                if rounded { 16.5 } else { 6.0 },
                Some(bg),
                None,
            );
            let mut tx = x + 14.0;
            if let Some(icon) = icon {
                let ix = if text.is_some() { x + 12.0 } else { x + 8.0 };
                vector(layout, icon, ix, y + 8.0, 17.0, fg);
                tx = x + 32.0;
            }
            if let Some(text) = text {
                add_text(layout, text, tx, y + 8.0);
            }
            x += w + 4.0;
        }
        y += if buttons.is_empty() { 33.0 } else { 37.0 };
    }
    y
}

fn handle(layout: &mut Layout, input: &ProfileInput, palette: &Palette, width: f32, y: f32) -> f32 {
    let profile = &input.profile;
    let colors = layout.colors;
    let follows = !input.blocked() && profile.viewer.followed_by.is_some();
    let new_account = input
        .snapshot()
        .zip(timestamp(&profile.created_at))
        .is_some_and(|(now, created)| (now - created).num_seconds() <= 7 * 86_400);
    let raw_handle = &profile.author.handle;
    let invalid = raw_handle == "handle.invalid" || !valid_handle(raw_handle);
    let row_height: f32 = if follows {
        25.0
    } else if new_account || invalid {
        24.0
    } else {
        20.0
    };
    let mut x = PAD;
    if new_account {
        vector(
            layout,
            "newskie",
            x,
            y + (row_height - 24.0) / 2.0,
            24.0,
            0xffc404,
        );
        x += 32.0;
    }
    if follows {
        let block = measure(
            layout,
            "Follows you",
            100.0,
            style(13.1, 17.0, colors.text, 400, Some(1)),
        );
        let w = block.width + 16.0;
        layout.rect([x, y, w, 25.0], 4.0, Some(palette.secondary), None);
        add_text(layout, block, x + 8.0, y + 4.0);
        x += w + 8.0;
    }
    let pronouns = profile.pronouns.trim();
    let pronouns = (!input.blocked() && input.body_visible() && !pronouns.is_empty()).then(|| {
        measure(
            layout,
            pronouns,
            (PAD + width - x - 34.0).max(1.0),
            style(15.0, 20.0, palette.pronouns, 400, Some(1)),
        )
    });
    let remaining = (PAD + width - x - pronouns.as_ref().map_or(0.0, |b| b.width + 10.0)).max(1.0);
    let text_y = y + (row_height - 20.0) / 2.0;
    let right = if invalid {
        let block = measure(
            layout,
            "⚠Invalid Handle",
            (remaining - 16.0).max(1.0),
            style(11.3, 15.0, colors.text, 400, Some(1)),
        );
        let w = (block.width + 16.0).min(remaining);
        layout.rect(
            [x, y + (row_height - 23.0) / 2.0, w, 23.0],
            4.0,
            None,
            Some(colors.border),
        );
        add_text(layout, block, x + 8.0, y + (row_height - 15.0) / 2.0);
        x + w
    } else {
        let block = measure(
            layout,
            &format!("@{raw_handle}"),
            remaining,
            style(15.0, 20.0, colors.muted, 400, Some(1)),
        );
        let right = x + block.width;
        add_text(layout, block, x, text_y);
        right
    };
    if let Some(block) = pronouns {
        add_text(layout, block, right + 10.0, text_y);
    }
    row_height
}

fn stats(layout: &mut Layout, p: &crate::profile::Profile, width: f32, y: f32) {
    let colors = layout.colors;
    let mut x = PAD;
    for (i, (count, word)) in [
        (
            p.followers_count,
            if p.followers_count == 1 {
                "follower"
            } else {
                "followers"
            },
        ),
        (p.follows_count, "following"),
        (
            p.posts_count,
            if p.posts_count == 1 { "post" } else { "posts" },
        ),
    ]
    .into_iter()
    .enumerate()
    {
        let w = (PAD + width - x - (2 - i) as f32 * 65.0).max(1.0);
        let number = measure(
            layout,
            &format!("{} ", profile_count(count)),
            w,
            style(15.0, 20.0, colors.text, 600, Some(1)),
        );
        let nx = x + number.width;
        add_text(layout, number, x, y);
        let label = measure(
            layout,
            word,
            (w - (nx - x)).max(1.0),
            style(15.0, 20.0, colors.muted, 400, Some(1)),
        );
        x = nx + label.width + 10.0;
        add_text(layout, label, nx, y);
    }
}

fn profile_count(n: u64) -> String {
    let (divisor, suffix) = match n {
        0..=999 => return n.to_string(),
        1_000..=999_999 => (1_000, "K"),
        1_000_000..=999_999_999 => (1_000_000, "M"),
        1_000_000_000..=999_999_999_999 => (1_000_000_000, "B"),
        _ => (1_000_000_000_000, "T"),
    };
    let fraction = n % divisor / (divisor / 10);
    if fraction == 0 {
        format!("{}{suffix}", n / divisor)
    } else {
        format!("{}.{fraction}{suffix}", n / divisor)
    }
}

struct Pill<'a> {
    text: TextBlock,
    width: f32,
    background: u32,
    color: u32,
    icon: Option<&'a str>,
    url: Option<&'a str>,
    arrow: bool,
}
impl Pill<'_> {
    fn new(layout: &mut Layout, label: &str, max: f32, background: u32, color: u32) -> Self {
        let text = measure(
            layout,
            label,
            max - 22.0,
            style(13.1, 17.0, color, 500, Some(1)),
        );
        Self {
            width: text.width + 22.0,
            text,
            background,
            color,
            icon: None,
            url: None,
            arrow: false,
        }
    }
}

fn profile_pills<'a>(
    layout: &mut Layout,
    input: &'a ProfileInput,
    palette: &Palette,
    width: f32,
) -> Vec<Pill<'a>> {
    let colors = layout.colors;
    let mut pills = Vec::new();
    for link in input.visible_links() {
        let provider = support_provider(&link.url);
        let host = url::Url::parse(&link.url)
            .ok()
            .and_then(|u| {
                u.host_str()
                    .map(|h| h.trim_start_matches("www.").to_owned())
            })
            .unwrap_or_default();
        let title: String = link.title.trim().graphemes(true).take(40).collect();
        let label = if !title.is_empty() {
            title
        } else if let Some(p) = provider {
            format!("Support on {}", string(p, "name"))
        } else {
            host
        };
        let (bg, fg) = if provider.is_some() {
            (palette.green, WHITE)
        } else {
            (palette.secondary, colors.text)
        };
        let mut pill = Pill::new(layout, &label, width - 30.0, bg, fg);
        pill.width += 30.0;
        pill.icon = Some(provider.map_or("globe", |p| string(p, "logo")));
        pill.url = if provider.is_some() {
            None
        } else {
            link.icon.as_deref()
        };
        pill.arrow = true;
        pills.push(pill);
    }
    if input.germ() {
        let mut pill = Pill::new(
            layout,
            "Germ DM",
            width - 30.0,
            palette.secondary,
            colors.text,
        );
        pill.width += if input.is_self() { 18.0 } else { 30.0 };
        pill.icon = Some("germ");
        pill.arrow = !input.is_self();
        pills.push(pill);
    }
    pills
}

fn draw_pills(
    layout: &mut Layout,
    mut pills: Vec<Pill>,
    width: f32,
    y: f32,
    overflow: Option<(u32, u32)>,
) -> f32 {
    let mut rows: Vec<(Vec<Pill>, f32)> = vec![(Vec::new(), 0.0)];
    let mut hidden = 0;
    for pill in pills.drain(..) {
        let last = rows.len() - 1;
        let (row, used) = &mut rows[last];
        if !row.is_empty() && *used + pill.width > width {
            if rows.len() == 2 && overflow.is_some() {
                hidden += 1;
                continue;
            }
            let used = pill.width + 8.0;
            rows.push((vec![pill], used));
        } else if hidden > 0 {
            hidden += 1;
        } else {
            *used += pill.width + 8.0;
            row.push(pill);
        }
    }
    if hidden > 0
        && let Some((bg, fg)) = overflow
        && let Some((row, used)) = rows.last_mut()
    {
        loop {
            let more = Pill::new(layout, &format!("+{hidden}"), width, bg, fg);
            if *used + more.width <= width || row.is_empty() {
                row.push(more);
                break;
            }
            if let Some(removed) = row.pop() {
                *used -= removed.width + 8.0;
                hidden += 1;
            }
        }
    }
    for (n, (row, _)) in rows.iter_mut().enumerate() {
        let mut x = PAD;
        let y = y + n as f32 * 37.0;
        for pill in row.drain(..) {
            layout.rect([x, y, pill.width, 29.0], 14.5, Some(pill.background), None);
            if let Some(icon) = pill.icon {
                if icon == "germ" {
                    if let Some(rect) = rect([x + 8.0, y + 6.5, 16.0, 16.0]) {
                        layout.items.push(Item::EmbeddedImage {
                            rect,
                            radius: 8.0,
                            bytes: include_bytes!("profile-germ.webp"),
                        });
                    }
                } else if let Some(url) = pill.url {
                    let fallback =
                        vector_path(icon, x + 8.0, y + 6.0, 17.0).map(|p| (p, pill.color));
                    media(
                        layout,
                        Some(url),
                        [x + 8.5, y + 6.5, 16.0, 16.0],
                        2.67,
                        ModerationAction::Show,
                        0.0,
                        fallback,
                    );
                } else {
                    vector(layout, icon, x + 8.0, y + 6.0, 17.0, pill.color);
                }
            }
            if pill.arrow {
                vector(
                    layout,
                    "out",
                    x + pill.width - 22.0,
                    y + 8.5,
                    12.0,
                    pill.color,
                );
            }
            add_text(
                layout,
                pill.text,
                x + if pill.icon.is_some() { 28.0 } else { 10.0 },
                y + 6.0,
            );
            x += pill.width + 8.0;
        }
    }
    rows.len() as f32 * 37.0 - 8.0
}

fn known_followers(
    layout: &mut Layout,
    input: &ProfileInput,
    followers: &[&crate::profile::Follower],
    width: f32,
    y: f32,
) -> f32 {
    let colors = layout.colors;
    for (i, f) in followers.iter().enumerate().rev() {
        avatar(
            layout,
            f.author.avatar.as_deref(),
            [PAD - 1.0 + i as f32 * 24.0, y - 1.0, 32.0],
            f.associated.labeler,
            input.follower_action(f),
            1.0,
            colors.background,
        );
    }
    let first = follower_name(input, followers[0]);
    let total = input
        .profile
        .viewer
        .known_followers
        .count
        .max(followers.len() as u64);
    let label = if let Some(second) = followers.get(1) {
        let second = follower_name(input, second);
        if total == 2 {
            format!("Followed by {first} and {second}")
        } else {
            format!("Followed by {first}, {second}, and {}", others(total - 2))
        }
    } else if total > 1 {
        format!("Followed by {first} and {}", others(total - 1))
    } else {
        format!("Followed by {first}")
    };
    let offset = 43.0 + (followers.len() - 1) as f32 * 24.0;
    let block = measure(
        layout,
        &label,
        width - offset,
        style(13.1, 17.0, colors.muted, 400, Some(2)),
    );
    let height = block.height.max(30.0);
    let top = y + (height - block.height) / 2.0;
    add_text(layout, block, PAD + offset, top);
    height
}

fn follower_name<'a>(input: &ProfileInput, f: &'a crate::profile::Follower) -> &'a str {
    if input.follower_action(f) == ModerationAction::Show {
        f.author.name()
    } else {
        &f.author.handle
    }
}
fn others(n: u64) -> String {
    format!("{n} other{}", if n == 1 { "" } else { "s" })
}

#[allow(clippy::too_many_arguments)]
fn avatar(
    layout: &mut Layout,
    url: Option<&str>,
    [x, y, size]: [f32; 3],
    square: bool,
    action: ModerationAction,
    border: f32,
    border_color: u32,
) {
    let radius = if square { 12.0 } else { size / 2.0 };
    let s = size - 2.0 * border;
    let shape = [x + border, y + border, s, s];
    layout.rect([x, y, size, size], radius, Some(border_color), None);
    layout.rect(shape, radius - border, Some(layout.colors.panel), None);
    let icon = Icon::Person.path().and_then(|p| {
        p.transform(
            Transform::from_scale(s * 0.56 / 24.0, s * 0.56 / 24.0)
                .post_translate(x + border + s * 0.22, y + border + s * 0.22),
        )
    });
    if url.is_none() || action == ModerationAction::Hide {
        if let Some(path) = icon {
            layout.items.push(Item::Path {
                path,
                color: layout.colors.muted,
            });
        }
    } else {
        media(
            layout,
            url,
            shape,
            radius - border,
            action,
            24.0,
            icon.map(|p| (p, layout.colors.muted)),
        );
    }
}

fn media(
    layout: &mut Layout,
    url: Option<&str>,
    frame: [f32; 4],
    radius: f32,
    action: ModerationAction,
    blur: f32,
    fallback: Option<(Path, u32)>,
) {
    if action != ModerationAction::Hide
        && let Some(url) = url.filter(|u| !u.is_empty())
        && let Some(rect) = rect(frame)
    {
        layout.items.push(Item::Image {
            rect,
            radius,
            url: url.to_owned(),
            cover: true,
            blur: if action == ModerationAction::Blur {
                blur
            } else {
                0.0
            },
            fallback,
        });
    }
}

fn rect([x, y, w, h]: [f32; 4]) -> Option<Rect> {
    Rect::from_xywh(x, y, w, h)
}
fn style(size: f32, spacing: f32, color: u32, weight: u16, lines: Option<usize>) -> TextStyle {
    TextStyle {
        size,
        spacing,
        color,
        weight,
        lines,
    }
}
fn measure(layout: &mut Layout, text: &str, width: f32, style: TextStyle) -> TextBlock {
    layout
        .text
        .measure_profile(&[(text.to_owned(), None)], width, style)
}
fn add_text(layout: &mut Layout, block: TextBlock, x: f32, y: f32) {
    layout.items.push(Item::Text { block, x, y });
}
fn vector(layout: &mut Layout, icon: &str, x: f32, y: f32, size: f32, color: u32) {
    if let Some(path) = vector_path(icon, x, y, size) {
        layout.items.push(Item::Path { path, color });
    }
}
fn vector_path(icon: &str, x: f32, y: f32, size: f32) -> Option<Path> {
    let (path, w, h) = match icon {
        "plus" => (Icon::Plus.path()?, 24.0, 24.0),
        "more" => (Icon::More.path()?, 24.0, 24.0),
        _ => {
            let icon = &profile_data()["icons"][icon];
            (
                svg_path(icon[0].as_str()?)?,
                icon[1].as_f64()? as f32,
                icon[2].as_f64()? as f32,
            )
        }
    };
    let k = size / w.max(h);
    path.transform(
        Transform::from_scale(k, k)
            .post_translate(x + (size - w * k) / 2.0, y + (size - h * k) / 2.0),
    )
}
