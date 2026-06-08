# WT_CLOSE_SESSION 送受信時の HTTP/2 END_STREAM 自動送信が未実装

- Priority: High
- Created: 2026-06-08
- Polished: 2026-06-08
- Model: deepseek-v4-pro
- Branch: feature/fix-wt-close-session-end-stream

## 目的

draft-ietf-webtrans-http2-14 Section 6.12 の MUST 要件に従い、WT_CLOSE_SESSION カプセル送信時に HTTP/2 END_STREAM で half-close し、受信時にも END_STREAM で応答（close）するようにする。

WT_CLOSE_SESSION はセッション終了の追加情報を伝達する任意のカプセルであり、本質的なセッション終了手段は HTTP/2 CONNECT ストリームの END_STREAM である（draft-ietf-webtrans-http2-14 Section 3.4, L346-L364）。

## 優先度根拠

仕様上の MUST 要件違反が 2 箇所ある。WT_CLOSE_SESSION を送信しても HTTP/2 ストリームが half-close されず、ピアがセッション終了を正しく検出できない。また受信側も END_STREAM で応答しないため、ピアの half-close に対してストリームが完全 close されない。

## 現状

draft-ietf-webtrans-http2-14 Section 6.12 (L1360-L1365):

> An endpoint that sends a WT_CLOSE_SESSION capsule MUST then half-close the stream by sending an HTTP/2 frame with the END_STREAM flag set (Section 5.1 of [HTTP2]).
>
> The recipient MUST close the stream upon receipt of the capsule by replying with an HTTP/2 frame with the END_STREAM flag set; note that it does not need to send a WT_CLOSE_SESSION capsule in response.

現在の実装は両 MUST 要件を満たしていない:

### 送信側（WT_CLOSE_SESSION 送信後に END_STREAM 未送信）

- `crates/tokio-http2/src/webtransport.rs:722-733` — `DriverCmd::Close` の処理。`flush_wt_output()` で WT_CLOSE_SESSION カプセルを送信後、`return Ok(false)` でドライバーを終了するのみ。CONNECT ストリームに END_STREAM を送っていない。
- `crates/tokio-http2/src/webtransport.rs:351-361` — `WtSessionHandle::close()` も同じ `DriverCmd::Close` を発行するため同様の問題がある。
- Sans I/O 層の `WtSession::close()` (`src/webtransport/mod.rs:446-462`) は WT_CLOSE_SESSION カプセルをエンコードして Closed 状態に遷移するだけ。

### 受信側（WT_CLOSE_SESSION 受信後に END_STREAM 未応答）

- `src/webtransport/mod.rs:685-691` — `Capsule::WtCloseSession` 受信時に `self.state = Closed` にして `WtEvent::SessionClosed` を発行するのみ。
- `crates/tokio-http2/src/webtransport.rs:916-918` — `dispatch_wt_event` で `WtEvent::SessionClosed` が明示的に無視されている（コメント: `// 何もしない (ユーザーに close/drain を通知する手段は将来追加)`）。
- **特に問題**: `end_stream=true` の DATA フレームで WT_CLOSE_SESSION カプセルが同梱された場合 (`crates/tokio-http2/src/webtransport.rs:789-792`)、`handle_event` が `return Err(Error::ConnectionClosed)` で即座に driver を終了する。この時点でカプセルは処理済みだが、応答 END_STREAM を送信する機会が一切なくなる。

## 完了条件

- WT_CLOSE_SESSION 送信時に CONNECT ストリームに END_STREAM が自動送信されること（half-close）
- WT_CLOSE_SESSION 受信時に CONNECT ストリームに END_STREAM が自動返信されること（close）
- `end_stream=true` と WT_CLOSE_SESSION が同一 DATA フレームで届いた場合でも END_STREAM が正しく返信されること
- 受信側は WT_CLOSE_SESSION カプセルを返送しないこと（仕様注記 L1364-L1365 に従う）
- 送信側は END_STREAM 送信後、当該ストリームで追加送信を行わないこと
- PBT および単体テストで検証されていること

## エッジケース

- **end_stream=true + WT_CLOSE_SESSION 同梱**: ピアが同じ DATA フレームで WT_CLOSE_SESSION と END_STREAM を送ってきた場合。受信側も END_STREAM を返信する必要がある。現在の `handle_event` は即座に driver を終了するため、END_STREAM 返信前に終了してしまう。
- **close() の重複呼び出し**: `WtSession::close()` は 2 回目で `SessionStateError` を返す。tokio-http2 層で `DriverCmd::Close` が 2 回到達した場合、1 回目の END_STREAM 送信は保証され、2 回目は Sans I/O 層のエラーを無視する。
- **drain → close 遷移**: drain 後に close が呼ばれた場合、WT_DRAIN_SESSION 送信後に WT_CLOSE_SESSION + END_STREAM が続く。これは仕様上正当な遷移 (drain → close) であり、特別な対応は不要。
- **自分が先に close した後のピアからの WT_CLOSE_SESSION 受信**: `src/webtransport/mod.rs:687` で `state != Closed` の場合のみ `SessionClosed` イベント発行するため、二重受信時に END_STREAM を二重送信することはない。送信側は既に END_STREAM 送信済みのため問題ない。
- **WT_CLOSE_SESSION なしで END_STREAM のみ受信**: draft-ietf-webtrans-http2-14 Section 6.12 L1367-L1370 により error_code=0 の WT_CLOSE_SESSION と等価。現在の `handle_event` はこのケースでも `return Err(Error::ConnectionClosed)` で driver を即終了するが、END_STREAM 返信は不要（既に相手が half-closed、または既に half-closed 状態）。

