import { profileExamples } from './profile-cases.mjs';

export async function checkProfiles(renderer) {
  const reference = await (await fetch('./fixtures/profile-chrome.json')).json();
  const urls = await (await fetch('./fixtures/assets.json')).json();
  const assets = new Map(await Promise.all(Object.entries(urls).filter(([, path]) => path.startsWith('profile-media/')).map(async ([url, path]) => {
    const bytes = await (await fetch(`./fixtures/${path}`)).arrayBuffer();
    return [url, new Uint8Array(bytes)];
  })));
  const inputs = Object.fromEntries(profileExamples);
  const actors = {
    'nasso.dev': 'profile-nasso',
    'tangled.org': 'profile-tangled',
    'anirudh.fi': 'profile-anirudh',
  };
  const issues = [];
  for (const ref of reference) {
    const input = structuredClone(inputs[actors[ref.actor]]);
    delete input.profile.viewer;
    delete input.context.viewerDid;
    const options = JSON.stringify({ width: ref.width, theme: 'dim', scale: 2 });
    const scene = renderer.profile_scene_layout(JSON.stringify(input), options);
    const key = `${ref.actor}/${ref.width}`;
    const check = (field, actual, expected) => {
      if (!Number.isFinite(actual) || Math.abs(actual - expected) > 1) issues.push({ key, field, actual, expected });
    };
    const text = item => item.block.lines.map(line => line.text).join(' ').trim();
    const at = (label, predicate, expected) => {
      const match = scene.text.find(item => predicate(text(item)));
      check(label + ' y', match?.y, expected.y);
      if (expected.h) check(label + ' height', match?.block.height, expected.h);
      return match;
    };
    check('header height with export padding', scene.height, ref.height + 8);
    const banner = scene.boxes.find(box => box.y === 0 && box.w === ref.width);
    check('banner height', banner?.h, ref.banner.h);
    const avatar = scene.boxes.find(box => box.x === ref.avatar.x && box.y === ref.avatar.y);
    check('avatar width', avatar?.w, ref.avatar.w);
    check('avatar height', avatar?.h, ref.avatar.h);
    at('name', t => t === (input.profile.displayName || input.profile.handle), ref.name);
    at('handle', t => t === '@' + input.profile.handle, { y: ref.handleY });
    at('followers', t => /^followers?$/.test(t), ref.followers);
    const bio = at('bio', t => t.startsWith(input.profile.description.split(' ').slice(0, 3).join(' ')), ref.bio);
    const lines = bio?.block.lines.map(line => line.text.trimEnd());
    if (JSON.stringify(lines) !== JSON.stringify(ref.bioLines)) issues.push({ key, bioLines: lines, expected: ref.bioLines });
    if (ref.pronouns) {
      const pronouns = at('pronouns', t => t === input.profile.pronouns, ref.pronouns);
      check('pronouns x', pronouns?.x, ref.pronouns.x);
    }
    const follow = scene.boxes.find(box => box.y === ref.follow.y && box.fill === ref.follow.fill);
    check('Follow x', follow?.x, ref.follow.x);
    check('Follow width', follow?.w, ref.follow.w);
    const pills = scene.boxes.filter(box => box.fill === ref.pills[0].fill && box.h === ref.pills[0].h);
    check('link count', pills.length, ref.pills.length);
    for (const [i, expected] of ref.pills.entries()) {
      for (const axis of ['x', 'y', 'w', 'h']) check(`link ${i + 1} ${axis}`, pills[i]?.[axis], expected[axis]);
    }
    const result = renderer.render_profile(JSON.stringify(input), options, assets);
    const png = new Blob([new Uint8Array(result.png)], { type: 'image/png' });
    result.free();
    const bitmap = await createImageBitmap(png);
    const ctx = new OffscreenCanvas(bitmap.width, bitmap.height).getContext('2d', { willReadFrequently: true });
    ctx.drawImage(bitmap, 0, 0);
    bitmap.close();
    for (const box of [follow, ...pills].filter(Boolean)) {
      const rgb = ctx.getImageData(Math.round((box.x + box.w - 3) * 2), Math.round((box.y + box.h / 2) * 2), 1, 1).data;
      const fill = rgb[0] * 65536 + rgb[1] * 256 + rgb[2];
      if (fill !== box.fill) issues.push({ key, box, fill });
    }
    for (const { sample, x, y, w, h } of ref.pixels) {
      const response = await fetch(`./fixtures/profile-chrome/${sample}`);
      const reference = await createImageBitmap(await response.blob());
      const referenceCtx = new OffscreenCanvas(w, h).getContext('2d');
      referenceCtx.drawImage(reference, 0, 0);
      reference.close();
      const expected = referenceCtx.getImageData(0, 0, w, h).data;
      const actual = ctx.getImageData(x, y, w, h).data;
      let error = 0;
      for (let i = 0; i < actual.length; i += 4) {
        for (let c = 0; c < 3; c++) error += Math.abs(actual[i + c] - expected[i + c]);
      }
      const meanChannelError = error / (w * h * 3);
      if (meanChannelError > 2) issues.push({ key, sample, meanChannelError });
    }
  }
  for (const [fixture, expected, absent] of [
    ['profile-tangled', ['Following', 'Followed by Anirudh Oppiliappan, akshay oppiliappan, and 3 others'], ['Follow']],
    ['profile-own', ['Edit Profile'], ['Follow', 'Following']],
    ['profile-follow-back', ['Follow back', 'Follows you'], ['Following']],
    ['profile-blocked', ['Unblock', 'Reposts muted'], ['he/him, they/them', 'Tangled']],
    ['profile-labeler', ['Unsubscribe', 'Liked by 15 users'], ['Follow']],
    ['profile-official-labeler', [], ['Unsubscribe', 'Liked by 15 users']],
    ['profile-live', ['LIVE'], []],
    ['profile-expired', [], ['LIVE']],
    ['profile-special-links', ['Support on Ko-fi', 'Germ DM'], []],
    ['profile-hidden', ['This profile is unavailable'], ['Tangled']],
  ]) {
    const scene = renderer.profile_scene_layout(JSON.stringify(inputs[fixture]), JSON.stringify({ width: 600 }));
    const text = scene.text.map(item => item.block.lines.map(line => line.text).join(' ').trim());
    for (const label of expected) if (!text.includes(label)) issues.push({ fixture, missing: label, text });
    for (const label of absent) if (text.includes(label)) issues.push({ fixture, unexpected: label });
  }
  return { references: reference.length, issues };
}
