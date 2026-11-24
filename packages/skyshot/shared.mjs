import * as wasm from './generated/skyshot.js';

const MAX_JSON = 2 * 1024 * 1024;
const MAX_IMAGE = 8 * 1024 * 1024;
const MAX_ASSETS = 32 * 1024 * 1024;

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

async function json(fetch, url, allowNotFound = false) {
  const { status, bytes } = await request(fetch, url, MAX_JSON);
  if (status >= 200 && status < 300) {
    return JSON.parse(new TextDecoder('utf-8', { fatal: true }).decode(bytes));
  }
  if (allowNotFound && (status === 400 || status === 404)) {
    const data = JSON.parse(new TextDecoder().decode(bytes));
    if (data.error === 'NotFound') {
      return { thread: { $type: 'app.bsky.feed.defs#notFoundPost' } };
    }
  }
  throw new Error(`Bluesky AppView returned HTTP ${status}`);
}

export function bind(fonts, fetch) {
  const engine = new wasm.Renderer(fonts);
  const localTimeZone = Intl.DateTimeFormat().resolvedOptions().timeZone;
  const hour12 = Intl.DateTimeFormat(undefined, { hour: 'numeric' }).resolvedOptions().hour12;
  return {
    async resolve(input, options = {}) {
      const handle = wasm.handle_url(input);
      const did = handle ? (await json(fetch, handle)).did : undefined;
      const ancestors = options.ancestors ?? 0;
      if (!Number.isInteger(ancestors) || ancestors < 0 || ancestors > 3) {
        throw new RangeError('ancestors must be an integer from 0 to 3');
      }
      const post = await json(fetch, wasm.thread_url(input, did, ancestors), true);
      const assets = new Map();
      let left = MAX_ASSETS;
      for (const raw of wasm.media_urls(JSON.stringify(post), JSON.stringify(options))) {
        const url = wasm.media_url(raw);
        if (!url || left === 0) continue;
        try {
          const { status, bytes } = await request(fetch, url, Math.min(MAX_IMAGE, left));
          if (status < 200 || status >= 300) continue;
          assets.set(raw, bytes);
          left -= bytes.length;
        } catch {
          // A failed optional asset is reported when its placeholder is rendered.
        }
      }
      return { post, assets };
    },
    render(input, options = {}) {
      const resolved = input.assets instanceof Map && Object.hasOwn(input, 'post');
      const data = resolved ? input.post : input;
      const assets = resolved ? input.assets : new Map();
      const config = { timeZone: localTimeZone, hour12, ...options };
      const result = engine.render(typeof data === 'string' ? data : JSON.stringify(data), JSON.stringify(config), assets);
      try {
        return { png: result.png, width: result.width, height: result.height, warnings: result.warnings };
      } finally {
        result.free();
      }
    },
  };
}
