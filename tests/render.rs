use std::collections::BTreeMap;

use serde_json::{Value, json};
use skyshot::{
    RenderError, RenderInput, RenderOptions, Renderer, Theme, Warning, post_lookup,
    trusted_media_url,
};

const POST: &str = include_str!("fixtures/post.json");
const AVATAR: &str = "https://cdn.bsky.app/img/avatar/plain/demo";
const PHOTO: &str = "https://cdn.bsky.app/img/feed_thumbnail/plain/demo";

fn post() -> Value {
    serde_json::from_str(POST).unwrap()
}

#[test]
fn profile_resolution_and_moderation_bound_visible_assets() {
    use skyshot::{ProfileInput, ProfileRenderOptions, profile_url};
    let mut renderer = Renderer::new().unwrap();
    let options = ProfileRenderOptions::default();
    let actor = "did:plc:aaaaaaaaaaaaaaaaaaaaaaaa";
    let request = profile_url(&format!("at://{actor}/app.bsky.actor.profile/self")).unwrap();
    assert_eq!(request.path(), "/xrpc/app.bsky.actor.getProfile");
    assert_eq!(request.query_pairs().next().unwrap().1, actor);
    for input in [
        "https://bsky.app/profile/a.test/post/foo",
        "https://example.org/profile/a.test",
        "at://a.test/app.bsky.actor.profile/other",
    ] {
        assert!(profile_url(input).is_err(), "{input}");
    }
    let mut value = json!({
        "profile": { "did": actor, "handle": "example.com", "avatar": AVATAR, "banner": PHOTO,
            "description": "A bio with @valid.test", "links": [
                {"uri":"one", "url":"https://example.com", "icon":"one.png"},
                {"uri":"two", "url":"https://bit.ly/example", "icon":"two.png"}
            ] },
        "context": { "moderation": { "avatar":"hide", "banner":"blur", "links":{"one":"hide"} } }
    });
    let input = ProfileInput::from_json(&value.to_string()).unwrap();
    assert_eq!(
        renderer.profile_asset_urls(&input, options).unwrap(),
        [PHOTO]
    );
    value["profile"]["labels"] = json!([{"val":"!no-unauthenticated"}]);
    let input = ProfileInput::from_json(&value.to_string()).unwrap();
    assert!(
        renderer
            .profile_asset_urls(&input, options)
            .unwrap()
            .is_empty()
    );
    assert!(input.bio_mention_requests().is_empty());
    value["profile"]["status"] = json!({"status":"app.bsky.actor.status#live"});
    let input = ProfileInput::from_json(&value.to_string()).unwrap();
    assert!(
        renderer
            .render_profile(&input, options, &BTreeMap::new())
            .is_err()
    );
}

#[test]
fn malformed_facets_leave_a_renderable_post() {
    let mut post = post();
    post["record"]["facets"]
        .as_array_mut()
        .unwrap()
        .push(json!({
            "index": {"byteStart": 1, "byteEnd": 2},
            "features": [{"$type": "app.bsky.richtext.facet#tag", "tag":"bad"}]
        }));
    let input = RenderInput::from_json(&post.to_string()).unwrap();
    let mut renderer = Renderer::new().unwrap();
    let assets = BTreeMap::from([(
        AVATAR.to_owned(),
        include_bytes!("fixtures/tiles.png").to_vec(),
    )]);
    let output = renderer
        .render(&input, RenderOptions::default(), &assets)
        .unwrap();
    assert_eq!(output.warnings, vec![Warning::InvalidFacet]);
    let decoded = image::load_from_memory(&output.png).unwrap().to_rgba8();
    assert_eq!(decoded.dimensions(), (1200, output.height));
    assert!(decoded.pixels().any(|p| p.0 == [0x00, 0x6a, 0xff, 0xff]));
}

