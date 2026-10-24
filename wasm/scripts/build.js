#!/usr/bin/env node
// Builds the WebAssembly module and its JavaScript glue into dist/.
//
//   npm run build
//
// Needs (once):  rustup target add wasm32-unknown-unknown
//                cargo install wasm-bindgen-cli --version <the wasm-bindgen version in rust/Cargo.lock>
// Optional:      wasm-opt (binaryen) for a smaller file.
import { spawnSync } from 'node:child_process';
import { brotliCompressSync, gzipSync, constants } from 'node:zlib';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const crate = path.join(root, 'rust');
const dist = path.join(root, 'dist');

// Prefer rustup's tools (they carry the wasm32 target) over a Homebrew/system Rust.
const extraPath = [path.join(os.homedir(), '.cargo', 'bin'), '/opt/homebrew/opt/rustup/bin'].filter(fs.existsSync);
const env = { ...process.env, PATH: [...extraPath, process.env.PATH].join(path.delimiter) };

function run(cmd, args, opts = {}) {
  const r = spawnSync(cmd, args, { stdio: 'inherit', env, ...opts });
  if (r.error?.code === 'ENOENT') fail(`\`${cmd}\` was not found.`);
  if (r.status !== 0) process.exit(r.status ?? 1);
}
function fail(message) {
  console.error(`\nbuild failed: ${message}`);
  process.exit(1);
}

const wanted = /name = "wasm-bindgen"\nversion = "([^"]+)"/.exec(fs.readFileSync(path.join(crate, 'Cargo.lock'), 'utf8'))?.[1];
const have = spawnSync('wasm-bindgen', ['--version'], { env, encoding: 'utf8' });
if (have.error) fail(`wasm-bindgen is not installed. Run: cargo install wasm-bindgen-cli --version ${wanted} --locked`);
if (wanted && !have.stdout.includes(wanted)) {
  fail(`wasm-bindgen ${have.stdout.trim()} does not match the crate (${wanted}). Run: cargo install wasm-bindgen-cli --version ${wanted} --locked --force`);
}

run('cargo', ['build', '--release', '--target', 'wasm32-unknown-unknown'], { cwd: crate });

const built = path.join(crate, 'target', 'wasm32-unknown-unknown', 'release', 'pdf_engine_wasm.wasm');
fs.rmSync(dist, { recursive: true, force: true });
run('wasm-bindgen', ['--target', 'web', '--out-dir', dist, built]);

const wasm = path.join(dist, 'pdf_engine_wasm_bg.wasm');
if (spawnSync('wasm-opt', ['--version'], { env }).status === 0) {
  run('wasm-opt', ['-Oz', '--enable-bulk-memory', '--enable-nontrapping-float-to-int', '--enable-sign-ext', '--enable-mutable-globals', wasm, '-o', wasm]);
} else {
  console.log('(wasm-opt not found: skipping the extra size optimisation)');
}

const bytes = fs.readFileSync(wasm);
const mb = (n) => (n / 1048576).toFixed(2);
const brotli = brotliCompressSync(bytes, { params: { [constants.BROTLI_PARAM_QUALITY]: 9 } }).length;
console.log(`\ndist/pdf_engine_wasm_bg.wasm: ${mb(bytes.length)} MB raw, ${mb(gzipSync(bytes, { level: 9 }).length)} MB gzip, ${mb(brotli)} MB brotli`);
