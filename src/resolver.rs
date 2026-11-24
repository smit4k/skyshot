use url::Url;

use crate::RenderError;

const APPVIEW: &str = "https://public.api.bsky.app/xrpc/";

/// The identity and record key from a validated public Bluesky post URL or AT URI.
pub struct PostLookup {
    actor: String,
    rkey: String,
}

impl PostLookup {
    /// Ask the public AppView for the DID if this locator uses a handle.
    pub fn handle_url(&self) -> Option<Url> {
        (!self.actor.starts_with("did:")).then(|| {
            request(
                "com.atproto.identity.resolveHandle",
                &[("handle", &self.actor)],
            )
        })
    }

    /// Build the public thread request by handle or DID. Use one parent for the default reply indicator.
    pub fn thread_url(&self, did: Option<&str>, ancestors: usize) -> Result<Url, RenderError> {
        if did.is_some_and(|did| !valid_did(did)) || ancestors > 3 {
            return Err(RenderError::Input("invalid DID or ancestor count"));
        }
        let actor = did.unwrap_or(&self.actor);
        let uri = format!("at://{actor}/app.bsky.feed.post/{}", self.rkey);
        Ok(request(
            "app.bsky.feed.getPostThread",
            &[
                ("uri", &uri),
                ("depth", "0"),
                ("parentHeight", &ancestors.max(1).to_string()),
            ],
        ))
    }
}

fn request(method: &str, query: &[(&str, &str)]) -> Url {
    // Only called with constant XRPC method names.
    Url::parse_with_params(&format!("{APPVIEW}{method}"), query).unwrap_or_else(|_| unreachable!())
}

/// Parse an HTTPS bsky.app post URL or an AT URI for app.bsky.feed.post.
pub fn post_lookup(input: &str) -> Result<PostLookup, RenderError> {
    const INVALID: RenderError =
        RenderError::Input("expected a bsky.app post URL or a post AT URI");
    if input.len() > 2048 || input.bytes().any(|b| b.is_ascii_control()) {
        return Err(INVALID);
    }
    let url;
    let (actor, rkey) = if let Some(path) = input.strip_prefix("at://") {
        let mut parts = path.split('/');
        match (parts.next(), parts.next(), parts.next(), parts.next()) {
            (Some(actor), Some("app.bsky.feed.post"), Some(rkey), None) => (actor, rkey),
            _ => return Err(INVALID),
        }
    } else {
        url = Url::parse(input).map_err(|_| INVALID)?;
        if url.scheme() != "https"
            || url.host_str() != Some("bsky.app")
            || url.port().is_some()
            || !url.username().is_empty()
            || url.password().is_some()
        {
            return Err(INVALID);
        }
        let mut parts = url.path().trim_start_matches('/').split('/');
        match (
            parts.next(),
            parts.next(),
            parts.next(),
            parts.next(),
            parts.next(),
        ) {
            (Some("profile"), Some(actor), Some("post"), Some(rkey), None | Some("")) => {
                (actor, rkey)
            }
            _ => return Err(INVALID),
        }
    };
    if !(valid_did(actor) || valid_handle(actor))
        || rkey.is_empty()
        || rkey.len() > 512
        || matches!(rkey, "." | "..")
        || !rkey
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b".-_:~".contains(&b))
    {
        return Err(INVALID);
    }
    Ok(PostLookup {
        actor: actor.to_owned(),
        rkey: rkey.to_owned(),
    })
}

fn valid_did(did: &str) -> bool {
    let Some((method, id)) = did
        .strip_prefix("did:")
        .and_then(|rest| rest.split_once(':'))
    else {
        return false;
    };
    !method.is_empty()
        && method.bytes().all(|b| b.is_ascii_lowercase())
        && !id.is_empty()
        && did.len() <= 2048
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b".-_:%".contains(&b))
}

fn valid_handle(handle: &str) -> bool {
    handle.len() <= 253
        && handle.contains('.')
        && handle.split('.').all(|segment| {
            !segment.is_empty()
                && segment.len() <= 63
                && !segment.starts_with('-')
                && !segment.ends_with('-')
                && segment
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-')
        })
        && handle
            .rsplit('.')
            .next()
            .is_some_and(|tld| tld.as_bytes()[0].is_ascii_alphabetic())
}

/// Permit production Bluesky image and video thumbnail CDNs. Redirects must be disabled by the caller.
pub fn trusted_media_url(raw: &str) -> Option<Url> {
    if raw.len() > 4096 || raw.bytes().any(|b| b.is_ascii_control()) {
        return None;
    }
    let mut url = Url::parse(raw).ok()?;
    if url.scheme() != "https"
        || url.port().is_some()
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return None;
    }
    match url.host_str() {
        Some("cdn.bsky.app") if url.path().starts_with("/img/") => Some(url),
        Some("video.bsky.app")
            if url.path().starts_with("/watch/") && url.path().ends_with("/thumbnail.jpg") =>
        {
            // AppView returns a URL that redirects to this fixed public CDN route.
            let path = url.path().replacen("/watch/", "/hls/", 1);
            url.set_host(Some("video.cdn.bsky.app")).ok()?;
            url.set_path(&path);
            Some(url)
        }
        Some("video.cdn.bsky.app")
            if url.path().starts_with("/hls/") && url.path().ends_with("/thumbnail.jpg") =>
        {
            Some(url)
        }
        _ => None,
    }
}
