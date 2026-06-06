# connection/data.rs と connection/settings.rs がモジュール宣言欠落でデッドコードになっている問題を修正する

- Priority: High
- Created: 2026-06-06
- Model: DeepSeek V4 Pro

## 目的

`src/connection/mod.rs` には `mod headers;` のみ宣言されており、`mod data;` / `mod settings;` が欠落している。そのため `src/connection/data.rs` (289 行) と `src/connection/settings.rs` (203 行) の全コードがコンパイル対象外の完全なデッドコードになっている。`CHANGES.md` には分割完了と記載されているが実態は未了であり、セキュリティ監査の前提を崩す問題。

## 優先度根拠

- `connection/settings.rs` の `handle_settings()` には RFC 8441 §3 の `SETTINGS_ENABLE_CONNECT_PROTOCOL` ダウングレード拒否チェックが欠落している（`mod.rs` 側には実装済み）
- 誤って `settings.rs` を有効化した場合、セキュリティバグを生む
- `settings.rs:109` で `self.remote_settings.initial_window_size` と private フィールドを直接アクセスしており、`mod.rs:1061` の `.initial_window_size().get()` と実装不一致
- コンパイル対象外の別実装が残っている状態は、いずれかの実装を誤って修正するリスクがある

## 現状

- `src/connection/mod.rs:21`: `mod headers;` のみ、`mod data;` / `mod settings;` が欠落
- `src/connection/data.rs`: `send_data` / `queue_data` / `flush_stream_data` / `flush_all_stream_data` / `handle_data` が `mod.rs` と重複定義
- `src/connection/settings.rs`: `initiate` / `send_settings` / `send_initial_connection_window_update` / `handle_settings` / `update_stream_windows` が `mod.rs` と重複定義
- `CHANGES.md:133`: 「`src/connection/mod.rs` を headers / settings / data サブモジュールに分割する」と記載されているが完了していない

## 設計方針

以下のいずれかで対応する:

1. **削除案**: `data.rs` / `settings.rs` を削除し、`mod.rs` の実装を唯一の正とする
2. **完全分割案**: `mod.rs` から `data.rs` / `settings.rs` の該当メソッドを削除し、`mod data;` / `mod settings;` を宣言した上で、`settings.rs` に欠落している `SETTINGS_ENABLE_CONNECT_PROTOCOL` ダウングレード拒否チェックを移植する

削除案がシンプルで推奨。分割案の場合はセキュリティチェックの移植が必須。

## 完了条件

- `src/connection/data.rs` と `src/connection/settings.rs` が削除されるか、正しくモジュール宣言される
- 両ファイルと `mod.rs` の実装重複が解消されている
- `CHANGES.md` の記述と実態が一致している
- 全テストが通過する

## 解決方法

1. 作業ブランチ `feature/fix-connection-dead-code` を切る
2. `src/connection/data.rs` と `src/connection/settings.rs` を削除する
3. `CHANGES.md:133` のエントリを修正する（実態と一致させる）
4. `cargo test --all` を実行して全テスト通過を確認する
