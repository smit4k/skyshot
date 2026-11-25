import { authorCases } from './chrome-cases.mjs';

export async function checkAuthors(renderer, plain, fontFamily) {
  const mismatches = [];
  const widthErrors = [];
  let headers = 0;
  const base = { ...plain, author: { ...plain.author, verification: null }, record: { ...plain.record, text: 'Plain text', facets: [] } };
  for (const context of ['selected', 'quote', 'ancestor']) for (const { name, ...fields } of authorCases) {
    const author = { ...base.author, ...fields };
    const post = context === 'selected' ? { ...base, author }
      : context === 'quote' ? { ...base, embed: { $type: 'app.bsky.embed.record#view', record: { $type: 'app.bsky.embed.record#viewRecord', author, value: base.record } } }
      : { thread: { $type: 'app.bsky.feed.defs#threadViewPost', post: base, parent: { $type: 'app.bsky.feed.defs#threadViewPost', post: { ...base, author } } } };
    const verified = fields.verification?.verifiedStatus === 'valid' || fields.verification?.trustedVerifierStatus === 'valid';
    for (const width of [280, 390, 600, 1000]) for (const theme of ['light', 'dim', 'dark']) for (const scale of [1, 2]) {
      const selected = context === 'selected';
      const x = context === 'quote' ? 50 : 70;
      const y = context === 'quote' ? 116 : 16;
      const w = selected ? width - 98 - (width >= 600 ? 86 : 68) : width - (context === 'quote' ? 78 : 86);
      const size = selected ? 16.9 : 15;
      const lineHeight = selected ? 22 : 18;
      const badgeSize = selected ? 14 : 12;
      const reserved = verified ? badgeSize + 2 : 0;
      const header = document.createElement('div');
      Object.assign(header.style, { display: 'flex', alignItems: 'center', width: `${w}px`, fontFamily, fontSize: `${size}px`, lineHeight: `${lineHeight}px`, fontFeatureSettings: '"calt" 0', whiteSpace: 'nowrap' });
      const displayName = document.createElement('span');
      displayName.textContent = (author.displayName?.trim() || author.handle).slice(0, 640).replace(/[\n\r]/g, ' ');
      Object.assign(displayName.style, { fontWeight: '600', flexShrink: '0', maxWidth: `calc(${selected ? 100 : 70}% - ${reserved}px)`, overflow: 'hidden', textOverflow: 'ellipsis' });
      header.append(displayName);
      if (verified) {
        const badge = document.createElement('span');
        Object.assign(badge.style, { width: `${badgeSize}px`, height: `${badgeSize}px`, marginLeft: '2px', flexShrink: '0' });
        header.append(badge);
      }
      if (!selected) {
        const handle = document.createElement('span');
        handle.textContent = `\u00a0@${author.handle}`;
        Object.assign(handle.style, { overflow: 'hidden', textOverflow: 'ellipsis', fontWeight: '400' });
        header.append(handle);
      }
      document.body.append(header);
      const expectedNameWidth = displayName.getBoundingClientRect().width;
      const clipped = displayName.scrollWidth > displayName.clientWidth;
      const options = JSON.stringify({ width, theme, scale, ancestors: 1, timeZone: 'UTC' });
      const layout = renderer.scene_layout(JSON.stringify(post), options);
      const actualName = layout.text.find(item => item.x === x && item.y === y);
      const actualWidth = actualName?.block.lines[0]?.width;
      const result = renderer.render(JSON.stringify(post), options, new Map());
      const bitmap = await createImageBitmap(new Blob([new Uint8Array(result.png)], { type: 'image/png' }));
      result.free();
      const canvas = document.createElement('canvas');
      canvas.width = bitmap.width;
      canvas.height = bitmap.height;
      const ctx = canvas.getContext('2d');
      ctx.drawImage(bitmap, 0, 0);
      bitmap.close();
      const pixels = ctx.getImageData(0, Math.floor(y * scale), width * scale, lineHeight * scale).data;
      const blue = [];
      const expectedBadgeX = x + actualWidth + 2;
      const muted = { light: [64, 81, 104], dim: [171, 184, 201], dark: [165, 178, 197] }[theme];
      let check = false;
      let mutedHandle = false;
      for (let i = 0; i < pixels.length; i += 4) {
        const px = i / 4 % (width * scale) / scale;
        if (pixels[i] === 0 && pixels[i + 1] === 106 && pixels[i + 2] === 255) blue.push(px);
        if (px >= expectedBadgeX && px < expectedBadgeX + badgeSize && pixels[i] > 192 && pixels[i + 1] > 220 && pixels[i + 2] > 240) check = true;
        if (px >= x + actualWidth + reserved && px < x + w && muted.every((value, channel) => pixels[i + channel] === value)) mutedHandle = true;
      }
      const handle = selected ? null : layout.text.find(item => Math.abs(item.x - (x + actualWidth + reserved)) < 0.01 && item.y === y);
      const errors = [];
      const key = `${context}/${name}/${width}/${theme}/${scale}`;
      if (!Number.isFinite(actualWidth) || actualWidth > (selected ? w : w * 0.7) - reserved + 1) errors.push('name leaves its available space');
      if (!clipped && Math.abs(actualWidth - expectedNameWidth) > 1) widthErrors.push({ key, chrome: expectedNameWidth, skyshot: actualWidth });
      if (Boolean(blue.length) !== verified) errors.push({ badge: { expected: verified, actual: Boolean(blue.length) } });
      if (verified && theme !== 'light' && !check) errors.push('missing white verification check');
      if (blue.some(px => px < expectedBadgeX - 1 || px > expectedBadgeX + badgeSize + 1)) errors.push('badge overlaps or leaves its reserved position');
      if (blue.some(px => px < x || px >= x + w)) errors.push('badge leaves the header');
      if (!selected && !handle) errors.push('missing handle after name and badge');
      if (!selected && !mutedHandle) errors.push('handle has the wrong theme color');
      if (errors.length) mismatches.push({ key, errors });
      header.remove();
      headers++;
    }
  }
  return { headers, mismatches, widthErrors };
}
