//! Template rendering: the shared JSON view (`template::render`) against the previous
//! approach of deep-copying the data into minijinja values (`Serde`).

use criterion::{BatchSize, Criterion, criterion_group, criterion_main};
use minijinja::{AutoEscape, Environment, Value, value::Serde};
use pdf_engine::template;
use serde_json::json;

const TEMPLATE: &str = "<table>{% for r in rows %}<tr><td>{{ r.sku }}</td><td>{{ r.name }}</td>\
<td>{{ r.qty }}</td><td>{{ r.price }}</td></tr>{% endfor %}</table><p>{{ customer.name }}</p>";

fn data(rows: usize) -> serde_json::Value {
    let rows: Vec<_> = (0..rows)
        .map(|i| json!({ "sku": format!("SKU-{i}"), "name": "Item <name>", "qty": i % 9, "price": 12.5 }))
        .collect();
    json!({ "rows": rows, "customer": { "name": "Jane & Co" } })
}

fn baseline(env: &Environment<'static>, d: &serde_json::Value) -> String {
    env.render_str(TEMPLATE, Value::from(Serde(d))).unwrap()
}

fn bench(c: &mut Criterion) {
    let mut env = Environment::new();
    env.set_auto_escape_callback(|_| AutoEscape::Html);
    for rows in [10, 1_000, 100_000] {
        let d = data(rows);
        let mut g = c.benchmark_group(format!("template_{rows}_rows"));
        g.bench_function("shared_view", |b| {
            b.iter_batched(
                || d.clone(),
                |d| template::render(TEMPLATE, d).unwrap(),
                BatchSize::LargeInput,
            )
        });
        g.bench_function("deep_copy_baseline", |b| b.iter(|| baseline(&env, &d)));
        g.finish();
    }
}

criterion_group!(benches, bench);
criterion_main!(benches);
