# with_separate_windows が受信側初期ウィンドウサイズを誤って参照するバグを修正する

Created: 2026-05-14
Priority: High
Model: deepseek-v4-pro

## 対象

- `src/flow_control.rs`
- `pbt/tests/prop_flow_control.rs`
- `tests/test_flow_control.rs` (新設)

## バグ内容

### 場所

`src/flow_control.rs:39-45`

### 内容

`with_separate_windows(send_initial, recv_initial)` が `initial_window_size` フィールドに `send_initial` のみを格納する。しかし `should_send_window_update()` (line 172-175) と `window_update_increment()` (line 179-187) はこの値を **受信側初期値** として使って WINDOW_UPDATE 発行タイミングと増分を計算する。

### 根拠

RFC 9113 Section 5.2:

> The initial window size for the stream-level flow control is set by the peer using the SETTINGS_INITIAL_WINDOW_SIZE setting. The connection-level flow control window is also set by the peer.

`Stream::new()` (connection/mod.rs) は `FlowControl::with_separate_windows(remote_settings.initial_window_size, local_settings.initial_window_size)` を呼ぶ。送信ウィンドウはリモート側、受信ウィンドウはローカル側の値で初期化される。`should_send_window_update()` と `window_update_increment()` は「受信ウィンドウが半分以下になったら WINDOW_UPDATE を発行」するための閾値計算であり、基準とすべきは `recv_initial` である。しかし現在は `send_initial` が格納されているため、`send_initial != recv_initial` の場合に閾値が誤る。

### 再現手順

`send_initial=65535`, `recv_initial=131072` の場合:
- `should_send_window_update()` は `recv_window < 65535/2` で判定する
- 正しくは `recv_window < 131072/2 = 65536` で判定すべき
- 結果として、実際の半分である 65536 の閾値より遥かに遅く (32767 まで低下しないと) 発火しない

### 修正方針

`initial_window_size` を削除し、`send_initial` と `recv_initial` の 2 つのフィールドに分割する:

```rust
pub struct FlowControl {
    send_window: i64,
    recv_window: i64,
    send_initial: u32,
    recv_initial: u32,
}
```

- `should_send_window_update()` は `recv_initial` を基準にする:
  ```rust
  pub fn should_send_window_update(&self) -> bool {
      self.recv_window < i64::from(self.recv_initial / 2)
  }
  ```
- `window_update_increment()` も同様に `recv_initial` を基準にする
- `update_initial_window_size()` は `send_initial` を参照して delta 計算する (現状維持)
- `initial_window_size()` getter は削除し、代わりに `send_initial()` / `recv_initial()` の 2 つの getter を追加する
- `with_separate_windows()` は `send_initial` と `recv_initial` をそれぞれ正しいフィールドに格納する
- `new()` は送受信共通のため `send_initial` と `recv_initial` に同じ値を格納する

## 修正後のテスト

### PBT

`pbt/tests/prop_flow_control.rs:34` の `prop_assert_eq!(fc.initial_window_size(), send_initial)` を `prop_assert_eq!(fc.send_initial(), send_initial)` と `prop_assert_eq!(fc.recv_initial(), recv_initial)` に変更する。

`prop_separate_windows_init` に `should_send_window_update` の閾値が `recv_initial` 基準であることを検証するケースを追加する。

### 単体テスト

`tests/test_flow_control.rs` を新設し、以下の境界値テストを追加する:

- `send_initial != recv_initial` の場合の `should_send_window_update` 閾値検証
- `send_initial != recv_initial` の場合の `window_update_increment` 戻り値検証
- `new()` で両フィールドが同じ値で初期化されることの検証

## CHANGES.md (実装時に追記)

- `## develop` に以下を追加する:
  - `[FIX]` `FlowControl::with_separate_windows` が受信側初期ウィンドウサイズを誤って参照するバグを修正する
    - @voluntas

## 受け入れ基準

- `cargo test --workspace` が通る
- `cargo clippy --all-targets -- -D warnings` が通る
- `cargo fmt --check` が通る
