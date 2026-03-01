// --- Global Styling ---
#set page(
  paper: "a4",
  margin: (x: 1.5cm, y: 2cm),
  header: align(right)[#text(size: 8pt, gray)[e-Coop Suite | Electronic Statement]],
  footer: [
    #line(length: 100%, stroke: 0.5pt + gray)
    #grid(
      columns: (1fr, 1fr),
      [#text(size: 8pt)[Generated on: 2026-03-01]],
      align(right)[#text(size: 8pt)[Page #counter(page).display()]]
    )
  ]
)

#set text(font: "Liberation Sans", size: 10pt)

// --- Header ---
#grid(
  columns: (1fr, 1fr),
  [
    #text(size: 18pt, weight: "bold", fill: blue.darken(20%))[VALDECO] \
    #text(size: 9pt, gray)[Valenzuela Development Cooperative] \
    #text(size: 8pt)[Poblacion, Valenzuela City, Philippines]
  ],
  align(right)[
    #text(size: 14pt, weight: "bold")[MEMBER STATEMENT] \
    #text(size: 10pt)[Account No: #strong("1029384756")]
  ]
)

#v(1cm)

// --- Summary Box ---
#rect(
  fill: gray.lighten(90%),
  inset: 12pt,
  radius: 4pt,
  width: 100%,
  stack(
    dir: ltr,
    spacing: 1fr,
    [#text(gray)[Starting Balance] \ #strong("₱ 50,250.00")],
    [#text(gray)[Total Credits] \ #text(fill: green.darken(20%))[₱ 12,500.00]],
    [#text(gray)[Total Debits] \ #text(fill: red.darken(20%))[₱ 2,150.00]],
    [#text(gray)[Current Balance] \ #strong("₱ 60,600.00")]
  )
)

#v(5mm)

// --- Transaction Table ---
#table(
  columns: (auto, 1fr, auto, auto, auto),
  inset: 8pt,
  align: (center, left, center, right, right),
  stroke: none,
  fill: (_, row) => if row == 0 { blue.darken(40%) } else if calc.even(row) { gray.lighten(95%) },
  
  // Header Row
  [*Date*], [*Description*], [*Ref*], [*Amount*], [*Balance*],
  
  // Data Rows (Text in Typst is very clean)
  [2026-02-01], [Opening Balance], [-], [-], [50,250.00],
  [2026-02-05], [Share Capital Deposit], [TXN-882], [+ 5,000.00], [55,250.00],
  [2026-02-12], [Loan Amortization Payment], [LN-441], [- 2,150.00], [53,100.00],
  [2026-02-28], [Dividend Credit], [DIV-2025], [+ 7,500.00], [60,600.00],
)

#v(1cm)

#align(center)[
  #text(size: 8pt, italic: true, gray)[This is a computer-generated document. No signature is required.]
]