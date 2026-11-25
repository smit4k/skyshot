const form = document.querySelector('form');
const input = document.querySelector('#bluesky-link');
const button = document.querySelector('button');
const themes = document.querySelector('#themes');
const status = document.querySelector('#status');
const output = document.querySelector('#output');
const preview = document.querySelector('#preview');
const download = document.querySelector('#download');
let worker;
let imageUrl;

function finish(error) {
  button.disabled = false;
  themes.disabled = false;
  output.setAttribute('aria-busy', 'false');
  status.toggleAttribute('data-error', Boolean(error));
  if (error) status.textContent = error;
}

function startWorker() {
  const instance = new Worker(new URL('./render-worker.mjs', import.meta.url), { type: 'module' });
  worker = instance;
  function fail(error) {
    instance.terminate();
    worker = undefined;
    if (button.disabled) finish(error);
  }
  instance.onmessage = ({ data }) => {
    if (worker !== instance) return;
    if (data.preloadFailed || data.error) {
      fail(data.error ?? 'Could not start the renderer. Please try again.');
      return;
    }
    if (data.status) {
      status.textContent = data.status;
      return;
    }
    if (imageUrl) URL.revokeObjectURL(imageUrl);
    imageUrl = URL.createObjectURL(new Blob([data.png], { type: 'image/png' }));
    preview.src = imageUrl;
    preview.width = data.width;
    preview.height = data.height;
    preview.alt = `Rendered Bluesky ${data.kind}`;
    download.href = imageUrl;
    download.download = data.kind === 'profile' ? 'skyshot-profile.png' : 'skyshot.png';
    preview.hidden = false;
    download.hidden = false;
    document.querySelector('#placeholder').hidden = true;
    status.textContent = `Rendered in ${data.renderMs.toFixed(1)} ms.`;
    if (data.warnings.length) status.textContent += ' Some details could not be rendered.';
    finish();
  };
  instance.onerror = event => {
    event.preventDefault();
    if (worker === instance) fail('Could not start the renderer. Please try again.');
  };
}

form.addEventListener('submit', event => {
  event.preventDefault();
  if (button.disabled) return;
  const link = input.value.trim();
  input.value = link;
  let url;
  try { url = new URL(link); } catch { /* The message below also covers malformed URLs. */ }
  const kind = /^\/profile\/[^/]+\/?$/.test(url?.pathname) ? 'profile' : 'post';
  if (!url || url.protocol !== 'https:' || url.hostname !== 'bsky.app'
      || url.username || url.password || url.port
      || (kind === 'post' && !/^\/profile\/[^/]+\/post\/[^/]+\/?$/.test(url.pathname))) {
    finish('Paste a Bluesky post or profile link, such as https://bsky.app/profile/name.bsky.social');
    input.focus();
    return;
  }
  button.disabled = true;
  themes.disabled = true;
  output.setAttribute('aria-busy', 'true');
  status.removeAttribute('data-error');
  status.textContent = 'Loading renderer…';
  try {
    if (!worker) startWorker();
    worker.postMessage({ link, kind, theme: form.elements.theme.value });
  } catch {
    worker?.terminate();
    worker = undefined;
    finish('WebAssembly rendering is unavailable in this browser.');
  }
});

themes.addEventListener('change', () => {
  if (imageUrl && !button.disabled) form.requestSubmit();
});

try {
  startWorker();
} catch {
  // Generate retries startup and reports an error if it still fails.
}
