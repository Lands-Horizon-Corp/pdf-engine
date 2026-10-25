// Where the two older HTML-to-PDF programs are: PhantomJS (used by html-pdf) and the wkhtmltopdf binary.
// `node setup-tools.mjs` installs them next to this folder when they are missing.
import { existsSync } from 'node:fs';
import { createRequire } from 'node:module';
import { resolve } from 'node:path';

const require = createRequire(import.meta.url);
const here = import.meta.dirname;
export const toolsDir = resolve(here, 'tools');
const exe = process.platform === 'win32' ? '.exe' : '';

/** The wkhtmltopdf executable: $WKHTMLTOPDF, then tools/wkhtmltox/bin, then the PATH. Null if there is none. */
export function wkhtmltopdfPath() {
  const candidates = [process.env.WKHTMLTOPDF, resolve(toolsDir, 'wkhtmltox', 'bin', `wkhtmltopdf${exe}`)].filter(Boolean);
  const found = candidates.find((p) => existsSync(p));
  if (found) return found;
  return 'wkhtmltopdf'; // the PATH; a missing program is reported when it is started
}

/** The PhantomJS executable html-pdf uses (from phantomjs-prebuilt), or null. */
export function phantomPath() {
  try {
    const path = require('phantomjs-prebuilt').path;
    return path && existsSync(path) ? path : null;
  } catch {
    return null;
  }
}
