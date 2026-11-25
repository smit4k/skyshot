#![forbid(unsafe_code)]

use std::{
    collections::BTreeMap,
    fs::{self, File},
    io::{self, Read, Write},
    path::{Path, PathBuf},
    time::Duration,
};

use clap::{Args, Parser, Subcommand, ValueEnum};
use reqwest::{Url, blocking::Client, redirect::Policy};
use serde::Deserialize;
use skyshot::{
    ProfileInput, ProfileLayout, ProfileRenderOptions, RenderInput, RenderOptions, RenderResult,
    Renderer, Theme, post_lookup, profile_url, trusted_media_url,
};

const MAX_JSON: usize = 2 * 1024 * 1024;
const MAX_IMAGE: usize = 8 * 1024 * 1024;
const ASSET_BUDGET: usize = 32 * 1024 * 1024;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

#[derive(Parser)]
#[command(version, about)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Render a public Bluesky post URL, AT URI, JSON file, or JSON on stdin (-).
    Render {
        #[command(flatten)]
        common: RenderArgs,
        #[arg(short, long, default_value = "post.png")]
        output: PathBuf,
        #[arg(long, default_value_t = 0)]
        ancestors: usize,
        /// IANA time zone for the timestamp. Defaults to the system time zone.
        #[arg(long)]
        time_zone: Option<chrono_tz::Tz>,
    },
    /// Render a public Bluesky profile URL, profile AT URI, JSON file, or stdin (-).
    RenderProfile {
        #[command(flatten)]
        common: RenderArgs,
        #[arg(short, long, default_value = "profile.png")]
        output: PathBuf,
        #[arg(long, default_value = "auto", value_parser = ["auto", "mobile", "desktop"])]
        layout: String,
        /// JSON file containing caller-supplied viewer context.
        #[arg(long)]
        context: Option<PathBuf>,
    },
}

#[derive(Args)]
struct RenderArgs {
    input: String,
    #[arg(long, default_value_t = 600, value_parser = clap::value_parser!(u32).range(280..=1000))]
    width: u32,
    #[arg(long, default_value_t = 2, value_parser = clap::value_parser!(u32).range(1..=3))]
    scale: u32,
    #[arg(long, value_enum, default_value_t = Mode::Light)]
    theme: Mode,
    /// JSON object mapping asset URLs to local files relative to the manifest.
    #[arg(long)]
    assets: Option<PathBuf>,
    /// Directory of fallback fonts. Defaults to SKYSHOT_FONTS or fonts/fallback.
    #[arg(long)]
    fonts: Option<PathBuf>,
}

impl RenderArgs {
    fn client(&self) -> Result<Option<Client>> {
        if !(self.input.starts_with("https://") || self.input.starts_with("at://")) {
            return Ok(None);
        }
        Ok(Some(
            Client::builder()
                .redirect(Policy::none())
                .connect_timeout(Duration::from_secs(4))
                .timeout(Duration::from_secs(8))
                .user_agent(concat!("skyshot/", env!("CARGO_PKG_VERSION")))
                .build()?,
        ))
    }

    fn read_input(&self) -> Result<Vec<u8>> {
        if self.input == "-" {
            read_limit(io::stdin().lock(), MAX_JSON)
        } else {
            read_limit(File::open(&self.input)?, MAX_JSON)
        }
    }

    fn renderer(&self) -> Result<Renderer> {
        let explicit = self
            .fonts
            .clone()
            .or_else(|| std::env::var_os("SKYSHOT_FONTS").map(PathBuf::from));
        let directory = explicit
            .as_deref()
            .unwrap_or_else(|| Path::new("fonts/fallback"));
        Ok(Renderer::with_fonts(load_fonts(
            directory,
            explicit.is_none(),
        )?)?)
    }
}

#[derive(Clone, Copy, ValueEnum)]
enum Mode {
    Light,
    Dark,
    Dim,
}
impl From<Mode> for Theme {
    fn from(value: Mode) -> Self {
        match value {
            Mode::Light => Self::Light,
            Mode::Dark => Self::Dark,
            Mode::Dim => Self::Dim,
        }
    }
}

