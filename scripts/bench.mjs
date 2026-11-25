import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { gzipSync } from 'node:zlib';
import { loadSkyshot } from '../packages/skyshot/node.mjs';
import { fontNames, fontNamesFor } from '../packages/skyshot/shared.mjs';
import { examples, files } from '../tests/corpus.mjs';
import { profileExamples } from '../tests/profile-cases.mjs';

const fixture = JSON.parse(await readFile(new URL('../tests/fixtures/post.json', import.meta.url)));
const avatar = await readFile(new URL('../tests/fixtures/avatar.jpg', import.meta.url));
const wasm = await readFile(new URL('../packages/skyshot/generated/skyshot_bg.wasm', import.meta.url));
const compressed = gzipSync(wasm, { level: 9 }).length;
assert.ok(compressed < 3_000_000, `WASM exceeds 3 MB gzip: ${compressed}`);
const start = performance.now();
const lazy = process.argv.includes('--lazy');
const renderer = await loadSkyshot({ lazy });
const loadMs = performance.now() - start;
const fonts = lazy ? new Set() : new Set(fontNames);
const rows = [];
const links = 'Hello Bluesky! Read https://bsky.app and https://example.com for more.';
for (const [name, text, withAvatar] of [
  ['links', links],
  ['links with avatar', links, true],
  ['multilingual', fixture.record.text],
]) {
  const url = withAvatar ? fixture.author.avatar : undefined;
  const post = { ...fixture, author: { ...fixture.author, avatar: url }, record: { ...fixture.record, text, facets: [] } };
  const input = { post, assets: new Map(url ? [[url, avatar]] : []) };
  for (const width of [390, 600]) {
    const options = { width, scale: 2, timeZone: 'UTC', hour12: true };
    const prepareStart = performance.now();
    await renderer.prepare(input);
    const prepareMs = performance.now() - prepareStart;
    if (lazy) for (const name of fontNamesFor(post)) fonts.add(name);
    const times = [];
    let firstMs;
    let png;
    for (let i = 0; i < 68; i++) {
      const before = performance.now();
      const result = renderer.render(input, options);
      const elapsed = performance.now() - before;
      firstMs ??= elapsed;
      if (i >= 8) times.push(elapsed);
      assert.deepEqual(result.warnings, []);
      png = result.png;
    }
    times.sort((a, b) => a - b);
    rows.push({ name, width, prepareMs, firstMs, medianMs: times[30], p95Ms: times[57], pngBytes: png.length });
  }
}
const media = { post: Object.fromEntries(examples).two, assets: files };
await renderer.prepare(media);
if (lazy) for (const name of fontNamesFor(media)) fonts.add(name);
const batchStart = performance.now();
const layouts = renderer.renderMany(media, [390, 600].map(width => ({ width, scale: 2, timeZone: 'UTC', hour12: true })));
const mediaBatch = { totalMs: performance.now() - batchStart, layouts: layouts.map(({ png, width, height, renderMs, warnings }) => {
  assert.deepEqual(warnings, []);
  return { width, height, renderMs, pngBytes: png.length };
}) };
const profileRows = [];
for (const [name, profile] of profileExamples.slice(0, 3)) {
  const input = { ...profile, assets: files };
  const prepareStarted = performance.now();
  await renderer.prepare(input);
  if (lazy) for (const name of fontNamesFor(input)) fonts.add(name);
  const prepareMs = performance.now() - prepareStarted;
  for (const width of [390, 600]) {
    const samples = [];
    let firstMs;
    let pngBytes;
    for (let i = 0; i < 68; i++) {
      const start = performance.now();
      const result = renderer.renderProfile(input, { width, scale: 2 });
      const elapsed = performance.now() - start;
      assert.deepEqual(result.warnings, []);
      firstMs ??= elapsed;
      pngBytes = result.png.length;
      if (i >= 8) samples.push(elapsed);
    }
    samples.sort((a, b) => a - b);
    profileRows.push({ name, width, prepareMs, firstMs, medianMs: samples[30], p95Ms: samples[57], pngBytes });
  }
}
const fontBytes = (await Promise.all([...fonts].map(name => readFile(new URL(`../packages/skyshot/fonts/${name}`, import.meta.url))))).reduce((n, bytes) => n + bytes.length, 0);
console.log(JSON.stringify({ node: process.version, platform: `${process.platform}/${process.arch}`, lazy, wasmBytes: wasm.length, gzipBytes: compressed, loadMs, fontBytes, samples: 60, warmups: 8, rows, mediaBatch, profileRows }, null, 2));
