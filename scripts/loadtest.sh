#!/usr/bin/env bash
# Production load test for pdf-engine. Prints markdown tables.
#
#   scripts/loadtest.sh [matrix|sweep|soak|all]      (default: all)
#
# Without PDF_URL it builds a release server, starts a fresh one for every scenario on
# LOCAL_PORT and measures its peak memory. With PDF_URL it
# load-tests that running instance instead (no memory numbers).
#
# Environment:
#   PDF_URL           test an existing server (e.g. https://pdf.example.com)
#   API_BEARER_TOKEN  token for PDF_URL (local runs use a throwaway token)
#   REQUESTS          requests per matrix scenario       (default 400)
#   CONCURRENCY       in-flight requests                  (default 32)
#   SWEEP             concurrency levels for the sweep    (default "1 2 5 10 20 40 80")
#   SOAK_ROUNDS       soak rounds of SOAK_BATCH invoices  (default 10)
#   SOAK_BATCH        invoices per soak round             (default 2000)
#   LOCAL_PORT        port for the local server           (default 7071)
#   CARGO_PROFILE_RELEASE_LTO=thin   speeds up the build; numbers stay within a few percent
set -uo pipefail

MODE=${1:-all}
REQUESTS=${REQUESTS:-400}
CONCURRENCY=${CONCURRENCY:-32}
SWEEP=${SWEEP:-"1 2 5 10 20 40 80"}
SOAK_ROUNDS=${SOAK_ROUNDS:-10}
SOAK_BATCH=${SOAK_BATCH:-2000}
LOCAL_PORT=${LOCAL_PORT:-7071}
ROOT=$(cd "$(dirname "$0")/.." && pwd)
BIN=${CARGO_TARGET_DIR:-$ROOT/target}/release/pdf-engine
SERVER_PID=""
PEAK_FILE=$(mktemp)

cleanup() { stop_server; rm -f "$PEAK_FILE"; }
trap cleanup EXIT

cd "$ROOT"
echo "Building release binaries..." >&2
cargo build --release -q || exit 1
cargo test --release -q --test stress_tests --no-run 2>/dev/null || exit 1

if [[ -n "${PDF_URL:-}" ]]; then
    URL=$PDF_URL
    TOKEN=${API_BEARER_TOKEN:?API_BEARER_TOKEN must be set with PDF_URL}
else
    URL=http://127.0.0.1:$LOCAL_PORT
    TOKEN=loadtest-token
fi

start_server() {
    [[ -n "${PDF_URL:-}" ]] && return
    local log
    log=$(mktemp)
    (cd "$(mktemp -d)" && API_BEARER_TOKEN=$TOKEN API_PORT=$LOCAL_PORT RUST_LOG=warn \
        STORAGE_BUCKET=loadtest STORAGE_URL=127.0.0.1:1 STORAGE_ACCESS_KEY=x STORAGE_SECRET_KEY=x \
        exec "$BIN" 2>"$log") &
    SERVER_PID=$!
    for _ in $(seq 1 240); do
        curl -s -o /dev/null "$URL" && break
        kill -0 "$SERVER_PID" 2>/dev/null || { cat "$log" >&2; exit 1; }
        sleep 0.25
    done
    echo 0 >"$PEAK_FILE"
    (
        while kill -0 "$SERVER_PID" 2>/dev/null; do
            total=0
            for p in "$SERVER_PID" $(pgrep -P "$SERVER_PID"); do
                rss=$(ps -o rss= -p "$p" 2>/dev/null | tr -d ' ')
                total=$((total + ${rss:-0}))
            done
            ((total > $(cat "$PEAK_FILE"))) && echo "$total" >"$PEAK_FILE"
            sleep 0.2
        done
    ) &
}

stop_server() {
    [[ -z "$SERVER_PID" ]] && return
    kill "$SERVER_PID" 2>/dev/null
    wait "$SERVER_PID" 2>/dev/null
    SERVER_PID=""
}

rss_mb() { echo $(($(ps -o rss= -p "$SERVER_PID" | tr -d ' ') / 1024)); }
peak_mb() { [[ -n "${PDF_URL:-}" ]] && echo "n/a" || echo "$(($(cat "$PEAK_FILE") / 1024)) MB"; }