fn main() {
    if let Err(error) = run() {
        eprintln!("skyshot: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    match Cli::parse().command {
        Command::Render {
            common,
            output,
            ancestors,
            time_zone,
        } => {
            if ancestors > 3 {
                return Err("width must be 280–1000, scale 1–3, and ancestors 0–3".into());
            }
            let options = RenderOptions {
                width: common.width,
                scale: common.scale,
                theme: common.theme.into(),
                ancestors,
                time_zone: time_zone.unwrap_or_else(local_time_zone),
                ..RenderOptions::default()
            };
            let client = common.client()?;
            let bytes = match &client {
                Some(client) => resolve(client, &common.input, ancestors)?,
                None => common.read_input()?,
            };
            let post = RenderInput::from_json(std::str::from_utf8(&bytes)?)?;
            let images = collect_assets(
                post.asset_urls(options)?,
                common.assets.as_deref(),
                client.as_ref(),
                1,
            )?;
            write_result(common.renderer()?.render(&post, options, &images)?, &output)
        }
        Command::RenderProfile {
            common,
            output,
            layout,
            context,
        } => {
            let options = ProfileRenderOptions {
                width: common.width,
                scale: common.scale,
                theme: common.theme.into(),
                layout: match layout.as_str() {
                    "mobile" => ProfileLayout::Mobile,
                    "desktop" => ProfileLayout::Desktop,
                    _ => ProfileLayout::Auto,
                },
            };
            let client = common.client()?;
            let bytes = match &client {
                Some(client) => fetch(client, profile_url(&common.input)?, MAX_JSON)?,
                None => common.read_input()?,
            };
            let mut input = parse_profile_input(&bytes, context.as_deref(), client.as_ref())?;
            if let Some(client) = &client {
                let profiles = input
                    .bio_mention_requests()
                    .into_iter()
                    .filter_map(|url| fetch(client, url, MAX_JSON).ok())
                    .filter_map(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok())
                    .filter_map(|mut value| value["profiles"].take().as_array().cloned())
                    .flatten()
                    .collect::<Vec<_>>();
                input.resolve_bio_mentions(&profiles);
            }
            let mut renderer = common.renderer()?;
            let urls = renderer.profile_asset_urls(&input, options)?;
            let images = collect_assets(urls, common.assets.as_deref(), client.as_ref(), 4)?;
            write_result(renderer.render_profile(&input, options, &images)?, &output)
        }
    }
}

fn parse_profile_input(
    bytes: &[u8],
    context: Option<&Path>,
    client: Option<&Client>,
) -> Result<ProfileInput> {
    use serde_json::{Value, json};
    let mut envelope: Value = serde_json::from_slice(bytes)?;
    if envelope.get("profile").is_none() {
        envelope = json!({ "profile": envelope });
    }
    if let Some(path) = context {
        envelope["context"] = serde_json::from_slice(&read_limit(File::open(path)?, MAX_JSON)?)?;
    }
    if client.is_some() {
        if envelope["context"].is_null() {
            envelope["context"] = json!({});
        }
        let context = envelope["context"]
            .as_object_mut()
            .ok_or("profile context must be an object")?;
        if context.get("snapshotAt").is_none_or(Value::is_null) {
            let now: chrono::DateTime<chrono::Utc> = std::time::SystemTime::now().into();
            context.insert("snapshotAt".into(), json!(now.to_rfc3339()));
        }
    }
    let input = ProfileInput::from_json(&envelope.to_string())?;
    if let (Some(client), Some(url)) = (client, input.labeler_request()) {
        let mut response: Value = serde_json::from_slice(&fetch(client, url, MAX_JSON)?)?;
        envelope["labeler"] = response["views"]
            .get_mut(0)
            .ok_or("Bluesky returned no labeler details")?
            .take();
        return Ok(ProfileInput::from_json(&envelope.to_string())?);
    }
    Ok(input)
}

fn collect_assets(
    urls: Vec<String>,
    manifest: Option<&Path>,
    client: Option<&Client>,
    concurrency: usize,
) -> Result<BTreeMap<String, Vec<u8>>> {
    let local = manifest.map(read_manifest).transpose()?.unwrap_or_default();
    let mut images = BTreeMap::new();
    let mut left = ASSET_BUDGET;
    let mut urls = urls.into_iter();
    while left > 0 {
        let limit = left.min(MAX_IMAGE);
        let count = concurrency.min((left / MAX_IMAGE).max(1));
        let batch: Vec<_> = urls.by_ref().take(count).collect();
        if batch.is_empty() {
            break;
        }
        let load = |url: &String| -> Result<Option<Vec<u8>>> {
            if let Some(path) = local.get(url) {
                Ok(Some(read_limit(File::open(path)?, limit)?))
            } else if let Some((client, approved)) = client.zip(trusted_media_url(url)) {
                Ok(fetch(client, approved, limit).ok())
            } else {
                Ok(None)
            }
        };
        // Only optional network reads are concurrent. Keep local filesystem errors on the CLI thread.
        let mut results: Vec<_> = batch
            .iter()
            .map(|url| local.contains_key(url).then(|| load(url)))
            .collect();
        if count > 1 && client.is_some() {
            std::thread::scope(|scope| {
                let pending: Vec<_> = batch
                    .iter()
                    .enumerate()
                    .filter(|(i, _)| results[*i].is_none())
                    .map(|(i, url)| (i, scope.spawn(move || load(url).ok().flatten())))
                    .collect();
                for (i, thread) in pending {
                    results[i] = Some(
                        thread
                            .join()
                            .map_err(|_| "asset request failed")
                            .map_err(Into::into),
                    );
                }
            });
        }
        for (url, result) in batch.into_iter().zip(results) {
            let bytes = match result {
                Some(result) => result?,
                None => load(&url)?,
            };
            if let Some(bytes) = bytes {
                left -= bytes.len();
                images.insert(url, bytes);
            }
        }
    }
    Ok(images)
}

fn write_result(rendered: RenderResult, output: &Path) -> Result<()> {
    for warning in &rendered.warnings {
        eprintln!("skyshot: {}", serde_json::to_string(warning)?);
    }
    if output.as_os_str() == "-" {
        io::stdout().lock().write_all(&rendered.png)?;
    } else {
        fs::write(output, rendered.png)?;
    }
    Ok(())
}

fn local_time_zone() -> chrono_tz::Tz {
    std::env::var("TZ")
        .ok()
        .and_then(|zone| zone.parse().ok())
        .or_else(|| iana_time_zone::get_timezone().ok()?.parse().ok())
        .unwrap_or(chrono_tz::UTC)
}

fn load_fonts(directory: &Path, optional: bool) -> Result<Vec<Vec<u8>>> {
    let entries = match fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(error) if optional && error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error.into()),
    };
    let mut paths = Vec::new();
    for entry in entries {
        let path = entry?.path();
        if path.extension().is_some_and(|v| v == "ttf" || v == "otf") {
            paths.push(path);
        }
    }
    paths.sort_unstable();
    paths
        .into_iter()
        .map(|path| read_limit(File::open(path)?, 32 * 1024 * 1024))
        .collect()
}