### 本 issue のスコープ外

- **既存 WT ストリームの暗黙クローズ**: WT_CLOSE_SESSION でセッション終了時に、開いている WT ストリーム群の明示的なリセット通知やアプリケーションへの伝達は、本 issue の範囲外とする。現在の `WtSession::close()` は `state = Closed` に遷移し、以降の `send_stream_data()` は `SessionStateError` で失敗する。完全なグレースフルクローズが必要な場合は `drain() → close()` のパスを使用する。

## 解決方法

### Sans I/O 層 (`src/webtransport/mod.rs`)

Sans I/O 層への変更は不要。理由:
- 送信側: `handle_cmd` が `close()` を呼んだ事実を driver 自身が把握しているため。
- 受信側: `WtEvent::SessionClosed` が既存のイベント機構で発行済みのため。
- `WtSession::close()` は既に WT_CLOSE_SESSION カプセルのエンコードと state 遷移を正しく行っている。

### tokio-http2 層 (`crates/tokio-http2/src/webtransport.rs`)

変更箇所は 3 つ:

#### 1. DriverCmd::Close 処理 (L722-L733): END_STREAM 送信を追加

```rust
DriverCmd::Close { error_code, reason, ack } => {
    let res = self.wt_session.close(error_code, &reason).map_err(wt_err);
    let _ = ack.send(res.clone());
    if res.is_ok() {
        self.flush_wt_output().await?;
        // draft-ietf-webtrans-http2-14 Section 6.12 (L1360-L1361):
        // WT_CLOSE_SESSION 送信後は MUST half-close the stream。
        // RFC 9113 §6.1: 空 DATA フレームに END_STREAM フラグを立てる。
        // RFC 9113 §6.9.1: 空 DATA + END_STREAM はフロー制御ウィンドウ空きなしでも送信可能。
        self.conn
            .send_data(self.connect_stream_id, vec![], true)
            .await?;
    }
    return Ok(false);
}
```

- `flush_wt_output()` は複数 DATA フレーム（`end_stream=false`）を生成しうる。WT_CLOSE_SESSION カプセルを含む最後の DATA フレームの後に、改めて空 DATA + END_STREAM を送信する。別フレームになることは仕様上問題ない。
- `ack` の送信タイミングを `flush_wt_output` の前に移動し、close の成功／失敗を呼び出し側に早期通知する。
- END_STREAM 送信失敗時は `?` でエラー伝播し、driver がエラー終了する。

#### 2. WtEvent::SessionClosed 受信時の END_STREAM 返信 (L916-L918)

`handle_event` の `poll_event` ループ (L763-L765) の後に処理を追加する:

```rust
// handle_event 内、poll_event ループの後
if self.wt_session.state() == WtSessionState::Closed
    && !self.responded_end_stream_on_close
{
    self.conn
        .send_data(self.connect_stream_id, vec![], true)
        .await?;
    self.responded_end_stream_on_close = true;
    return Err(Error::ConnectionClosed);
}
```

`DriverState` に `responded_end_stream_on_close: bool` フィールドを追加し、END_STREAM 返信の重複を防止する。

#### 3. end_stream=true 同梱時の早期 driver 終了対策 (L789-L792)

`handle_event` で `end_stream=true` を受信した場合、以下の順序で処理する必要がある:

1. `feed` + `process` + `poll_event` ループ（既存の L760-L765）
2. WT_CLOSE_SESSION カプセルが含まれていた場合、上記 #2 の END_STREAM 返信処理
3. その後に `return Err(Error::ConnectionClosed)`

現在のコードは `#1` の直後に `if end_stream { return Err(Error::ConnectionClosed); }` (L789-L792) があるため、`#2` の前に driver が終了する。この順序を入れ替える。

### テスト戦略

#### 単体テスト (`tests/test_webtransport/`)

- **送信側**: `WtSession::close()` 後の state が `Closed` であること。`close()` の重複呼び出しが `SessionStateError` になること。
- **受信側**: WT_CLOSE_SESSION カプセルデコード後、`WtEvent::SessionClosed` が発行されること。2 回目の WT_CLOSE_SESSION 受信時はイベントが発行されないこと。

#### PBT (`pbt/tests/prop_webtransport/main.rs`)

- 任意の WT_CLOSE_SESSION カプセル（error_code + reason）をデコード → エンコード → 再デコードのラウンドトリップが一致すること。
- セッションライフサイクル PBT: drain → close、close 単独、ピアからの close 受信 → 自分からの close 送信など、状態遷移の組み合わせでパニックしないこと。

#### tokio-http2 層 (`crates/tokio-http2/tests/test_webtransport.rs`)

- WT_CLOSE_SESSION 送信後にピア側が END_STREAM を受信すること（E2E）
- WT_CLOSE_SESSION 受信後に END_STREAM が返信されること（E2E）

## 参照仕様

- draft-ietf-webtrans-http2-14 Section 3.4 (Session Termination and Error Handling), L346-L364
- draft-ietf-webtrans-http2-14 Section 6.12 (WT_CLOSE_SESSION Capsule), L1321-L1370
- RFC 9113 Section 5.1 (Stream States)
- RFC 9113 Section 6.1 (DATA フレーム END_STREAM flag)
- RFC 9113 Section 6.9.1 (空 DATA フレーム + END_STREAM の送信許容)
- RFC 9113 Section 8.5 (CONNECT メソッド)
