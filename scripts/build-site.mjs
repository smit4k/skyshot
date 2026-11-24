import { access, cp, mkdir } from 'node:fs/promises';

const root = new URL('../', import.meta.url);
const pkg = new URL('packages/skyshot/', root);
const dist = new URL('dist/', root);

try {
  await Promise.all(['web.mjs', 'shared.mjs', 'media-fetch.mjs', 'generated/skyshot.js', 'generated/skyshot_bg.wasm', 'fonts/NotoColorEmoji.ttf']
    .map(path => access(new URL(path, pkg))));
} catch (cause) {
  throw new Error('Build the WebAssembly package first: npm run build:wasm (see README.md for prerequisites).', { cause });
}

await mkdir(new URL('skyshot/', dist), { recursive: true });
await cp(new URL('site/', root), dist, { recursive: true });
await mkdir(new URL('warmup/', dist), { recursive: true });
for (const path of ['avatar.jpg', 'tiles.png']) {
  await cp(new URL(`tests/fixtures/${path}`, root), new URL(`warmup/${path}`, dist));
}
for (const path of ['web.mjs', 'shared.mjs', 'media-fetch.mjs', 'generated', 'fonts', 'LICENSE-Bluesky-icons']) {
  await cp(new URL(path, pkg), new URL(`skyshot/${path}`, dist), { recursive: true });
}
console.log('Static site built in dist/');
