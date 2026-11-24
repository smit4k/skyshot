import init from './generated/skyshot.js';
import { createMediaFetch } from './media-fetch.mjs';
import { bind, fontNames } from './shared.mjs';

let ready;
let defaultFonts;

export async function loadSkyshot({ fonts, fetch = globalThis.fetch } = {}) {
  await (ready ??= init());
  fonts ??= await (defaultFonts ??= Promise.all(fontNames.map(async name => {
    const response = await globalThis.fetch(new URL(`./fonts/${name}`, import.meta.url));
    if (!response.ok) throw new Error(`Could not load ${name} (HTTP ${response.status})`);
    return new Uint8Array(await response.arrayBuffer());
  })));
  return bind(fonts, createMediaFetch(fetch));
}
