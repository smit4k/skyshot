import { loadSkyshot } from './skyshot/web.mjs';

let hasRequest = false;
const ready = loadSkyshot({ lazy: true });
ready.then(async skyshot => {
  // Exercise the default layout and PNG encoder while the visitor enters a link.
  const sample = {
    author: { displayName: 'Skyshot Demo', handle: 'skyshot.example' },
    record: { text: 'The quick brown fox jumps over the lazy dog.', createdAt: '2025-03-04T12:30:00Z' },
    replyCount: 14, repostCount: 123, likeCount: 3456, quoteCount: 3, bookmarkCount: 12,
  };
  skyshot.render(sample);
  if (hasRequest) return;

  const avatar = './warmup/avatar.jpg';
  const thumb = './warmup/tiles.png';
  const assets = new Map(await Promise.all([avatar, thumb].map(async path => {
    const response = await fetch(path);
    if (!response.ok) throw new Error('Could not load warmup asset');
    return [path, new Uint8Array(await response.arrayBuffer())];
  })));
  if (hasRequest) return;

  skyshot.render({
    post: {
      ...sample,
      author: { ...sample.author, avatar },
      embed: { $type: 'app.bsky.embed.images#view', images: [{ thumb, aspectRatio: { width: 48, height: 32 } }] },
    },
    assets,
  });
}, () => self.postMessage({ preloadFailed: true })).catch(() => {});

let resolved;
let currentLink;

self.onmessage = async ({ data: { link, theme } }) => {
  hasRequest = true;
  try {
    const skyshot = await ready;
    if (link !== currentLink) {
      self.postMessage({ status: 'Fetching post…' });
      resolved = await skyshot.resolve(link);
      currentLink = link;
    }
    self.postMessage({ status: 'Rendering image…' });
    const started = performance.now();
    const result = skyshot.render(resolved, { theme });
    const renderMs = performance.now() - started;
    self.postMessage({ ...result, renderMs }, [result.png.buffer]);
  } catch (error) {
    self.postMessage({ error: `Could not render this post. ${error.message || 'Please try again.'}` });
  }
};
