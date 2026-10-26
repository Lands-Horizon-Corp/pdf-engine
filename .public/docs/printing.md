# Printing

pdf-engine finds printers and prints to them: **receipt** (thermal) printers, **label** printers and **office**
printers. It works in Electron and Node (`printers()` and `print()`), in Tauri and any other program (the
`pdf-engine printers` and `pdf-engine print` commands), and not in a web page (a browser cannot see printers; the
browser package's `print()` opens the browser's print dialog instead).

```ts
import { PDFEngine } from '@lands-horizon-corp/pdf-engine';

const pdf = new PDFEngine();

const printers = await pdf.printers();   // the ones the system knows, and USB receipt and label printers
// [{ id: 'system://POS-58', name: 'POS-58', type: 'thermal', status: 'ready', transport: 'system', default: false },
//  { id: 'system://HP_LaserJet_P1102', name: 'HP LaserJet P1102', type: 'document', status: 'ready', ... }]
// A network printer is not in that list (it cannot be discovered): print to it by its address.
// await pdf.print({ printerId: 'net://192.168.1.150:9100', paper: '80mm', template, data });

await pdf.print({
  printerId: 'system://POS-58',
  paper: '58mm',
  template: '<b>STORE</b>{% for i in items %}<div>{{ i.name }} ₱{{ i.price }}</div>{% endfor %}<b>TOTAL ₱{{ total }}</b>',
  data: { items: [{ name: 'Sardines', price: 24.5 }], total: 24.5 },
});
```

## What each kind of printer gets

| Type | Printer | The engine sends |
| --- | --- | --- |
| `thermal` | Receipt printer (58, 80 or 112 mm roll) | The receipt as a picture in **ESC/POS** commands, a paper feed and a cut |
| `label` | Label and barcode printer (Zebra and compatible) | The label as a **ZPL** graphic |
| `document` | Office laser or inkjet printer | The **PDF** |

