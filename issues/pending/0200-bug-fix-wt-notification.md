# driver の StopSending と正常終了をアプリへ通知する

- Created: 2026-11-16
- Completed: {YYYY-MM-DD}
- Branch: feature/fix-wt-notification
- Polished: {YYYY-MM-DD}

## 目的

open issue 0120 は `WtEvent::SessionClosed` / `SessionDraining` の通知を扱うが、`WtEvent::StopSending` は明示的に「別の設計判断」として対象外にしている。また driver は正常なピア終了も `Err(Error::ConnectionClosed)` として扱い、ピアの WT_RESET_STREAM は `Error::InvalidArgument` に変換して `error_code` も構造化されない。アプリが停止要求・正常終了・異常終了を区別できるようにする。

## 現状

`crates/tokio-http2/src/webtransport.rs` の `dispatch_wt_event` は `WtEvent::StopSending { .. }` を「現在の API では送信側にシグナルを伝達しない」として無視する。

`DriverState::handle_event` はピアの END_STREAM、WT_CLOSE_SESSION、CONNECT の `StreamReset` / `StreamClosed` のいずれも `Err(Error::ConnectionClosed)` で終了する。`WtSessionParts::driver` を await しても正常終了と異常終了を区別できない。

`WtBidiStream::recv` / `WtUniRecvStream::recv` はピアの WT_RESET_STREAM を `Error::InvalidArgument("stream reset by peer")` に変換し、`error_code` を文字列に埋め込む。`WtEvent::StreamReset` の `reliable_size` は driver で捨てられる。

## 設計方針

- `WtSessionEvent` または専用の通知チャネルを追加し、`StopSending` を送信側 API へ伝える。
- 正常終了は `Ok(())`、異常終了は `Err` に分け、終了理由を enum で返す。
- ピアリセット用の専用エラー variant を追加し、`error_code` と `reliable_size` を構造化して返す。
- 0120 の `WtSessionEvent` 設計と重複しないよう、StopSending と終了理由に範囲を限定する。
- StopSending / 正常終了 / 異常終了 / ピアリセットのテストを追加する。

## 完了条件

- ピアの WT_STOP_SENDING をアプリが観測できること。
- 正常終了と異常終了が `driver` の結果または終了理由で区別できること。
- ピアリセットの `error_code` / `reliable_size` が構造化されて取得できること。
- 0120 の SessionClosed / SessionDraining 通知と整合していること。
- 上記のテストが追加されていること。
- `cargo fmt --all -- --check` / `cargo clippy --workspace --all-targets -- -D warnings` / `cargo test --workspace` が通ること。
