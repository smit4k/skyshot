import { readFile } from 'node:fs/promises';

const fixture = name => readFile(new URL(`./fixtures/${name}`, import.meta.url));
const demo = JSON.parse(await fixture('post.json'));
const tile = await fixture('tiles.png');
const files = new Map(await Promise.all(Object.entries(JSON.parse(await fixture('assets.json')))
  .map(async ([url, name]) => [url, await fixture(name)])));
const [avatar, photo, video] = files.keys();

const typed = (type, fields = {}) => ({ $type: type, ...fields });
const embed = (type, fields) => typed(`app.bsky.embed.${type}#view`, fields);
const notice = type => embed('record', {record: typed(`app.bsky.embed.record#${type}`)});
const landscape = { width: 48, height: 32 };
const portrait = { width: 9, height: 16 };
const images = (count, ratio = landscape) => embed('images', {images: Array.from({length: count}, () => ({
  thumb: photo, fullsize: photo, alt: 'Test pattern', aspectRatio: ratio,
}))});
const gallery = embed('gallery', {items: Array.from({length: 20}, () => typed('app.bsky.embed.gallery#viewImage', {
  thumbnail: photo, fullsize: photo, alt: '', aspectRatio: landscape,
}))});
const external = uri => embed('external', { external: {uri, title:'Example', description:'A sample caption', thumb: photo} });
const quote = embed('record', {record: typed('app.bsky.embed.record#viewRecord', {
  author: demo.author, value: demo.record, embeds: [images(1, portrait)],
})});
const thread = post => typed('app.bsky.feed.defs#threadViewPost', { post });
const examples = Object.entries({
  text: demo,
  emoji: { ...demo, record: {...demo.record, text: '👋🏽 👩‍💻 🏳️‍🌈', facets: []}},
  one: { ...demo, embed: images(1, portrait) },
  two: { ...demo, embed: images(2) },
  three: { ...demo, embed: images(3) },
  four: { ...demo, embed: images(4) },
  gallery: {...demo, embed: gallery},
  card: {...demo, embed: external('https://example.com/full-article')},
  player: {...demo, embed: external('https://www.youtube.com/watch?v=example')},
  gif: {...demo, embed: external('https://tenor.com/view/test')},
  video: {...demo, embed: embed('video', {thumbnail: video, aspectRatio: landscape})},
  'video-gif': {...demo, embed: embed('video', {thumbnail: video, aspectRatio: portrait, presentation:'gif'})},
  quote: {...demo, embed: quote},
  'quote-with-media': {...demo, embed: embed('recordWithMedia', {record: quote, media: images(2)})},
  'quote-deleted': {...demo, embed: notice('viewNotFound')},
  'quote-blocked': {...demo, embed: notice('viewBlocked')},
  'quote-detached': {...demo, embed: notice('viewDetached')},
  feed: {...demo, embed: embed('record', {record: typed('app.bsky.feed.defs#generatorView', {displayName: 'A feed', creator: demo.author, avatar})})},
  list: {...demo, embed: embed('record', {record: typed('app.bsky.graph.defs#listView', {name: 'A list', creator: demo.author, avatar})})},
  'starter-pack': {...demo, embed: embed('record', {record: typed('app.bsky.graph.defs#starterPackViewBasic', {record: {name:'Starter pack'}, creator: demo.author})})},
  hidden: {...demo, labels: [{val: 'porn'}], embed: images(1)},
  warned: {...demo, labels: [{val: 'sensitive'}], embed: images(1)},
  unavailable: {thread: typed('app.bsky.feed.defs#blockedPost')},
  ancestors: { thread: {...thread(demo), parent: {...thread(demo), parent: typed('app.bsky.feed.defs#notFoundPost') }} },
});

export { demo, tile, files, thread, examples, external };

export const optionsFor = i => ({
  width: i % 2 ? 390 : 600, scale: i % 3 + 1, theme: ['light', 'dim', 'dark'][i % 3], ancestors: 2,
  timeZone: i % 2 ? 'America/Detroit' : 'UTC', hour12: true,
});
