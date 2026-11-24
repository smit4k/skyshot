import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { mkdir, mkdtemp, readFile, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { test } from 'node:test';

import { loadSkyshot } from '../packages/skyshot/node.mjs';
import { bind } from '../packages/skyshot/shared.mjs';
import { createMediaFetch } from '../packages/skyshot/media-fetch.mjs';

import { demo, tile, files, thread, examples, external, optionsFor } from './corpus.mjs';

test('native and WASM produce identical PNGs across the frozen post corpus', async () => {
  const renderer = await loadSkyshot();
  if (process.env.SKYSHOT_PARITY_DIR) await mkdir(process.env.SKYSHOT_PARITY_DIR, { recursive: true });
  const scratch = await mkdtemp(join(tmpdir(), 'skyshot-'));
  const out = join(scratch, 'native.png');
  const manifest = new URL('./fixtures/assets.json', import.meta.url).pathname;
  try {
    for (const [i, [variant, post]] of examples.entries()) {
      const options = optionsFor(i);
      const rendered = renderer.render({ post, assets: files }, options);
      assert.deepEqual(rendered.warnings, [], variant);
      await writeFile(join(scratch, 'post.json'), JSON.stringify(post));
      execFileSync(process.env.SKYSHOT_CLI ?? './target/release/skyshot', [
        'render', join(scratch, 'post.json'), '--assets', manifest,
        '--width', options.width, '--scale', options.scale, '--theme', options.theme,
        '--ancestors', options.ancestors, '--time-zone', options.timeZone, '-o', out,
      ]);
      assert.deepEqual(Buffer.from(rendered.png), await readFile(out), variant);
      if (process.env.SKYSHOT_PARITY_DIR) await writeFile(join(process.env.SKYSHOT_PARITY_DIR, `${variant}.png`), rendered.png);
      assert.deepEqual(rendered.png, renderer.render({ post, assets: files }, options).png, variant);
    }
  } finally {
    await rm(scratch, { recursive: true, force: true });
  }
});

test('a reused renderer updates the avatar if its source bytes change', async () => {
  const renderer = await loadSkyshot({ fonts: [] });
  const url = demo.author.avatar;
  const assets = new Map([[url, files.get(url)]]);
  const original = renderer.render({ post: demo, assets });
  assets.set(url, tile);
  const changed = renderer.render({ post: demo, assets });
  assert.notDeepEqual(changed.png, original.png);
  assert.deepEqual(changed.warnings, []);
  assets.set(url, new Uint8Array([0, 1, 2]));
  assert.deepEqual(renderer.render({ post: demo, assets }).warnings, [{ code: 'invalidAsset', detail: url }]);
});

test('a timed batch matches independent renders across sizes and ancestor counts', async () => {
  const renderer = await loadSkyshot({ fonts: [] });
  const post = Object.fromEntries(examples).one;
  const parentAvatar = 'https://cdn.bsky.app/img/avatar/plain/parent';
  const parent = { ...demo, author: { ...demo.author, avatar: parentAvatar } };
  const assets = new Map([...files, [parentAvatar, tile]]);
  const input = { post: { thread: { ...thread(post), parent: thread(parent) } }, assets };
  const options = [{ width: 390, scale: 2 }, { width: 600, scale: 3, ancestors: 1, theme: 'dark' }];
  const originalNow = performance.now.bind(performance);
  const events = [];
  const assetMap = new Map(assets);
  assetMap.get = url => { events.push('asset'); return Map.prototype.get.call(assetMap, url); };
  Object.defineProperty(performance, 'now', { configurable: true, value: () => {
    events.push('clock');
    return originalNow();
  } });
  let batch;
  try {
    batch = renderer.renderMany({ ...input, assets: assetMap }, options);
  } finally {
    delete performance.now;
  }
  assert.deepEqual(events, ['asset', 'asset', 'asset', 'clock', 'clock', 'clock', 'clock']);
  const standalone = await loadSkyshot({ fonts: [] });
  for (const [i, { renderMs, ...result }] of batch.entries()) {
    assert.ok(Number.isFinite(renderMs) && renderMs >= 0);
    assert.deepEqual(result.warnings, []);
    assert.deepEqual(result, standalone.render(input, options[i]));
  }
  assert.deepEqual(renderer.renderMany(input, []), []);
  assert.throws(() => renderer.renderMany(input, [{ width: 390 }, { width: 120 }]), /width must/);
  assert.throws(() => renderer.renderMany(input, Array(9).fill({})), /at most eight/);
});

test('resolver leaves restricted assets and arbitrary URLs untouched', async () => {
  const seen = [];
  const renderer = await loadSkyshot({ fonts: [], fetch: async (url, options) => {
    assert.equal(options.redirect, 'error');
    seen.push(new URL(url));
    return Response.json({ thread: thread({
      ...demo, labels: [{val: 'porn'}], embed: external('https://example.com'),
    }) });
  }});
  const result = await renderer.resolve('https://bsky.app/profile/skyshot.example/post/3lsample');
  assert.equal(seen.length, 1);
  assert.equal(seen[0].hostname, 'public.api.bsky.app');
  assert.equal(seen[0].pathname, '/xrpc/app.bsky.feed.getPostThread');
  assert.equal(seen[0].searchParams.get('uri'), 'at://skyshot.example/app.bsky.feed.post/3lsample');
  assert.equal(result.assets.size, 0);
  assert.deepEqual(renderer.render(result).warnings, []);
});

test('browser downloads media four at a time with one verified repository lookup', { timeout: 2000 }, async () => {
  await loadSkyshot({ fonts: [] });
  const did = 'did:plc:z72i7hdynmk6r22z27h6tvur';
  const cid = 'bafkreihwihm6kpd6zuwhhlro75p5qks5qtrcu55jp3gddbfjsieiv7wuka';
  const url = `https://cdn.bsky.app/img/avatar/plain/${did}/${cid}@jpeg`;
  const thumbs = ['a', 'b', 'c', 'd'].map(suffix => `https://cdn.bsky.app/img/feed_thumbnail/plain/${did}/${cid}${suffix}@jpeg`);
  const post = {
    ...demo, author: { ...demo.author, did, avatar: url },
    embed: { $type: 'app.bsky.embed.images#view', images: thumbs.map(thumb => ({ thumb })) },
  };
  const seen = [];
  const blobs = [];
  let firstBatchStarted;
  const firstBatch = new Promise(resolve => { firstBatchStarted = resolve; });
  let releaseMedia;
  const mediaReady = new Promise(resolve => { releaseMedia = resolve; });
  const fetch = async (raw, options) => {
    assert.equal(options.credentials, 'omit');
    assert.equal(options.redirect, 'error');
    const request = new URL(raw);
    seen.push(request.hostname);
    if (request.hostname === 'public.api.bsky.app') return Response.json({ thread: thread(post) });
    if (request.hostname === 'plc.directory') return Response.json({
      id: did,
      service: [
        { id: '#atproto_pds', type: 'AtprotoPersonalDataServer', serviceEndpoint: 'https://puffball.us-east.host.bsky.network' },
      ],
    });
    assert.equal(request.hostname, 'puffball.us-east.host.bsky.network');
    assert.equal(request.pathname, '/xrpc/com.atproto.sync.getBlob');
    assert.equal(request.searchParams.get('did'), did);
    blobs.push(request.searchParams.get('cid'));
    if (blobs.length === 4) firstBatchStarted();
    await mediaReady;
    if (request.searchParams.get('cid') === `${cid}b`) {
      return new Response(tile, { headers: { 'content-length': String(8 * 1024 * 1024 + 1) } });
    }
    return new Response(tile);
  };
  const renderer = bind([], createMediaFetch(fetch));
  const pending = renderer.resolve(`at://${did}/app.bsky.feed.post/3lsample`);
  await firstBatch;
  assert.deepEqual(blobs, [cid, `${cid}a`, `${cid}b`, `${cid}c`]);
  releaseMedia();
  const resolved = await pending;
  assert.deepEqual(seen, ['public.api.bsky.app', 'plc.directory', ...Array(5).fill('puffball.us-east.host.bsky.network')]);
  assert.deepEqual([...resolved.assets.keys()], [url, thumbs[0], thumbs[2], thumbs[3]]);
  assert.deepEqual(Buffer.from(resolved.assets.get(url)), tile);
  assert.deepEqual(renderer.render(resolved).warnings, [{ code: 'missingAsset', detail: thumbs[1] }]);
});
