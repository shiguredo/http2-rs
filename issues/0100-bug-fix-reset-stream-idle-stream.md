# 明示 reset_stream が idle ストリームに RST_STREAM を送信する問題を修正する

- Priority: Medium
- Created: 2026-08-07
- Completed: {YYYY-MM-DD}
- Branch: feature/fix-reset-stream-idle-stream
- Polished: {YYYY-MM-DD}

## 目的

`Connection::reset_stream` が一度も開かれていない idle ストリームへの明示呼び出しで RST_STREAM フレームを送信する。RFC 9113 Section 6.4 は idle ストリームへの RST_STREAM 送信を MUST NOT で禁止しており、受信したピアは PROTOCOL_ERROR の接続エラーにするため、接続全体が終了してしまう。idle ストリームへの明示リセットは RST_STREAM を送信せずエラーを返すべきである。

## 現状

`src/connection.rs` の `Connection::reset_stream` は `StreamId::Connection` (stream_id = 0) のみを拒否し、`streams` に存在しないストリーム ID に対しては無条件に RST_STREAM フレームを送信する。`streams` に存在しないストリームには「クローズ済み」と「idle (一度も開かれていない)」の 2 種類があり、後者への RST_STREAM 送信は RFC 9113 Section 6.4 の MUST NOT 違反である。

## 設計方針

`Connection::reset_stream` の送信前に idle ストリームの検査を追加し、idle と判定された場合は RST_STREAM を送信せず `Error::stream_error` を返す。idle 判定には `is_idle_stream` を使用する。`is_idle_stream` はストリームを `streams` から即時削除する別の修正 (遅延到着フレームの処理) で `closed_streams` を考慮するようになるため、その修正の後に実装する (クローズ済みストリームを idle と誤判定しないため)。内部呼び出し 3 箇所 (`Connection::process` / `Connection::handle_data` / `Connection::handle_window_update`) はすべて非 idle のストリームに対してのみ呼ばれるため、この変更の影響を受けない。

## 完了条件

- idle ストリーム ID を `Connection::reset_stream` に渡すと RST_STREAM を送信せずエラーを返す
- クローズ済みストリームへの明示リセットは既存挙動 (RST_STREAM 送信のみ) を維持する
- 上記を検証するテストが追加され、全テストが通過する

## 解決方法

1. `src/connection.rs` の `Connection::reset_stream` に idle ストリームの検査を追加する
2. テストを追加する (`tests/test_connection.rs` の単体テストとして):
   - 未開設のストリーム ID に `reset_stream` を呼ぶとエラーになり、RST_STREAM が送信されないことを検証する
   - クローズ済みストリームへの `reset_stream` が既存挙動を維持することを検証する
3. `CHANGES.md` の `## develop` に `[FIX]` エントリを追加する (shiguredo-changelog スキルを参照)
4. `cargo fmt --all -- --check` / `cargo test --workspace` / `cargo clippy --workspace --all-targets -- -D warnings` を実行する

## 参照

- `src/connection.rs` — `Connection::reset_stream` / `is_idle_stream` / `check_not_idle_stream`
- `refs/rfc9113.txt` — Section 6.4 (idle ストリームへの RST_STREAM 送信禁止)
