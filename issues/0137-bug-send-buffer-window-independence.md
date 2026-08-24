# 送信バッファ容量をピアの SETTINGS_INITIAL_WINDOW_SIZE に固定し、超過時に接続エラーを返す

- Created: 2026-08-24
- Completed: {YYYY-MM-DD}
- Branch: feature/fix-send-buffer-window-independence
- Polished: {YYYY-MM-DD}

## 目的

`Stream` の送信バッファ容量 (`SendBuffer`) がピアの SETTINGS_INITIAL_WINDOW_SIZE と同値に固定され、ウィンドウ枯渇時のバッファ超過を接続エラー (GOAWAY 相当) として返す問題を修正する。RFC 9113 Section 6.9 のフロー制御は「送信不可なら送らない (保留)」であり、ウィンドウ不足はエラーではない。

## 現状

`src/stream.rs` の `Stream::new` は `SendBuffer::new(send_initial_window_size as usize)` で送信バッファを生成する (`SendBuffer::new` は `src/stream/buffer.rs`)。ピアが `SETTINGS_INITIAL_WINDOW_SIZE=0` を広告した場合、バッファ max が 0 になり、あらゆる `Connection::send_data` (`src/connection.rs` の `queue_data`) が `connection_error(FlowControlError)` (GOAWAY 送信を要求する接続エラー種別) を返す。

RFC 9113 Section 6.9 のフロー制御は「送信ウィンドウが枯渇したら送信を保留する」設計であり、ウィンドウ回復後に送信を再開できる。`Connection::flush_stream_data` は保留モデルで実装されているが、バッファ上限がウィンドウと同値に固定されているため、保留が機能せず接続エラーになる。`SETTINGS_INITIAL_WINDOW_SIZE` はストリームレベルの初期ウィンドウであり、送信バッファ容量とは独立の概念である。

## 設計方針

- 送信バッファの容量をピアの `SETTINGS_INITIAL_WINDOW_SIZE` から分離する (独立した上限値を持つか、`Connection::queue_data` のバッファ超過をストリームエラー / ローカル API エラーに変更する)
- `SETTINGS_INITIAL_WINDOW_SIZE=0` を広告したピアに対して送信データがエラーにならず、WINDOW_UPDATE 受信後に送信されることを検証するテストを追加する

## 完了条件

- ピアが `SETTINGS_INITIAL_WINDOW_SIZE=0` を広告しても `send_data` が接続エラーにならないこと
- ウィンドウ回復 (WINDOW_UPDATE) 後に滞留データが送信されること
- テストが追加され、`cargo test --all` が通過すること
