import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { test } from 'node:test';
import { bind, cacheFonts, fontNamesFor } from '../packages/skyshot/shared.mjs';
import { loadSkyshot } from '../packages/skyshot/node.mjs';
import { Renderer } from '../packages/skyshot/generated/skyshot.js';

const demo = JSON.parse(await readFile(new URL('./fixtures/post.json', import.meta.url)));

test('ASCII posts need no fallback packs, including JSON with $type keys', () => {
  assert.deepEqual(fontNamesFor({ record: { $type: 'app.bsky.feed.post', text: 'Read https://bsky.app + $10' } }), []);
});

test('script and emoji selection covers supplementary characters and escaped JSON', () => {
  for (const [text, name] of [
    ['مرحبا', 'Arabic'], ['שלום', 'Hebrew'], ['नमस्ते', 'Devanagari'], ['ไทย', 'Thai'],
    ['안녕', 'KR'], ['こんにちは', 'JP'], ['你好 𠀀', 'SC'], ['∑ ∞', 'Symbols2'],
  ]) assert.ok(fontNamesFor({ text }).includes(`NotoSans${name}.ttf`), text);
  for (const text of ['👩‍💻', '🇺🇸', '1️⃣']) assert.ok(fontNamesFor({ text }).includes('NotoColorEmoji.ttf'), text);
  assert.ok(fontNamesFor('{"text":"\\u0627"}').includes('NotoSansArabic.ttf'));
});

test('font byte cache shares concurrent reads and retries failed reads', async () => {
  let reads = 0;
  const load = cacheFonts(async () => {
    if (++reads === 1) throw new Error('offline');
    return new Uint8Array([1]);
  });
  await assert.rejects(load('font'), /offline/);
  const [a, b] = await Promise.all([load('font'), load('font')]);
  assert.equal(a, b);
  assert.equal(reads, 2);
});

test('incremental font loading validates types, invalid bytes and the cumulative byte budget', async () => {
  await loadSkyshot({ fonts: [] });
  const renderer = new Renderer([]);
  try {
    assert.throws(() => renderer.add_font({}), /Uint8Array/);
    assert.throws(() => renderer.add_font(new Uint8Array([1, 2, 3])), /font data/);
    const font = await readFile(new URL('../packages/skyshot/fonts/NotoSansArabic.ttf', import.meta.url));
    renderer.add_font(font);
    assert.throws(() => renderer.add_font(new Uint8Array(80 * 1024 * 1024 - font.length + 1)), /font byte limit/);
  } finally { renderer.free(); }
});

test('lazy preparation loads in parallel once and matches eager PNGs as scripts are added', async () => {
  await loadSkyshot({ fonts: [] });
  const eager = await loadSkyshot();
  const reads = [];
  const renderer = bind([], undefined, async name => {
    reads.push(name);
    return readFile(new URL(`../packages/skyshot/fonts/${name}`, import.meta.url));
  });
  for (const text of ['Plain text with https://bsky.app', demo.record.text, 'שלום مرحبا', '日本語 こんにちは 한국어', 'नमस्ते ไทย ∑ ∞', '👩‍💻 🇺🇸 1️⃣']) {
    const post = { ...demo, author: { ...demo.author, avatar: undefined }, record: { ...demo.record, text, facets: [] } };
    if (fontNamesFor(post).some(name => !reads.includes(name))) assert.throws(() => renderer.render(post), /prepare/);
    const before = reads.length;
    const a = renderer.prepare(post);
    const b = renderer.prepare(post);
    assert.ok(reads.length >= before);
    assert.equal(await a, post);
    await b;
    assert.deepEqual(renderer.render(post).png, eager.render(post).png, text);
    assert.equal(new Set(reads).size, reads.length, 'each font is installed only once');
  }
});

test('lazy preparation retries a failed font without losing the renderer', async () => {
  await loadSkyshot({ fonts: [] });
  let offline = true;
  const renderer = bind([], undefined, async name => {
    if (offline) throw new Error('offline');
    return readFile(new URL(`../packages/skyshot/fonts/${name}`, import.meta.url));
  });
  await assert.rejects(renderer.prepare(demo), /offline/);
  offline = false;
  await renderer.prepare(demo);
  assert.ok(renderer.render(demo).png.length > 0);
});