#[test]
fn follow_button_uses_theme_colors_at_both_widths() {
    let input = RenderInput::from_json(POST).unwrap();
    let mut renderer = Renderer::new().unwrap();
    for (theme, fill, text) in [
        (Theme::Light, [0x23, 0x2e, 0x3e, 0xff], [0xff; 4]),
        (
            Theme::Dim,
            [0xe2, 0xe7, 0xee, 0xff],
            [0x15, 0x1d, 0x28, 0xff],
        ),
        (Theme::Dark, [0xdc, 0xe2, 0xea, 0xff], [0, 0, 0, 0xff]),
    ] {
        for width in [390, 600] {
            let output = renderer
                .render(
                    &input,
                    RenderOptions {
                        theme,
                        width,
                        scale: 1,
                        ..RenderOptions::default()
                    },
                    &BTreeMap::new(),
                )
                .unwrap();
            let png = image::load_from_memory(&output.png).unwrap().to_rgba8();
            let (fill, text) = if width == 390 {
                ([0xe2, 0xe7, 0xee, 0xff], [0x15, 0x1d, 0x28, 0xff])
            } else {
                (fill, text)
            };
            assert_eq!(png.get_pixel(width - 28, 36).0, fill, "{theme:?}, {width}");
            let button = image::imageops::crop_imm(&png, width - 80, 22, 48, 30);
            assert!(
                button.to_image().pixels().any(|pixel| pixel.0 == text),
                "Follow foreground: {theme:?}, {width}"
            );
        }
    }
}