# run_scenario <test> <requests> <concurrency>  -> "rps|p50|p99|max|statuses|pass"
run_scenario() {
    local out status
    out=$(STRESS_URL=$URL API_BEARER_TOKEN=$TOKEN STRESS_REQUESTS=$2 STRESS_CONCURRENCY=$3 \
        cargo test --release -q --test stress_tests -- \
        --ignored --exact "tests::$1" --nocapture --test-threads=1 2>&1)
    status=$?
    local rps p50 p99 max statuses
    rps=$(sed -nE 's/.*=> ([0-9.]+) req\/s.*/\1/p' <<<"$out" | head -1)
    p50=$(sed -nE 's/.*p50 ([^ ]+) .*/\1/p' <<<"$out" | head -1)
    p99=$(sed -nE 's/.*p99 ([^ ]+) .*/\1/p' <<<"$out" | head -1)
    max=$(sed -nE 's/.*max ([^ ]+).*/\1/p' <<<"$out" | head -1)
    statuses=$(sed -nE 's/.*statuses: (.*)/\1/p' <<<"$out" | head -1)
    echo "${rps:-?}|${p50:-?}|${p99:-?}|${max:-?}|${statuses:-?}|$([[ $status == 0 ]] && echo pass || echo FAIL)"
}

matrix() {
    echo
    echo "### Scenario matrix ($REQUESTS requests per scenario, concurrency $CONCURRENCY)"
    echo
    echo "| Scenario | PDFs/s | p50 | p99 | max | Peak memory | Result |"
    echo "| --- | --- | --- | --- | --- | --- | --- |"
    local scenarios="stress_to_bytes_small stress_to_bytes_invoice stress_to_bytes_invoice_large stress_to_bytes_report_multipage stress_to_bytes_encrypted stress_to_bytes_heavy_template stress_to_bytes_mixed"
    for t in $scenarios; do
        start_server
        IFS='|' read -r rps p50 p99 max statuses result <<<"$(run_scenario "$t" "$REQUESTS" "$CONCURRENCY")"
        echo "| ${t#stress_to_bytes_} | $rps | $p50 | $p99 | $max | $(peak_mb) | $result $statuses |"
        stop_server
    done
}

sweep() {
    echo
    echo "### Concurrency sweep (invoice, $REQUESTS requests per level)"
    echo
    echo "| Concurrency | PDFs/s | p50 | p99 | Peak memory |"
    echo "| --- | --- | --- | --- | --- |"
    for c in $SWEEP; do
        start_server
        IFS='|' read -r rps p50 p99 _ _ _ <<<"$(run_scenario stress_to_bytes_invoice "$REQUESTS" "$c")"
        echo "| $c | $rps | $p50 | $p99 | $(peak_mb) |"
        stop_server
    done
}

soak() {
    echo
    echo "### Soak ($((SOAK_ROUNDS * SOAK_BATCH)) invoices on one server, concurrency $CONCURRENCY)"
    echo
    echo "| PDFs so far | PDFs/s | p50 | p99 | Server memory after round | Result |"
    echo "| --- | --- | --- | --- | --- | --- |"
    start_server
    [[ -z "${PDF_URL:-}" ]] && echo "| 0 (idle) | | | | $(rss_mb) MB | |"
    for r in $(seq 1 "$SOAK_ROUNDS"); do
        IFS='|' read -r rps p50 p99 _ statuses result <<<"$(run_scenario stress_to_bytes_invoice "$SOAK_BATCH" "$CONCURRENCY")"
        local mem="n/a"
        [[ -z "${PDF_URL:-}" ]] && mem="$(rss_mb) MB"
        echo "| $((r * SOAK_BATCH)) | $rps | $p50 | $p99 | $mem | $result |"
    done
    stop_server
}

echo "# pdf-engine load test — $(date -u +%Y-%m-%dT%H:%MZ) — $(uname -sm), $(getconf _NPROCESSORS_ONLN) CPUs"
case $MODE in
    matrix) matrix ;;
    sweep) sweep ;;
    soak) soak ;;
    all) matrix; sweep; soak ;;
    *) echo "usage: $0 [matrix|sweep|soak|all]" >&2; exit 2 ;;
esac
