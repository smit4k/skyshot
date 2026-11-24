import { readFile } from 'node:fs/promises';
import init from './generated/skyshot.js';
import { bind, fontNames } from './shared.mjs';

let ready;
let defaultFonts;

export async function loadSkyshot({ fonts, fetch = globalThis.fetch } = {}) {
  ready ??= readFile(new URL('./generated/skyshot_bg.wasm', import.meta.url))
    .then(module_or_path => init({ module_or_path }));
  await ready;
  fonts ??= await (defaultFonts ??= Promise.all(fontNames.map(
    name => readFile(new URL(`./fonts/${name}`, import.meta.url)),
  )));
  return bind(fonts, fetch);
}
