# flow_control の致命的バグ 2 件を修正する

Created: 2026-05-14
Model: deepseek-v4-pro

## 対象

- `src/flow_control.rs`
- `src/connection/mod.rs`

## バグ 1: `update_initial_window_size` に負値ウィンドウ検出が欠落している

### 場所

`src/flow_control.rs:152-166`

### 内容

SETTINGS_INITIAL_WINDOW_SIZE 減少時に `send_window + delta` が負になるケースでエラーが返らず、後続の `send_available()` (line 67-73) で `self.send_window as usize` がパニックする。

### 根拠

RFC 9113 Section 6.5.2:

> A change to SETTINGS_INITIAL_WINDOW_SIZE can cause the available space in a flow-control window to become negative. A sender MUST track the negative space and MUST NOT send new flow-controlled frames until it receives WINDOW_UPDATE frames that cause the available space to become positive.

また同節では:

> A receiver MUST treat this as a stream error of type FLOW_CONTROL_ERROR if the change causes the available space to become negative.

### 再現手順

`send_initial=65535`, `send_window=65535`, `new_size=100` の場合:
- `delta = 100 - 65535 = -65435`
- `new_window = 65535 + (-65435) = -65435`
- `new_window > i64::from(MAX_WINDOW_SIZE)` のみチェックで通過
- `send_window = -65435` (負値)
- 次の `send_available()` 呼び出しで `self.send_window as usize` がパニック

### 修正方針

`update_initial_window_size` に `new_window >= 0` の下限チェックを追加し、負値時に `FlowControlError` を返す。

## バグ 2: `with_separate_windows` の閾値計算が誤っている

### 場所

`src/flow_control.rs:43`

### 内容

`with_separate_windows` が `initial_window_size` フィールドに `send_initial` のみを格納する。しかし `should_send_window_update()` (line 174) と `window_update_increment()` (line 179) はこの値を **受信側初期値** として使って WINDOW_UPDATE 発行タイミングと増分を計算する。

### 根拠

`Stream::new()` は `FlowControl::with_separate_windows(remote_settings.initial_window_size, local_settings.initial_window_size)` を呼ぶ。`send_initial` と `recv_initial` が異なる場合（対向が異なる SETTINGS_INITIAL_WINDOW_SIZE を送ってきた場合）、WINDOW_UPDATE の閾値計算が完全に誤る。

例: `send_initial=65535`, `recv_initial=131072` の場合、`should_send_window_update` は `recv_window < 65535/2` で判定し、実際の半分である 65536 の閾値より遥かに遅く発火する。

### 修正方針

`initial_window_size` を送信用と受信用に分割するか、`with_separate_windows` で `recv_initial` も保存し、`should_send_window_update()` と `window_update_increment()` で `recv_initial` を基準にする。
