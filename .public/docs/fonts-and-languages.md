# Fonts and languages

Text in practically any living language renders, as long as a font with that script is available. Where fonts come from
depends on where the engine runs.

| Where | Fonts |
| --- | --- |
| **Docker server** | About 150 Noto fonts (curated list in `.public/assets/fonts/fonts.conf`), plus emoji, symbols and math |
| **Browser package** | Noto Sans Regular and Bold: Latin (including all European accents and Vietnamese), Cyrillic and Greek. Anything else: add a font |
| **Node and Electron** | The operating system's fonts, plus any you add with the `fonts` option |
| **Command line and sidecar** | The operating system's fonts |

## Scripts the server image covers

- **Latin, Greek, Cyrillic** (and Vietnamese), plus serif and monospace designs
- **Chinese** (simplified and traditional), **Japanese**, **Korean**
- **Arabic, Urdu, Hebrew, Thaana, Syriac**, all right-to-left
- **Indic:** Devanagari (Hindi, Marathi, Nepali), Bengali, Tamil, Telugu, Kannada, Malayalam, Gujarati, Gurmukhi
  (Punjabi), Sinhala, Oriya and more
- **South-East Asian:** Thai, Lao, Khmer, Myanmar, Javanese, Balinese, Sundanese and more
- **Others:** Ethiopic, Georgian, Armenian, Tibetan, Mongolian, Cherokee, Canadian Aboriginal, N'Ko, Tifinagh, Vai,
  Adlam, Yi and more
- **Color emoji, flags, symbols and math**

If the font named in your CSS lacks a character, the engine falls back to a font that has it. A character no font has
shows as an empty box; the cure is to add a font that covers it.

## Choosing a font in the template

```html
<style>
  body { font-family: 'Noto Sans', sans-serif; }
  .mono { font-family: monospace; }
</style>
<p>₱1,500 · Привет · Γειά σου</p>
```

`'Noto Sans', sans-serif` looks the same everywhere. `@font-face` is not supported: add fonts with the `fonts` option
(below) and use their family name.

## Adding fonts

**Browser package:** URLs (fetched once), bytes, or `File`/`Blob`:

```ts
const pdf = new PDFEngine({ fonts: ['/fonts/NotoSansSC-Regular.otf', '/fonts/NotoNaskhArabic-Regular.ttf'] });
await pdf.loadFonts(['/fonts/NotoSansJP-Regular.otf'])   // replace the extra fonts later
```

Serve the font files from your site (Google's Noto fonts are free, SIL Open Font License). A browser has no fonts the
engine can use, so non-Latin text needs one. Keep to the scripts you need: a CJK font is several megabytes.

**Node and Electron:** files or folders:

```ts
const pdf = new PDFEngine({ fonts: [path.join(process.resourcesPath, 'fonts')] });
await pdf.loadFonts(['/path/to/Inter.ttf']);   // replaces the earlier set
await pdf.reloadFonts();                        // after installing a font on the system
```

**Server:** copy the font into `/usr/share/fonts/pdf-engine` (in the image: mount a volume there), then
`POST /api/fonts/reload`, or run with `FONT_WATCH=true` and it reloads when the folder changes.

## Notes

- Fonts are loaded once per process and shared between documents, so adding fonts costs memory, not render time.
- Right-to-left scripts (Arabic, Urdu, Hebrew, Thaana, Syriac) are laid out right to left.
- The test `tests/languages.rs` renders about 30 scripts; it runs where the curated font set is installed (the Docker
  test image).
