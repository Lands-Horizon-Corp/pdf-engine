export const median = (xs) => [...xs].sort((a, b) => a - b)[Math.floor(xs.length / 2)];
export const p95 = (xs) => [...xs].sort((a, b) => a - b)[Math.min(xs.length - 1, Math.floor(xs.length * 0.95))];
export const fmt = (ms) => (ms < 10 ? ms.toFixed(1) : Math.round(ms).toString());

/** Characters of selectable text in a PDF (0 for PDFs that are just pictures). */
export async function textChars(bytes) {
  const pdfjs = await import('pdfjs-dist/legacy/build/pdf.mjs');
  const doc = await pdfjs.getDocument({ data: new Uint8Array(bytes), useSystemFonts: false, verbosity: 0 }).promise;
  let chars = 0;
  for (let i = 1; i <= doc.numPages; i++) {
    const page = await doc.getPage(i);
    chars += (await page.getTextContent()).items.reduce((n, it) => n + (it.str?.length || 0), 0);
  }
  return { chars, pages: doc.numPages };
}

import os from 'node:os';
/** What the numbers were measured on, saved next to them. */
export const machine = () => ({
  date: new Date().toISOString().slice(0, 10),
  cpu: os.cpus()[0]?.model,
  cores: os.cpus().length,
  platform: `${os.platform()} ${os.arch()}`,
  node: process.version,
});
