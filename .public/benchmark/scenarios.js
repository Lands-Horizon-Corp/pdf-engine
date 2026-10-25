// The documents of the benchmark: HTML every library turns into a PDF. Each document has its own invoice number.

const css = `
  body { font-family: 'Noto Sans', Arial, sans-serif; font-size: 12px; color: #1f2937; margin: 0; padding: 32px; }
  .top { display: flex; justify-content: space-between; border-bottom: 3px solid #4f46e5; padding-bottom: 10px; }
  .brand { font-size: 22px; font-weight: 700; color: #4f46e5; }
  h3 { margin: 18px 0 0; color: #3730a3; }
  table { width: 100%; border-collapse: collapse; margin-top: 8px; }
  th { background: #eef2ff; color: #3730a3; text-align: left; padding: 3px 8px; }
  td { padding: 3px 8px; border-bottom: 1px solid #e5e7eb; }
  tr { page-break-inside: avoid; }
  .r { text-align: right; }
  .sub td { font-weight: 700; }
  .total { margin-top: 12px; text-align: right; font-size: 15px; font-weight: 700; }
`;

const rows = (count, offset = 0) =>
  Array.from({ length: count }, (_, i) => ({
    name: `Item ${offset + i + 1} · service line`,
    qty: ((offset + i) % 5) + 1,
    price: 100 + (((offset + i) * 37) % 900),
  }));

/** @param {{title:string, sections:{heading?:string, rows:object[]}[]}} shape */
function scenario(name, label, shape) {
  const sections = shape.sections.map((s) => ({ ...s, subtotal: s.rows.reduce((n, r) => n + r.qty * r.price, 0) }));
  const total = sections.reduce((n, s) => n + s.subtotal, 0);
  return {
    name,
    label,
    rows: sections.reduce((n, s) => n + s.rows.length, 0),
    /** The document with its own number (`i`), as HTML. */
    render(i) {
      const no = `2026-${String(i).padStart(6, '0')}`;
      const body = `
        <div class="top"><div><div class="brand">Lands Horizon</div>123 Ayala Ave, Makati</div>
          <div class="r"><b style="font-size:16px">${shape.title}</b><br>#${no}<br>24 Oct 2026</div></div>
        <p>Billed to <b>Acme Corp</b></p>
        ${sections
          .map(
            (s) => `${s.heading ? `<h3>${s.heading}</h3>` : ''}<table><tr><th>Description</th><th class="r">Qty</th><th class="r">Unit</th><th class="r">Amount</th></tr>
            ${s.rows.map((r) => `<tr><td>${r.name}</td><td class="r">${r.qty}</td><td class="r">₱${r.price}</td><td class="r">₱${r.qty * r.price}</td></tr>`).join('')}
            ${sections.length > 1 ? `<tr class="sub"><td colspan="3">Subtotal</td><td class="r">₱${s.subtotal}</td></tr>` : ''}</table>`,
          )
          .join('')}
        <div class="total">Total ₱${total}</div>`;
      return { css, body, html: `<!DOCTYPE html><html><head><meta charset="utf-8"><style>${css}</style></head><body>${body}</body></html>` };
    },
  };
}

export const scenarios = {
  small: scenario('small', 'Small: a receipt (8 lines, 1 page)', { title: 'RECEIPT', sections: [{ rows: rows(8) }] }),
  large: scenario('large', 'Large: one invoice of 300 lines (about 8 pages)', { title: 'INVOICE', sections: [{ rows: rows(300) }] }),
  multitable: scenario('multitable', 'Multi-table: 5 tables of 35 lines with subtotals (about 5 pages)', {
    title: 'STATEMENT',
    sections: Array.from({ length: 5 }, (_, k) => ({ heading: `Section ${k + 1}`, rows: rows(35, k * 35) })),
  }),
};

// The per-document benchmark (benchmark.md): a receipt and a 30-line invoice.
scenarios.receipt = scenarios.small;
scenarios.invoice = scenario('invoice', 'Invoice (30 lines, 1 page)', { title: 'INVOICE', sections: [{ rows: rows(30) }] });
