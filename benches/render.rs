//! Render-path benchmarks.
//!
//!   cargo bench --bench render
//!
//! Fonts come from fontconfig, like the server: set `FONTCONFIG_FILE=fonts/fonts.conf`
//! (as the Docker image does) to benchmark the production font set.

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use flate2::{Compression, write::GzEncoder};
use pdf_engine::{
    fulgur_engine::{FulgurEngine, InProcessRenderer, PageMm, RenderJob},
    pdf,
};
use std::{hint::black_box, io::Write, path::Path};

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
        ("cjk", "<p>你好世界 こんにちは 안녕하세요</p>".repeat(20)),
    ]
}

fn job(html: &str) -> RenderJob {
    RenderJob {
        html: html.to_string(),
        page: PageMm::parse("8.5in", "11in", "portrait").unwrap(),
        title: None,
        images: Vec::new(),
    }
}

fn bench_fulgur(c: &mut Criterion) {
    let engine = FulgurEngine::default();
    let mut g = c.benchmark_group("fulgur");
    for (name, html) in documents() {
        let job = job(&html);
        g.bench_with_input(BenchmarkId::from_parameter(name), &job, |b, job| {
            b.iter(|| engine.render(black_box(job)).unwrap())
        });
    }
    g.finish();
}

fn bench_pipeline(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let engine = FulgurEngine::default();
    let pdf_bytes = engine.render(&job("<h1>Invoice</h1><p>body</p>")).unwrap();

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
            .iter(|| pdf::finalize(pdf_bytes.clone(), None))
    });
    c.bench_function("finalize/encrypt", |b| {
        b.to_async(&rt)
            .iter(|| pdf::finalize(pdf_bytes.clone(), Some("secret".into())))
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

criterion_group!(benches, bench_fulgur, bench_pipeline);
criterion_main!(benches);
