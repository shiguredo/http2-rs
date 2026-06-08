# WT_STOP_SENDING 受信時の WT_RESET_STREAM 自動応答が未実装

- Priority: High
- Created: 2026-06-08
- Polished: 2026-06-08
- Model: deepseek-v4-pro
- Branch: feature/fix-wt-stop-sending-auto-reset

## 目的

draft-ietf-webtrans-http2-14 Section 6.3 の MUST 要件に従い、WT_STOP_SENDING カプセル受信時に、対象ストリームが Ready または Send 状態であれば WT_RESET_STREAM カプセルを同一 error_code で自動応答する。

## 優先度根拠

仕様上の MUST 要件違反。ピアが WT_STOP_SENDING を送信しても、該当ストリームの送信側に WT_RESET_STREAM が返らず、ストリームのクリーンアップが正しく行われない。

## 現状

draft-ietf-webtrans-http2-14 Section 6.3 (L874-L877):

> As defined in Section 3.5 of [RFC9000], the recipient of a WT_STOP_SENDING capsule sends a WT_RESET_STREAM capsule in response, including the same error code, if the stream is the "Ready" or "Send" state.

RFC 9000 Section 3.5 (L984-L985) ではこの送信が MUST で規定されている。error_code のコピーは SHOULD (L992-L993)。

現在の実装:

- `src/webtransport/mod.rs:616-635` — `Capsule::WtStopSending` のハンドラは `set_stop_sending_received()` の呼び出しと `WtEvent::StopSending` のイベント発行のみ。WT_RESET_STREAM カプセルの自動生成・出力は一切行っていない。
- `crates/tokio-http2/src/webtransport.rs:910-911` — `dispatch_wt_event` の `StopSending` 分岐には「現在の API では送信側にシグナルを伝達しない (将来の拡張)」のコメントがある。これはアプリケーション通知に関するもので、本 issue の自動リセット要件とは別の関心事だが、Sans I/O 層が自動応答すれば tokio-http2 層の修正は不要。

## 完了条件

- Ready 状態のストリームに WT_STOP_SENDING 受信 → WT_RESET_STREAM が出力バッファに含まれること
- Send 状態のストリームに WT_STOP_SENDING 受信 → WT_RESET_STREAM が出力バッファに含まれること
- WT_RESET_STREAM の `error_code` が WT_STOP_SENDING の `error_code` と同一であること
- DataSent / DataRecvd / ResetSent / ResetRecvd の各状態では WT_RESET_STREAM が生成されないこと
- 存在しないストリーム ID では何もせず、WtEvent::StopSending を引き続き発行すること
- 重複 WT_STOP_SENDING 受信のエラー検出（既存挙動）が維持されていること
- PBT および単体テストで検証されていること

### DataSent 状態の扱い

RFC 9000 Section 3.5 (L986-L987) は DataSent での STOP_SENDING 受信時に RESET_STREAM の MAY defer を認めている。しかし draft-ietf-webtrans-http2-14 Section 6.3 は「Ready または Send 状態」のみを明示しており、DataSent への言及がない。また HTTP/2 版ではパケット単位の ack が存在せず defer の概念がないため、本実装では DataSent で auto-reset しない方針とする。

## 解決方法

### Sans I/O 層 (`src/webtransport/mod.rs`)

`handle_capsule` の `Capsule::WtStopSending` 分岐 (L616-L635) を修正する。変更は 1 箇所のみ。

```rust
Capsule::WtStopSending {
    stream_id,
    error_code,
} => {
    let should_reset = self
        .streams
        .get(&stream_id)
        .is_some_and(|s| s.can_send());

    if let Some(stream) = self.streams.get_mut(&stream_id) {
        if stream.stop_sending_received() {
            return Err(WtError::stream_state_error(
                "duplicate WT_STOP_SENDING received",
            ));
        }
        stream.set_stop_sending_received();
    }

    // draft-ietf-webtrans-http2-14 Section 6.3 + RFC 9000 Section 3.5:
    // Ready または Send 状態のストリームには WT_RESET_STREAM を MUST 応答する。
    // error_code のコピーは RFC 9000 Section 3.5 の SHOULD に従う。
    // 借用回避のため、get_mut スコープを抜けてから reset_stream を呼ぶ。
    if should_reset {
        let _ = self.reset_stream(stream_id, error_code);
    }

    self.events.push_back(WtEvent::StopSending {
        stream_id,
        error_code,
    });
}
```

**借用関係の説明**: `if let Some(stream) = self.streams.get_mut(...)` のブロックを抜ければ `stream` への可変借用は解放される。その後に `self.reset_stream(stream_id, error_code)` を呼び出しても `&mut self` の再借用に競合はない。`reset_stream` は内部で再度 `self.streams.get_mut()` を行うが、最初の借用はブロック終了時にドロップされるため問題ない。

**`let _ =` の理由**: `reset_stream` は `stream.can_send()` が true なら確実に成功する。`should_reset` で事前チェック済みのため、エラーは発生しない。ただし防衛的に戻り値を捨てている。

**`WtEvent::StopSending` は引き続き発行する**: アプリケーション層が送信キューをフラッシュするなどの対応を取れるよう、イベント通知は維持する。auto-reset により Sans I/O 層の処理は完結しているため、イベント受信後に `reset_stream` を呼ぶ必要はない。tokio-http2 層の `dispatch_wt_event` `StopSending` 分岐 (L910-L911) は現状維持でよい。

### tokio-http2 層

修正不要。Sans I/O 層で WT_RESET_STREAM カプセルが出力バッファに自動追加され、既存の `flush_wt_output` で送信される。

### テスト戦略

#### 単体テスト (`tests/test_webtransport/`)

- Ready 状態 + WT_STOP_SENDING → `poll_output()` に WT_RESET_STREAM カプセルが含まれる
- Send 状態 + WT_STOP_SENDING → `poll_output()` に WT_RESET_STREAM カプセルが含まれる
- DataSent 状態 + WT_STOP_SENDING → WT_RESET_STREAM が生成されない
- 存在しないストリーム ID + WT_STOP_SENDING → エラーにならず `WtEvent::StopSending` が発行される
- 重複 WT_STOP_SENDING → `WtError::stream_state_error`（既存挙動の退行確認）

#### PBT (`pbt/tests/prop_webtransport/main.rs`)

- 任意の WT_STOP_SENDING カプセルをデコード → エンコード → 再デコードのラウンドトリップ
- WT_STOP_SENDING 受信 → WT_RESET_STREAM 自動応答 → カプセルが正しい error_code と reliable_size を持つこと（reliable_size = send_offset）

#### Fuzzing

不要。自動応答の検証は状態遷移の正常系であり、fuzzing の対象ではない。

## 参照仕様

- draft-ietf-webtrans-http2-14 Section 6.3 (WT_STOP_SENDING Capsule), L845-L883
- RFC 9000 Section 3.1 (Stream States), L743-L767
- RFC 9000 Section 3.5 (Solicited State Transitions), L980-L995
