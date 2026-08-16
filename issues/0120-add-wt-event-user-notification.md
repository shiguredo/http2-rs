# WtEvent の SessionClosed / SessionDraining をユーザーに通知する

- Created: 2026-08-16
- Completed: {YYYY-MM-DD}
- Branch: feature/add-wt-event-user-notification
- Polished: {YYYY-MM-DD}

## 目的

ピアから `WT_CLOSE_SESSION` や `WT_DRAIN_SESSION` を受信した際に、ユーザーコードに通知する仕組みを追加する。

## 現状

`tokio-http2` の `dispatch_wt_event()`（`crates/tokio-http2/src/webtransport.rs` の `DriverState` 型の `dispatch_wt_event` メソッド）では、以下の WtEvent がユーザーに通知されていない:

- `WtEvent::StopSending` — コメントに「送信側にシグナルを伝達しない」とある
- `WtEvent::SessionDraining` — コメントに「ユーザーに通知する手段は将来追加」とある
- `WtEvent::SessionClosed` — コメントに「ユーザーに通知する手段は将来追加」とある

ピアが `WT_DRAIN_SESSION` や `WT_CLOSE_SESSION` を送信しても、サーバー側のユーザーコードはこれらのイベントを検知できない。

## 設計方針

- `WtServerSession` に `SessionClosed` / `SessionDraining` を通知するチャネルまたはメソッドを追加する
- `dispatch_wt_event()` でこれらのイベントを受信した際に、適切なチャネル経由でユーザーに通知する
- `StopSending` については、送信側ストリームへのシグナル伝達方法を検討する（本 issue のスコープ外とするか検討）

## 完了条件

- `SessionClosed` イベントがユーザーコードに通知されること
- `SessionDraining` イベントがユーザーコードに通知されること
- 統合テストが追加されていること
- `cargo test -p tokio-http2` が全件通過すること
