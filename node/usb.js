'use strict';

/**
 * Printing straight to a USB device (vendor id and product id), without the operating system's print queue.
 *
 * Uses the optional `usb` package (WebUSB API, prebuilt for macOS, Linux and Windows):
 *     npm install usb
 *
 * Platform notes:
 *   Windows  the device needs a WinUSB driver (replace the printer's driver with Zadig, https://zadig.akeo.ie),
 *            which stops it appearing as a normal printer. Most people should use system://<name> instead.
 *   Linux    the user needs permission on the device (a udev rule), or run as root. A kernel `usblp` driver
 *            that holds the printer is detached for the job.
 *   macOS    works with the printer's default driver not installed or not claiming the device.
 */

const PRINTER_CLASS = 7;
/** Vendors of receipt printers that expose a vendor-specific interface instead of the printer class. */
const THERMAL_VENDORS = new Set([0x04b8, 0x0519, 0x0483, 0x0416, 0x28e9, 0x0dd4, 0x0fe6, 0x1504, 0x0525, 0x154f, 0x20d1, 0x6868]);
const CHUNK = 16 * 1024;

function hex4(n) {
  return n.toString(16).padStart(4, '0');
}

function loadUsb(requireFn = require) {
  try {
    const mod = requireFn('usb');
    return mod.usb || mod;
  } catch (e) {
    throw Object.assign(new Error('Printing straight to a USB device needs the `usb` package: npm install usb. (Or use the printer through the system: system://<name>.)'), { code: 'PRINT_FAILED' });
  }
}

/** The bulk OUT endpoint of a device's printer interface, as `{ interfaceNumber, endpointNumber }`. */
function findOutEndpoint(device) {
  const found = [];
  for (const config of device.configurations || (device.configuration ? [device.configuration] : [])) {
    for (const iface of config.interfaces || []) {
      for (const alt of iface.alternates || []) {
        const out = (alt.endpoints || []).find((e) => e.direction === 'out' && e.type === 'bulk');
        if (out) found.push({ configurationValue: config.configurationValue, interfaceNumber: iface.interfaceNumber, alternateSetting: alt.alternateSetting, endpointNumber: out.endpointNumber, printerClass: alt.interfaceClass === PRINTER_CLASS });
      }
    }
  }
  return found.find((f) => f.printerClass) || found[0] || null;
}

function isPrinter(device) {
  const out = findOutEndpoint(device);
  if (!out) return false;
  return out.printerClass || THERMAL_VENDORS.has(device.vendorId);
}

/** USB devices that look like printers, as `{ vendorId, productId, name, manufacturer }`. */
async function listUsbDevices({ usbModule } = {}) {
  const usb = usbModule || loadUsb();
  const devices = await usb.getDevices();
  return devices
    .filter(isPrinter)
    .map((d) => ({ vendorId: d.vendorId, productId: d.productId, name: d.productName || '', manufacturer: d.manufacturerName || '', serial: d.serialNumber || '' }));
}

/** Send bytes to a USB printer. */
async function sendUsb(vendorId, productId, bytes, { usbModule } = {}) {
  const usb = usbModule || loadUsb();
  const id = `${hex4(vendorId)}:${hex4(productId)}`;
  const device = (await usb.getDevices()).find((d) => d.vendorId === vendorId && d.productId === productId);
  if (!device) {
    throw Object.assign(new Error(`No USB device ${id} is connected.`), { code: 'PRINTER_UNREACHABLE' });
  }
  const target = findOutEndpoint(device);
  if (!target) throw Object.assign(new Error(`USB device ${id} has no printer endpoint.`), { code: 'PRINT_FAILED' });

  await device.open();
  let claimed = false;
  try {
    if (device.configuration == null || device.configuration.configurationValue !== target.configurationValue) {
      await device.selectConfiguration(target.configurationValue);
    }
    try {
      await device.claimInterface(target.interfaceNumber);
    } catch (e) {
      // Linux: the kernel's usblp driver holds the printer. Take it for this job.
      if (typeof device.detachKernelDriver !== 'function') throw e;
      await device.detachKernelDriver(target.interfaceNumber);
      await device.claimInterface(target.interfaceNumber);
    }
    claimed = true;
    const data = Buffer.from(bytes);
    for (let at = 0; at < data.length; at += CHUNK) {
      const result = await device.transferOut(target.endpointNumber, data.subarray(at, at + CHUNK));
      if (result && result.status && result.status !== 'ok') throw new Error(`USB transfer ${result.status}`);
    }
    return { bytes: data.length };
  } catch (e) {
    const hint = /access|permission|denied|NOT_SUPPORTED|busy/i.test(String(e.message)) ? ' (permission or driver: see the notes in node/usb.js)' : '';
    throw Object.assign(new Error(`USB ${id}: ${e.message}${hint}`), { code: 'PRINT_FAILED' });
  } finally {
    if (claimed) await device.releaseInterface(target.interfaceNumber).catch(() => {});
    await device.close().catch(() => {});
  }
}

module.exports = { findOutEndpoint, hex4, isPrinter, listUsbDevices, loadUsb, sendUsb };
