import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { mkdir, writeFile } from 'node:fs/promises';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { examples, optionsFor } from '../tests/corpus.mjs';

const root = new URL('../', import.meta.url);
const output = process.argv[2] ?? 'target/parity-native';
const cli = process.env.SKYSHOT_CLI ?? fileURLToPath(new URL('target/release/skyshot', root));
const manifest = fileURLToPath(new URL('tests/fixtures/assets.json', root));
await mkdir(output, { recursive: true });
for (const [i, [name, post]] of examples.entries()) {
  const options = optionsFor(i);
  const png = execFileSync(cli, ['render', '-', '--assets', manifest,
    '--width', options.width, '--scale', options.scale, '--theme', options.theme,
    '--ancestors', options.ancestors, '--time-zone', options.timeZone, '-o', '-'],
  { input: JSON.stringify(post), maxBuffer: 32 * 1024 * 1024 });
  assert.deepEqual([...png.subarray(0, 8)], [137, 80, 78, 71, 13, 10, 26, 10], name);
  await writeFile(join(output, `${name}.png`), png);
}
console.log(`Rendered ${examples.length} native parity fixtures into ${output}`);
