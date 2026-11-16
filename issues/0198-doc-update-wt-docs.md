# WT 関連の SKILL / README / rustdoc を実装に合わせる

- Created: 2026-11-16
- Completed: {YYYY-MM-DD}
- Branch: feature/update-wt-docs
- Polished: {YYYY-MM-DD}

## 目的

WebTransport 関連の公開ドキュメントに、実装と一致しない記述が複数残っている。SKILL の `initiate` / `webtransport(...)` 必須、README の PADDING 分類 / exporter 経路 / peer_config 例、tokio-http2 README の `drain`、interop README のクライアント SETTINGS、rustdoc の `Draining` / `varint::encoded_len` / TLS 1.2 の記述を実装と仕様に合わせる。SKILL の version / ビルド方式 / `mod.rs` / `ConnectionState` / Event 表は open 0118 の範囲とし、本 issue では扱わない。

## 現状

- `skills/shiguredo-http2/SKILL.md`:
  - `WtSession::initiate()` を「初期 SETTINGS を送信し Active に遷移」とするが、実装は状態を `Active` にするだけ。
  - WebTransport に `webtransport(...)` が必須とするが、`LimitsBuilder` の必須条件は `wt_enabled(true)` と `enable_connect_protocol(true)` のみ。
  - version / ビルド方式 / `src/connection/mod.rs` / `ConnectionState` / Event 表は open 0118 の範囲であり、本 issue では変更しない。
- `README.md`:
  - PADDING を RFC 9297 の Capsule Protocol に分類しているが、PADDING は draft-16 Section 6.1。
  - TLS Keying Material Exporter を `WtServerRequest::accept()` 経由とするが、実際は `WtServerSession::export_keying_material` / `WtSessionHandle::export_keying_material`。
  - WT クライアント例が peer 用に `WtConfig::default()` を渡している。`WtConfig::peer_default()` から開始する規約と矛盾する。
- `crates/tokio-http2/README.md`: `drain()` を「送信中データの完了を待つ」とするが、実装は WT_DRAIN_SESSION を出力して `Ok` を返すだけ。
- `interop/browser/README.md`: `enable_connect_protocol` / `wt_enabled` を「仕様上サーバーが送る設定であり、クライアントからは送られない」とするが、仕様はクライアント送信を禁止していない。
- rustdoc: `WtSessionState::Draining` が「新規ストリーム受付停止」となっているが、実装と draft-16 Section 6.13 は新規ストリーム開設を許可する。`varint::encoded_len` の `# Panics` は実装と一致しない。TLS 1.2 テストのコメントは「TLS 1.2 は malformed」とするが、draft-16 Section 7 は TLS 1.2 + EMS を許容する。

## 設計方針

- SKILL / README / rustdoc の WT 関連記述を実装と一次資料に合わせて修正する。
- `initiate` / `Draining` / `drain` / exporter / peer_config の説明を実装に合わせる。
- TLS 1.2 は「TLS 1.3 のみを許可する実装ポリシー」と明記し、仕様上 malformed という表現を削除する。
- open 0118 が扱う version / ビルド方式 / `mod.rs` / `ConnectionState` / Event 表には変更を加えない。

## 完了条件

- SKILL / README / rustdoc の WT 関連記述が実装と一致していること。
- `WtSessionState::Draining` と `varint::encoded_len` の doc が修正されていること。
- TLS 1.2 の仕様要件と実装ポリシーが doc / テストコメントで一致していること。
- open 0118 の範囲 (version / ビルド方式 / `mod.rs` / `ConnectionState` / Event 表) に変更が入っていないこと。
- `cargo fmt --all -- --check` / `cargo clippy --workspace --all-targets -- -D warnings` / `cargo test --workspace` が通ること。
