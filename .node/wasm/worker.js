/** A Web Worker started by `new PDFEngine({ workers: N })` (see worker-core.js). */
import { serve } from './worker-core.js';

serve(
  (message, transfer) => self.postMessage(message, transfer || []),
  (handler) => {
    self.onmessage = (event) => handler(event.data);
  },
);
