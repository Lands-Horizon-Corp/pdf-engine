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

    /// Unauthorized requests must be rejected quickly and never reach Prince.
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

    /// Spike far above the Prince semaphore size: nothing should 500 or deadlock.
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
