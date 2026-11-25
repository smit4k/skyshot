import * as wasm from './generated/skyshot.js';

const MAX_JSON = 2 * 1024 * 1024;
const MAX_IMAGE = 8 * 1024 * 1024;
const MAX_ASSETS = 32 * 1024 * 1024;
const json = value => typeof value === 'string' ? value : JSON.stringify(value);

export const fontNames = [
  'NotoColorEmoji.ttf',
  'NotoSans.ttf',
  'NotoSansArabic.ttf',
  'NotoSansDevanagari.ttf',
  'NotoSansHebrew.ttf',
  'NotoSansJP.ttf',
  'NotoSansKR.ttf',
  'NotoSansSC.ttf',
  'NotoSansSymbols2.ttf',
  'NotoSansThai.ttf',
];

const scriptFonts = [
  [/\p{Script=Arabic}/u, 'NotoSansArabic.ttf'],
  [/\p{Script=Hebrew}/u, 'NotoSansHebrew.ttf'],
  [/\p{Script=Devanagari}/u, 'NotoSansDevanagari.ttf'],
  [/\p{Script=Thai}/u, 'NotoSansThai.ttf'],
  [/\p{Script=Hangul}/u, 'NotoSansKR.ttf'],
  [/\p{Script=Hiragana}|\p{Script=Katakana}/u, 'NotoSansJP.ttf'],
  [/\p{Script=Han}/u, 'NotoSansSC.ttf'],
  [/\p{Extended_Pictographic}|\p{Regional_Indicator}|\u20e3/u, 'NotoColorEmoji.ttf'],
  [/[^\u0000-\u024f]/u, 'NotoSans.ttf'],
  [/(?![\u0000-\u024f])\p{Symbol}/u, 'NotoSansSymbols2.ttf'],
];

export function fontNamesFor(input) {
  // shortcut: metadata can request extra fonts; inspect visible text when subsetting packs.
  let data = input;
  if (input?.assets instanceof Map) {
    data = Object.hasOwn(input, 'post') ? input.post : {
      profile: input.profile, context: input.context, labeler: input.labeler,
    };
  }
  const text = JSON.stringify(typeof data === 'string' ? JSON.parse(data) : data);
  return scriptFonts.filter(([pattern]) => pattern.test(text)).map(([, name]) => name);
}

export function cacheFonts(load) {
  const pending = new Map();
  return name => {
    if (!pending.has(name)) pending.set(name, load(name).catch(error => {
      pending.delete(name);
      throw error;
    }));
    return pending.get(name);
  };
}

async function request(fetch, url, limit) {
  const response = await fetch(url, {
    redirect: 'error',
    credentials: 'omit',
    signal: AbortSignal.timeout(8000),
  });
  const reader = response.body?.getReader();
  if (!reader || Number(response.headers.get('content-length')) > limit) {
    await reader?.cancel().catch(() => {});
    throw new Error('Missing or oversized response body');
  }
  const chunks = [];
  let size = 0;
  for (;;) {
    const { done, value } = await reader.read();
    if (done) break;
    size += value.length;
    if (size > limit) {
      await reader.cancel().catch(() => {});
      throw new Error('Oversized response body');
    }
    chunks.push(value);
  }
  const bytes = new Uint8Array(size);
  let offset = 0;
  for (const chunk of chunks) {
    bytes.set(chunk, offset);
    offset += chunk.length;
  }
  return { status: response.status, bytes };
}

async function getThread(fetch, url) {
  const { status, bytes } = await request(fetch, url, MAX_JSON);
  if (status >= 200 && status < 300) {
    return JSON.parse(new TextDecoder('utf-8', { fatal: true }).decode(bytes));
  }
  if (status === 400 || status === 404) {
    const data = JSON.parse(new TextDecoder().decode(bytes));
    if (data.error === 'NotFound') {
      return { thread: { $type: 'app.bsky.feed.defs#notFoundPost' } };
    }
  }
  throw new Error(`Bluesky AppView returned HTTP ${status}`);
}

async function getProfileData(fetch, url) {
  const { status, bytes } = await request(fetch, url, MAX_JSON);
  if (status < 200 || status >= 300) throw new Error(`Bluesky AppView returned HTTP ${status}`);
  return JSON.parse(new TextDecoder('utf-8', { fatal: true }).decode(bytes));
}

function mediaRequests(urls) {
  return urls.flatMap(raw => {
    const url = wasm.media_url(raw);
    return url ? [[raw, url]] : [];
  });
}

async function fetchAssets(fetch, urls, assets) {
  let left = MAX_ASSETS;
  for (const bytes of assets.values()) left -= bytes.length;
  for (let next = 0; next < urls.length && left > 0;) {
    // Reserve a full per-image allowance per active request to stay within the post budget.
    const count = Math.max(1, Math.min(4, Math.floor(left / MAX_IMAGE)));
    const limit = Math.min(MAX_IMAGE, left);
    const batch = urls.slice(next, next + count);
    const loaded = await Promise.all(batch.map(async ([raw, url]) => {
      try {
        const { status, bytes } = await request(fetch, url, limit);
        if (status >= 200 && status < 300) return [raw, bytes];
      } catch {
        // A failed optional asset is reported when its placeholder is rendered.
      }
    }));
    for (const asset of loaded) {
      if (!asset) continue;
      const [raw, bytes] = asset;
      assets.set(raw, bytes);
      left -= bytes.length;
    }
    next += batch.length;
  }
}

