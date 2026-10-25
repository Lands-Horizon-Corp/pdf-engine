/** A worker thread started by `new PDFEngine({ workers: N })` under Node (see worker-core.js). */
import { parentPort } from 'node:worker_threads';
import { serve } from './worker-core.js';

serve(
  (message, transfer) => parentPort.postMessage(message, transfer || []),
  (handler) => parentPort.on('message', handler),
);
