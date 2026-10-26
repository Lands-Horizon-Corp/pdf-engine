#!/usr/bin/env node
// Builds the WebAssembly module and its JavaScript glue into dist/.
//
//   npm run build
//
// Needs (once):  rustup target add wasm32-unknown-unknown
//                cargo install wasm-bindgen-cli --version <the wasm-bindgen version in rust/Cargo.lock>
// Optional:      wasm-opt (binaryen) for speed (-O3).
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

// The default fonts live in .public/assets/fonts (one copy for the whole repository); the npm package carries its own.
const fonts = path.join(root, 'fonts');
fs.mkdirSync(fonts, { recursive: true });
for (const file of ['NotoSans-Regular.ttf', 'NotoSans-Bold.ttf', 'OFL.txt']) {
  fs.copyFileSync(path.join(root, '..', '..', '.public', 'assets', 'fonts', file), path.join(fonts, file));
}

const built = path.join(crate, 'target', 'wasm32-unknown-unknown', 'release', 'pdf_engine_wasm.wasm');
fs.rmSync(dist, { recursive: true, force: true });
run('wasm-bindgen', ['--target', 'web', '--out-dir', dist, built]);

// After a trap (a crash inside WebAssembly) the instance cannot be used again, and the generated init() returns
// the old one. Add a function that forgets it, so the next init() makes a fresh instance.
const glue = path.join(dist, 'pdf_engine_wasm.js');
let glueSource = fs.readFileSync(glue, 'utf8');
for (const needle of ['let wasmModule, wasmInstance, wasm;', 'let cachedDataViewMemory0 = null;', 'let cachedUint8ArrayMemory0 = null;', 'let heap = new Array(1024).fill(undefined);', 'let heap_next = heap.length;', 'let WASM_VECTOR_LEN = 0;']) {
  if (!glueSource.includes(needle)) fail(`wasm-bindgen's output changed (missing "${needle}"); update the reset() patch in scripts/build.js.`);
}
glueSource += `
/** pdf-engine: forget the instance, so the next init() creates a fresh one (the old one is unusable after a trap). */
export function reset() {
    wasmModule = wasmInstance = wasm = undefined;
    cachedDataViewMemory0 = null;
    cachedUint8ArrayMemory0 = null;
    heap = new Array(1024).fill(undefined);
    heap_next = heap.length;
    WASM_VECTOR_LEN = 0;
}
`;
// An engine made before a reset must never be freed in the new instance (its pointer means something else there).
const registryOld = 'new FinalizationRegistry(ptr => wasm.__wbg_engine_free(ptr, 1))';
const registerOld = 'EngineFinalization.register(this, this.__wbg_ptr, this);';
if (!glueSource.includes(registryOld) || !glueSource.includes(registerOld)) fail("wasm-bindgen's output changed (the finalizer); update the patch in scripts/build.js.");
glueSource = glueSource
  .replace(registryOld, 'new FinalizationRegistry(held => { if (held.gen === instanceGeneration && wasm) wasm.__wbg_engine_free(held.ptr, 1); })')
  .replace(registerOld, 'EngineFinalization.register(this, { ptr: this.__wbg_ptr, gen: instanceGeneration }, this);')
  .replace('let wasmModule, wasmInstance, wasm;', 'let wasmModule, wasmInstance, wasm;\nlet instanceGeneration = 0;')
  .replace('export function reset() {\n', 'export function reset() {\n    instanceGeneration++;\n');
fs.writeFileSync(glue, glueSource);
fs.appendFileSync(path.join(dist, 'pdf_engine_wasm.d.ts'), '\n/** Forget the WebAssembly instance (after a trap); the next `init()` makes a fresh one. */\nexport function reset(): void;\n');

const wasm = path.join(dist, 'pdf_engine_wasm_bg.wasm');
if (spawnSync('wasm-opt', ['--version'], { env }).status === 0) {
  run('wasm-opt', ['-O3', '--enable-simd', '--enable-multivalue', '--enable-reference-types', '--enable-bulk-memory', '--enable-nontrapping-float-to-int', '--enable-sign-ext', '--enable-mutable-globals', wasm, '-o', wasm]);
} else {
  console.log('(wasm-opt not found: skipping the extra speed optimisation)');
}

const bytes = fs.readFileSync(wasm);
const mb = (n) => (n / 1048576).toFixed(2);
const brotli = brotliCompressSync(bytes, { params: { [constants.BROTLI_PARAM_QUALITY]: 9 } }).length;
console.log(`\ndist/pdf_engine_wasm_bg.wasm: ${mb(bytes.length)} MB raw, ${mb(gzipSync(bytes, { level: 9 }).length)} MB gzip, ${mb(brotli)} MB brotli`);
