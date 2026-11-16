# 空 WT_STREAM capsule の送受信契約を仕様に合わせる

- Created: 2026-11-16
- Completed: {YYYY-MM-DD}
- Branch: feature/fix-wt-empty-stream-contract
- Polished: 2026-11-17

## 目的

draft-ietf-webtrans-http2-16 Section 6.4 は「空の WT_STREAM capsule は、ストリームを開くか閉じる場合を除いて使用してはならない (MUST NOT)。エンドポイントは、ストリームを開始も終了もしない空の WT_STREAM capsule を、セッションエラーとして扱ってもよい (MAY)」と定める。実装はピアに既知のストリームへ空の非 FIN WT_STREAM を送信でき、tokio の `recv` は空 FIN を `Ok(Some(Vec::new()))` として返すため、送受信の契約を仕様に合わせる。

## 現状

`src/webtransport.rs` の `WtSession::send_stream_data` は `data.is_empty()` を検査せず、`fin = false` でも `Capsule::WtStream` を生成する。`WtStream::send_data` はサイズ 0 でも送信状態を `Send` に進める (RFC 9000 Section 3.1 の Ready→Send と同じ) ため、`send_stream_data` に空データの検査がないと同じストリームへの 2 回目以降の空 WT_STREAM も通る。ピアに既知かどうかの追跡はない。

`src/webtransport.rs` の `WtSession::handle_stream_data` には、受信側の空の非 FIN capsule を `has_received_data` で拒否する検査が既にある (draft-ietf-webtrans-http2-16 Section 6.4 の MAY に基づく部分的な検査)。本 issue ではこの受信側の検査を変更しない。

`crates/tokio-http2/src/webtransport.rs` の `WtBidiStream::recv` / `WtUniRecvStream::recv` は `fin = true` でも `Ok(Some(data))` を返す。空 FIN を受けると `Ok(Some(Vec::new()))` になり、`examples/wt_server` の `handle_bidi` / `handle_uni` はそれを通常データとして `send(data, false)` するため、example 自身が空の非 FIN WT_STREAM を送信する。

## 設計方針

- ストリームがピアに既知かを追跡する。ピアに既知とは、ピア開始ストリームであるか、自らがそのストリームへ WT_STREAM / WT_MAX_STREAM_DATA / WT_STOP_SENDING を送信済みであるか、自らが同一型・同方向の上位 ID へ送信したことでピアが下位 ID を暗黙に開いた状態をいう (RFC 9000 Section 2.1 / Section 3.2 / Section 3.3 / draft-ietf-webtrans-http2-16 Section 6.3 / Section 6.6 / Section 6.7)。
- 空かつ FIN なしの送信は、その capsule がピアにとって新しいストリームを開く場合 (ピアに未通知のストリームへの最初の WT_STREAM) だけ許可する。ピアに既知のストリームへの空かつ FIN なしの送信は `stream_state_error` を返す。このエラーはローカル検証エラーであり、セッションは継続する。空 FIN はストリームを閉じるため、ピアに既知でも許可する。
- 空データの検査は既存の `can_send` / `has_send_part` の検査の後、フロー制御検査の前に行い、拒否時に送信状態・送信済みバイト数・出力を変更しない (既存のフロー制御検査と同じ扱い)。`WtStream::send_data` のサイズ 0 での `Send` 遷移は QUIC の状態遷移どおりなので変更しない。
- `recv` は空 FIN を `Ok(None)` として返す。既存の「`Ok(None)` は FIN」という契約を維持し、戻り値型は変更しない。非空の FIN は既存どおり `Ok(Some(data))` を返し、次の呼び出しで `Ok(None)` を返す。
- `examples/wt_server` は空データをエコーしないように修正する。
- Sans I/O と tokio の各層に空 FIN・空非 FIN のテストを追加する。example のエコー経路は tokio の E2E テストで同じループを実行し、空データを送信しないことを確認する。
- `WtSession::send_stream_data` と、それを委譲する tokio の `WtBidiStream::send` / `WtUniSendStream::send` の rustdoc に、空の非 FIN WT_STREAM を送れる条件と `stream_state_error` を返す条件を明記する。

## 完了条件

- ピアに既知のストリーム (ピア開始ストリーム、自らが WT_STREAM / WT_MAX_STREAM_DATA / WT_STOP_SENDING を送信済みのストリーム、自らが同一型・同方向の上位 ID へ送信したことで暗黙に開かれたストリーム) への空の非 FIN WT_STREAM の送信が `stream_state_error` になり、拒否時に送信状態・送信済みバイト数・出力が変化せず、拒否後もセッションが継続すること。
- ピアに未通知のローカル開始ストリームへの最初の空の非 FIN WT_STREAM が許可され、capsule が送信されること (ストリームを開くための空 WT_STREAM を維持する。0179 が reset 前に空 WT_STREAM を送る方針を採る場合も、この契約に依存する)。
- 空 FIN の送信が、ピアに既知のストリームでも許可されること。
- tokio の `WtBidiStream::recv` / `WtUniRecvStream::recv` が空 FIN を `Ok(None)` として返すこと (Sans I/O の `WtEvent::StreamData` は `fin` を明示済み)。
- `examples/wt_server` が空の非 FIN WT_STREAM を送信しないこと。
- 空 FIN / 空非 FIN の送受信テストが Sans I/O と tokio の層に追加されていること。
- `WtSession::send_stream_data` と tokio の `WtBidiStream::send` / `WtUniSendStream::send` の rustdoc に新しい契約が明記されていること。
- `CHANGES.md` は `CODEBASE.md` の指示に従い変更しないこと。
- `cargo fmt --all -- --check` / `cargo clippy --workspace --all-targets -- -D warnings` / `cargo test --workspace` が通ること。
