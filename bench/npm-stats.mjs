// Live npm facts for the packages people reach for when they search "pdf".   node npm-stats.mjs
const packages = [
  'puppeteer', 'jspdf', 'pdfkit', 'pdfmake', 'pdf-lib', '@react-pdf/renderer', 'html2pdf.js', 'puppeteer-core',
  'html-pdf', 'wkhtmltopdf', 'jspdf-html2canvas', 'pdf-creator-node', 'html-pdf-node', 'vue-html2pdf',
  'html-export-pdf-cli', 'pdf-node', '@adobe/pdfservices-node-sdk',
];

const ago = (iso) => {
  const days = (Date.now() - new Date(iso)) / 864e5;
  return days < 45 ? `${Math.round(days)} days ago` : days < 700 ? `${Math.round(days / 30)} months ago` : `${Math.round(days / 365)} years ago`;
};

const rows = [];
for (const name of packages) {
  try {
    const meta = await (await fetch(`https://registry.npmjs.org/${name.replace('/', '%2f')}`, { headers: { accept: 'application/vnd.npm.install-v1+json' } })).json();
    const latest = meta['dist-tags'].latest;
    const dl = await (await fetch(`https://api.npmjs.org/downloads/point/last-week/${name}`)).json();
    rows.push({ name, version: latest, weekly: dl.downloads, modified: meta.modified });
  } catch (e) {
    rows.push({ name, error: String(e.message) });
  }
}
rows.sort((a, b) => (b.weekly || 0) - (a.weekly || 0));
console.log('| Package | Weekly downloads | Version | Updated |\n| --- | ---: | --- | --- |');
for (const r of rows) {
  console.log(r.error ? `| ${r.name} | n/a | | ${r.error} |` : `| ${r.name} | ${r.weekly.toLocaleString('en-US')} | ${r.version} | ${ago(r.modified)} |`);
}
console.log(`\n(npm registry, ${new Date().toISOString().slice(0, 10)})`);
