import { useEffect, useState } from 'react';
import { createRoot } from 'react-dom/client';
import { usePDFEngine } from '@lands-horizon-corp/pdf-engine-wasm/react';
import { runSuite, publish, wasmUrl } from './suite.js';

function App() {
  const [result, setResult] = useState(null);
  const { generate, busy, error } = usePDFEngine({ wasm: wasmUrl });
  useEffect(() => {
    (async () => {
      const r = await runSuite();
      // The hook: a document, then an error, and busy comes back to false.
      const bytes = await generate({ template: '<p>hook {{ n }}</p>', data: { n: 1 } });
      const failure = await generate({ template: '<p>\n{% if %}' }).then(() => null, (e) => e.code);
      r.hook = { header: String.fromCharCode(...bytes.subarray(0, 5)), failure };
      setResult(r);
      publish(r);
    })();
  }, []);
  return (
    <div>
      <h1>React</h1>
      <pre id="result">{result ? JSON.stringify(result, null, 2) : 'running…'}</pre>
    </div>
  );
}

createRoot(document.getElementById('root')).render(<App />);
