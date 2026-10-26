// The scale benchmark: every library renders up to 10,000 different documents per scenario, each in
// its own process, timed. (The memory of the whole process tree is sampled too, for the record.)   node scale.mjs [--count 10000] [--budget 150] [--scenarios small,large,multitable]
//                                           [--libs id,id] [--force]
// A run stops at `count` documents or after `budget` seconds. A library that hits the budget is reported with
// what it finished, and its time for `count` is a projection (marked as such).
import { execFileSync, spawn } from 'node:child_process';
import { existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { machine } from './stats.js';

const arg = (name, fallback) => {
  const i = process.argv.indexOf(`--${name}`);
  return i > 0 ? process.argv[i + 1] : fallback;
};
const count = Number(arg('count', 10000));
const budget = Number(arg('budget', 150));
const scenarioNames = arg('scenarios', 'small,large,multitable').split(',');
const all = ['pdf-engine', 'wasm', 'ironpress', 'dompdf', 'puppeteer-shell', 'puppeteer', 'html2pdf', 'jspdf-html2canvas', 'html-pdf', 'wkhtmltopdf'];
const libs = arg('libs', all.join(',')).split(',');
const force = process.argv.includes('--force');

const file = resolve(import.meta.dirname, 'results', 'scale.json');
mkdirSync(resolve(import.meta.dirname, 'results'), { recursive: true });
const saved = existsSync(file) && !force ? JSON.parse(readFileSync(file, 'utf8')) : { rows: [], skipped: [] };
const save = () => writeFileSync(file, JSON.stringify({ machine: machine(), count, budgetSeconds: budget, rows: saved.rows, skipped: saved.skipped }, null, 2));

/** Resident memory (MB) of `pid` and everything it started. Shared pages are counted per process. */
function treeRssMb(pid) {
  const rows = execFileSync('ps', ['-A', '-o', 'pid=,ppid=,rss='], { encoding: 'utf8' })
    .trim()
    .split('\n')
    .map((l) => l.trim().split(/\s+/).map(Number));
  const kids = new Map();
  for (const [p, pp, rss] of rows) kids.set(pp, [...(kids.get(pp) || []), [p, rss]]);
  const own = new Map(rows.map(([p, , rss]) => [p, rss]));
  let total = 0;
  const stack = [pid];
  while (stack.length) {
    const p = stack.pop();
    total += own.get(p) || 0;
    for (const [child] of kids.get(p) || []) stack.push(child);
  }
  return total / 1024;
}

function runOne(id, scenario) {
  return new Promise((done) => {
    const child = spawn(process.execPath, ['scale-worker.mjs', id, scenario, String(count), String(budget)], { cwd: import.meta.dirname, stdio: ['ignore', 'pipe', 'pipe'] });
    let out = '';
    let err = '';
    child.stdout.on('data', (d) => (out += d));
    child.stderr.on('data', (d) => (err += d));
    const samples = [];
    const timer = setInterval(() => {
      try {
        samples.push(treeRssMb(child.pid));
      } catch {}
    }, 250);
    const killer = setTimeout(() => child.kill('SIGKILL'), (budget + 180) * 1000);
    child.on('exit', () => {
      clearInterval(timer);
      clearTimeout(killer);
      const line = out.split('\n').find((l) => l.startsWith('RESULT '));
      if (!line) return done({ failed: (err || out).split('\n').filter((l) => l.trim() && !/access policy/.test(l)).slice(-2).join(' ').slice(0, 160) || 'no result' });
      const r = JSON.parse(line.slice(7));
      done({ ...r, peakMb: Math.max(...samples, 0), avgMb: samples.reduce((a, b) => a + b, 0) / (samples.length || 1) });
    });
  });
}

console.log(`Benchmark: up to ${count} documents per scenario, ${budget} s budget per run. Results: ${file}\n`);
for (const scenario of scenarioNames) {
  for (const id of libs) {
    if (saved.rows.some((r) => r.id === id && r.scenario === scenario)) continue;
    process.stdout.write(`${scenario.padEnd(11)} ${id.padEnd(18)} `);
    const started = Date.now();
    const r = await runOne(id, scenario);
    if (r.failed) {
      console.log(`not run: ${r.failed}`);
      if (!saved.skipped.some((s) => s.id === id)) saved.skipped.push({ id, reason: r.failed });
      save();
      continue;
    }
    r.perSecond = r.completed / r.seconds;
    r.projectedSeconds = r.completed ? (r.seconds * count) / r.completed : null;
    r.projected = r.completed < count;
    saved.rows.push(r);
    save();
    console.log(`${r.completed}/${count} in ${r.seconds.toFixed(1)} s (${r.perSecond.toFixed(0)}/s)${r.projected ? ` -> ${(r.projectedSeconds / 60).toFixed(1)} min for ${count} (projected)` : ''}, peak ${r.peakMb.toFixed(0)} MB, ${r.pages} pages${r.errors ? `, ${r.errors} errors: ${r.firstError}` : ''}  [${((Date.now() - started) / 1000).toFixed(0)} s]`);
  }
}
console.log('\ndone');
