import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { spawn } from 'node:child_process';
import { createServer } from 'node:http';
import { mkdtemp, readFile, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { extname, join, resolve, sep } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = fileURLToPath(new URL('../', import.meta.url));
const mime = { '.html': 'text/html', '.mjs': 'text/javascript', '.js': 'text/javascript', '.wasm': 'application/wasm', '.ttf': 'font/ttf', '.png': 'image/png' };
const server = createServer(async (request, response) => {
  try {
    const path = resolve(root, `.${decodeURIComponent(new URL(request.url, 'http://localhost').pathname)}`);
    if (!path.startsWith(root.endsWith(sep) ? root : root + sep)) throw new Error('Invalid path');
    response.setHeader('content-type', mime[extname(path)] ?? 'application/octet-stream');
    response.end(await readFile(path));
  } catch { response.writeHead(404).end(); }
});
await new Promise((resolve, reject) => { server.once('error', reject); server.listen(0, '127.0.0.1', resolve); });
const profile = await mkdtemp(join(tmpdir(), 'skyshot-chrome-'));
const executable = process.env.CHROME ?? (process.platform === 'darwin' ? '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome' : 'google-chrome');
const chrome = spawn(executable, ['--headless=new', '--no-first-run', '--no-default-browser-check', '--remote-debugging-pipe', `--user-data-dir=${profile}`], { stdio: ['ignore', 'ignore', 'pipe', 'pipe', 'pipe'] });
let sequence = 0;
let buffer = '';
let stderr = '';
const pending = new Map();
chrome.stderr.on('data', data => { stderr += data; });
const rejectPending = error => { for (const { reject } of pending.values()) reject(error); pending.clear(); };
chrome.on('error', rejectPending);
chrome.on('exit', code => rejectPending(new Error(`Chrome exited ${code}: ${stderr}`)));
chrome.stdio[3].on('error', rejectPending);
chrome.stdio[4].setEncoding('utf8');
chrome.stdio[4].on('data', data => {
  buffer += data;
  for (let end; (end = buffer.indexOf('\0')) >= 0;) {
    const message = JSON.parse(buffer.slice(0, end));
    buffer = buffer.slice(end + 1);
    const waiter = pending.get(message.id);
    if (waiter) {
      pending.delete(message.id);
      if (message.error) waiter.reject(new Error(JSON.stringify(message.error)));
      else waiter.resolve(message.result);
    }
  }
});
function send(method, params = {}, sessionId) {
  return new Promise((resolve, reject) => {
    const id = ++sequence;
    pending.set(id, { resolve, reject });
    chrome.stdio[3].write(JSON.stringify({ id, method, params, sessionId }) + '\0');
  });
}
const timeout = setTimeout(() => { rejectPending(new Error('Chrome oracle timed out')); chrome.kill(); }, 90_000);
try {
  const { targetId } = await send('Target.createTarget', { url: 'about:blank' });
  const { sessionId } = await send('Target.attachToTarget', { targetId, flatten: true });
  await send('Page.navigate', { url: `http://127.0.0.1:${server.address().port}/tests/chrome.html` }, sessionId);
  const result = await send('Runtime.evaluate', { expression: '(async () => { while (!globalThis.oracleDone) await new Promise(r => setTimeout(r, 10)); return await globalThis.oracleDone; })()', awaitPromise: true, returnByValue: true }, sessionId);
  assert.ok(!result.exceptionDetails, JSON.stringify(result.exceptionDetails));
  const report = result.result.value;
  await writeFile(join(root, 'target/chrome-report.json'), JSON.stringify(report, null, 2) + '\n');
  console.log(JSON.stringify({ chrome: report.userAgent, browserPerformance: report.browserPerformance, layouts: report.layouts, lineBreakMismatches: report.mismatches.length, widthErrorsOver1px: report.widthErrors.length, cards: report.cards, cardMismatches: report.cardMismatches.length, authorHeaders: report.authors.headers, authorMismatches: report.authors.mismatches.length, authorWidthErrorsOver1px: report.authors.widthErrors.length, profileReferences: report.profiles.references, profileIssues: report.profiles.issues.length, report: 'target/chrome-report.json' }, null, 2));
  assert.equal(report.layouts, 2688, 'The complete text corpus must execute');
  assert.equal(report.cards, 192);
  assert.equal(report.authors.headers, 648);
  assert.equal(report.authors.mismatches.length, 0, 'Author-header regressions; inspect target/chrome-report.json');
  assert.equal(report.profiles.references, 6, 'All captured profile references must execute');
  assert.deepEqual(report.profiles.issues, [], 'Profile parity regressions');
  const differences = [...report.mismatches.map(item => ['text', item]), ...report.cardMismatches.map(item => ['card', item])];
  const fingerprints = differences.map(([kind, item]) => `${kind}/${item.key}:${createHash('sha256').update(JSON.stringify(item)).digest('hex')}`);
  const baselinePath = join(root, 'tests/fixtures/chrome-known-differences.json');
  if (process.argv.includes('--update-baseline')) await writeFile(baselinePath, JSON.stringify(fingerprints, null, 2) + '\n');
  const known = new Set(JSON.parse(await readFile(baselinePath, 'utf8')));
  const regressions = fingerprints.filter(key => !known.has(key));
  assert.deepEqual(regressions, [], 'New Chrome differences; inspect target/chrome-report.json');
  if (process.argv.includes('--strict')) assert.equal(differences.length + report.widthErrors.length + report.authors.widthErrors.length, 0, 'Chrome parity still has known differences');
} finally {
  clearTimeout(timeout);
  chrome.kill();
  server.closeAllConnections();
  await new Promise(resolve => server.close(resolve));
  await rm(profile, { recursive: true, force: true, maxRetries: 3 });
}