A receipt is rendered like any other template (so all of [CSS](templates.md#css), fonts, ₱ and non-Latin text work),
rasterised at 203 dpi (the resolution of almost every receipt printer), trimmed of blank paper at the top and bottom,
and encoded. The picture approach means it prints the same on every printer model; it is not the printer's own text mode.

The type is guessed from the printer's name (`POS-58`, `TM-T82`, `TSP143`, `RP326`, `XP-80C` are receipt printers;
`Zebra`, `TSC`, `Label`, `Brother QL` are label printers; everything else is an office printer). Pass `type` to override.

## Printer ids

| Id | How it reaches the printer | Notes |
| --- | --- | --- |
| `system://<name>` | The operating system's print queue (CUPS `lp` on macOS and Linux, the Windows spooler) | Works for any printer with its driver installed: USB, Bluetooth and network. The simplest choice. |
| `net://<host>[:<port>]` | A TCP socket straight to the printer (port 9100 unless given) | Most network receipt and label printers. Cannot be discovered: pass the id to `print()` yourself. |
| `usb://<vendor>:<product>` | The USB device directly (hexadecimal ids, like `usb://0483:5740`) | Receipt and label printers only. See [USB](#usb) below. |

`printers()` returns, for each: `id`, `name`, `type`, `status` (`ready`, `busy`, `offline` or `unknown`), `transport`
(`system`, `net` or `usb`), `default` (the system's default printer) and, when known, `uri` (where the system says it is
connected).

## `print(options)`

| Option | Meaning |
| --- | --- |
| `printerId` (required) | From `printers()`, or a `net://` / `usb://` id. |
| `template` (required), `data` | The document, as in [Templates](templates.md). |
| `type` | `'thermal' \| 'label' \| 'document'`, instead of the guess. |
| `paper` | Roll width for `thermal` and `label`: `'58mm'` (384 dots), `'80mm'` (576, the default), `'112mm'` (832), or the printable width in dots (8 to 4096). |
| `language` | `'escpos'` or `'zpl'`. Default: `escpos` for `thermal`, `zpl` for `label`. |
| `cut` | Cut the paper after printing (ESC/POS; default `true`). |
| `feed` | Blank lines to feed before the cut (ESC/POS; default 3). |
| `dither` | Spread grey into dots, for photos and logos (default `false`; text is sharper without it). |
| `copies` | 1 to 100 (default 1). |
| `width`, `height`, `orientation` | Page size, for `document` printers (default Letter). |
| `images` | As in [`generate()`](desktop-api.md#generateoptions-promisebuffer). |

It resolves with `{ printerId, type, language, bytes, copies }`: what was sent and how much.

A receipt has no fixed length: the page is made tall, the blank paper below the content is cut off, so a 3-line and a
300-line receipt both use only the paper they need. Set `height` (for example `'400mm'`) to cap it.

## Printing from Tauri, scripts and other languages

The same engine runs from the command line. The request is JSON with the fields above:

```sh
pdf-engine printers                                                    # all printers, as JSON
pdf-engine printers --json '{"printers":[{"id":"net://192.168.1.150:9100"}]}'

pdf-engine print --request job.json                                    # the request in a file
pdf-engine print --request - < job.json                                # or on standard input
pdf-engine print --json '{"printerId":"net://192.168.1.150:9100","paper":"80mm","template":"<b>Hello</b>"}'
```

The answer is one line of JSON on standard output; the exit status is 0 when `"ok": true` and 1 otherwise:

```json
{ "ok": true, "op": "print", "printerId": "net://192.168.1.150:9100", "type": "thermal", "language": "escpos", "bytes": 8377, "copies": 1 }
{ "ok": false, "error": { "code": "PRINTER_UNREACHABLE", "error": "Could not reach the printer: ...", "status": 502, "hint": "..." } }
```

Tauri runs it as a sidecar: [Tauri guide](sample-tauri.md#printing-receipt-label-and-office-printers). Images in a
command-line job must be `data:` URIs (or hosts you allow with `ASSET_ALLOWED_HOSTS`).

## USB

The simple way is the operating-system queue: add the printer (on Windows the "Generic / Text Only" driver, on macOS and
Linux a raw queue) and use its `system://` id.

To talk to the device directly with `usb://<vendor>:<product>`, nothing extra is installed; `printers()` lists the receipt
and label printers it sees. Find the ids in System Information (macOS), `lsusb` (Linux) or Device Manager > Details >
Hardware Ids (Windows: `VID_0483&PID_5740` is `usb://0483:5740`).

- **Windows:** the device needs a WinUSB driver (replace its printer driver with [Zadig](https://zadig.akeo.ie)), after which it
  no longer appears as a normal printer. Most Windows apps should use `system://` instead.
- **Linux:** the user needs permission on the device (a udev rule); a kernel `usblp` driver holding the printer is detached for the job.
- **macOS:** works when the system driver is not claiming the device.

## Windows notes

- A receipt printer should use a **"Generic / Text Only"** driver (or its maker's raw driver) so the commands reach it
  unchanged. A driver that "renders" the job (an inkjet or PDF driver) would print the command bytes as text.
- Office printers print the PDF silently with **SumatraPDF** when it is on the `PATH`; otherwise the default PDF app's
  "print to" command is used, which depends on that app.
- Listing needs PowerShell, which every supported Windows has.

## Errors

| Code | Meaning | Try |
| --- | --- | --- |
| `PRINTER_UNREACHABLE` (502) | A network printer did not answer, or a USB device is not connected | Check the address, cable and power; network printers listen on 9100 |
| `PRINT_FAILED` (502) | The operating system queue or the USB device refused the job | The message has the system's reason; check paper, driver and permissions |
| `INVALID_FIELD` (400) | Bad `printerId`, `paper`, `type` or `language`; a `usb://` id with `type: 'document'` | The message says which |
| `MISSING_FIELD` (400) | No `printerId` or no `template` | |

More: [Errors](errors.md). Verified so far: the output bytes against a fake network printer on Linux, macOS and Windows,
and a real print to a virtual receipt printer through the Windows spooler. Physical printers have not been tried yet, so
report what your model does.
