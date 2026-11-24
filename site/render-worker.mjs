import { loadSkyshot } from './skyshot/web.mjs';

let skyshot;
let resolved;
let currentLink;

self.onmessage = async ({ data: { link, theme } }) => {
  try {
    skyshot ??= await loadSkyshot();
    if (link !== currentLink) {
      self.postMessage({ status: 'Fetching post…' });
      resolved = await skyshot.resolve(link);
      currentLink = link;
    }
    self.postMessage({ status: 'Rendering image…' });
    const result = skyshot.render(resolved, { theme });
    self.postMessage(result, [result.png.buffer]);
  } catch (error) {
    self.postMessage({ error: `Could not render this post. ${error.message || 'Please try again.'}` });
  }
};
