# Parity with Chrome

How close does pdf-engine look to headless Chrome, page by page? The score is on the [demo page](https://lands-horizon-corp.github.io/pdf-engine/)
and is made by one command:

```sh
node .public/parity/parity.mjs                   # run, write the report and the demo's parity.json
node .public/parity/parity.mjs --check           # exit 1 unless HEALTHY (for CI)
node .public/parity/parity.mjs --update-baseline # accept today's scores as the new baseline
```

It needs the puppeteer install in `.public/benchmark` (`npm ci` there). The report, with a Chrome | pdf-engine | difference
image for every case, is `.public/parity/out/report.html`.

## What is compared

Every HTML file in `tests/fixtures/css` (20 documents), `tests/fixtures/probes` (64 one-feature pages) and
`.public/parity/cases` (right-to-left and mixed-direction pages) is made into a PDF by Chrome (Letter, no margin,
backgrounds on, `@page` size respected) and by pdf-engine. **Both PDFs are drawn by the same rasteriser** (hayro, at 96 dpi),
so the difference is in the PDFs, not in the viewer.

## The score

- **Layout score** (the headline, 0 to 100): the pages are cut into 6 mm squares and the mean colour of each square is
  compared. A square counts only if it has ink in either page. A few pixels of drift in a line of text average out; a missing
  image, a wrong background or text on the wrong side does not. **A different page count scores 0** for that case.
- **Pixel score** (shown beside it, stricter): pixels that still differ after forgiving a one-pixel shift. Text that sits
  3 px lower than Chrome's loses points here and not in the layout score.
- A case **passes** at 90 or more, is **close** at 70 or more, and **fails** below. The overall score is the mean of the cases.
- **Health**: `HEALTHY` is 85 or more, no errors and no case more than 2 points below `baseline.json`. `DEGRADED` is 60 to 85
  or a regression. `BROKEN` is below 60.

No tuning per case, no alignment of the two pages before comparing, and the thresholds above are fixed.

## What it does not tell you

- The reference is Chrome, not the CSS specification. Where Chrome is wrong, matching it is still a loss here.
- Fonts differ: Chrome uses the system fonts, pdf-engine its bundled ones, so text-heavy pages never reach 100.
- A mirrored right-to-left line can score high on the layout score because the filled area is the same. The exact order of
  runs is checked by `tests/bidi.rs`, not here.
- 88 cases are a sample, not the whole of CSS.

## Where it stands (28 October 2026, pdf-engine 1.4.1 plus the patches below, Chrome 156)

Score **87.86, HEALTHY**: 65 pass, 11 close, 12 fail (of 88). It was 80.3 at the first run.
What moved it:

- **Both sides use standards mode.** Several fixtures have no `<!doctype>`, which Chrome renders in quirks mode (it drops the
  first element's top margin). The driver now adds a doctype to both renderers. This is most of the first jump, and it is a
  change to the comparison, not to the engine. pdf-engine has no quirks mode.
- **`dir` and `direction: rtl` now set a paragraph's direction** (patched parley and blitz-dom), and a flex row flips in an rtl box.
- **An `inline-block` with text now sits on its text baseline** (badges and buttons used to be about 70 px too low).

Still failing: `<img>` of an SVG, `outline`, a span with only a space (its background is not drawn), `transform` and `opacity`
pages, `column-rule`, `::before`/`::after` blocks, a long table across pages, and `@page { size }`, which is not applied (the
page size comes from the request).
