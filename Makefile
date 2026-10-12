# pdf-engine tasks.
#
#   make test     functional test suites (fast, hermetic: no server, no network)
#   make bench    criterion micro-benchmarks
#   make stress   load test: scenario matrix, concurrency sweep, soak
#
# Every target has a `docker-` variant that runs inside Dockerfile.dev (Rust toolchain plus
# the production font set), for machines without Rust or with a different font setup.

FUNCTIONAL_TESTS := happy_path sad_path poison_pill chaos integration_chaos race_conditions \
                    zero_absence smoke compression css

# Parallel build jobs. Linking many test binaries at once is memory hungry; keep this low on
# machines (or Docker VMs) with less than ~16 GB.
JOBS ?= 2
# loadtest.sh mode: all | matrix | sweep | soak
MODE ?= all

# Use the production font set when it's installed (fonts/install.sh), like the Docker image.
ifneq ($(wildcard /usr/share/fonts/pdf-engine),)
export FONTCONFIG_FILE ?= $(CURDIR)/fonts/fonts.conf
endif

DEV_IMAGE := pdf-engine-dev
DOCKER_RUN := docker run --rm $(shell [ -t 1 ] && echo -t) \
	-v "$(CURDIR)":/app -w /app \
	-v pdf-engine-cargo:/usr/local/cargo/registry \
	-v pdf-engine-target:/target -e CARGO_TARGET_DIR=/target \
	-e JOBS=$(JOBS) -e MODE=$(MODE) \
	$(DEV_IMAGE)

.DEFAULT_GOAL := help
.PHONY: help test test-unit fmt fmt-check check bench stress test-live \
        docker-dev-image docker-test docker-bench docker-stress docker-image

help: ## Show this help
	@grep -E '^[a-z-]+:.*## ' $(MAKEFILE_LIST) | awk -F ':.*## ' '{printf "  \033[36m%-18s\033[0m %s\n", $$1, $$2}'

# ---- functional tests -----------------------------------------------------------------------

test: ## Run all functional test suites (no benchmarks, no load tests)
	cargo test -j $(JOBS) --lib $(addprefix --test ,$(FUNCTIONAL_TESTS))

test-unit: ## Run only the unit tests inside src/
	cargo test -j $(JOBS) --lib

fmt: ## Format the code
	cargo fmt

fmt-check: ## Fail if the code isn't formatted
	cargo fmt --check

check: fmt-check test ## Formatting check + functional tests (what CI should run)

test-live: ## Smoke + integration tests against a running server (SMOKE_URL, API_BEARER_TOKEN)
	cargo test --test smoke -- --ignored live_
	cargo test --test integration_tests

# ---- performance ----------------------------------------------------------------------------

bench: ## Criterion micro-benchmarks (render per document type, templating, encryption, gzip)
	CARGO_PROFILE_BENCH_LTO=thin CARGO_PROFILE_BENCH_CODEGEN_UNITS=16 cargo bench --bench render

stress: ## Load test via scripts/loadtest.sh (MODE=all|matrix|sweep|soak, PDF_URL to target a server)
	CARGO_PROFILE_RELEASE_LTO=thin CARGO_PROFILE_RELEASE_CODEGEN_UNITS=16 scripts/loadtest.sh $(MODE)

# ---- docker ---------------------------------------------------------------------------------

docker-dev-image: ## Build the dev/test image (Dockerfile.dev)
	docker build -q -f Dockerfile.dev -t $(DEV_IMAGE) . >/dev/null

docker-test: docker-dev-image ## `make check` inside the dev image
	$(DOCKER_RUN) make check

docker-bench: docker-dev-image ## `make bench` inside the dev image
	$(DOCKER_RUN) make bench

docker-stress: docker-dev-image ## `make stress` inside the dev image
	$(DOCKER_RUN) make stress

docker-image: ## Build the production image
	docker build -t pdf-engine .
