// Renders one document of each scenario with every library and prints what came out, so you can
// see that they all made the same document before trusting any timing.   node check.mjs [id...]
import { mkdirSync, writeFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { driverIds, loadAll } from './libs.mjs';
import { scenarios } from './scenarios.js';
import { textChars } from './stats.js';

const ids = process.argv.slice(2).length ? process.argv.slice(2) : driverIds;
const { ok, skipped } = await loadAll(ids);
mkdirSync(resolve(import.meta.dirname, 'out'), { recursive: true });
console.log('| Library | Scenario | Pages | Text characters | Size (KB) | Time (ms) |\n| --- | --- | ---: | ---: | ---: | ---: |');
for (const lib of ok) {
  const handle = await lib.make();
  for (const name of ['small', 'large', 'multitable']) {
    const started = performance.now();
    try {
      const bytes = await lib.render(handle, scenarios[name].render(1));
      const ms = performance.now() - started;
      const t = await textChars(bytes);
      writeFileSync(resolve(import.meta.dirname, 'out', `check-${lib.id}-${name}.pdf`), bytes);
      console.log(`| ${lib.name} | ${name} | ${t.pages} | ${t.chars} | ${(bytes.length / 1024).toFixed(0)} | ${ms.toFixed(0)} |`);
    } catch (e) {
      console.log(`| ${lib.name} | ${name} | FAILED: ${String(e.message).split('\n')[0].slice(0, 70)} | | | |`);
    }
  }
  await lib.close(handle);
}
for (const s of skipped) console.log(`\nnot available here: ${s.id}: ${s.reason.slice(0, 120)}`);
