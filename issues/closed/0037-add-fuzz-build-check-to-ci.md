# CI で fuzz クレートのビルド確認を行う

Created: 2026-05-23
Model: Opus 4.7

## 内容

`.github/workflows/ci.yml` に fuzz クレートの `cargo check` ステップを追加し、ルートクレートの API 変更により `fuzz/fuzz_targets/*` がコンパイル不能になる regression を CI で検出できるようにする。

## 背景

- `fuzz/` は `Cargo.toml` の `[workspace] exclude = ["fuzz"]` (root `Cargo.toml` L33) で workspace から除外されており、`cargo test --workspace` でビルド検査されない。
- issue 0024 (HeaderField 構築時検査リファクタリング) の作業中、`HeaderField` API 変更により `fuzz/fuzz_targets/*` が一時的にコンパイル不能になったが、CI では検出されず /review-diff-code 経由で初めて発覚した。同種 regression を防ぐ。

## 設計方針

### ステップの内容

`.github/workflows/ci.yml` の `jobs.ci.steps` に以下を `cargo clippy` の **次**に追加する。

```yaml
      - name: Check fuzz crate
        if: matrix.os == 'ubuntu-24.04'
        run: cargo check --manifest-path fuzz/Cargo.toml
```

### OS 限定の根拠

`libfuzzer-sys = "0.4"` (`fuzz/Cargo.toml` L12) は `build.rs` で C 製 `libfuzzer` を `cc` クレートでコンパイルする。Windows (MSVC) では libfuzzer のサニタイザ ABI が通らないことが既知の問題として報告されている。macOS でも環境依存で失敗しうる。`cargo-fuzz` 自体が慣習的に Linux 限定で運用されている (rust-fuzz/cargo-fuzz README、s2n-quic、quinn 等の前例) ため、Linux runner のみで実行する。

さらに matrix 内の `ubuntu-24.04` と `ubuntu-24.04-arm` の 2 runner で同じ check を並列実行する利得 (arch 差異検出) は本 issue の目的 (API 不整合検出) には不要なため、`if: matrix.os == 'ubuntu-24.04'` で **`ubuntu-24.04` のみに絞る**。arch 差異の検証が必要になれば別 issue で `ubuntu-24.04-arm` も追加する。

### `cargo check` を採用、`cargo fuzz build` を採用しない根拠

- 目的は「fuzz_targets が API 変更でコンパイル不能になる regression の検出」。`cargo check` で型チェックと build script の実行まで通り、API 不整合は確実に検出される。`libfuzzer-sys` の `build.rs` (`cc` クレート経由の C コンパイル) も `cargo check` で実行されるため、C ビルド段階の成否も同時に検証される (`cargo build` まで進めずとも済む)。
- `cargo fuzz build` / `cargo fuzz check` は cargo subcommand で nightly toolchain を要する。CI に nightly を増やすと cache・ビルド時間コストが二重になる。本 issue では stable + `cargo check` のみに絞る。
- 将来的に sanitizer 込みのリンク段階まで CI で確認したい需要が出たら別 issue として `cargo fuzz build` 追加を検討する (本 issue のスコープ外)。

### job 分離せず既存 `ci` job に inline 追加する

新 job を作ると既存の `slack_notify` job (`needs: [ci]`, `job.status` 連動) と `actions/checkout` キャッシュ設定を別途揃える必要がある。`ci` job に step として追加する方が単純で、通知連動も自動的に効く (`ci` job の status に fuzz check の失敗が反映される)。`ubuntu-24.04` 以外の runner では `if` 条件でスキップ扱いになる。`if` で skip された step は GitHub Actions の標準動作で job status に影響しない (失敗扱いにならない) ため、`ubuntu-24.04-arm` / Windows / macOS で `ci` job が誤って赤くなることはない。

### CI 実行時間

`libfuzzer-sys` の `cc` ビルドで cold cache 時に追加時間が発生する (推定 1-3 分だが実測値は本 issue 実装時の最初の CI 実行で確定し PR 本文に追記する)。現状 `timeout-minutes: 15` のヘッドルームに収まる想定。超過した場合は別 issue で `timeout-minutes` 引き上げを検討する。

## 完了条件

- [ ] `.github/workflows/ci.yml` に `Check fuzz crate` ステップが `cargo clippy` 直後に追加され、`if: matrix.os == 'ubuntu-24.04'` 条件付きで `cargo check --manifest-path fuzz/Cargo.toml` を実行する
- [ ] 本 PR の CI 実行で `ubuntu-24.04` において新 step が成功する
- [ ] 本 PR の CI 実行で `ubuntu-24.04-arm` / Windows / macOS runner において新 step が **skipped** で完了する (失敗扱いにならない)
- [ ] regression 検出機能の確認: ローカルで `fuzz/fuzz_targets/fuzz_validation.rs` 内の `HeaderField::from_validated_parts` を一時的に存在しないシンボル (例: `HeaderField::__nonexistent`) に書き換えて `cargo check --manifest-path fuzz/Cargo.toml` を実行し、コンパイルエラーになることを目視確認する。確認後は元に戻し、コンパイラ出力ログを PR 説明に添付する (PR コミット履歴を汚さない)
- [ ] 既存 `cargo fmt --all --check` / `cargo check --workspace` / `cargo test --workspace` / `cargo clippy --workspace -- -D warnings` がすべて引き続き通る
- [ ] CI 全体の実行時間が `timeout-minutes: 15` の範囲に収まる (Linux runner で実測値を PR 説明に追記)
- [ ] CHANGES.md `### misc` に下記文面を追記

## CHANGES.md エントリ

`## develop` の `### misc` に追記する:

```
- [ADD] CI に `cargo check --manifest-path fuzz/Cargo.toml` ステップを追加し、ルートクレートの API 変更による fuzz_targets のコンパイル不能 regression を検出する
  - @voluntas
```

## ブランチ命名

`feature/add-fuzz-build-check-to-ci` を使用する (本 issue category=add)。

## スコープ外

- `cargo fuzz build` / `cargo fuzz check` (nightly toolchain 必要) の CI 追加 → 別 issue
- Windows / macOS での fuzz クレートビルド対応 → `libfuzzer-sys` の上流対応が必要で本 issue では扱わない
- `timeout-minutes: 15` の引き上げ → 本 issue 実装後の実測で必要性を判断
- fuzz_targets の自動実行 (定期 cron 等) → 別 issue (本 issue はビルド可否のみ確認)
- `crates/nghttp2-sys`, `crates/shiguredo_nghttp2`, `crates/tokio-nghttp2`, `crates/tokio-http2` の fuzz クレート (現状未設置) → 本 issue のスコープはルートクレートの fuzz/ のみ
- `.github/workflows/release.yml` への fuzz check 追加 → release.yml は `cargo publish` 中心で tag push 時には ci.yml が直前に通っているため、fuzz check の二重実行は不要

## 関連

- 動機: [[0024-change-header-field-construct-time-validation]] (本 issue が防ぐべき regression の実例)
