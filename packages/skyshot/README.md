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
