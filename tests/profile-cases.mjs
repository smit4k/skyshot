const fixtures = await (async () => {
  const url = new URL('./fixtures/profiles.json', import.meta.url);
  return globalThis.document ? (await fetch(url)).json()
    : JSON.parse(await (await import('node:fs/promises')).readFile(url, 'utf8'));
})();
const { profile: plain, context } = fixtures['nasso.dev'];
const did = 'did:plc:skyshotviewer';
const following = `at://${did}/app.bsky.graph.follow/sample`;
const viewer = { viewerDid: did, snapshotAt: context.snapshotAt };
const example = (profile = {}, metadata = {}) => ({
  profile: { ...plain, ...profile }, context: { ...context, ...metadata },
});
const links = Array.from({ length: 10 }, (_, i) => ({
  url: `https://example.org/${i}`, title: `A longer link for the profile ${i}`,
  icon: plain.avatar,
}));
const labeler = {
  ...example({ associated: { labeler: true } }, { ...viewer, labelerSubscribed: true }),
  labeler: { likeCount: 15, viewer: { like: `at://${did}/app.bsky.feed.like/sample` } },
};
const live = example({
  status: {
    status: 'app.bsky.actor.status#live', expiresAt: '2026-11-26T04:46:22Z',
    embed: { external: { uri: 'https://stream.place/example' } },
  },
});
export const profileExamples = Object.entries({
  'profile-nasso': fixtures['nasso.dev'],
  'profile-tangled': fixtures['tangled.org'],
  'profile-anirudh': fixtures['anirudh.fi'],
  'profile-minimal': { did, handle: 'example.com' },
  'profile-own': example({}, { viewerDid: plain.did }),
  'profile-follow-back': example({ viewer: { followedBy: following } }, viewer),
  'profile-notifications': example({ viewer: { following, activitySubscription: { post: true } } }, viewer),
  'profile-blocked': example({ viewer: { blocking: following, mutedOnlyReposts: true } }, viewer),
  'profile-labeler': labeler,
  'profile-official-labeler': { ...labeler, profile: { ...labeler.profile, did: 'did:plc:ar7c4by46qjdydhdevvrndac' } },
  'profile-live': live,
  'profile-expired': { ...live, context: { ...context, snapshotAt: '2026-11-27T04:46:22Z' } },
  'profile-links': example({ links }),
  'profile-special-links': example({
    links: [{ url: 'https://ko-fi.com/example' }],
    associated: { germ: { showButtonTo: 'everyone', messageMeUrl: 'https://germ.network/example' } },
  }),
  'profile-blur': example({ labels: [{ val: 'sensitive' }] }),
  'profile-hidden': example({ labels: [{ val: '!no-unauthenticated' }] }),
  'profile-unicode': example({
    handle: 'a-long-handle-for-testing-profile-width-and-pronoun-layout.example',
    displayName: 'مرحبا 👩‍💻 にほんご ✨ مؤلف طويل جدًا',
    description: 'שלום #סקיי\nEmoji and links 🌈',
    pronouns: 'he/him, they/them',
    followersCount: 44_950,
  }, { descriptionFacets: undefined }),
});
export const profileOptionsFor = i => ({
  width: [390, 600, 280, 1000][i % 4], scale: 1 + i % 3,
  theme: ['dark', 'light', 'dim'][i % 3],
});
