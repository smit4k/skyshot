import { readFile } from 'node:fs/promises';
import init from './generated/skyshot.js';
import { bind, cacheFonts, fontNames } from './shared.mjs';

let ready;
const loadFont = cacheFonts(name => readFile(new URL(`./fonts/${name}`, import.meta.url)));

export async function loadSkyshot({ fonts, lazy = false, fetch = globalThis.fetch } = {}) {
  ready ??= readFile(new URL('./generated/skyshot_bg.wasm', import.meta.url))
    .then(module_or_path => init({ module_or_path }));
  const deferred = lazy && fonts === undefined;
  const [, data] = await Promise.all([ready, fonts ?? (deferred ? [] : Promise.all(fontNames.map(loadFont)))]);
  return bind(data, fetch, deferred ? loadFont : undefined);
}
