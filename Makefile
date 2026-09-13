.PHONY: test interop-test interop-test-browser cover pbt-with-cover fuzzing fuzzing-list check clippy fmt clean

# 全テストを実行する
test:
	cargo test --workspace

# interop テスト (tokio-nghttp2 との疎通確認) を実行する
interop-test:
	cargo test -p interop_h2

# interop テスト (WebKit との WebTransport over HTTP/2 の疎通確認) を実行する
#
# npm とブラウザの導入に時間がかかるため interop-test には含めない。
interop-test-browser:
	cargo build -p wt_server
	cd interop/browser && npm ci && npx playwright install webkit && node run.mjs

# 全テストカバレッジ付きで実行する
cover:
	cargo llvm-cov --tests --workspace

# PBT をカバレッジ付きで実行する
pbt-with-cover:
	cargo llvm-cov -p pbt --tests

# Fuzzing を全ターゲットで 30 秒ずつ実行する
fuzzing:
	@for target in $$(cargo fuzz list); do \
		echo "=== Fuzzing $$target ==="; \
		cargo +nightly fuzz run $$target -- -max_total_time=30 || exit 1; \
	done

# Fuzzing ターゲット一覧を表示する
fuzzing-list:
	cargo fuzz list

# cargo check を実行する
check:
	cargo check --workspace

# cargo clippy を実行する
clippy:
	cargo clippy --workspace --all-targets -- -D warnings

# cargo fmt を実行する
fmt:
	cargo fmt --all

# ビルド成果物を削除する
clean:
	cargo clean
