//! Load / stress tests against a running pdf-engine instance.
//!
//! These are `#[ignore]`d so a normal `cargo test` doesn't hammer the server.
//! Start the server (ideally `cargo run --release`), then:
//!
//!   cargo test --release --test stress_tests -- --ignored --nocapture --test-threads=1
//!
//! Tunables (env vars):
//!   STRESS_URL          base url              (default http://localhost:6767)
//!   STRESS_REQUESTS     requests per scenario (default 200)
//!   STRESS_CONCURRENCY  in-flight requests    (default 32)
//!   STRESS_MAX_P99_MS   fail if p99 exceeds   (default: no limit)

#[cfg(test)]
mod tests {
    use futures_util::{StreamExt, stream};
    use reqwest::{Client, StatusCode, multipart};
    use std::{
        collections::BTreeMap,
        env,
        time::{Duration, Instant},
    };

    struct Config {
        base_url: String,
        token: String,
        requests: usize,
        concurrency: usize,
        max_p99: Option<Duration>,
    }

    fn config() -> Config {
        dotenvy::dotenv().ok();
        let parse = |k: &str, d: usize| env::var(k).ok().and_then(|v| v.parse().ok()).unwrap_or(d);
        Config {
            base_url: env::var("STRESS_URL").unwrap_or_else(|_| "http://localhost:6767".into()),
            token: env::var("API_BEARER_TOKEN").expect("API_BEARER_TOKEN must be set for tests"),
            requests: parse("STRESS_REQUESTS", 200),
            concurrency: parse("STRESS_CONCURRENCY", 32),
            max_p99: env::var("STRESS_MAX_P99_MS")
                .ok()
                .and_then(|v| v.parse().ok())
                .map(Duration::from_millis),
        }
    }

    fn client(concurrency: usize) -> Client {
        Client::builder()
            .pool_max_idle_per_host(concurrency)
            .timeout(Duration::from_secs(90))
            .build()
            .unwrap()
    }

    struct Report {
        name: &'static str,
        latencies: Vec<Duration>,
        statuses: BTreeMap<String, usize>,
        wall: Duration,
    }

    impl Report {
        fn pct(&self, p: f64) -> Duration {
            if self.latencies.is_empty() {
                return Duration::ZERO;
            }
            let idx = ((self.latencies.len() as f64 - 1.0) * p).round() as usize;
            self.latencies[idx]
        }

        fn count(&self, status: StatusCode) -> usize {
            *self.statuses.get(status.as_str()).unwrap_or(&0)
        }

        fn total(&self) -> usize {
            self.latencies.len()
        }

        fn print(&self) {
            let rps = self.total() as f64 / self.wall.as_secs_f64();
            println!(
                "\n[{}] {} reqs in {:.2?} => {:.1} req/s\n  p50 {:.2?} | p90 {:.2?} | p99 {:.2?} | max {:.2?}\n  statuses: {:?}",
                self.name,
                self.total(),
                self.wall,
                rps,
                self.pct(0.50),
                self.pct(0.90),
                self.pct(0.99),
                self.pct(1.0),
                self.statuses,
            );
        }

        fn assert_all(&self, expected: StatusCode, max_p99: Option<Duration>) {
            self.print();
            assert_eq!(
                self.count(expected),
                self.total(),
                "[{}] not every request returned {}: {:?}",
                self.name,
                expected,
                self.statuses
            );
            if let Some(limit) = max_p99 {
                assert!(
                    self.pct(0.99) <= limit,
                    "[{}] p99 {:?} exceeded limit {:?}",
                    self.name,
                    self.pct(0.99),
                    limit
                );
            }
        }
    }

    /// Fire `total` requests with at most `concurrency` in flight.
    /// `check` validates a successful body; failures are recorded as "bad-body".
    async fn run_load<F, C>(
        name: &'static str,
        client: &Client,
        url: &str,
        token: &str,
        total: usize,
        concurrency: usize,
        make_form: F,
        check: C,
    ) -> Report
    where
        F: Fn(usize) -> multipart::Form,
        C: Fn(&[u8]) -> bool,
    {
        let start = Instant::now();
        let results: Vec<(Duration, String)> = stream::iter(0..total)
            .map(|i| {
                let req = client.post(url).bearer_auth(token).multipart(make_form(i));
                let check = &check;
                async move {
                    let t = Instant::now();
                    let label = match req.send().await {
                        Ok(resp) => {
                            let status = resp.status();
                            match resp.bytes().await {
                                Ok(b) if status.is_success() && !check(&b) => "bad-body".into(),
                                Ok(_) => status.as_str().to_string(),
                                Err(_) => "body-error".into(),
                            }
                        }
                        Err(e) if e.is_timeout() => "client-timeout".into(),
                        Err(_) => "conn-error".into(),
                    };
                    (t.elapsed(), label)
                }
            })
            .buffer_unordered(concurrency)
            .collect()
            .await;
        let wall = start.elapsed();

        let mut latencies = Vec::with_capacity(results.len());
        let mut statuses = BTreeMap::new();
        for (d, s) in results {
            latencies.push(d);
            *statuses.entry(s).or_insert(0) += 1;
        }
        latencies.sort_unstable();
        Report {
            name,
            latencies,
            statuses,
            wall,
        }
    }

