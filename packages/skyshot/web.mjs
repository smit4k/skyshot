import init from './generated/skyshot.js';
import { createMediaFetch } from './media-fetch.mjs';
import { bind, cacheFonts, fontNames } from './shared.mjs';

let ready;
const loadFont = cacheFonts(async name => {
  const response = await globalThis.fetch(new URL(`./fonts/${name}`, import.meta.url));
  if (!response.ok) throw new Error(`Could not load ${name} (HTTP ${response.status})`);
  return new Uint8Array(await response.arrayBuffer());
});

export async function loadSkyshot({ fonts, lazy = false, fetch = globalThis.fetch } = {}) {
  const deferred = lazy && fonts === undefined;
  const [, data] = await Promise.all([ready ??= init(), fonts ?? (deferred ? [] : Promise.all(fontNames.map(loadFont)))]);
  return bind(data, createMediaFetch(fetch), deferred ? loadFont : undefined);
}
