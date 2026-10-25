/**
 * React hook for pdf-engine in the browser.
 *
 *   import { usePDFEngine } from '@lands-horizon-corp/pdf-engine-wasm/react';
 *
 *   function InvoiceButton({ invoice }) {
 *     const { download, busy, error } = usePDFEngine();
 *     return <button disabled={busy} onClick={() => download({ template, data: invoice }, 'invoice.pdf')}>Download</button>;
 *   }
 *
 * One engine is made on first use and closed when the component unmounts (React StrictMode's pretend unmount is
 * fine: it is made again on the next call). `busy` is true while a document is being made, `error` is the last
 * failure (a PDFEngineError) or null. The methods still reject, so you can also `try`/`catch`.
 */
import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { PDFEngine } from './index.js';

export function usePDFEngine(options) {
  const optionsRef = useRef(options);
  optionsRef.current = options;
  const engineRef = useRef(null);
  const running = useRef(0);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState(null);

  const engine = useCallback(() => (engineRef.current ??= new PDFEngine(optionsRef.current)), []);
  useEffect(
    () => () => {
      engineRef.current?.close();
      engineRef.current = null;
    },
    [],
  );

  const wrap = useCallback(
    (method) =>
      async (...args) => {
        running.current++;
        setBusy(true);
        setError(null);
        try {
          return await engine()[method](...args);
        } catch (e) {
          setError(e);
          throw e;
        } finally {
          running.current--;
          if (running.current === 0) setBusy(false);
        }
      },
    [engine],
  );

  return useMemo(
    () => ({
      /** Make a PDF: resolves with its bytes. */
      generate: wrap('generate'),
      /** Make a PDF and save it through the browser's download. */
      download: wrap('download'),
      /** Make a PDF and show it in a new tab. */
      open: wrap('open'),
      /** Make a PDF and open the browser's print dialog. */
      print: wrap('print'),
      /** Show a live preview (see `PDFEngine.preview`). */
      preview: wrap('preview'),
      /** Load the engine now, to hide the first-call delay. */
      ping: wrap('ping'),
      busy,
      error,
      /** Forget the last error. */
      clearError: () => setError(null),
    }),
    [wrap, busy, error],
  );
}
