/**
 * A few workers, each with its own copy of the renderer, so several documents are made at the same time and the
 * page's own thread stays free. Used by `new PDFEngine({ workers: N })`.
 */
const isNode = typeof process !== 'undefined' && !!process.versions?.node && typeof window === 'undefined';

/** Start one worker and give it a uniform `{ post, on, terminate }` face. */
async function startWorker() {
  if (isNode) {
    // Not called `Worker`: bundlers treat `new Worker(new URL(...))` as a browser worker to bundle.
    const { Worker: NodeThread } = await import(/* webpackIgnore: true */ /* @vite-ignore */ 'node:worker_threads');
    const worker = new NodeThread(new URL('./worker-node.js', import.meta.url));
    return {
      post: (message, transfer) => worker.postMessage(message, transfer || []),
      on: (handlers) => {
        worker.on('message', handlers.message);
        worker.on('error', handlers.error);
        worker.on('exit', handlers.exit);
      },
      terminate: () => worker.terminate(),
    };
  }
  // The literal form below is what bundlers (Vite, webpack, Rollup, Parcel) look for to include the worker file.
  const url = new URL('./worker.js', import.meta.url);
  let worker;
  if (typeof location !== 'undefined' && /^https?:$/.test(url.protocol) && url.origin !== location.origin) {
    // Loaded from a CDN: a page cannot start a worker from another origin, but a worker made from a same-origin
    // blob can import a module from there (the CDN sends the CORS header that needs).
    const blob = new Blob([`import ${JSON.stringify(url.href)};`], { type: 'text/javascript' });
    worker = new Worker(URL.createObjectURL(blob), { type: 'module' });
  } else {
    worker = new Worker(new URL('./worker.js', import.meta.url), { type: 'module' });
  }
  return {
    post: (message, transfer) => worker.postMessage(message, transfer || []),
    on: (handlers) => {
      worker.onmessage = (event) => handlers.message(event.data);
      worker.onerror = (event) => handlers.error(new Error(event.message || 'the worker failed to start'));
    },
    terminate: () => worker.terminate(),
  };
}

export class WorkerPool {
  #slots = [];
  #next = 0;
  #nextId = 1;
  #closed = false;
  #makeError;

  /**
   * @param {number} count how many workers
   * @param {object} initOptions what each worker's `PDFEngine` is created with (must be cloneable)
   * @param {(error: object) => Error} makeError builds the package's error from `{ code, message, ... }`
   */
  constructor(count, initOptions, makeError) {
    this.#makeError = makeError;
    for (let i = 0; i < count; i++) this.#slots.push(this.#spawn(initOptions));
  }

  get size() {
    return this.#slots.length;
  }

  #spawn(initOptions) {
    const slot = { worker: null, pending: new Map(), ready: null, dead: null };
    slot.ready = startWorker().then((worker) => {
      slot.worker = worker;
      const fail = (reason) => {
        slot.dead = reason;
        for (const [, p] of slot.pending) p.reject(this.#makeError({ code: 'RENDER_FAILED', message: `A worker stopped: ${reason}`, status: 500 }));
        slot.pending.clear();
      };
      worker.on({
        message: (message) => {
          if (message.type === 'imageError') {
            slot.pending.get(message.id)?.onImageError?.(message.src, message.reason);
            return;
          }
          const p = slot.pending.get(message.id);
          if (!p) return;
          slot.pending.delete(message.id);
          if (message.ok) p.resolve(message);
          else p.reject(this.#makeError(message.error));
        },
        error: (e) => fail(e?.message || String(e)),
        exit: () => fail('it exited'),
      });
      worker.post({ type: 'init', options: initOptions });
    });
    return slot;
  }

  /** The worker with the fewest documents in progress (the next one when equal). */
  #pick() {
    const alive = this.#slots.filter((s) => !s.dead);
    if (alive.length === 0) throw this.#makeError({ code: 'RENDER_FAILED', message: 'Every worker has stopped.', status: 500 });
    const fewest = Math.min(...alive.map((s) => s.pending.size));
    const candidates = alive.filter((s) => s.pending.size === fewest);
    return candidates[this.#next++ % candidates.length];
  }

  async #send(slot, message, transfer, onImageError) {
    await slot.ready;
    if (slot.dead) throw this.#makeError({ code: 'RENDER_FAILED', message: `A worker stopped: ${slot.dead}`, status: 500 });
    const id = this.#nextId++;
    return new Promise((resolve, reject) => {
      slot.pending.set(id, { resolve, reject, onImageError });
      slot.worker.post({ ...message, id }, transfer);
    });
  }

  /** Make one document on the least busy worker. Resolves with the PDF bytes. */
  async generate(options, onImageError) {
    if (this.#closed) throw this.#makeError({ code: 'ENGINE_CLOSED', message: 'This PDFEngine was closed; create a new one.' });
    const reply = await this.#send(this.#pick(), { type: 'generate', options }, [], onImageError);
    return new Uint8Array(reply.bytes);
  }

  async ping() {
    if (this.#closed) throw this.#makeError({ code: 'ENGINE_CLOSED', message: 'This PDFEngine was closed; create a new one.' });
    const replies = await Promise.all(this.#slots.filter((s) => !s.dead).map((s) => this.#send(s, { type: 'ping' })));
    return { version: replies[0]?.version };
  }

  async loadFonts(fonts) {
    await Promise.all(this.#slots.filter((s) => !s.dead).map((s) => this.#send(s, { type: 'loadFonts', fonts })));
  }

  close() {
    this.#closed = true;
    for (const slot of this.#slots) {
      slot.ready.then(() => slot.worker?.terminate());
      for (const [, p] of slot.pending) p.reject(this.#makeError({ code: 'ENGINE_CLOSED', message: 'This PDFEngine was closed; create a new one.' }));
      slot.pending.clear();
    }
  }
}
