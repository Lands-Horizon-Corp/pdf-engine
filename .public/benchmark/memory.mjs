// The memory benchmark: every library makes ONE PDF of 1, 100 and 1000 pages, each in a fresh process (cold, nothing
// warmed up), and the peak resident memory of that process and everything it started (Chromium, PhantomJS,
// wkhtmltopdf) is sampled every 20 ms, with the time to finish.
//   node memory.mjs [--scenarios pages1,pages100,pages1000] [--libs id,id] [--limit 600] [--force]
// A run that does not finish within `limit` seconds is stopped and reported as such, with the peak it reached.
import { execFileSync, spawn } from 'node:child_process';
import { existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { machine } from './stats.js';

const arg = (name, fallback) => {
  const i = process.argv.indexOf(`--${name}`);
  return i > 0 ? process.argv[i + 1] : fallback;
};
const limit = Number(arg('limit', 600));
const scenarioNames = arg('scenarios', 'pages1,pages100,pages1000').split(',');
const all = ['pdf-engine', 'wasm', 'ironpress', 'dompdf', 'puppeteer-shell', 'puppeteer', 'html2pdf', 'jspdf-html2canvas', 'html-pdf', 'wkhtmltopdf'];
const libs = arg('libs', all.join(',')).split(',');
const force = process.argv.includes('--force');

const file = resolve(import.meta.dirname, 'results', 'memory.json');
mkdirSync(resolve(import.meta.dirname, 'results'), { recursive: true });
const saved = existsSync(file) && !force ? JSON.parse(readFileSync(file, 'utf8')) : { rows: [] };
const save = () => writeFileSync(file, JSON.stringify({ machine: machine(), limitSeconds: limit, rows: saved.rows }, null, 2));

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
    const started = Date.now();
    const child = spawn(process.execPath, ['scale-worker.mjs', id, scenario, '1', String(limit)], {
      cwd: import.meta.dirname,
      stdio: ['ignore', 'pipe', 'pipe'],
      env: { ...process.env, WARMUP: '0' },
    });
    let out = '';
    let err = '';
    child.stdout.on('data', (d) => (out += d));
    child.stderr.on('data', (d) => (err += d));
    let peak = 0;
    let sampling = false;
    const timer = setInterval(() => {
      if (sampling) return;
      sampling = true;
      try {
        peak = Math.max(peak, treeRssMb(child.pid));
      } catch {}
      sampling = false;
    }, 20);
    let timedOut = false;
    const killer = setTimeout(() => {
      timedOut = true;
      child.kill('SIGKILL');
    }, limit * 1000);
    child.on('exit', () => {
      clearInterval(timer);
      clearTimeout(killer);
      const wall = (Date.now() - started) / 1000;
      const line = out.split('\n').find((l) => l.startsWith('RESULT '));
      if (timedOut) return done({ status: `did not finish in ${limit} s`, peakMb: peak, wallSeconds: wall });
      if (!line) return done({ status: (err || out).split('\n').filter((l) => l.trim() && !/access policy/.test(l)).slice(-2).join(' ').slice(0, 160) || 'no result', peakMb: peak, wallSeconds: wall });
      const r = JSON.parse(line.slice(7));
      if (!r.completed) return done({ ...r, status: r.firstError || 'failed', peakMb: peak, wallSeconds: wall });
      done({ ...r, status: 'ok', peakMb: peak, wallSeconds: wall });
    });
  });
}

console.log(`Memory benchmark: one PDF per run, ${limit} s limit. Results: ${file}\n`);
for (const scenario of scenarioNames) {
  for (const id of libs) {
    if (saved.rows.some((r) => r.id === id && r.scenario === scenario)) continue;
    process.stdout.write(`${scenario.padEnd(10)} ${id.padEnd(18)} `);
    const r = await runOne(id, scenario);
    saved.rows.push({ id, scenario, ...r });
    save();
    console.log(r.status === 'ok' ? `peak ${r.peakMb.toFixed(0)} MB, ${r.seconds.toFixed(2)} s render (${r.wallSeconds.toFixed(1)} s with startup), ${r.pages} pages, ${(r.kb / 1024).toFixed(2)} MB` : `${r.status} (peak ${r.peakMb.toFixed(0)} MB)`);
  }
}
console.log('\ndone');
