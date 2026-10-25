// Installs what the benchmark needs besides npm packages: PhantomJS (for html-pdf) and wkhtmltopdf, from their
// official downloads, without administrator rights.   node setup-tools.mjs
//
// Both are abandoned projects with only Intel builds for macOS. On an Apple Silicon Mac they run under Rosetta 2, which
// is slower than native; the benchmark README says so next to the numbers.
import { execFileSync } from 'node:child_process';
import { chmodSync, createWriteStream, existsSync, mkdirSync, readdirSync, rmSync, writeFileSync } from 'node:fs';
import { createRequire } from 'node:module';
import os from 'node:os';
import { resolve } from 'node:path';
import { pipeline } from 'node:stream/promises';
import { phantomPath, toolsDir, wkhtmltopdfPath } from './tools.mjs';

const require = createRequire(import.meta.url);
const download = async (url, file) => {
  const res = await fetch(url, { redirect: 'follow' });
  if (!res.ok) throw new Error(`${url}: ${res.status}`);
  await pipeline(res.body, createWriteStream(file));
};
const run = (cmd, args, opts = {}) => execFileSync(cmd, args, { stdio: 'inherit', ...opts });

// ---- PhantomJS ------------------------------------------------------------------------------------------------
async function phantom() {
  if (phantomPath()) return console.log('PhantomJS: found');
  let pkg;
  try {
    pkg = resolve(require.resolve('phantomjs-prebuilt/package.json'), '..');
  } catch {
    return console.log('PhantomJS: run `npm install` first (phantomjs-prebuilt is a dependency)');
  }
  // Its own installer first; on a new Node it can fail while unpacking, so unpack the download ourselves.
  try {
    run(process.execPath, [resolve(pkg, 'install.js')], { stdio: 'ignore' });
  } catch {}
  if (phantomPath()) return console.log('PhantomJS: installed');
  const platform = { darwin: 'macosx', linux: 'linux-x86_64', win32: 'windows' }[process.platform];
  if (!platform) return console.log(`PhantomJS: no build for ${process.platform}`);
  const zip = resolve(os.tmpdir(), 'phantomjs', `phantomjs-2.1.1-${platform}.${platform === 'linux-x86_64' ? 'tar.bz2' : 'zip'}`);
  if (!existsSync(zip)) {
    mkdirSync(resolve(zip, '..'), { recursive: true });
    await download(`https://github.com/Medium/phantomjs/releases/download/v2.1.1/${zip.split('/').pop()}`, zip);
  }
  const dest = resolve(pkg, 'lib', 'phantom');
  mkdirSync(dest, { recursive: true });
  if (zip.endsWith('.zip')) run('unzip', ['-q', '-o', zip, '-d', dest]);
  else run('tar', ['-xjf', zip, '-C', dest]);
  const folder = readdirSync(dest).find((d) => d.startsWith('phantomjs-2.1.1'));
  const bin = resolve(dest, folder, 'bin', `phantomjs${process.platform === 'win32' ? '.exe' : ''}`);
  if (process.platform !== 'win32') chmodSync(bin, 0o755);
  writeFileSync(resolve(pkg, 'lib', 'location.js'), `module.exports = { location: ${JSON.stringify(bin)}, platform: ${JSON.stringify(process.platform)}, arch: 'x64' };\n`);
  console.log(phantomPath() ? 'PhantomJS: installed' : 'PhantomJS: could not be installed');
}

// ---- wkhtmltopdf ----------------------------------------------------------------------------------------------
async function wkhtmltopdf() {
  try {
    execFileSync(wkhtmltopdfPath(), ['--version'], { stdio: 'ignore' });
    return console.log('wkhtmltopdf: found');
  } catch {}
  const target = resolve(toolsDir, 'wkhtmltox');
  if (process.platform === 'darwin') {
    const tmp = resolve(os.tmpdir(), 'wkhtmltox-setup');
    rmSync(tmp, { recursive: true, force: true });
    mkdirSync(tmp, { recursive: true });
    await download('https://github.com/wkhtmltopdf/packaging/releases/download/0.12.6-2/wkhtmltox-0.12.6-2.macos-cocoa.pkg', resolve(tmp, 'wk.pkg'));
    run('pkgutil', ['--expand-full', resolve(tmp, 'wk.pkg'), resolve(tmp, 'out')]);
    mkdirSync(target, { recursive: true });
    run('tar', ['-xzf', resolve(tmp, 'out/Payload/usr/local/share/wkhtmltox-installer/wkhtmltox.tar.gz'), '-C', target, '--strip-components', '0']);
    rmSync(tmp, { recursive: true, force: true });
    console.log('wkhtmltopdf: installed in', target);
  } else if (process.platform === 'linux') {
    console.log('wkhtmltopdf: install it with your package manager (apt install wkhtmltopdf) or set WKHTMLTOPDF=/path/to/wkhtmltopdf');
  } else {
    console.log('wkhtmltopdf: install it from https://wkhtmltopdf.org/downloads.html or set WKHTMLTOPDF=C:\\path\\wkhtmltopdf.exe');
  }
}

await phantom();
await wkhtmltopdf();
