import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { mkdtemp, readFile, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { test } from 'node:test';

import { loadSkyshot } from '../packages/skyshot/node.mjs';
import { bind } from '../packages/skyshot/shared.mjs';
import { createMediaFetch } from '../packages/skyshot/media-fetch.mjs';

const fixture = name => readFile(new URL(`./fixtures/${name}`, import.meta.url));
const demo = JSON.parse(await fixture('post.json'));
const tile = await fixture('tiles.png');
const files = new Map(Object.keys(JSON.parse(await fixture('assets.json'))).map(url => [url, tile]));
const [avatar, photo, video] = files.keys();

const typed = (type, fields = {}) => ({ $type: type, ...fields });
const embed = (type, fields) => typed(`app.bsky.embed.${type}#view`, fields);
const notice = type => embed('record', {record: typed(`app.bsky.embed.record#${type}`)});
const landscape = { width: 48, height: 32 };
const portrait = { width: 9, height: 16 };
const images = (count, ratio = landscape) => embed('images', {images: Array.from({length: count}, () => ({
  thumb: photo, fullsize: photo, alt: 'Test pattern', aspectRatio: ratio,
}))});
const gallery = embed('gallery', {items: Array.from({length: 20}, () => typed('app.bsky.embed.gallery#viewImage', {
  thumbnail: photo, fullsize: photo, alt: '', aspectRatio: landscape,
}))});
const external = uri => embed('external', { external: {uri, title:'Example', description:'A sample caption', thumb: photo} });
const quote = embed('record', {record: typed('app.bsky.embed.record#viewRecord', {
  author: demo.author, value: demo.record, embeds: [images(1, portrait)],
})});
const thread = post => typed('app.bsky.feed.defs#threadViewPost', { post });
const examples = Object.entries({
  text: demo,
  emoji: { ...demo, record: {...demo.record, text: '👋🏽 👩‍💻 🏳️‍🌈', facets: []}},
  one: { ...demo, embed: images(1, portrait) },
  two: { ...demo, embed: images(2) },
  three: { ...demo, embed: images(3) },
  four: { ...demo, embed: images(4) },
  gallery: {...demo, embed: gallery},
  card: {...demo, embed: external('https://example.com/full-article')},
  player: {...demo, embed: external('https://www.youtube.com/watch?v=example')},
  gif: {...demo, embed: external('https://tenor.com/view/test')},
  video: {...demo, embed: embed('video', {thumbnail: video, aspectRatio: landscape})},
  'video-gif': {...demo, embed: embed('video', {thumbnail: video, aspectRatio: portrait, presentation:'gif'})},
  quote: {...demo, embed: quote},
  'quote-with-media': {...demo, embed: embed('recordWithMedia', {record: quote, media: images(2)})},
  'quote-deleted': {...demo, embed: notice('viewNotFound')},
  'quote-blocked': {...demo, embed: notice('viewBlocked')},
  'quote-detached': {...demo, embed: notice('viewDetached')},
  feed: {...demo, embed: embed('record', {record: typed('app.bsky.feed.defs#generatorView', {displayName: 'A feed', creator: demo.author, avatar})})},
  list: {...demo, embed: embed('record', {record: typed('app.bsky.graph.defs#listView', {name: 'A list', creator: demo.author, avatar})})},
  'starter-pack': {...demo, embed: embed('record', {record: typed('app.bsky.graph.defs#starterPackViewBasic', {record: {name:'Starter pack'}, creator: demo.author})})},
  hidden: {...demo, labels: [{val: 'porn'}], embed: images(1)},
  warned: {...demo, labels: [{val: 'sensitive'}], embed: images(1)},
  unavailable: {thread: typed('app.bsky.feed.defs#blockedPost')},
  ancestors: { thread: {...thread(demo), parent: {...thread(demo), parent: typed('app.bsky.feed.defs#notFoundPost') }} },
});

test('native and WASM produce identical PNGs across the frozen post corpus', async () => {
  const renderer = await loadSkyshot();
  const scratch = await mkdtemp(join(tmpdir(), 'skyshot-'));
  const out = join(scratch, 'native.png');
  const manifest = new URL('./fixtures/assets.json', import.meta.url).pathname;
  try {
    for (const [i, [variant, post]] of examples.entries()) {
      const options = {
        width: i % 2 ? 390 : 600, scale: i % 3 + 1, theme: ['light', 'dim', 'dark'][i % 3], ancestors: 2,
        timeZone: i % 2 ? 'America/Detroit' : 'UTC', hour12: true,
      };
      const rendered = renderer.render({ post, assets: files }, options);
      assert.deepEqual(rendered.warnings, [], variant);
      await writeFile(join(scratch, 'post.json'), JSON.stringify(post));
      execFileSync(process.env.SKYSHOT_CLI ?? './target/release/skyshot', [
        'render', join(scratch, 'post.json'), '--assets', manifest,
        '--width', options.width, '--scale', options.scale, '--theme', options.theme,
        '--ancestors', options.ancestors, '--time-zone', options.timeZone, '-o', out,
      ]);
      assert.deepEqual(Buffer.from(rendered.png), await readFile(out), variant);
      assert.deepEqual(rendered.png, renderer.render({ post, assets: files }, options).png, variant);
    }
  } finally {
    await rm(scratch, { recursive: true, force: true });
  }
});

test('resolver leaves restricted assets and arbitrary URLs untouched', async () => {
  const seen = [];
  const renderer = await loadSkyshot({ fonts: [], fetch: async (url, options) => {
    assert.equal(options.redirect, 'error');
    seen.push(url);
    if (url.includes('resolveHandle')) return Response.json({ did: 'did:plc:example' });
    return Response.json({ thread: thread({
      ...demo, labels: [{val: 'porn'}], embed: external('https://example.com'),
    }) });
  }});
  const result = await renderer.resolve('https://bsky.app/profile/skyshot.example/post/3lsample');
  assert.equal(seen.length, 2);
  assert.ok(seen.every(url => new URL(url).hostname === 'public.api.bsky.app'));
  assert.equal(result.assets.size, 0);
  assert.deepEqual(renderer.render(result).warnings, []);
});

test('browser resolves a Bluesky-hosted avatar through its public repository', async () => {
  await loadSkyshot({ fonts: [] });
  const did = 'did:plc:z72i7hdynmk6r22z27h6tvur';
  const cid = 'bafkreihwihm6kpd6zuwhhlro75p5qks5qtrcu55jp3gddbfjsieiv7wuka';
  const url = `https://cdn.bsky.app/img/avatar/plain/${did}/${cid}@jpeg`;
  const post = { ...demo, author: { ...demo.author, did, avatar: url } };
  const seen = [];
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
    assert.equal(request.searchParams.get('cid'), cid);
    return new Response(tile);
  };
  const renderer = bind([], createMediaFetch(fetch));
  const resolved = await renderer.resolve(`at://${did}/app.bsky.feed.post/3lsample`);
  assert.deepEqual(seen, ['public.api.bsky.app', 'plc.directory', 'puffball.us-east.host.bsky.network']);
  assert.deepEqual(Buffer.from(resolved.assets.get(url)), tile);
  assert.deepEqual(renderer.render(resolved).warnings, []);
});
