//! Render-path benchmarks.
//!
//!   cargo bench --bench render
//!
//! Prince benchmarks run only when `prince` is on PATH. Fulgur uses the fonts in
//! `FULGUR_FONT_DIR` (default `/usr/share/fonts/truetype/noto`), like the server.

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use flate2::{Compression, write::GzEncoder};
use pdf_engine::{
    fulgur_engine::{FulgurEngine, InProcessRenderer, PageMm},
    pdf, prince, router,
};
use std::{hint::black_box, io::Write, path::Path, process::Command};

fn fixture(name: &str) -> String {
    let p = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/css")
        .join(name);
    std::fs::read_to_string(p).unwrap()
}

fn documents() -> Vec<(&'static str, String)> {
    let rows: String = (0..300)
        .map(|i| {
            format!(
                "<tr><td>{i}</td><td>Item {i}</td><td>Line item {i}</td><td>{}.00</td></tr>",
                i * 7
            )
        })
        .collect();
    vec![
        (
            "tiny",
            "<!DOCTYPE html><html><body><h1>Hello</h1><p>World</p></body></html>".into(),
        ),
        ("flexbox", fixture("05_flexbox.html")),
        ("grid", fixture("06_grid.html")),
        ("invoice", fixture("08_invoice.html")),
        (
            "table_300_rows",
            format!("<table style=\"width:100%;border-collapse:collapse\">{rows}</table>"),
        ),
        (
            "cjk_system_fonts",
            "<p>你好世界 こんにちは 안녕하세요</p>".repeat(20),
        ),
    ]
}

fn font_dir() -> String {
    std::env::var("FULGUR_FONT_DIR").unwrap_or_else(|_| "/usr/share/fonts/truetype/noto".into())
}

fn bench_fulgur(c: &mut Criterion) {
    let engine = FulgurEngine::from_font_dir(Path::new(&font_dir()));
    let letter = PageMm::parse("8.5in", "11in", "portrait").unwrap();
    let mut g = c.benchmark_group("fulgur");
    for (name, html) in documents() {
        g.bench_with_input(BenchmarkId::from_parameter(name), &html, |b, html| {
            b.iter(|| engine.render(black_box(html), letter, None).unwrap())
        });
    }
    g.finish();
}

fn bench_prince(c: &mut Criterion) {
    let available = Command::new("prince")
        .arg("--version")
        .output()
        .is_ok_and(|o| o.status.success());
    if !available {
        eprintln!("prince not on PATH; skipping Prince benchmarks");
        return;
    }
    let rt = tokio::runtime::Runtime::new().unwrap();
    let mut g = c.benchmark_group("prince");
    g.sample_size(20);
    for (name, html) in documents() {
        g.bench_with_input(BenchmarkId::from_parameter(name), &html, |b, html| {
            b.to_async(&rt).iter(|| async {
                let raw = prince::render(html, "8.5in", "11in", "portrait", "prince")
                    .await
                    .unwrap();
                pdf::finalize(raw, true, None).await.unwrap()
            })
        });
    }
    g.finish();
}

fn bench_pipeline(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let invoice = fixture("08_invoice.html");
    let engine = FulgurEngine::from_font_dir(Path::new(&font_dir()));
    let letter = PageMm::parse("8.5in", "11in", "portrait").unwrap();
    let pdf_bytes = engine
        .render("<h1>Invoice</h1><p>body</p>", letter, None)
        .unwrap();

    c.bench_function("router/invoice", |b| {
        b.iter(|| router::prince_reason(black_box(&invoice)))
    });

    let template = "<table>{% for r in rows %}<tr><td>{{ r.name }}</td><td>{{ r.price }}</td></tr>{% endfor %}</table>";
    let data = serde_json::json!({
        "rows": (0..500).map(|i| serde_json::json!({ "name": format!("Item ₱ {i}"), "price": i })).collect::<Vec<_>>()
    });
    c.bench_function("template/500_rows", |b| {
        b.to_async(&rt)
            .iter(|| pdf::render_template(template.to_string(), data.clone()))
    });

    c.bench_function("finalize/passthrough", |b| {
        b.to_async(&rt)
            .iter(|| pdf::finalize(pdf_bytes.clone(), false, None))
    });
    c.bench_function("finalize/encrypt", |b| {
        b.to_async(&rt)
            .iter(|| pdf::finalize(pdf_bytes.clone(), false, Some("secret".into())))
    });

    let mut g = c.benchmark_group("gzip");
    g.throughput(Throughput::Bytes(pdf_bytes.len() as u64));
    for level in [1, 6, 9] {
        g.bench_with_input(BenchmarkId::new("level", level), &level, |b, &level| {
            b.iter(|| {
                let mut e = GzEncoder::new(Vec::new(), Compression::new(level));
                e.write_all(black_box(&pdf_bytes)).unwrap();
                e.finish().unwrap()
            })
        });
    }
    g.finish();
}

criterion_group!(benches, bench_fulgur, bench_prince, bench_pipeline);
criterion_main!(benches);
