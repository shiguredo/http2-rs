# SKILL.md と README のドキュメントを最新化し、CHANGES.md の draft-15 参照を履歴として保全する

- Created: 2026-08-16
- Completed: {YYYY-MM-DD}
- Branch: feature/update-skill-md-and-readme
- Polished: 2026-11-16

## 目的

`skills/shiguredo-http2/SKILL.md` のバージョン番号・ビルド方式・モジュールパス・`ConnectionState` 表・Event 表の記述を現状に合わせて修正し、`README.md` と `crates/nghttp2-sys/README.md` のビルド方式の記述も合わせて修正する。`CHANGES.md` の既存の draft-15 参照は履歴エントリとして保全し、本 issue では変更しない。

## 現状

1. `skills/shiguredo-http2/SKILL.md` のバージョン表記が `2026.1.0-canary.8` だが、`Cargo.toml` の実際のバージョンは `2026.1.0-canary.18`
2. `skills/shiguredo-http2/SKILL.md` の nghttp2-sys のビルド方式が `cmake` と記載されているが、実際は `shiguredo_cmake` に切り替え済み（`crates/nghttp2-sys/Cargo.toml` の build-dependencies と `crates/nghttp2-sys/build.rs` の `shiguredo_cmake::set_cmake_env` / `shiguredo_cmake::Config` で確認できる）
3. 同一の古い `cmake` 記述が `README.md` と `crates/nghttp2-sys/README.md` にも残っている（後者はビルド方式の説明に加えて「必要なツール」リストにも `- cmake` がある）。また、`shiguredo_cmake` が要求する `curl` / `tar` / SHA256 検証コマンドが「必要なツール」リストにない
4. `skills/shiguredo-http2/SKILL.md` の「既知の未対応 / 制限」が存在しない `src/connection/mod.rs` を参照している（実際の構成は `src/connection.rs` と `src/connection/headers.rs`）
5. `skills/shiguredo-http2/SKILL.md` の `ConnectionState` 表が `Idle` / `Open` / `Closing` / `Closed` と記載されているが、実際の `ConnectionState` は `WaitingPreface` / `Active` / `GoawaySent` / `GoawayReceived` / `Closed`
6. `skills/shiguredo-http2/SKILL.md` の Event 表は、`DataDiscarded` の行と `StreamClosed` の行の間に「補充は RFC 9113 Section 6.9.1 の受信ウィンドウ上限 (2^31-1) を超えない範囲で行うこと (超過すると `FLOW_CONTROL_ERROR` の接続エラーになる)。」という段落が挿入されており、Markdown の表が分断されている
7. `CHANGES.md` には draft-15 参照の履歴エントリが複数残っている。これらは当時の仕様を記録した履歴であり、本 issue では変更しない。draft-16 への移行記載の追加は 0199 の範囲

## 設計方針

- SKILL.md のバージョン番号を `Cargo.toml` の `version` に合わせる（現時点では `2026.1.0-canary.18`）
- SKILL.md の nghttp2-sys ビルド方式の記述を `shiguredo_cmake` に修正する（`crates/nghttp2-sys` の実際の実装に合わせる）
- README.md と crates/nghttp2-sys/README.md の `cmake` 記述も `shiguredo_cmake` に修正する。crates/nghttp2-sys/README.md の「必要なツール」リストから `- cmake` を削除し、`shiguredo_cmake` が要求する `curl` / `tar` / SHA256 検証コマンドを追記する
- SKILL.md の「既知の未対応 / 制限」の `src/connection/mod.rs` 参照を `src/connection.rs` に修正する
- SKILL.md の `ConnectionState` 表を実際のバリアント（`WaitingPreface` / `Active` / `GoawaySent` / `GoawayReceived` / `Closed`）に修正する
- SKILL.md の Event 表の「補充は RFC 9113 Section 6.9.1 ...」の段落を表の後に移動し、表が正しく描画されるようにする
- 0198 が `ConnectionState` / Event 表を本 issue の範囲としているため、本 issue で修正する
- CHANGES.md の既存の draft-15 参照エントリは履歴として保全し、本 issue では CHANGES.md を変更しない

## 完了条件

- SKILL.md のバージョン番号が `Cargo.toml` と一致していること
- SKILL.md のビルド方式の記述が実際の実装（`crates/nghttp2-sys` の `shiguredo_cmake` 使用）と一致していること
- README.md と crates/nghttp2-sys/README.md のビルド方式の記述が実際の実装と一致していること
- crates/nghttp2-sys/README.md の「必要なツール」リストから `- cmake` が削除され、`curl` / `tar` / SHA256 検証コマンドが記載されていること
- SKILL.md の「既知の未対応 / 制限」のモジュールパス参照が実際の構成と一致していること
- SKILL.md の `ConnectionState` 表が実際のバリアントと一致していること
- SKILL.md の Event 表が正しい Markdown 表として描画され、`Event` の全バリアントと一致していること
- 本 issue の作業で CHANGES.md に差分がないこと（既存の draft-15 参照エントリが保全されていること）
- `cargo test --workspace` が全件通過すること