fn read_manifest(path: &Path) -> Result<BTreeMap<String, PathBuf>> {
    let raw = read_limit(File::open(path)?, MAX_JSON)?;
    let mut entries: BTreeMap<String, PathBuf> = serde_json::from_slice(&raw)?;
    if let Some(base) = path.parent() {
        for value in entries.values_mut() {
            *value = base.join(&value);
        }
    }
    Ok(entries)
}

fn read_limit(reader: impl Read, limit: usize) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    reader.take(limit as u64 + 1).read_to_end(&mut bytes)?;
    if bytes.len() > limit {
        return Err(format!("response exceeds {limit} bytes").into());
    }
    Ok(bytes)
}

fn fetch(client: &Client, url: Url, limit: usize) -> Result<Vec<u8>> {
    let response = client.get(url).send()?.error_for_status()?;
    if !response.status().is_success() {
        return Err(format!("unexpected HTTP status {}", response.status()).into());
    }
    if response
        .content_length()
        .is_some_and(|size| size > limit as u64)
    {
        return Err(format!("response exceeds {limit} bytes").into());
    }
    read_limit(response, limit)
}

fn resolve(client: &Client, input: &str, ancestors: usize) -> Result<Vec<u8>> {
    #[derive(Deserialize)]
    struct Identity {
        did: String,
    }
    let lookup = post_lookup(input)?;
    let did = lookup
        .handle_url()
        .map(|url| -> Result<String> {
            let bytes = fetch(client, url, MAX_JSON)?;
            Ok(serde_json::from_slice::<Identity>(&bytes)?.did)
        })
        .transpose()?;
    let url = lookup.thread_url(did.as_deref(), ancestors)?;
    let response = client.get(url).send()?;
    let status = response.status();
    let bytes = read_limit(response, MAX_JSON)?;
    if status.is_success() {
        return Ok(bytes);
    }
    if matches!(status.as_u16(), 400 | 404)
        && serde_json::from_slice::<serde_json::Value>(&bytes)
            .ok()
            .is_some_and(|v| v["error"] == "NotFound")
    {
        return Ok(br#"{"thread":{"$type":"app.bsky.feed.defs#notFoundPost"}}"#.to_vec());
    }
    Err(format!("Bluesky AppView returned HTTP {status}").into())
}
