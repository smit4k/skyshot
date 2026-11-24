# Skyshot

Render Bluesky posts to PNG in Node or web workers using one WebAssembly renderer.

```js
import { loadSkyshot } from 'skyshot';
const skyshot = await loadSkyshot();
const options = { width: 390, theme: 'dim', scale: 2 };
const post = await skyshot.resolve('https://bsky.app/profile/bsky.app/post/3n2ozugncr22j', options);
const { png, width, height, warnings } = skyshot.render(post, options);
```

Pass raw AppView JSON or `{ post, assets }` to render offline. Assets are a `Map<string, Uint8Array>` keyed by the exact URLs from AppView. Default fallback fonts are shipped under the included OFL notices. Pass `{ fonts: [fontBytes] }` to load a custom set.

Reuse the renderer to retain its caches. Pass `{ lazy: true }` to load fallback fonts on demand. `resolve()` prepares them automatically; for raw JSON, call `await skyshot.prepare(post)` before synchronous `render()`. The default still loads all fonts. An explicit `fonts` array disables automatic font loading.

Use `skyshot.renderMany(post, [{ width: 390 }, { width: 600, theme: 'dark' }])` to render up to eight layouts from one resolved post. Results follow the options order. Each result adds `renderMs`, which measures layout, image processing on cache misses, painting, and PNG encoding. Post parsing and copying assets into WASM happen once before those timers start. Pass the highest ancestor count to `resolve()` when any layout needs thread context.
