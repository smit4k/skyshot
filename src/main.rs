#![forbid(unsafe_code)]

use std::{
    collections::BTreeMap,
    fs::{self, File},
    io::{self, Read, Write},
    path::{Path, PathBuf},
    time::Duration,
};

use clap::{Parser, Subcommand, ValueEnum};
use reqwest::{Url, blocking::Client, redirect::Policy};
use serde::Deserialize;
use skyshot::{RenderInput, RenderOptions, Renderer, Theme, post_lookup, trusted_media_url};

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
        input: String,
        #[arg(short, long, default_value = "post.png")]
        output: PathBuf,
        #[arg(long, default_value_t = 600)]
        width: u32,
        #[arg(long, default_value_t = 2)]
        scale: u32,
        #[arg(long, value_enum, default_value_t = Mode::Light)]
        theme: Mode,
        #[arg(long, default_value_t = 0)]
        ancestors: usize,
        /// IANA time zone for the timestamp. Defaults to the system time zone.
        #[arg(long)]
        time_zone: Option<chrono_tz::Tz>,
        /// JSON object mapping asset URLs to local files relative to the manifest.
        #[arg(long)]
        assets: Option<PathBuf>,
        /// Directory of fallback fonts. Defaults to SKYSHOT_FONTS or fonts/fallback.
        #[arg(long)]
        fonts: Option<PathBuf>,
    },
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
    let Command::Render {
        input,
        output,
        width,
        scale,
        theme,
        ancestors,
        time_zone,
        assets,
        fonts,
    } = Cli::parse().command;
    let options = RenderOptions {
        width,
        scale,
        theme: theme.into(),
        ancestors,
        time_zone: time_zone.unwrap_or_else(local_time_zone),
        ..RenderOptions::default()
    };
    if ancestors > 3 || !(280..=1000).contains(&width) || !(1..=3).contains(&scale) {
        return Err("width must be 280–1000, scale 1–3, and ancestors 0–3".into());
    }
    let remote = input.starts_with("https://") || input.starts_with("at://");
    let client = remote
        .then(|| {
            Client::builder()
                .redirect(Policy::none())
                .connect_timeout(Duration::from_secs(4))
                .timeout(Duration::from_secs(8))
                .user_agent(concat!("skyshot/", env!("CARGO_PKG_VERSION")))
                .build()
        })
        .transpose()?;
    let bytes = if let Some(client) = &client {
        resolve(client, &input, ancestors)?
    } else if input == "-" {
        read_limit(io::stdin().lock(), MAX_JSON)?
    } else {
        read_limit(File::open(input)?, MAX_JSON)?
    };
    let post = RenderInput::from_json(std::str::from_utf8(&bytes)?)?;
    let local = assets
        .as_deref()
        .map(read_manifest)
        .transpose()?
        .unwrap_or_default();
    let mut images = BTreeMap::new();
    let mut left = ASSET_BUDGET;
    for url in post.asset_urls(options)? {
        let limit = left.min(MAX_IMAGE);
        if limit == 0 {
            break;
        }
        let bytes = if let Some(path) = local.get(&url) {
            read_limit(File::open(path)?, limit)?
        } else if let Some((client, approved)) = client.as_ref().zip(trusted_media_url(&url)) {
            let Ok(bytes) = fetch(client, approved, limit) else {
                continue;
            };
            bytes
        } else {
            continue;
        };
        left -= bytes.len();
        images.insert(url, bytes);
    }
    let explicit = fonts.or_else(|| std::env::var_os("SKYSHOT_FONTS").map(PathBuf::from));
    let directory = explicit
        .as_deref()
        .unwrap_or_else(|| Path::new("fonts/fallback"));
    let fallback = load_fonts(directory, explicit.is_none())?;
    let rendered = Renderer::with_fonts(fallback)?.render(&post, options, &images)?;
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
