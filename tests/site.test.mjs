import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { readFile } from 'node:fs/promises';
import { test } from 'node:test';
import { fontNames } from '../packages/skyshot/shared.mjs';
import { createMediaFetch } from '../packages/skyshot/media-fetch.mjs';

const root = new URL('../', import.meta.url);

test('static build includes the page and the complete browser renderer', async () => {
  execFileSync(process.execPath, ['scripts/build-site.mjs'], { cwd: root });
  const html = await readFile(new URL('dist/index.html', root), 'utf8');
  assert.match(html, /<h1>Skyshot<\/h1>/);
  assert.match(html, /<form>/);
  assert.match(html, /<img id="preview"/);
  assert.deepEqual([...html.matchAll(/name="theme" value="(.*?)"/g)].map(match => match[1]), ['light', 'dim', 'dark']);
  for (const path of ['style.css', 'app.mjs', 'render-worker.mjs', 'skyshot/media-fetch.mjs', 'skyshot/web.mjs', 'skyshot/shared.mjs', 'skyshot/generated/skyshot.js', ...fontNames.map(name => `skyshot/fonts/${name}`)]) {
    assert.ok((await readFile(new URL(`dist/${path}`, root))).length > 0, path);
  }
  const wasm = await readFile(new URL('dist/skyshot/generated/skyshot_bg.wasm', root));
  assert.deepEqual([...wasm.subarray(0, 4)], [0, 97, 115, 109]);
});

test('browser media uses a verified first-party PDS and retains fetch safeguards', async t => {
  const did = 'did:plc:aaaaaaaaaaaaaaaaaaaaaaaa';
  const endpoint = 'https://puffball.us-east.host.bsky.network';
  const options = { redirect: 'error', credentials: 'omit', signal: AbortSignal.timeout(8000) };
  const requests = [];
  const mediaFetch = createMediaFetch((url, opts) => fetch(url, opts));
  t.mock.method(globalThis, 'fetch', async (url, received) => {
    assert.equal(received, options);
    requests.push(url);
    return url.startsWith('https://plc.directory/')
      ? Response.json({ id: did, service: [{ id: '#atproto_pds', type: 'AtprotoPersonalDataServer', serviceEndpoint: endpoint }] })
      : new Response(new Uint8Array([1, 2, 3]));
  });
  const image = `https://cdn.bsky.app/img/avatar/plain/${did}/bafkreia@jpeg`;
  assert.deepEqual([...new Uint8Array(await (await mediaFetch(image, options)).arrayBuffer())], [1, 2, 3]);
  const target = new URL(requests[1]);
  assert.equal(target.origin, endpoint);
  assert.equal(target.pathname, '/xrpc/com.atproto.sync.getBlob');
  assert.equal(target.searchParams.get('did'), did);
  assert.equal(target.searchParams.get('cid'), 'bafkreia');
  await mediaFetch(image, options);
  assert.equal(requests.length, 3, 'the verified server is reused');
  const api = 'https://public.api.bsky.app/xrpc/app.bsky.feed.getPostThread';
  await mediaFetch(api, options);
  assert.equal(requests.at(-1), api, 'API requests are unchanged');
});

test('browser media rejects untrusted hosts, mismatched identities and oversized documents', async t => {
  const did = 'did:plc:bbbbbbbbbbbbbbbbbbbbbbbb';
  const image = `https://cdn.bsky.app/img/feed_thumbnail/plain/${did}/bafkreib`;
  let body;
  let requests = 0;
  const mediaFetch = createMediaFetch((url, opts) => fetch(url, opts));
  t.mock.method(globalThis, 'fetch', async url => {
    assert.equal(url, `https://plc.directory/${did}`);
    requests++;
    return new Response(body);
  });
  for (const endpoint of ['https://evil.example', 'https://puffball.us-east.host.bsky.network.evil.example', 'http://puffball.us-east.host.bsky.network', 'https://user@puffball.us-east.host.bsky.network']) {
    body = JSON.stringify({ id: did, service: [{ id: '#atproto_pds', type: 'AtprotoPersonalDataServer', serviceEndpoint: endpoint }] });
    await assert.rejects(mediaFetch(image), /Unsupported media host/);
  }
  body = JSON.stringify({ id: 'did:plc:cccccccccccccccccccccccc', service: [{ id: '#atproto_pds', type: 'AtprotoPersonalDataServer', serviceEndpoint: 'https://puffball.us-east.host.bsky.network' }] });
  await assert.rejects(mediaFetch(image), /Unsupported media host/);
  body = ' '.repeat(64 * 1024 + 1);
  await assert.rejects(mediaFetch(image), /Oversized identity document/);
  assert.equal(requests, 6);
});
