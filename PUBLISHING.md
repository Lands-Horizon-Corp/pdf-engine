# Before publishing

A checklist for releasing the browser package (`@lands-horizon-corp/pdf-engine-wasm`) to npm, which also puts
it on the CDNs (jsDelivr, unpkg), with fresh benchmarks. Do the steps in order; each has a command and what
"good" looks like.

The browser package is published (1.2.0 is on npm, jsDelivr and unpkg). Not covered: the Node package (`@lands-horizon-corp/pdf-engine`) is **not published yet**. It needs the
native binary built for each platform first (macOS, Linux, Windows; x64 and arm64). Do not run
`npm publish` in `.node/desktop/` until that exists.

## 0. Once

- An npm account that belongs to the `@lands-horizon-corp` organisation, with 2FA on.
- `npm login` and `npm whoami` show that account.
- Rust with the `wasm32-unknown-unknown` target, `wasm-bindgen-cli` at the version in
  `.node/wasm/rust/Cargo.lock`, and (optional, for speed) `wasm-opt` from binaryen. `.node/wasm/README.md` has the commands.
- For releasing from GitHub (step 4b): an npm token of type **granular**, permission **Read and write (publish and
  stage)** (not "stage only"), with **Bypass two-factor authentication (2FA)** ticked, for all packages or the
  `@lands-horizon-corp` scope. Without the bypass the job fails with `EOTP`; a stage-only token cannot publish.
- Playwright browsers for the checks: `cd .node/wasm/e2e && npx playwright install chromium webkit firefox`.

## 1. The version

The version is in four places and they should match (the engine reports the one in `Cargo.toml`):

| File | Field |
| --- | --- |
| `Cargo.toml` | `version` |
| `.node/wasm/rust/Cargo.toml` | `version` |
| `.node/wasm/package.json` | `version` |
| `.node/desktop/package.json` | `version` |

Follow [Semantic Versioning](https://semver.org/): new features = minor, fixes = patch. Then move the
`[Unreleased]` entries of `CHANGELOG.md` under the new version and date.

The demo page (`.public/demo/index.html`) pins the package from the CDN in its `VERSION` constant: change it to the
new version once it is on npm (deploys to GitHub Pages on push).

The README on the npm page is the one inside the published version, so a change to a README or the docs reaches npm
only with a new version (a patch release).

The links in the docs that pin a version (`@1`, `@1.1.1` in `.node/wasm/README.md`, `README.md`) should point at the
release you are about to make.

## 2. Tests

All of these must pass. They take a few minutes.

```sh
cargo clippy --all-targets                       # no warnings in src/ and tests/
cargo test                                       # the engine, IPC, errors, CORS, languages, images
cd .node/wasm && npm run build && npm test             # the browser package (builds the .wasm first)
cd ../desktop && node --test                        # the Node client (needs `cargo build` first)
cd ../wasm/e2e && npm install && npx playwright test    # Chromium, WebKit, Firefox: plain JS, React, Vue
npm run test:cdn                                 # a plain HTML page importing the package like a CDN does
```

Firefox may not start on some Macs; CI runs it on Linux. A failing browser on your machine is fine only if
CI is green for it.

### Printing needs a real printer

The printing code is tested against fake printers (a TCP socket, a stand-in for `lp`) and the receipt bitmap
and ESC/POS/ZPL bytes are checked exactly, but nobody has yet printed on hardware from this repository. Before
the **Node** package is published, print on at least:

- a receipt printer by network (`net://host:9100`) and through the system queue (`system://...`) on macOS or
  Linux, and on **Windows** (the raw-print PowerShell path has never run on Windows);
- an office printer with a PDF (`type: 'document'`) on each system;
- a USB receipt printer directly (`usb://vendor:product`), on Windows with a WinUSB
  driver and on Linux with a udev rule;
- a label printer, if ZPL is advertised for that release.

Look for: the right width (no cut-off at the right edge for `58mm` / `80mm`), no blank metres of paper after the
content, the cut, and ₱ and non-Latin text printing correctly.

## 3. What goes into the package

```sh
cd .node/wasm && npm pack --dry-run
```

Expected: `index.js`, `routing.js`, `index.d.ts`, `dist/pdf_engine_wasm.js`, `dist/pdf_engine_wasm_bg.wasm`,
`fonts/` (two Noto Sans files and their licence) and `README.md`, about 5 MB compressed. If a file that
`index.js` imports is missing, the CDN import fails, so check `routing.js` is in the list. Delete the
`.tgz` it leaves behind.

## 4. Benchmarks

The README charts and tables come from `.public/benchmark/results/*.json`. Refresh them on the release commit, with
nothing else running (close Chrome and other heavy programs, plug in the laptop). See
[`.public/benchmark/README.md`](.public/benchmark/README.md) for what each one measures.

```sh
cargo build --release && (cd .node/wasm && npm run build)     # the code being measured
make bench-setup                                         # once: tools and browsers
make bench-check                                         # every library makes the same document
make bench                                               # per document: Chromium, WebKit, Node (about 10 min)
caffeinate -i make bench-scale                           # 10,000 documents each (about an hour; macOS)
make bench-charts                                        # redraws .public/benchmark/charts and the README tables
```

Then read the results before trusting them:

- `bench-check` shows the same page count and selectable text for every library.
- In the README tables, any library that did not finish is marked **projected**.
- Only HTML-to-PDF packages are measured (pdf-engine, Puppeteer, html2pdf.js, jspdf-html2canvas, html-pdf,
  wkhtmltopdf). `make bench-setup` installs PhantomJS and wkhtmltopdf; on Apple Silicon they run under Rosetta and the
  README says so. Losses are shown as they are.
  Do not edit a number by hand; if the README text claims something the charts no longer show, change the text.
- `git diff --stat .public/benchmark/results README.md .public/benchmark/charts` should show only the numbers and charts changing.

## 4b. The quick way: let GitHub publish it

`.github/workflows/release.yml` builds, tests and publishes the browser package to npm **and** the Docker image
to Docker Hub (amd64 and arm64) when you push a version tag. Once, add three repository secrets (Settings >
Secrets and variables > Actions): `NPM_TOKEN` (an npm Automation token for an account in the
`@lands-horizon-corp` organisation), `DOCKERHUB_USERNAME` and `DOCKERHUB_TOKEN`. Then, after steps 1 to 4:

```sh
git tag v<version> && git push origin v<version>
```

A plain version becomes npm's `latest`; a brand-new package can take a few minutes to appear after the job ends (npm may
show a `0.0.0-stage` placeholder meanwhile); a suffix (`v1.2.0-rc.1`) is published as `next`. The job stops if the tag,
`.node/wasm/package.json` and `Cargo.toml` disagree. Watch it under the repository's Actions tab, then do step 6
(check it from the CDN). The manual way is steps 5 and 8.

