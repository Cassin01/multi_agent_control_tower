.PHONY: build test check clean install fmt fmt-check lint ci demo-gif-validate demo-gif
build: ## Build the project in release mode
	cargo build --release

test: ## Run the test suite
	cargo test

check: ## Check compilation without building artifacts
	cargo check

clean: ## Clean build artifacts and report files
	cargo clean
	rm -rf .macot/reports/*.yaml

install: build ## Install the binary from the local source
	cargo install --path .

fmt: ## Format Rust source code
	cargo fmt

fmt-check: ## Verify Rust formatting without writing files
	cargo fmt --check

lint: ## Run clippy lints and fail on warnings
	cargo clippy -- -D warnings

ci: build lint fmt-check test ## Run local CI checks (build, lint, format, test)

demo-gif-validate: ## Validate the VHS tape for the README demo
	vhs validate assets/demo-quickstart.tape

demo-gif: demo-gif-validate ## Regenerate assets/demo-quickstart.gif
	vhs assets/demo-quickstart.tape
	tmp_gif="$$(mktemp "$${TMPDIR:-/tmp}/demo-quickstart.XXXXXX.gif")"; \
	ffmpeg -y -i assets/demo-quickstart.gif -vf "fps=12,scale=800:-1:flags=lanczos,split[s0][s1];[s0]palettegen=stats_mode=diff[p];[s1][p]paletteuse=dither=bayer:bayer_scale=5:diff_mode=rectangle" -loop 0 "$$tmp_gif" >/dev/null 2>&1; \
	mv "$$tmp_gif" assets/demo-quickstart.gif
