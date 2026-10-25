#!/usr/bin/env node
'use strict';

// Builds the Rust engine and copies it to bin/<platform>-<arch>/, where the client looks first.
//   npm run build:native            release build for this machine
//   npm run build:native -- --debug debug build (faster to compile)
const { spawnSync } = require('node:child_process');
const fs = require('node:fs');
const path = require('node:path');

const debug = process.argv.includes('--debug');
const root = path.resolve(__dirname, '..', '..');
const exe = process.platform === 'win32' ? 'pdf-engine.exe' : 'pdf-engine';

const build = spawnSync('cargo', ['build', ...(debug ? [] : ['--release'])], { cwd: root, stdio: 'inherit' });
if (build.status !== 0) process.exit(build.status ?? 1);

const from = path.join(process.env.CARGO_TARGET_DIR || path.join(root, 'target'), debug ? 'debug' : 'release', exe);
const dir = path.resolve(__dirname, '..', 'bin', `${process.platform}-${process.arch}`);
fs.mkdirSync(dir, { recursive: true });
fs.copyFileSync(from, path.join(dir, exe));
fs.chmodSync(path.join(dir, exe), 0o755);
console.log(`copied ${from} -> ${path.join(dir, exe)}`);
