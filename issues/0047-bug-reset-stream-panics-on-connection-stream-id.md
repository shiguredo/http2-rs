# Connection::reset_stream が StreamId::Connection でパニックするバグを修正する

- Priority: High
- Created: 2026-05-24
- Model: Opus 4.7
- Branch: feature/fix-reset-stream-panic

## 目的

`Connection::reset_stream` に `StreamId::Connection` (stream_id = 0) を渡すと `.expect()` でパニックする。公開 API がパニックすることは許容されず、`Result::Err` を返すべきである。

## 優先度根拠

High。公開 API のパニックはライブラリ利用者のプロセスを異常終了させるため、最優先で修正する。

## 現状

`src/connection/mod.rs:761-773`:

```rust
pub fn reset_stream(&mut self, stream_id: StreamId, error_code: ErrorCode) -> Result<()> {
    if let Some(stream) = self.streams.get_mut(&stream_id.as_u32()) {
        stream.state_machine_mut().send_rst_stream();
    }

    let nz_stream_id = stream_id
        .non_zero()
        .expect("RST_STREAM requires non-zero stream ID");
    let rst_frame = RstStreamFrame::new(nz_stream_id, error_code.as_u32());
    self.send_frame(&Frame::RstStream(rst_frame))?;

    Ok(())
}
```

`stream_id` が `StreamId::Connection` の場合、`.non_zero()` が `None` を返し、`.expect()` でパニックする。

### 再現手順

`fuzz_connection_interactive` の fuzzing (30 秒実行) で検出された。再現アーティファクト: `fuzz/artifacts/fuzz_connection_interactive/crash-a93c8f4004bdf6be2da1178a71349333022584ba`

## 設計方針

RFC 9113 §6.4: RST_STREAM は非ゼロストリーム ID に関連付けなければならない。stream_id = 0 は PROTOCOL_ERROR として `Result::Err` を返す。

`.expect()` を削除し、`stream_id.non_zero()` が `None` の場合にエラーを返す。

## 完了条件

- [ ] `Connection::reset_stream` に `StreamId::Connection` を渡したときにパニックせず `Err` を返す
- [ ] `fuzz_connection_interactive` のクラッシュアーティファクトを再実行してパニックしないことを確認する
- [ ] 既存テストが全て通る

## 解決方法

`src/connection/mod.rs` の `reset_stream` メソッドで `.expect()` を `ok_or_else(|| ...)` に置き換える。