## 5. Publish to npm (by hand)

This is what worked for 1.2.0 when the token in GitHub was not allowed to bypass 2FA. First as a pre-release tag, so
nobody gets it by accident (or drop `--tag next` to publish `latest` directly):

```sh
cd .node/wasm
npm run build                       # prepublishOnly also runs build and tests
npm publish --tag next              # asks for your 2FA code
```

A published version can never be replaced, only superseded. If something is wrong, publish the next patch
version, and mark the bad one with `npm deprecate @lands-horizon-corp/pdf-engine-wasm@<version> "use <next>"`.

## 6. Check it from the CDN

jsDelivr and unpkg serve every npm package within a minute or two. Use an exact version in the URL:

```
https://cdn.jsdelivr.net/npm/@lands-horizon-corp/pdf-engine-wasm@<version>/index.js
https://unpkg.com/@lands-horizon-corp/pdf-engine-wasm@<version>/index.js
```

1. Open `https://cdn.jsdelivr.net/npm/@lands-horizon-corp/pdf-engine-wasm@<version>/` and check the file list
   matches step 3.
2. Save the snippet from the README ("Plain HTML, Vanilla JS, jQuery") as `test.html`, serve it
   (`npx serve .`; modules do not load from `file://`) and click the button in Chrome and Safari. A PDF should
   download.
3. In a browser console on that page: `await new (await import(url)).PDFEngine().ping()` returns the version.
4. A fresh React or Vue app: `npm install @lands-horizon-corp/pdf-engine-wasm@next`, call `generate()`.

Use the `/index.js` path. Do not use jsDelivr's `+esm` form or `esm.sh`: they rewrite the files and may leave
the `.wasm` behind. Avoid an unpinned `@latest` link in docs: jsDelivr caches it for up to 12 hours (purge with
`https://purge.jsdelivr.net/npm/<package>@<tag>/index.js` if you must).

## 7. Make it the default

When step 6 passes:

```sh
npm dist-tag add @lands-horizon-corp/pdf-engine-wasm@<version> latest
```

Then commit and tag:

```sh
git add -A && git commit -m "release: v<version>"
git tag v<version> && git push origin master --tags
```

## 8. The Docker image (the server)

If the server changed in this release (it did if `src/` or `vendor/` changed), publish the image too:

```sh
make release-image                  # multi-arch: <version>, <major.minor>, <major>, latest
```

The package's `url` option talks to this image, so release them together.

## If something goes wrong

| Problem | Fix |
| --- | --- |
| `npm publish` says the scope is not found or forbidden | You are not in the `@lands-horizon-corp` org, or not logged in: `npm whoami`, `npm org ls @lands-horizon-corp` |
| `E403 ... cannot publish over the previously published versions` | Bump the version; a version number can be used once |
| The CDN shows the old files | Use an exact version; wait a minute; purge the CDN cache |
| The page loads but `generate()` fails with `WASM_LOAD_FAILED` | The `.wasm` was not served as `application/wasm`, or it is not in the package: check steps 3 and 6 |
| A bad version was published | Publish a fixed patch version; `npm deprecate` the bad one. Within 72 hours `npm unpublish` is possible, but breaks anyone who pinned it |
