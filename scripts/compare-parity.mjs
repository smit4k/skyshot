import assert from 'node:assert/strict';
import { readFile, readdir } from 'node:fs/promises';
import { join } from 'node:path';

const [reference, ...directories] = process.argv.slice(2);
assert.ok(reference && directories.length, 'Pass at least two corpus directories');
const names = (await readdir(reference)).filter(name => name.endsWith('.png')).sort();
assert.ok(names.length > 0, 'Parity corpus must not be empty');
for (const directory of directories) {
  assert.deepEqual((await readdir(directory)).filter(name => name.endsWith('.png')).sort(), names, directory);
  for (const name of names) assert.deepEqual(await readFile(join(directory, name)), await readFile(join(reference, name)), `${directory}/${name}`);
}
console.log(`${names.length} PNGs are byte-identical across ${directories.length + 1} builds`);
