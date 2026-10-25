// One page layout for the libraries that draw from code (pdfkit, pdf-lib, jsPDF), so they all draw exactly the
// same page. Pure JavaScript with no Node imports: the browser benchmark page uses it too.

export const PAGE = { w: 612, h: 792, margin: 32, row: 17, limit: 792 - 40 };

/** Pages of drawing operations (text, filled rectangles, lines) for `data`. `peso` is the currency sign. */
export function layout(data, peso) {
  const pages = [[]];
  let y = PAGE.margin;
  const ops = () => pages[pages.length - 1];
  const need = (h) => {
    if (y + h > PAGE.limit) {
      pages.push([]);
      y = PAGE.margin;
    }
  };
  const text = (t, x, w, align, o = {}) => ops().push({ k: 'text', t: String(t), x, y, w, align, bold: !!o.bold, size: o.size || 9, color: o.color || '#1f2937' });
  const rect = (x, w, h, color) => ops().push({ k: 'rect', x, y, w, h, color });
  const line = (y2) => ops().push({ k: 'line', x1: PAGE.margin, x2: PAGE.w - PAGE.margin, y: y2, color: '#e5e7eb' });

  text('Lands Horizon', 32, 300, 'left', { bold: true, size: 16, color: '#4f46e5' });
  text(data.title, 280, 300, 'right', { bold: true, size: 12 });
  y += 22;
  text('123 Ayala Ave, Makati', 32, 300, 'left');
  text(`#${data.no}`, 280, 300, 'right');
  y += 12;
  text('24 Oct 2026', 280, 300, 'right');
  y += 22;
  text('Billed to Acme Corp', 32, 300, 'left');
  y += 20;

  for (const s of data.sections) {
    if (s.heading) {
      need(PAGE.row * 3);
      text(s.heading, 32, 300, 'left', { bold: true, size: 12, color: '#3730a3' });
      y += 20;
    }
    need(PAGE.row * 2);
    rect(32, 548, PAGE.row, '#eef2ff');
    text('Description', 38, 300, 'left', { bold: true, color: '#3730a3' });
    text('Qty', 340, 40, 'right', { bold: true, color: '#3730a3' });
    text('Unit', 400, 70, 'right', { bold: true, color: '#3730a3' });
    text('Amount', 490, 84, 'right', { bold: true, color: '#3730a3' });
    y += PAGE.row;
    for (const r of s.rows) {
      need(PAGE.row);
      text(r.name, 38, 300, 'left');
      text(r.qty, 340, 40, 'right');
      text(`${peso}${r.price}`, 400, 70, 'right');
      text(`${peso}${r.qty * r.price}`, 490, 84, 'right');
      y += PAGE.row;
      line(y - 3);
    }
    if (data.sections.length > 1) {
      need(PAGE.row);
      text('Subtotal', 38, 300, 'left', { bold: true });
      text(`${peso}${s.subtotal}`, 490, 84, 'right', { bold: true });
      y += PAGE.row;
    }
  }
  need(30);
  y += 8;
  text(`Total ${peso}${data.total}`, 32, 548, 'right', { bold: true, size: 12 });
  return pages;
}

