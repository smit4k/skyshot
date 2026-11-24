const form = document.querySelector('form');
const input = document.querySelector('#post-link');
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
  worker = new Worker(new URL('./render-worker.mjs', import.meta.url), { type: 'module' });
  worker.onmessage = ({ data }) => {
    if (data.status) {
      status.textContent = data.status;
      return;
    }
    if (data.error) {
      worker.terminate();
      worker = undefined;
      finish(data.error);
      return;
    }
    if (imageUrl) URL.revokeObjectURL(imageUrl);
    imageUrl = URL.createObjectURL(new Blob([data.png], { type: 'image/png' }));
    preview.src = imageUrl;
    preview.width = data.width;
    preview.height = data.height;
    download.href = imageUrl;
    document.querySelector('figure').hidden = false;
    document.querySelector('#placeholder').hidden = true;
    status.textContent = data.warnings.length
      ? 'Image ready. Some details could not be rendered.'
      : 'Image ready.';
    finish();
  };
  worker.onerror = event => {
    event.preventDefault();
    worker.terminate();
    worker = undefined;
    finish('Could not start the renderer. Please try again.');
  };
}

form.addEventListener('submit', event => {
  event.preventDefault();
  if (button.disabled) return;
  const link = input.value.trim();
  input.value = link;
  let url;
  try { url = new URL(link); } catch { /* The message below also covers malformed URLs. */ }
  if (!url || url.protocol !== 'https:' || url.hostname !== 'bsky.app'
      || url.username || url.password || url.port
      || !/^\/profile\/[^/]+\/post\/[^/]+\/?$/.test(url.pathname)) {
    finish('Paste a Bluesky post link, such as https://bsky.app/profile/name.bsky.social/post/…');
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
    worker.postMessage({ link, theme: form.elements.theme.value });
  } catch {
    worker?.terminate();
    worker = undefined;
    finish('WebAssembly rendering is unavailable in this browser.');
  }
});

themes.addEventListener('change', () => {
  if (imageUrl && !button.disabled) form.requestSubmit();
});
