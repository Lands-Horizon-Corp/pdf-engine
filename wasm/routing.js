/**
 * Where a document is rendered: in this page (WebAssembly) or on one of your pdf-engine servers.
 *
 * Pure logic, no network and no WebAssembly, so it is easy to test:
 *   - `parseBytes('2mb')`                         size strings
 *   - `serversOf({ url, token })`                  one or many servers, each with its token
 *   - `choose({ pick, autoMaxBytes, ... })`        'server', 'browser' or 'auto' (size and connectivity)
 *   - `new ServerPool(servers)`                    round-robin with failover, remembers a server that failed
 */

const UNITS = { b: 1, kb: 1024, mb: 1024 ** 2, gb: 1024 ** 3 };

/** `1500`, `'512kb'`, `'2mb'`, `'1.5 GB'` to a number of bytes. Throws on anything else. */
export function parseBytes(value) {
  if (typeof value === 'number' && Number.isFinite(value) && value >= 0) return Math.floor(value);
  const match = /^\s*(\d+(?:\.\d+)?)\s*(b|kb|mb|gb)?\s*$/i.exec(String(value));
  if (!match) throw new RangeError(`autoMaxBytes must be a number of bytes or a size like '2mb', not ${JSON.stringify(value)}.`);
  return Math.floor(Number(match[1]) * UNITS[(match[2] || 'b').toLowerCase()]);
}

/**
 * The servers to use: `url` (a string or an array) with `token` (one token for all, or an array that
 * lines up with `url`). Blank entries are dropped.
 * @returns {{ url: string, token?: string }[]}
 */
export function serversOf({ url, token }) {
  const urls = [].concat(url ?? []);
  const tokens = Array.isArray(token) ? token : token == null ? [] : urls.map(() => token);
  return urls
    .map((u, i) => ({ url: String(u ?? '').trim().replace(/\/+$/, ''), token: tokens[i] || undefined }))
    .filter((s) => s.url !== '');
}

/** The size of a request in bytes, without building one big string when `data` is huge. */
export function requestBytes({ template, data }) {
  const encoder = new TextEncoder();
  let bytes = typeof template === 'string' ? encoder.encode(template).length : 0;
  if (data !== undefined) bytes += encoder.encode(JSON.stringify(data) ?? '').length;
  return bytes;
}

/**
 * Decide where one document goes.
 * @param {{ pick?: 'server'|'browser'|'auto', autoMaxBytes?: number|string, hasServers: boolean,
 *           online?: boolean, size: () => number }} input  `size` is only called when it matters.
 * @returns {'server'|'browser'}
 */
export function choose({ pick = 'auto', autoMaxBytes, hasServers, online = true, size }) {
  if (!['server', 'browser', 'auto'].includes(pick)) {
    throw new RangeError(`pick must be 'server', 'browser' or 'auto', not ${JSON.stringify(pick)}.`);
  }
  if (pick === 'browser' || !hasServers) return 'browser';
  if (!online) return 'browser';
  if (pick === 'server') return 'server';
  // auto: small documents are quicker in the page, big ones belong on the server.
  if (autoMaxBytes === undefined || autoMaxBytes === null) return 'server';
  return size() > parseBytes(autoMaxBytes) ? 'server' : 'browser';
}

/** Is this failure one that another server could fix (a network error or a server-side error)? */
export function isRetryable(error) {
  if (error?.code === 'SERVER_UNREACHABLE') return true;
  const status = error?.status;
  return status === 502 || status === 503 || status === 504 || status === 429;
}

/**
 * Round-robin over the servers, skipping one that just failed for `cooldownMs`.
 * `run(server)` does one request; a retryable failure moves on to the next server.
 */
export class ServerPool {
  #servers;
  #next = 0;
  #down = new Map();
  #cooldownMs;
  #now;

  constructor(servers, { cooldownMs = 30_000, now = () => Date.now() } = {}) {
    this.#servers = servers;
    this.#cooldownMs = cooldownMs;
    this.#now = now;
  }

  get size() {
    return this.#servers.length;
  }

  /** Servers in the order to try them: from the next in line, those on cooldown last. */
  #order() {
    const start = this.#next++ % this.#servers.length;
    const rotated = [...this.#servers.slice(start), ...this.#servers.slice(0, start)];
    const now = this.#now();
    const healthy = rotated.filter((s) => (this.#down.get(s.url) ?? 0) <= now);
    const cooling = rotated.filter((s) => !healthy.includes(s));
    return [...healthy, ...cooling];
  }

  /** Try the servers in turn. Resolves with the first success; throws the last error when all fail. */
  async run(run) {
    let last;
    for (const server of this.#order()) {
      try {
        const result = await run(server);
        this.#down.delete(server.url);
        return result;
      } catch (error) {
        last = error;
        if (!isRetryable(error)) throw error;
        this.#down.set(server.url, this.#now() + this.#cooldownMs);
      }
    }
    throw last;
  }
}

/**
 * `data` without the top-level keys the template never names, so less is sent to the renderer (and over
 * the network). Every identifier inside `{{ }}`, `{% %}` and `{# #}` counts as used, so a key that is only
 * reached as `item.price` keeps `item`. Templates that look keys up dynamically keep everything.
 */
export function pruneData(template, data) {
  if (typeof template !== 'string' || data === null || typeof data !== 'object' || Array.isArray(data)) return data;
  if (/\b(?:self|context|namespace)\b|\{%-?\s*(?:include|extends|import|from)\b/.test(template)) return data;
  const used = new Set();
  for (const tag of template.matchAll(/\{\{[\s\S]*?\}\}|\{%[\s\S]*?%\}|\{#[\s\S]*?#\}/g)) {
    for (const name of tag[0].matchAll(/[A-Za-z_][A-Za-z0-9_]*/g)) used.add(name[0]);
  }
  const keys = Object.keys(data);
  const kept = keys.filter((key) => used.has(key));
  if (kept.length === keys.length) return data;
  const pruned = {};
  for (const key of kept) pruned[key] = data[key];
  return pruned;
}