export function bind(fonts, fetch, loadFont) {
  const engine = new wasm.Renderer(fonts);
  const loaded = new Set();
  const pending = new Map();
  const localTimeZone = Intl.DateTimeFormat().resolvedOptions().timeZone;
  const hour12 = Intl.DateTimeFormat(undefined, { hour: 'numeric' }).resolvedOptions().hour12;
  const config = options => ({ timeZone: localTimeZone, hour12, ...options });
  const unpack = result => ({ png: result.png, width: result.width, height: result.height, warnings: result.warnings });
  function requireFonts(input) {
    if (loadFont && fontNamesFor(input).some(name => !loaded.has(name))) {
      throw new Error('Load fonts with await skyshot.prepare(input) before synchronous rendering');
    }
  }
  function renderInput(input) {
    requireFonts(input);
    const resolved = input.assets instanceof Map && Object.hasOwn(input, 'post');
    const post = resolved ? input.post : input;
    return [typeof post === 'string' ? post : JSON.stringify(post), resolved ? input.assets : new Map()];
  }
  async function prepare(input) {
    if (loadFont) await Promise.all(fontNamesFor(input).map(name => {
      if (!pending.has(name)) pending.set(name, loadFont(name).then(bytes => {
        engine.add_font(bytes);
        loaded.add(name);
      }).catch(error => {
        pending.delete(name);
        throw error;
      }));
      return pending.get(name);
    }));
    return input;
  }
  return {
    prepare,
    async resolve(input, options = {}) {
      const ancestors = options.ancestors ?? 0;
      if (!Number.isInteger(ancestors) || ancestors < 0 || ancestors > 3) {
        throw new RangeError('ancestors must be an integer from 0 to 3');
      }
      const post = await getThread(fetch, wasm.thread_url(input, undefined, ancestors));
      const urls = mediaRequests(wasm.media_urls(JSON.stringify(post), JSON.stringify(options)));
      const resolved = { post, assets: new Map() };
      await Promise.all([fetchAssets(fetch, urls, resolved.assets), prepare(resolved)]);
      return resolved;
    },
    async resolveProfile(input, options = {}) {
      const snapshotAt = new Date().toISOString();
      const profile = await getProfileData(fetch, wasm.profile_request(input));
      const resolved = { profile, context: { snapshotAt }, assets: new Map() };
      const raw = json({ profile, context: resolved.context });
      const profiles = [];
      const config = json(options);
      const primary = mediaRequests(wasm.profile_primary_assets(raw, config));
      const primaryRequests = new Set(primary.map(([url]) => url));
      const dependencies = Promise.all(wasm.profile_dependencies(raw).map(async url => {
        if (new URL(url).pathname.endsWith('.getServices')) {
          const data = await getProfileData(fetch, url);
          if (!data.views?.[0]) throw new Error('Bluesky returned no labeler details');
          resolved.labeler = data.views[0];
        } else {
          try {
            const data = await getProfileData(fetch, url);
            if (Array.isArray(data.profiles)) profiles.push(...data.profiles.slice(0, 25));
          } catch {
            // An unavailable mention remains text.
          }
        }
      }));
      await Promise.all([
        dependencies,
        fetchAssets(fetch, primary, resolved.assets),
        prepare(resolved),
      ]);
      const facets = JSON.parse(wasm.profile_bio_facets(raw, json(profiles)));
      if (facets) resolved.context.descriptionFacets = facets;
      if (resolved.labeler) await prepare(resolved);
      const { assets, ...envelope } = resolved;
      const urls = mediaRequests(engine.profile_media_urls(json(envelope), config).filter(url => !primaryRequests.has(url)));
      await fetchAssets(fetch, urls, assets);
      return resolved;
    },
    renderProfile(input, options = {}) {
      requireFonts(input);
      const { assets, ...envelope } = typeof input === 'string' ? {} : input;
      const result = engine.render_profile(json(assets instanceof Map ? envelope : input), json(options), assets instanceof Map ? assets : new Map());
      try {
        return unpack(result);
      } finally {
        result.free();
      }
    },
    render(input, options = {}) {
      const [post, assets] = renderInput(input);
      const result = engine.render(post, JSON.stringify(config(options)), assets);
      try {
        return unpack(result);
      } finally {
        result.free();
      }
    },
    renderMany(input, options) {
      const [post, assets] = renderInput(input);
      const results = engine.render_many(post, JSON.stringify(options.map(config)), assets);
      try {
        return results.map(result => ({ ...unpack(result), renderMs: result.render_ms }));
      } finally {
        for (const result of results) result.free();
      }
    },
  };
}
