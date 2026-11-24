# Skyshot

Skyshot renders a public Bluesky post to PNG. The same Rust renderer runs natively and in WebAssembly. It measures the post and paints it into a pixel buffer using the supplied JSON, image bytes, and fonts.

## CLI

Build with Rust 1.89 or newer. Use Node 20 or newer to download the pinned emoji and script fallback fonts.

```sh
node scripts/fonts.mjs
cargo build --release --locked
target/release/skyshot render 'https://bsky.app/profile/bsky.app/post/3n2ozugncr22j' -o post.png
```

The CLI also accepts a post AT URI, a `PostView` or `getPostThread` JSON file, or JSON on stdin. JSON input renders offline. Use `--assets` with a JSON object mapping URLs to files relative to the manifest.

```sh
target/release/skyshot render tests/fixtures/post.json \
  --assets tests/fixtures/assets.json --width 390 --scale 2 --theme dim -o post.png
```

Options include `--width 280..1000`, `--scale 1..3`, `--theme light|dark|dim`, and `--ancestors 0..3`. Defaults are 600 pixels, 2×, light, and no ancestors. CLI timestamps use the system time zone. Pass `--time-zone America/Detroit` or `--time-zone UTC` to pin it. Date labels use English. `-o -` writes PNG bytes to stdout. Set `SKYSHOT_FONTS` or pass `--fonts` to use a different fallback font directory. Without that directory, the CLI uses the bundled Inter font.

## Rust

Use `default-features = false` for a build without CLI dependencies. The host owns resource loading. Supply Noto font bytes to `Renderer::with_fonts` for emoji and additional scripts.

```rust
use std::collections::BTreeMap;
use skyshot::{RenderInput, RenderOptions, Renderer};

fn render(json: &str, assets: &BTreeMap<String, Vec<u8>>) -> Result<Vec<u8>, skyshot::RenderError> {
    let mut renderer = Renderer::new()?;
    let input = RenderInput::from_json(json)?;
    Ok(renderer.render(&input, RenderOptions::default(), assets)?.png)
}
```

Reuse the renderer across posts to retain its font and glyph caches. `input.asset_urls(options)` returns URLs for visible media only. Missing or invalid optional assets produce placeholders and warnings.

## JavaScript and WebAssembly

```sh
rustup target add wasm32-unknown-unknown
cargo install wasm-bindgen-cli --version 0.2.130 --locked
sh scripts/build-wasm.sh
```

The build stages an ES module, one WASM binary, and the fallback fonts in `packages/skyshot`. It can be installed as a local npm dependency from that directory.

```js
import { writeFile } from 'node:fs/promises';
import { loadSkyshot } from './packages/skyshot/node.mjs';

const skyshot = await loadSkyshot();
const options = { width: 390, theme: 'dim' };
const post = await skyshot.resolve('https://bsky.app/profile/bsky.app/post/3n2ozugncr22j', options);
const { png } = skyshot.render(post, options);
await writeFile('post.png', png);
```

Use the web entry point in a browser or worker and serve the generated WASM and fonts alongside it. Pass `{ fonts: [...] }` to customize the fonts, and `{ fetch }` to provide the resolver's HTTP implementation. The browser and Node wrappers use the local time zone and hour cycle by default. Set `timeZone` and `hour12` to pin those values. `render` is synchronous and accepts fetched data, raw AppView JSON, or `{ post, assets }` with a URL-to-`Uint8Array` map. `renderMany(input, optionsList)` renders up to eight layouts with shared image caches and reports `renderMs` for each PNG after batch setup.

For smaller startup downloads, use `loadSkyshot({ lazy: true })`. `resolve()` automatically prepares fallback fonts; for offline JSON, call `await skyshot.prepare(post)` before `skyshot.render(post)`. Required fonts load concurrently and their bytes are cached between loaders. An ASCII post uses bundled Inter without downloading fallback fonts. Rendering unprepared text in lazy mode throws instead of silently omitting its fonts. Supplying an explicit `fonts` array keeps the caller in control and disables automatic font loading. The default remains eager loading for compatibility.

## Website

The single-page site is statically generated with Node, with no npm dependencies or server runtime. It uses the browser WebAssembly package in a web worker. Paste a public Bluesky post link to preview and download its PNG. Three theme boxes select Light, Dim, or pure-black Dark; changing the theme regenerates the preview without fetching the post again.

After building WebAssembly with the prerequisites above:

```sh
npm run build
python3 -m http.server 4321 --directory dist --bind 127.0.0.1
```

Open `http://127.0.0.1:4321`. Deploy the contents of `dist/` to any static host; relative asset paths also work under a subdirectory. Serve `.wasm` as `application/wasm`. The page follows the system light or dark theme; the image theme defaults to Light.

The renderer loads in a worker when the page opens and requires a browser with WebAssembly SIMD support. The worker warms the text renderer, then decodes and renders a bundled JPEG and PNG while waiting for the first submission. The site downloads fallback fonts only when the supplied post needs their scripts. An ASCII post needs no fallback download. The current WASM core is about 1.83 MiB gzipped. The complete optional fallback set is still about 51 MiB. Subsequent submissions reuse the worker, loaded fonts, and recently prepared images. Posts are fetched by handle or DID in one request to Bluesky's public API. Visible media downloads run up to four at a time within the existing byte limits. Because the image CDN blocks browser reads, the site resolves PLC identities and fetches image blobs from verified Bluesky-hosted PDS servers. Other media hosts, unavailable blobs, and blocked video thumbnails use the renderer's placeholders with a warning.

```sh
npm run test:site
```

## Verification

```sh
cargo fmt --check
cargo clippy --locked --all-targets --all-features -- -D warnings
cargo test --locked
cargo build --release --locked
sh scripts/build-wasm.sh
node --test tests/wasm.test.mjs
npm run test:fonts
npm run bench
SKYSHOT_VERIFY=1 sh scripts/build-wasm.sh
npm run test:chrome
```

The Node tests compare encoded PNG bytes from both builds for the same inputs and assets. CI also compares the same frozen PNG corpus across Linux, macOS, and WASM. The Chrome check reports known layout differences and rejects new ones; `npm run test:chrome -- --strict` requires zero differences and currently fails. See [PERFORMANCE.md](PERFORMANCE.md) for measurements and oracle scope, and [SPEC.md](SPEC.md) for the visual reference, visibility rules, limits, and known differences.