#[test]
fn verification_badges_render_in_every_author_header() {
    let mut renderer = Renderer::new().unwrap();
    for context in ["selected", "quote", "ancestor"] {
        for (status, visible) in [
            (json!(null), false),
            (json!({"verifiedStatus":"invalid"}), false),
            (json!({"verifiedStatus":"valid"}), true),
            (json!({"trustedVerifierStatus":"valid"}), true),
        ] {
            for name in ["Marisa Kabas", "", &"Long name ".repeat(60)] {
                let mut post = post();
                post["record"]["text"] = json!("Plain text");
                post["record"]["facets"] = json!([]);
                post["author"]["verification"] = json!(null);
                let mut author = post["author"].clone();
                author["displayName"] = json!(name);
                author["verification"] = status.clone();
                let input = match context {
                    "quote" => {
                        post["embed"] = json!({
                            "$type":"app.bsky.embed.record#view",
                            "record": {
                                "$type":"app.bsky.embed.record#viewRecord",
                                "author":author, "value":post["record"],
                            }
                        });
                        post
                    }
                    "ancestor" => {
                        let mut parent = post.clone();
                        parent["author"] = author;
                        json!({"thread": {
                            "$type":"app.bsky.feed.defs#threadViewPost", "post":post,
                            "parent":{"$type":"app.bsky.feed.defs#threadViewPost", "post":parent}
                        }})
                    }
                    _ => {
                        post["author"] = author;
                        post
                    }
                };
                let input = RenderInput::from_json(&input.to_string()).unwrap();
                for width in [280, 390, 600] {
                    for theme in [Theme::Light, Theme::Dim, Theme::Dark] {
                        let output = renderer
                            .render(
                                &input,
                                RenderOptions {
                                    width,
                                    theme,
                                    scale: 1,
                                    ancestors: 1,
                                    ..RenderOptions::default()
                                },
                                &BTreeMap::new(),
                            )
                            .unwrap();
                        let png = image::load_from_memory(&output.png).unwrap().to_rgba8();
                        let blue = png
                            .enumerate_pixels()
                            .filter(|(_, _, pixel)| pixel.0 == [0x00, 0x6a, 0xff, 0xff])
                            .map(|(x, y, _)| (x, y))
                            .collect::<Vec<_>>();
                        assert_eq!(
                            !blue.is_empty(),
                            visible,
                            "{context}, {status}, {width}, {theme:?}, {name}"
                        );
                        assert!(
                            blue.iter().all(|(x, _)| *x < width - 16),
                            "badge must stay inside its header"
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn blocked_and_sensitive_content_never_request_assets() {
    let mut post = post();
    post["embed"] = json!({"$type":"app.bsky.embed.images#view","images":[{"thumb":PHOTO}]});
    for value in ["porn", "!no-unauthenticated", "!hide", "!warn"] {
        post["labels"] = json!([{"val": value}]);
        let input = RenderInput::from_json(&post.to_string()).unwrap();
        assert!(
            input
                .asset_urls(RenderOptions::default())
                .unwrap()
                .is_empty()
        );
    }
    post["labels"] = json!([]);
    post["record"]["labels"] =
        json!({"$type":"com.atproto.label.defs#selfLabels","values":[{"val":"porn"}]});
    let input = RenderInput::from_json(&post.to_string()).unwrap();
    assert!(
        input
            .asset_urls(RenderOptions::default())
            .unwrap()
            .is_empty()
    );
    post["record"].as_object_mut().unwrap().remove("labels");
    post["labels"] = json!([{"val":"sensitive"}]);
    let input = RenderInput::from_json(&post.to_string()).unwrap();
    assert_eq!(
        input.asset_urls(RenderOptions::default()).unwrap(),
        vec![AVATAR]
    );
}

#[test]
fn ancestors_are_ordered_and_only_opted_in_assets_are_listed() {
    let mut parent = post();
    parent["author"]["avatar"] = json!(PHOTO);
    let thread = json!({"thread": {
        "$type":"app.bsky.feed.defs#threadViewPost", "post":post(),
        "parent": {"$type":"app.bsky.feed.defs#threadViewPost", "post": parent}
    }});
    let input = RenderInput::from_json(&thread.to_string()).unwrap();
    assert_eq!(
        input.asset_urls(RenderOptions::default()).unwrap(),
        vec![AVATAR]
    );
    let options = RenderOptions {
        ancestors: 1,
        ..RenderOptions::default()
    };
    assert_eq!(input.asset_urls(options).unwrap(), vec![AVATAR, PHOTO]);
}

#[test]
fn invalid_width_and_oversized_payloads_fail_before_painting() {
    let options = RenderOptions {
        width: u32::MAX,
        ..RenderOptions::default()
    };
    assert!(
        RenderInput::from_json(POST)
            .unwrap()
            .asset_urls(options)
            .is_err()
    );
    assert!(matches!(
        RenderInput::from_json(&"a".repeat(2 * 1024 * 1024 + 1)),
        Err(RenderError::Limit(_))
    ));
}

#[test]
fn resolver_only_builds_public_requests() {
    let lookup =
        post_lookup("https://bsky.app/profile/bsky.app/post/3n2ozugncr22j?ref=share").unwrap();
    assert_eq!(
        lookup.handle_url().unwrap().host_str(),
        Some("public.api.bsky.app")
    );
    let request = lookup.thread_url(Some("did:plc:example"), 0).unwrap();
    assert_eq!(request.query_pairs().collect::<Vec<_>>()[2].1, "1");
    let direct = lookup.thread_url(None, 0).unwrap();
    assert_eq!(
        direct.query_pairs().next().unwrap().1,
        "at://bsky.app/app.bsky.feed.post/3n2ozugncr22j"
    );
    assert!(lookup.thread_url(Some("not-a-did.example"), 0).is_err());
    assert!(post_lookup("https://bsky.app@other.example/profile/bsky.app/post/test").is_err());
    assert!(post_lookup("at://did:plc:example/app.bsky.feed.like/test").is_err());
    assert!(trusted_media_url("https://cdn.bsky.app.evil.example/img/avatar/demo").is_none());
    assert!(trusted_media_url("https://cdn.bsky.app:8080/img/avatar/demo").is_none());
    assert!(trusted_media_url("https://video.bsky.app/watch/test/index.m3u8").is_none());
}

#[test]
fn corrupt_optional_assets_and_unknown_embeds_have_distinct_warnings() {
    let mut post = post();
    post["embed"] = json!({"$type":"app.bsky.embed.future#view"});
    let input = RenderInput::from_json(&post.to_string()).unwrap();
    let failed = BTreeMap::from([(AVATAR.to_owned(), b"bad image".to_vec())]);
    let mut renderer = Renderer::new().unwrap();
    let result = renderer
        .render(&input, RenderOptions::default(), &failed)
        .unwrap();
    assert_eq!(
        result.warnings,
        vec![
            Warning::UnsupportedEmbed("app.bsky.embed.future#view".into()),
            Warning::InvalidAsset(AVATAR.into()),
        ]
    );
}

#[test]
fn video_posters_map_to_the_first_party_cdn_without_following_redirects() {
    let original = "https://video.bsky.app/watch/did%3Aplc%3Aexample/bafkrei-example/thumbnail.jpg";
    let expected =
        "https://video.cdn.bsky.app/hls/did%3Aplc%3Aexample/bafkrei-example/thumbnail.jpg";
    assert_eq!(trusted_media_url(original).unwrap().as_str(), expected);
}
