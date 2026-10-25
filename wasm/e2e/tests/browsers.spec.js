import { expect, test } from '@playwright/test';

for (const app of ['plain', 'react', 'vue']) {
  test(`${app}: renders PDFs in the browser`, async ({ page }) => {
    const problems = [];
    page.on('pageerror', (e) => problems.push(`pageerror: ${e.message}`));
    // The suite makes one request that the server rejects on purpose (a template error, HTTP 400); the
    // browser logs that as a failed resource. Anything else on the console is a real problem.
    page.on('console', (m) => {
      if (m.type() === 'error' && !/Failed to load resource.*\b400\b/.test(m.text())) problems.push(`console.error: ${m.text()}`);
    });

    await page.goto(`/${app}/`);
    await page.waitForFunction(() => window.__result, null, { timeout: 90_000 });
    const r = await page.evaluate(() => window.__result);

    expect(r.fatal, 'the suite must not fail').toBeUndefined();
    expect(r.invoice.header).toBe('%PDF-');
    expect(r.invoice.bytes).toBeGreaterThan(2000);
    expect(r.invoice.latinFont, 'bundled Noto Sans Regular is embedded').toBe(true);
    expect(r.invoice.boldFont, 'bundled Noto Sans Bold is embedded').toBe(true);
    expect(r.encrypted, 'a password encrypts the PDF').toBe(true);
    expect(r.receiptPage, '80 mm x 200 mm page size').toBe(true);
    expect(r.image, 'a data: URI image is embedded').toBe(true);
    expect(r.error).toMatchObject({ code: 'TEMPLATE_ERROR', status: 400, line: 2, isEngineError: true });
    expect(r.remote, 'rendering on a server from another origin (CORS)').toBe('%PDF-');
    expect(r.remoteError).toBe('TEMPLATE_ERROR');
    expect(r.repeated).toBe(20);
    expect(problems).toEqual([]);
  });
}
