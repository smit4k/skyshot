use std::collections::BTreeMap;

use serde_json::{Value, json};
use skyshot::{
    RenderError, RenderInput, RenderOptions, Renderer, Warning, post_lookup, trusted_media_url,
};

const POST: &str = include_str!("fixtures/post.json");
const AVATAR: &str = "https://cdn.bsky.app/img/avatar/plain/demo";
const PHOTO: &str = "https://cdn.bsky.app/img/feed_thumbnail/plain/demo";

fn post() -> Value {
    serde_json::from_str(POST).unwrap()
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
