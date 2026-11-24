import { createHash } from 'node:crypto';
import { mkdir, readFile, writeFile } from 'node:fs/promises';

const base = new URL('../fonts/', import.meta.url);
const entries = JSON.parse(await readFile(new URL('manifest.json', base), 'utf8'));
await mkdir(new URL('fallback/', base), { recursive: true });
await Promise.all(entries.map(async ({ path, url, sha256 }) => {
  const target = new URL(path, base);
  const matches = bytes => createHash('sha256').update(bytes).digest('hex') === sha256;
  const existing = await readFile(target).catch(error => {
    if (error.code !== 'ENOENT') throw error;
  });
  if (existing && matches(existing)) return;
  const response = await fetch(url, { signal: AbortSignal.timeout(60000) });
  if (!response.ok) throw new Error(`Could not download ${path}: HTTP ${response.status}`);
  const bytes = new Uint8Array(await response.arrayBuffer());
  if (!matches(bytes)) throw new Error(`Hash mismatch for ${path}`);
  await writeFile(target, bytes);
}));
