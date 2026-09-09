# 送信バッファ容量をピアの SETTINGS_INITIAL_WINDOW_SIZE に固定し、超過時に接続エラーを返す

- Created: 2026-08-24
- Completed: 2026-09-10
- Branch: feature/fix-send-buffer-window-independence
- Polished: 2026-09-09

## 目的

`Stream` の送信バッファ容量 (`SendBuffer`) がピアの SETTINGS_INITIAL_WINDOW_SIZE と同値に固定され、ウィンドウ枯渇時のバッファ超過を接続エラー (GOAWAY 相当) として返す問題を修正する。RFC 9113 Section 6.9 のフロー制御は「送信不可なら送らない (保留)」であり、ウィンドウ不足はエラーではない。

## 現状

`src/stream.rs` の `Stream::new` は `SendBuffer::new(send_initial_window_size as usize)` で送信バッファを生成する (`SendBuffer::new` は `src/stream/buffer.rs`)。ピアが `SETTINGS_INITIAL_WINDOW_SIZE=0` を広告した場合、バッファ max が 0 になり、非空データの `Connection::send_data` (`src/connection.rs` の `queue_data`) が `connection_error(FlowControlError)` (GOAWAY 送信を要求する接続エラー種別) を返す (空 DATA + END_STREAM は送信できる)。

RFC 9113 Section 6.9 のフロー制御は「送信ウィンドウが枯渇したら送信を保留する」設計であり、ウィンドウ回復後に送信を再開できる。`Connection::flush_stream_data` は保留モデルで実装されているが、バッファ上限がウィンドウと同値に固定されているため、保留が機能せず接続エラーになる。`SETTINGS_INITIAL_WINDOW_SIZE` はストリームレベルの初期ウィンドウであり、送信バッファ容量とは独立の概念である。

## 設計方針

- `Stream::new` の `send_buffer: SendBuffer::new(send_initial_window_size as usize)` をやめ、送信バッファ容量をピアの `SETTINGS_INITIAL_WINDOW_SIZE` から分離する。容量は固定値 `DEFAULT_INITIAL_WINDOW_SIZE` (65535) とする (既存 issue 0041 の「Sans I/O 層の送信バッファ上限 65535」と整合し、既存テストの 60000 バイト送信も収まる)
- `Connection::queue_data` のバッファ超過は、接続エラー (`FlowControlError`) ではなくストリームエラー (`Error::stream_error(ErrorCode::FlowControlError, ...)`) とする。ローカルな資源上限であり、接続全体を GOAWAY で落とす必要はない
- バッファ超過時は部分 push しない。受け入れ可能量を超える `send_data` は全量を拒否し、バッファとストリームの状態を変更しない (原子性)。`SendBuffer::push` が部分挿入して残バイト数を返す現状の挙動は接続エラーで接続が終了する前提だったため問題にならなかったが、ストリームエラーに変えると部分データが残り、再送でデータが破損しうる
- `SETTINGS_INITIAL_WINDOW_SIZE=0` を広告したピアに対して送信データがエラーにならず、WINDOW_UPDATE 受信後に送信されることを検証するテストを追加する
- 0138 は `send_data` が「送信できずにバッファへ積んだだけ」でも Ok を返す挙動を扱う。本 issue は容量の分離と超過時のエラー種別に閉じ、`send_data` の Ok 返却仕様の是非は 0138 に委ねる。0134・0135 とも変更対象が近接するが、本 issue は `Stream::new` と `queue_data` に閉じる

## 完了条件

- ピアが `SETTINGS_INITIAL_WINDOW_SIZE=0` を広告しても、固定容量内の非空データの `send_data` が接続エラーにならず `send_buffer` に滞留すること
- ウィンドウ回復 (WINDOW_UPDATE) 後に滞留データが送信されること
- 送信バッファ容量がピアの初期ウィンドウに依存しないこと (ピアが 0 を広告しても容量が 0 にならない)
- バッファ超過時に部分データがバッファに残らないこと (全量拒否)
- テストが追加され、`cargo test --all` が通過すること

## 解決方法

- `src/stream.rs` の `Stream::new` の送信バッファ容量をピアの `SETTINGS_INITIAL_WINDOW_SIZE` から分離し、固定値 `DEFAULT_INITIAL_WINDOW_SIZE` (65535) とした。ピアが 0 を広告しても容量が 0 にならない
- `src/stream/buffer.rs` の `SendBuffer` に `can_push` を追加し、`src/connection.rs` の `queue_data` は全量を追加できる場合のみ `push` するようにした (部分挿入を避ける原子性)。バッファ超過は接続エラーではなく `Error::stream_error(ErrorCode::FlowControlError, ...)` のストリームエラーとした
- `send_data` と `Stream::new` の doc に固定容量とエラー契約を追記し、`crates/tokio-http2` の陳腐化したコメントを修正した
- `tests/test_connection.rs` に、ピアが 0 を広告してもデータが滞留し WINDOW_UPDATE 後に送信されること、固定容量超過がストリームエラーになり部分挿入されないこと、容量がピアウィンドウに依存せず固定であること、累積容量が既存データ長を含めて判定されることを検証するテストを追加した
- `CHANGES.md` の `## develop` に `[FIX]` エントリを追加した

## 残懸念

- `crates/tokio-http2/src/webtransport.rs` の `flush_wt_output` が WT 出力全体を 1 回の `send_data` で送るため、ピアが 65535 超の初期ウィンドウを広告する構成で大きな WT 送信がストリームエラーになる退行がある。本 issue の変更対象外のため別 issue (`issues/0144-bug-wt-flush-output-split-large-send.md`) で対応する
