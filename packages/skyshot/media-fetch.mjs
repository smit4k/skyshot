export function createMediaFetch(fetch) {
  const servers = new Map();

  async function resolveServer(did, options) {
    // The image CDN blocks browser reads; use the author's original blob instead.
    const response = await fetch(`https://plc.directory/${did}`, options);
    if (!response.ok) {
      await response.body?.cancel().catch(() => {});
      throw new Error(`Could not look up media host (HTTP ${response.status})`);
    }
    let size = 0;
    const document = await new Response(response.body.pipeThrough(new TransformStream({
      transform(chunk, controller) {
        size += chunk.byteLength;
        if (size > 64 * 1024) throw new Error('Oversized identity document');
        controller.enqueue(chunk);
      },
    }))).json();
    const service = document.service?.find(entry =>
      [ '#atproto_pds', `${did}#atproto_pds` ].includes(entry.id)
      && entry.type === 'AtprotoPersonalDataServer');
    const endpoint = new URL(service?.serviceEndpoint);
    // shortcut: only Bluesky-hosted PLC accounts, expand the allowlist when supporting other PDS hosts.
    if (document.id !== did || !/^https:\/\/[a-z0-9-]+\.[a-z0-9-]+\.host\.bsky\.network$/.test(endpoint.origin)
        || endpoint.username || endpoint.password || endpoint.pathname !== '/' || endpoint.search || endpoint.hash) {
      throw new Error('Unsupported media host');
    }
    return endpoint.origin;
  }

  return async function mediaFetch(url, options) {
    const source = new URL(url);
    const image = source.origin === 'https://cdn.bsky.app'
      && /^\/img\/[^/]+\/plain\/(did:plc:[a-z2-7]{24})\/(b[a-z2-7]+)(?:@(?:jpeg|png|webp))?$/.exec(source.pathname);
    if (!image) return fetch(url, options);

    const [, did, cid] = image;
    let server = servers.get(did);
    if (!server) {
      server = resolveServer(did, options).catch(error => {
        servers.delete(did);
        throw error;
      });
      servers.set(did, server);
    }
    const blob = new URL('/xrpc/com.atproto.sync.getBlob', await server);
    blob.search = new URLSearchParams({ did, cid });
    return fetch(blob.href, options);
  };
}
