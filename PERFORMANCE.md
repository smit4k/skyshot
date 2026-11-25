# Performance and Chrome verification

Measurements below are local arm64 macOS results, including PNG encoding, excluding post/media network requests. They are observations, not a device-independent latency promise.

| Text/link post, WASM at 2× | Before, median | After, median |
| --- | ---: | ---: |
| 390 CSS px | 5.71 ms | 1.76 ms |
| 600 CSS px | 7.90 ms | 2.02 ms |

The PNG encoder uses its existing fast compression with a fixed `Up` filter instead of evaluating five scanline filters. The decoded pixels are unchanged by filtering. The text benchmark PNGs grow about 18–22%. Release builds use thin LTO and one code generation unit. Before profile support, the production WASM measured about 5.2 MiB raw and **1.83 MiB gzip**. The compressed budget in `scripts/bench.mjs` is 3 MB.

The site opts into lazy fonts. An ASCII post requests **zero fallback packs**. Unicode scripts and emoji trigger concurrent, cached reads of the existing pinned fonts. This conservatively scans all strings in the supplied JSON, so metadata or hidden text can cause extra font downloads. The complete optional set remains about 51 MiB; this change does not subset those fonts. Default package loading stays eager for compatibility. Explicit `fonts` arrays retain their previous behavior.

JPEGs that shrink by at least 2× can decode at reduced resolution, so a 1,000 × 1,000 original avatar typically decodes at 125 × 125 for an 84 × 84 slot. Decoded pixels and painted tiles share a 32 MiB cache. Layouts reuse decoded pixels when they select the same JPEG reduction. The bilinear sampler computes interpolation weights once per row and column and interpolates premultiplied RGBA. Media pixels can differ from builds that used tiny-skia’s sampler.

