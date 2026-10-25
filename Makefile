# pdf-engine tasks.
#
#   make test     functional test suites (fast, hermetic: no server, no network)
#   make node     Node.js client tests (needs `cargo build` first)
#   make bench    speed per document, then redraw the charts   (.public/benchmark/README.md explains it all)
#   make bench-scale   10,000 documents per library and scenario
#
# `make docker-test` runs the checks inside Dockerfile.dev (Rust toolchain plus the production
# font set), for machines without Rust or with a different font setup.

FUNCTIONAL_TESTS := happy_path sad_path poison_pill chaos integration_chaos race_conditions \
                    zero_absence smoke compression css assets fonts_reload languages ipc error_codes node_contract cors

# Parallel build jobs. Linking many test binaries at once is memory hungry; keep this low on
# machines (or Docker VMs) with less than ~16 GB.
JOBS ?= 2

# Use the production font set when it's installed (.public/scripts/install-fonts.sh), like the Docker image.
ifneq ($(wildcard /usr/share/fonts/pdf-engine),)
export FONTCONFIG_FILE ?= $(CURDIR)/.public/assets/fonts/fonts.conf
endif

# `make bench-scale` options: documents per run, seconds per run, and optionally which scenarios
# (small,large,multitable) and libraries (ids in .public/benchmark/libs.mjs), e.g.
#   make bench-scale BENCH_COUNT=1000 BENCH_BUDGET=60 BENCH_LIBS=pdf-engine,puppeteer-shell
BENCH_COUNT ?= 10000
BENCH_BUDGET ?= 150
BENCH_SCENARIOS ?= small,large,multitable
BENCH_LIBS ?=

# Release version comes from Cargo.toml; images go to $(IMAGE) for $(PLATFORMS).
VERSION := $(shell sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
IMAGE ?= zalven88/pdf-engine
PLATFORMS ?= linux/amd64,linux/arm64

DEV_IMAGE := pdf-engine-dev
DOCKER_RUN := docker run --rm $(shell [ -t 1 ] && echo -t) \
	-v "$(CURDIR)":/app -w /app \
	-v pdf-engine-cargo:/usr/local/cargo/registry \
	-v pdf-engine-target:/target -e CARGO_TARGET_DIR=/target \
	-e JOBS=$(JOBS) \
	$(DEV_IMAGE)

.DEFAULT_GOAL := help
.PHONY: help test test-unit fmt fmt-check check node \
        bench-setup bench-check bench bench-scale bench-charts bench-stats \
        docker-dev-image docker-test docker-image \
        version release-image release push

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

node: ## Node.js client tests against the debug binary (run `cargo build` first)
	cd .node/desktop && PDF_ENGINE_BIN=$(CURDIR)/target/debug/pdf-engine node --test

# ---- benchmarks (.public/benchmark/README.md) ------------------------------------------------------------

bench-setup: ## Build what is measured and install the benchmark tools and browsers (once)
	cargo build --release
	cd .node/wasm && npm run build
	cd .public/benchmark && npm install && npx playwright install chromium webkit firefox \
		&& npx puppeteer browsers install chrome chrome-headless-shell && npm run build

bench-check: ## Render one document of each kind with every library, to see they match
	cd .public/benchmark && npm run check

bench: ## Speed per document in the browser and Node, then redraw the charts (about 5 minutes)
	cd .public/benchmark && npm run bench

bench-scale: ## 10,000 documents per library and scenario (30-90 min); see BENCH_* above
	cd .public/benchmark && $(if $(shell command -v caffeinate),caffeinate -i )node scale.mjs --count $(BENCH_COUNT) --budget $(BENCH_BUDGET) \
		--scenarios $(BENCH_SCENARIOS) $(if $(BENCH_LIBS),--libs $(BENCH_LIBS)) && node charts.mjs

bench-charts: ## Redraw the README charts from .public/benchmark/results
	cd .public/benchmark && npm run charts

bench-stats: ## Live weekly downloads and last update of each package, from the npm registry
	cd .public/benchmark && npm run stats

# ---- docker ---------------------------------------------------------------------------------

docker-dev-image: ## Build the dev/test image (Dockerfile.dev)
	docker build -q -f Dockerfile.dev -t $(DEV_IMAGE) . >/dev/null

docker-test: docker-dev-image ## `make check` inside the dev image
	$(DOCKER_RUN) make check

docker-image: ## Build the production image for this machine (tagged pdf-engine:$(VERSION))
	docker build --build-arg VERSION=$(VERSION) -t pdf-engine:$(VERSION) .

# ---- release --------------------------------------------------------------------------------

version: ## Print the version from Cargo.toml
	@echo $(VERSION)

release-image: ## Push the multi-arch image: $(IMAGE):<version>, :<major.minor>, :<major>, :latest
	.public/scripts/release-image.sh $(IMAGE) $(VERSION) $(PLATFORMS) \
		$(VERSION) \
		$(word 1,$(subst ., ,$(VERSION))).$(word 2,$(subst ., ,$(VERSION))) \
		$(word 1,$(subst ., ,$(VERSION))) \
		latest

release: ## Tag v<version> in git, push the tag, and push the image (bump Cargo.toml + CHANGELOG.md first)
	@git diff --quiet && git diff --cached --quiet || { echo "commit your changes first" >&2; exit 1; }
	@! git rev-parse -q --verify "refs/tags/v$(VERSION)" >/dev/null || { echo "tag v$(VERSION) already exists" >&2; exit 1; }
	git tag -a v$(VERSION) -m "pdf-engine v$(VERSION)"
	git push origin v$(VERSION)
	$(MAKE) release-image

push: ## Bump to VERSION, commit everything with MESSAGE, push, and release: make push VERSION=1.2.3 MESSAGE="..."
	.public/scripts/push.sh --version="$(VERSION)" --message="$(MESSAGE)"
