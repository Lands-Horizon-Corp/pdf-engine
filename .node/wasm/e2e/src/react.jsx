import { useEffect, useState } from 'react';
import { createRoot } from 'react-dom/client';
import { runSuite, publish } from './suite.js';

function App() {
  const [result, setResult] = useState(null);
  useEffect(() => {
    runSuite().then((r) => {
      setResult(r);
      publish(r);
    });
  }, []);
  return (
    <div>
      <h1>React</h1>
      <pre id="result">{result ? JSON.stringify(result, null, 2) : 'running…'}</pre>
    </div>
  );
}

createRoot(document.getElementById('root')).render(<App />);
