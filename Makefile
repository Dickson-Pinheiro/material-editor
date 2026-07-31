.PHONY: help build build-browser build-wasi test test-all example clean size fmt lint editor demo demo-serve

CRATE      := diagramador
WASM_NAME  := diagramador
BROWSER_FEATURES := browser,images
WASI_FEATURES    := wasi-lib,images
PKG        := packages/editor/src/wasm
DIST       := packages/editor/dist
## Subpath the demo will be served from. GitHub Pages uses /<repo>/.
BASE_PATH  ?= /

help:
	@echo "make build          — both wasm targets"
	@echo "make build-browser  — wasm-bindgen bundle for the editor  → $(PKG)/"
	@echo "make build-wasi     — C-ABI module for Python/Go          → wasm/"
	@echo "make test           — rust unit tests"
	@echo "make example        — render examples/material.json to out.pdf"
	@echo "make editor         — run the browser editor (needs make build-browser first)"
	@echo "make demo           — static bundle for GitHub Pages       → $(DIST)/"
	@echo "make demo-serve     — build and serve it at BASE_PATH, as Pages would"

# ─── Build ────────────────────────────────────────────────────────────────────

build: build-browser build-wasi

## Browser bundle (wasm-bindgen). Consumed by packages/editor.
build-browser:
	cargo build --target wasm32-unknown-unknown \
	  --no-default-features --features $(BROWSER_FEATURES) --release
	wasm-bindgen --target web --out-dir $(PKG) \
	  target/wasm32-unknown-unknown/release/$(WASM_NAME).wasm
	@if command -v wasm-opt >/dev/null 2>&1; then \
	  echo "wasm-opt…"; \
	  wasm-opt -Oz --strip-debug --strip-producers \
	    --enable-bulk-memory --enable-sign-ext --enable-nontrapping-float-to-int \
	    $(PKG)/$(WASM_NAME)_bg.wasm -o $(PKG)/$(WASM_NAME)_bg.wasm; \
	else \
	  echo "wasm-opt não encontrado, pulando"; \
	fi
	@echo "→ $(PKG)/"

## WASI C-ABI module for Python, Go and other hosts.
build-wasi:
	cargo build --target wasm32-wasip1 \
	  --no-default-features --features $(WASI_FEATURES) --release
	mkdir -p wasm
	cp target/wasm32-wasip1/release/$(WASM_NAME).wasm wasm/$(WASM_NAME).wasm
	@if command -v wasm-opt >/dev/null 2>&1; then \
	  wasm-opt -Oz --strip-debug --strip-producers \
	    --enable-bulk-memory --enable-sign-ext --enable-nontrapping-float-to-int \
	    wasm/$(WASM_NAME).wasm -o wasm/$(WASM_NAME).wasm; \
	fi
	@echo "→ wasm/$(WASM_NAME).wasm"

# ─── Test ─────────────────────────────────────────────────────────────────────

test:
	cargo test --no-default-features --features $(WASI_FEATURES)

## Every feature combination that ships.
test-all: test
	cargo test --no-default-features --features images
	cargo check --no-default-features
	cargo check --target wasm32-unknown-unknown --no-default-features --features $(BROWSER_FEATURES)
	cargo check --target wasm32-wasip1 --no-default-features --features $(WASI_FEATURES)

example:
	cargo run --release --example render --no-default-features --features images \
	  -- examples/material.json out.pdf

editor:
	cd packages/editor && npm run dev

# ─── Demo ─────────────────────────────────────────────────────────────────────

## The same two halves the Pages workflow runs: engine to wasm, editor around it.
demo: build-browser
	cd packages/editor && npm ci && BASE_PATH=$(BASE_PATH) npm run build
	@echo "→ $(DIST)/ (base $(BASE_PATH))"

## Serve it under the subpath, so a wrong BASE_PATH fails here and not in the air.
demo-serve: demo
	@if [ "$(BASE_PATH)" = "/" ]; then \
	  echo "servindo em http://127.0.0.1:8000/"; \
	  cd $(DIST) && python3 -m http.server 8000; \
	else \
	  root=$$(mktemp -d); \
	  mkdir -p "$$root$(BASE_PATH)"; \
	  cp -r $(DIST)/. "$$root$(BASE_PATH)"; \
	  echo "servindo em http://127.0.0.1:8000$(BASE_PATH)"; \
	  cd "$$root" && python3 -m http.server 8000; \
	fi

# ─── Housekeeping ─────────────────────────────────────────────────────────────

fmt:
	cargo fmt

lint:
	cargo clippy --no-default-features --features $(WASI_FEATURES) -- -D warnings

size:
	@[ -f $(PKG)/$(WASM_NAME)_bg.wasm ] && { \
	  raw=$$(ls -lh $(PKG)/$(WASM_NAME)_bg.wasm | awk '{print $$5}'); \
	  gz=$$(gzip -c $(PKG)/$(WASM_NAME)_bg.wasm | wc -c | awk '{printf "%.0fKB", $$1/1024}'); \
	  echo "browser: $$raw cru / $$gz gzip"; \
	} || echo "browser: não construído"
	@[ -f wasm/$(WASM_NAME).wasm ] && { \
	  raw=$$(ls -lh wasm/$(WASM_NAME).wasm | awk '{print $$5}'); \
	  gz=$$(gzip -c wasm/$(WASM_NAME).wasm | wc -c | awk '{printf "%.0fKB", $$1/1024}'); \
	  echo "wasi:    $$raw cru / $$gz gzip"; \
	} || echo "wasi: não construído"

clean:
	cargo clean
	rm -rf $(PKG) wasm out.pdf