    fn is_pdf(b: &[u8]) -> bool {
        b.starts_with(b"%PDF")
    }

    fn is_s3_success(b: &[u8]) -> bool {
        serde_json::from_slice::<serde_json::Value>(b)
            .map(|j| j["status"] == "success" && j["file_size"].as_i64().unwrap_or(0) > 0)
            .unwrap_or(false)
    }

    /// Server must still answer a plain request correctly after a load scenario.
    async fn assert_healthy(cfg: &Config) {
        let resp = client(1)
            .post(format!("{}/api/to-bytes", cfg.base_url))
            .bearer_auth(&cfg.token)
            .multipart(multipart::Form::new().text("template", "<p>health</p>"))
            .send()
            .await
            .expect("server unreachable after load");
        assert_eq!(resp.status(), 200, "server unhealthy after load");
        assert!(is_pdf(&resp.bytes().await.unwrap()));
    }

    #[tokio::test(flavor = "multi_thread")]
    #[ignore]
    async fn stress_to_bytes_small() {
        let cfg = config();
        let c = client(cfg.concurrency);
        let report = run_load(
            "to-bytes/small",
            &c,
            &format!("{}/api/to-bytes", cfg.base_url),
            &cfg.token,
            cfg.requests,
            cfg.concurrency,
            |i| {
                multipart::Form::new()
                    .text("template", "<h1>Hello {{ name }} #{{ n }}</h1>")
                    .text("data", format!(r#"{{"name":"stress","n":{i}}}"#))
                    .text("filename", format!("stress_{i}.pdf"))
            },
            is_pdf,
        )
        .await;
        report.assert_all(StatusCode::OK, cfg.max_p99);
        assert_healthy(&cfg).await;
    }

    /// Big table, loops, and lots of non-ASCII (exercises template render + entity escaping).
    #[tokio::test(flavor = "multi_thread")]
    #[ignore]
    async fn stress_to_bytes_heavy_template() {
        let cfg = config();
        let c = client(cfg.concurrency);
        let rows: Vec<_> = (0..500)
            .map(|i| serde_json::json!({ "id": i, "name": format!("Item ₱€¥ 你好 🚀 #{i}"), "price": i as f64 * 1.25 }))
            .collect();
        let data = serde_json::json!({ "title": "Invoice — ✅", "rows": rows }).to_string();
        let template = r#"<html><body class="invoice"><h1>{{ title }}</h1>
            <table>{% for r in rows %}<tr><td>{{ r.id }}</td><td>{{ r.name }}</td><td>{{ r.price }}</td></tr>{% endfor %}</table>
            </body></html>"#;

        let requests = (cfg.requests / 4).max(1);
        let report = run_load(
            "to-bytes/heavy",
            &c,
            &format!("{}/api/to-bytes", cfg.base_url),
            &cfg.token,
            requests,
            cfg.concurrency,
            |_| {
                multipart::Form::new()
                    .text("template", template)
                    .text("data", data.clone())
            },
            is_pdf,
        )
        .await;
        report.assert_all(StatusCode::OK, cfg.max_p99);
        assert_healthy(&cfg).await;
    }

    /// A typical invoice template.
    const INVOICE: &str = r#"<!DOCTYPE html><html><head><title>Invoice {{ no }}</title><style>
        body { font-family: 'Noto Sans', sans-serif; font-size: 12px; color: #1f2937; padding: 40px; }
        .hdr { display: flex; justify-content: space-between; border: 0; padding-bottom: 12px; }
        .brand { font-size: 24px; font-weight: 800; color: #4f46e5; }
        table { width: 100%; border-collapse: collapse; margin-top: 20px; }
        th { background: #eef2ff; color: #3730a3; text-align: left; padding: 6px; }
        td { padding: 6px; border: 1px solid #e5e7eb; }
        .r { text-align: right; } .tot { margin-top: 12px; text-align: right; font-size: 15px; font-weight: 800; }
        </style></head><body>
        <div class="hdr"><div class="brand">Lands Horizon</div><div class="r">INVOICE #{{ no }}<br>{{ date }}</div></div>
        <p><b>{{ customer }}</b><br>{{ email }}</p>
        <table><tr><th>Description</th><th class="r">Qty</th><th class="r">Unit</th><th class="r">Amount</th></tr>
        {% for l in lines %}<tr><td>{{ l.d }}</td><td class="r">{{ l.q }}</td><td class="r">₱{{ l.u }}</td><td class="r">₱{{ l.q * l.u }}</td></tr>{% endfor %}
        </table><div class="tot">Total ₱{{ total }}</div></body></html>"#;

    fn invoice_form(i: usize, lines: usize) -> multipart::Form {
        let rows: Vec<_> = (1..=lines)
            .map(|k| serde_json::json!({ "d": format!("Service line {k} for order {i}"), "q": k, "u": 125 + i % 50 }))
            .collect();
        let data = serde_json::json!({
            "no": format!("2026-{i:06}"), "date": "2026-10-12", "customer": format!("Customer {i}"),
            "email": format!("billing{i}@example.ph"), "lines": rows, "total": 26250 + i,
        });
        multipart::Form::new()
            .text("template", INVOICE)
            .text("data", data.to_string())
    }

    /// A typical invoice: flex header, bordered 20-line table, totals; data differs per request.
    #[tokio::test(flavor = "multi_thread")]
    #[ignore]
    async fn stress_to_bytes_invoice() {
        let cfg = config();
        let c = client(cfg.concurrency);
        let report = run_load(
            "to-bytes/invoice",
            &c,
            &format!("{}/api/to-bytes", cfg.base_url),
            &cfg.token,
            cfg.requests,
            cfg.concurrency,
            |i| invoice_form(i, 20),
            is_pdf,
        )
        .await;
        report.assert_all(StatusCode::OK, cfg.max_p99);
    }

    /// A 200-line invoice (several pages).
    #[tokio::test(flavor = "multi_thread")]
    #[ignore]
    async fn stress_to_bytes_invoice_large() {
        let cfg = config();
        let c = client(cfg.concurrency);
        let report = run_load(
            "to-bytes/invoice-200-lines",
            &c,
            &format!("{}/api/to-bytes", cfg.base_url),
            &cfg.token,
            (cfg.requests / 2).max(1),
            cfg.concurrency,
            |i| invoice_form(i, 200),
            is_pdf,
        )
        .await;
        report.assert_all(StatusCode::OK, cfg.max_p99);
    }

    /// A ~10-page report: headings, paragraphs, a grid of KPI cards and page breaks.
    #[tokio::test(flavor = "multi_thread")]
    #[ignore]
    async fn stress_to_bytes_report_multipage() {
        let cfg = config();
        let c = client(cfg.concurrency);
        let template = r#"<!DOCTYPE html><html><head><style>
            body { font-family: 'Noto Sans', sans-serif; font-size: 11px; line-height: 1.5; padding: 40px; }
            h1 { color: #0f172a; } h2 { color: #4f46e5; break-before: page; }
            .kpis { display: grid; grid-template-columns: repeat(4, 1fr); gap: 8px; }
            .kpi { border: 1px solid #c7d2fe; border-radius: 6px; padding: 8px; background: #eef2ff; }
            </style></head><body><h1>Quarterly report {{ q }}</h1>
            {% for s in sections %}<h2>{{ s.title }}</h2>
            <div class="kpis">{% for k in s.kpis %}<div class="kpi"><b>{{ k.v }}</b><br>{{ k.l }}</div>{% endfor %}</div>
            {% for p in s.paras %}<p>{{ p }}</p>{% endfor %}{% endfor %}</body></html>"#;
        let para = "Revenue grew steadily across all regions while operating costs remained flat. \
            The team shipped three major releases and reduced incident response time significantly. "
            .repeat(4);
        let report = run_load(
            "to-bytes/report-10-pages",
            &c,
            &format!("{}/api/to-bytes", cfg.base_url),
            &cfg.token,
            (cfg.requests / 2).max(1),
            cfg.concurrency,
            |i| {
                let sections: Vec<_> = (1..=10)
                    .map(|s| serde_json::json!({
                        "title": format!("Section {s}"),
                        "kpis": (0..8).map(|k| serde_json::json!({ "v": format!("{}%", (i + s + k) % 100), "l": format!("Metric {k}") })).collect::<Vec<_>>(),
                        "paras": vec![para.clone(); 5],
                    }))
                    .collect();
                multipart::Form::new()
                    .text("template", template)
                    .text("data", serde_json::json!({ "q": format!("Q{}", i % 4 + 1), "sections": sections }).to_string())
            },
            is_pdf,
        )
        .await;
        report.assert_all(StatusCode::OK, cfg.max_p99);
    }

    /// Production-like mix: 60% one-page invoices, 30% two-page invoices, 10% CJK documents
    /// (which use the slower system-font path).
    #[tokio::test(flavor = "multi_thread")]
    #[ignore]
    async fn stress_to_bytes_mixed() {
        let cfg = config();
        let c = client(cfg.concurrency);
        let report = run_load(
            "to-bytes/mixed-60inv-30long-10cjk",
            &c,
            &format!("{}/api/to-bytes", cfg.base_url),
            &cfg.token,
            cfg.requests,
            cfg.concurrency,
            |i| match i % 10 {
                0..=5 => invoice_form(i, 20),
                6..=8 => invoice_form(i, 60),
                _ => multipart::Form::new()
                    .text("template", "<h1>{{ t }}</h1>{% for r in rows %}<p>{{ r }}</p>{% endfor %}")
                    .text("data", serde_json::json!({ "t": "請求書 / 청구서", "rows": vec!["你好世界 こんにちは 안녕하세요 🚀"; 30] }).to_string()),
            },
            is_pdf,
        )
        .await;
        report.assert_all(StatusCode::OK, cfg.max_p99);
    }

    #[tokio::test(flavor = "multi_thread")]
    #[ignore]
    async fn stress_to_bytes_encrypted() {
        let cfg = config();
        let c = client(cfg.concurrency);
        let requests = (cfg.requests / 2).max(1);
        let report = run_load(
            "to-bytes/encrypted",
            &c,
            &format!("{}/api/to-bytes", cfg.base_url),
            &cfg.token,
            requests,
            cfg.concurrency,
            |i| {
                multipart::Form::new()
                    .text("template", "<h1>Secret</h1><p>page 1</p><div style='page-break-before:always'>page 2</div>")
                    .text("password", format!("pw{i}"))
            },
            is_pdf,
        )
        .await;
        report.assert_all(StatusCode::OK, cfg.max_p99);
    }

    #[tokio::test(flavor = "multi_thread")]
    #[ignore]
    async fn stress_to_s3() {
        let cfg = config();
        let c = client(cfg.concurrency);
        let requests = (cfg.requests / 2).max(1);
        let report = run_load(
            "to-s3",
            &c,
            &format!("{}/api/to-s3", cfg.base_url),
            &cfg.token,
            requests,
            cfg.concurrency,
            |i| {
                multipart::Form::new()
                    .text("template", "<h1>S3 {{ n }}</h1>")
                    .text("data", format!(r#"{{"n":{i}}}"#))
            },
            is_s3_success,
        )
        .await;
        report.assert_all(StatusCode::OK, cfg.max_p99);
    }

    /// Unauthorized requests must be rejected quickly and never reach the renderer.
    #[tokio::test(flavor = "multi_thread")]
    #[ignore]
    async fn stress_auth_rejection() {
        let cfg = config();
        let c = client(cfg.concurrency);
        let report = run_load(
            "auth/reject",
            &c,
            &format!("{}/api/to-bytes", cfg.base_url),
            "definitely-wrong-token",
            cfg.requests * 5,
            cfg.concurrency * 4,
            |_| multipart::Form::new().text("template", "<h1>nope</h1>"),
            |_| true,
        )
        .await;
        report.assert_all(StatusCode::UNAUTHORIZED, Some(Duration::from_millis(500)));
        assert_healthy(&cfg).await;
    }

    /// Mix of bad templates and missing fields: server must return 400s, not 500s or hang.
    #[tokio::test(flavor = "multi_thread")]
    #[ignore]
    async fn stress_bad_input() {
        let cfg = config();
        let c = client(cfg.concurrency);
        let report = run_load(
            "bad-input",
            &c,
            &format!("{}/api/to-bytes", cfg.base_url),
            &cfg.token,
            cfg.requests,
            cfg.concurrency,
            |i| match i % 2 {
                0 => multipart::Form::new().text("template", "{% for x in %}broken"),
                _ => multipart::Form::new().text("width", "10in"),
            },
            |_| true,
        )
        .await;
        report.assert_all(StatusCode::BAD_REQUEST, None);
        assert_healthy(&cfg).await;
    }

    /// Spike far above the render semaphore size: nothing should 500 or deadlock.
    /// Requests may legitimately hit the 45s server timeout (408) if the box is slow.
    #[tokio::test(flavor = "multi_thread")]
    #[ignore]
    async fn stress_burst() {
        let cfg = config();
        let burst = cfg.concurrency * 8;
        let c = client(burst);
        let report = run_load(
            "burst",
            &c,
            &format!("{}/api/to-bytes", cfg.base_url),
            &cfg.token,
            burst,
            burst,
            |i| multipart::Form::new().text("template", format!("<h1>burst {i}</h1>")),
            is_pdf,
        )
        .await;
        report.print();
        let ok = report.count(StatusCode::OK);
        let timed_out = report.count(StatusCode::REQUEST_TIMEOUT);
        assert_eq!(
            ok + timed_out,
            report.total(),
            "unexpected failures during burst: {:?}",
            report.statuses
        );
        assert!(ok > 0, "no request succeeded during burst");
        assert_healthy(&cfg).await;
    }
}
