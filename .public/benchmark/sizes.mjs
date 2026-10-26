// What a browser app downloads for each library: its code (minified JavaScript) and any data files it ships
// (pdf-engine's WebAssembly module). Each library is bundled alone with Vite, then compressed with gzip and
// brotli like a real server does.   node sizes.mjs   (writes results/sizes.json)
//
// Fonts are not counted: html2pdf.js and jspdf-html2canvas draw the page as an image and need none;
// pdf-engine ships Noto Sans Regular and Bold (Latin, Cyrillic, Greek, Vietnamese) as separate files, and
// `defaultFonts: false` skips them.
import { build } from 'vite';
import { brotliCompressSync, constants, gzipSync } from 'node:zlib';
import { mkdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { machine } from './stats.js';

const here = import.meta.dirname;
const tmp = resolve(here, '.sizes');
rmSync(tmp, { recursive: true, force: true });
mkdirSync(tmp, { recursive: true });

const entries = {
  'pdf-engine (WebAssembly)': `import { PDFEngine } from '@lands-horizon-corp/pdf-engine-wasm'; window.x = PDFEngine;`,
  'ironpress': `import init, { HtmlConverter } from 'ironpress'; window.x = [init, HtmlConverter];`,
  'dompdf.js': `import dompdf from 'dompdf.js'; window.x = dompdf;`,
  'html2pdf.js': `import html2pdf from 'html2pdf.js'; window.x = html2pdf;`,
  'jspdf-html2canvas': `import h from 'jspdf-html2canvas'; window.x = h;`,
};

const brotli = (buf) => brotliCompressSync(buf, { params: { [constants.BROTLI_PARAM_QUALITY]: 9 } }).length;
const rows = [];
for (const [name, code] of Object.entries(entries)) {
  const entry = resolve(tmp, `${name.replace(/[^a-z0-9]+/gi, '_')}.js`);
  writeFileSync(entry, code);
  const result = await build({
    root: here,
    configFile: false,
    logLevel: 'silent',
    build: {
      write: false,
      minify: true,
      assetsInlineLimit: 0,
      lib: { entry, formats: ['es'], fileName: 'out' },
      rollupOptions: { output: { inlineDynamicImports: true } },
    },
  });
  const outputs = (Array.isArray(result) ? result : [result]).flatMap((r) => r.output);
  let js = Buffer.alloc(0);
  let assetBytes = 0;
  let assetBrotli = 0;
  for (const o of outputs) {
    // Library mode inlines files a page would fetch (the .wasm, fonts) as data: URLs; they are counted
    // separately below, so cut them out of the JavaScript.
    if (o.type === 'chunk') js = Buffer.concat([js, Buffer.from(o.code.replace(/data:[a-z/+.-]+;base64,[A-Za-z0-9+/=]+/g, 'data:'))]);
    else if (/\.(wasm)$/.test(o.fileName)) {
      const data = Buffer.from(o.source);
      assetBytes += data.length;
      assetBrotli += brotli(data);
    }
  }
  // The WebAssembly file is fetched separately (not bundled); count it as the browser downloads it.
  if (name.startsWith('pdf-engine')) {
    const wasm = readFileSync(resolve(here, '../../.node/wasm/dist/pdf_engine_wasm_bg.wasm'));
    assetBytes += wasm.length;
    assetBrotli += brotli(wasm);
  }
  if (name === 'ironpress') {
    const wasm = readFileSync(resolve(here, 'node_modules/ironpress/ironpress_bg.wasm'));
    assetBytes += wasm.length;
    assetBrotli += brotli(wasm);
  }
  // dompdf.js carries its WebAssembly inside the JavaScript (base64), so it is already counted above.
  rows.push({ library: name, jsRaw: js.length, jsGzip: gzipSync(js, { level: 9 }).length, jsBrotli: brotli(js), dataRaw: assetBytes, dataBrotli: assetBrotli });
}
rmSync(tmp, { recursive: true, force: true });

mkdirSync(resolve(here, 'results'), { recursive: true });
writeFileSync(resolve(here, 'results', 'sizes.json'), JSON.stringify({ machine: machine(), rows }, null, 2));
const kb = (n) => `${(n / 1024).toFixed(0)} KB`;
console.log('| Library | JavaScript (minified) | JavaScript, brotli | WebAssembly, brotli | Total download (brotli) |\n| --- | ---: | ---: | ---: | ---: |');
for (const r of rows) console.log(`| ${r.library} | ${kb(r.jsRaw)} | ${kb(r.jsBrotli)} | ${r.dataBrotli ? kb(r.dataBrotli) : 'none'} | ${kb(r.jsBrotli + r.dataBrotli)} |`);