In Node WASM at 600 px and 2×, the Zed post took **14.8 ms** with an uncached attachment on a warm renderer, down from **20.9 ms** before the sampler change. The cached median was **4.9 ms**. In a Chrome smoke test after the change, the first render took 64.9 ms in a fresh worker and repeats took 4.6–5.8 ms. [Zed post](https://bsky.app/profile/zed.dev/post/3n25hrtucgm2w)

`renderMany` parses a post and copies its assets once. Its per-layout `renderMs` includes layout, any image preparation, painting, and PNG encoding. The site's single-render timer still brackets the complete `render()` call.

The site warms the render worker with a bundled text post when the page opens. It then renders a local JPEG avatar and PNG attachment if a visitor has not submitted a post. The two samples add about 30 kB to page startup. With 650 ms between opening the page and submitting, the first Zed render fell from 52.8 ms with text-only warmup to 31.7 ms with image warmup in a local Chrome smoke test. The preview stayed empty until submission. The render timer excludes warmup and network requests. New images still need to be decoded and sampled.

Fetching can dominate the delay before the preview. In one Chrome run before and one after switching to direct handle lookup, a post with text and an avatar went from 1.11 s to 0.79 s from Generate to preview. Rendering took about 39 ms in both runs. The browser skips the separate handle lookup and downloads visible media in batches of up to four. Network timing varies across runs.

Before the sampler change, the local site’s Generate flow was measured in headless Chrome 158 at the default 600 px width and 2× scale:

| Post | Repeats before | Repeats after | Fresh worker after |
| --- | ---: | ---: | ---: |
| Text with a 1,000 × 1,000 JPEG avatar | 20.9–21.9 ms | 2.7–8.2 ms | 56.8 ms |
| Text with avatar and a 1,432 × 908 PNG attachment | 75.4–77.8 ms | 4.9–6.8 ms | 80.0 ms |

Opening the avatar-only post after the attachment post in the same worker took 24.9 ms on its first render. The render timer includes image preparation, layout, painting, and PNG encoding when those operations run. It starts after network and module loading. [Avatar-only post](https://bsky.app/profile/stumptoemerald.bsky.social/post/3n2ytlof3mk27) · [Attachment post](https://bsky.app/profile/zed.dev/post/3n25hrtucgm2w)

The benchmark reports initialization, font preparation, first render, warm median, p95, PNG size, and core size separately. Only the first row is a cold renderer. Subsequent cases share its caches.

```sh
sh scripts/build-wasm.sh
npm run bench
npm run bench -- --lazy
```

The benchmark uses text with and without an avatar plus multilingual text, widths 390 and 600, scale 2, eight warmups and 60 measured samples. Run it without concurrent builds for useful comparisons. Repeating the same post benefits from the glyph and image caches.

## Chrome oracle

```sh
SKYSHOT_VERIFY=1 sh scripts/build-wasm.sh
npm run test:chrome
npm run test:chrome -- --strict
```

Set `CHROME` if the executable is elsewhere. The checker uses a temporary headless Chrome profile and a localhost server; it adds no npm dependency. Reports are written to `target/chrome-report.json`. Diagnostic WASM exports are compiled only with `verify` and staged separately in `target/chrome`, outside the shipped package.

The controlled DOM reference uses the pinned [Bluesky detail layout](https://github.com/bluesky-social/social-app/blob/c85e67b62082df8a2b0a45640298f18e8c48ce3d/src/screens/PostThread/components/ThreadItemAnchor.tsx), [word wrapping](https://github.com/bluesky-social/social-app/blob/c85e67b62082df8a2b0a45640298f18e8c48ce3d/src/style.css), [Inter feature settings](https://github.com/bluesky-social/social-app/blob/c85e67b62082df8a2b0a45640298f18e8c48ce3d/src/alf/fonts.ts), and the same pinned font files as the renderer. It measures the production text/layout path; it does not reproduce the complete React app, its system fallback fonts, or browser rasterization.

Current Chrome 158 post results:

- **2,433 / 2,688 text layouts match visible line breaks.** The corpus includes URLs, whitespace, ligatures, emoji, CJK, Arabic, Hebrew, Devanagari and Thai at 14 widths, three sizes and two weights. Trailing spaces/newlines are normalized for this comparison; their exact hanging-space behavior is not certified.
- **186 / 192 text-card geometries match.** Eight text cases × four widths × three themes × two scales, checking output dimensions and body position/height. Six failures are one narrow URL case across themes and scales. These are text cards with/without stats, not media or external-link cards. Theme pixels are not checked by this geometry test.
- Author-header checks cover main posts, quoted posts and ancestors across nine verification/name cases, four widths, three themes and two scales. They inspect rendered PNGs for badge presence, placement after the name and clipping, and require zero failures outside the known-differences baseline. Unclipped name-width differences from Chrome are reported separately; clipped names can also differ in ellipsis placement.
- Matching line breaks do not imply matching advances or pixels. The report includes advance differences above 1 px. Variable-font optical sizing is not supported by the current cosmic-text font instance path, whereas Chrome uses it automatically. URL breaking, bidi and language-specific segmentation also need further work. Browser/system emoji can differ from the pinned Noto emoji.

`test:chrome` rejects differences not recorded in `tests/fixtures/chrome-known-differences.json`; improvements may remove recorded failures. Author badge, clipping and theme-color checks always require zero failures. `--strict` also requires zero line-break, text-width and card geometry differences. `--update-baseline` explicitly accepts the current text/card failure fingerprints and must only follow review of the detailed report. A passing regression check does not certify full Chrome parity. Chrome/ICU changes can require baseline review.

## Deterministic PNGs

`tests/wasm.test.mjs` compares 51 frozen post variants and 17 profile variants byte for byte between native and WASM. Font tests also compare lazy and eager output as scripts are added. CI builds the same corpus on Linux and macOS for comparison with WASM artifacts.

## Profile measurements

On arm64 macOS with Node 22.16, the three frozen profiles with their original media took 2.9–7.1 ms median and 3.2–7.2 ms p95 with warm caches at 390 and 600 CSS pixels, scale 2. These measurements include PNG encoding. First renders for new media took 121–200 ms at 390 pixels, including image decoding and excluding network time. Decode and tile caches share the existing 32 MiB allowance.

The production core is 2.74 MB gzip with the profile renderer and its Inter optical instances. The `npm run bench` post medians changed by −2.1% to +1.4% on the same host. The profile section of the Chrome report passed six captured headers, including sampled raster crops. See [SPEC.md](SPEC.md) for the checked regions and tolerances.

In one live Chrome site check, nasso.dev took about 1.54 s from submission to the first preview, including network requests. Rendering took 86 ms. Switching the theme reused the resolved data and took 10 ms to render.
