# その他改善点を修正する

Created: 2026-05-14
Model: deepseek-v4-pro

## 1. FlowControl のエラー型安全性

### 場所

`src/flow_control.rs:110-128`、`src/connection/mod.rs:1643-1652`

### 内容

`FlowControl` は接続レベルとストリームレベルの両方で使われるが、`recv_window_update` 等が常に `Error::connection_error` を返す。ストリームレベルでのフロー制御違反は RST_STREAM であるべきだが、呼び出し側が `is_connection_error()` で判定して RST_STREAM に変換している。

`connection/mod.rs:1643-1652`:
```rust
if let Err(e) = stream.flow_control_mut().recv_window_update(...) {
    if e.is_connection_error() {
        self.reset_stream(stream_id, ErrorCode::FlowControlError)?;
        return Ok(());
    }
    return Err(e);
}
```

このパターンでは、`recv_window_update` が常に `connection_error` を返すため、`return Err(e)` 分岐が到達不能。

## 2. `validate_stream_id_parity` の無意味な role match

### 場所

`src/connection/mod.rs:1761-1782`

### 内容

`Role::Server` と `Role::Client` の分岐が全く同一のロジックを持つ。エラーメッセージを変えるためだけの分岐であり、YAGNI 違反。

修正案:
```rust
if stream_id % 2 == 0 {
    return Err(Error::connection_error(...));
}
Ok(())
```

## 3. DATA フレーム処理での部分ウィンドウ消費

### 場所

`src/connection/mod.rs:1000, 1016`

### 内容

`handle_data` で接続レベルの `consume_recv` (line 1000) が成功したあとに、ストリームレベルの `consume_recv` (line 1016) が失敗すると、接続ウィンドウのみ消費された状態が残る。対向との永続的なウィンドウ不一致状態を引き起こす可能性がある。

RFC 9113 Section 6.9.1 の動作としては正しいが、コメントで意図を明示する。

## 4. 到達不能分岐

### 場所

`src/connection/mod.rs:1643-1650`

### 内容

`recv_window_update` は常に `connection_error` を返すため、`return Err(e)` 分岐が到達不能。

## 5. ポート番号の範囲未検証

### 場所

`src/validation.rs:684-713`

### 内容

`is_valid_connect_authority` がポート番号の範囲 (0-65535) を検証していない。`99999` のような無効なポート番号も受理する。

## 6. `to_settings_list` の initial capacity が不足

### 場所

`src/settings.rs:289`

### 内容

`Vec::with_capacity(8)` は、WebTransport 設定 (最大 6) とオプショナル設定 (最大 4) を含めると最大 16 エントリが必要で不足。

## 7. `recv_headers` の不明瞭な「状態変更なし」分岐

### 場所

`src/stream/state.rs:180-191`

### 内容

`Open | HalfClosedLocal` アームで `end_stream == false` のときに `self.state = self.state` と実質的に何も起きない分岐がある。意図をコメントで明示するか、分岐を整理する。

## 8. `SettingsFrame` の `new()` と `Default` の重複

### 場所

`src/frame/mod.rs:301-330`

### 内容

`new()` と `Default` の内容が完全に同一。

## 9. `Limits::new()` が `default()` のラッパーのみ

### 場所

`src/limits.rs:55-58`

### 内容

`pub fn new()` が単に `Self::default()` を呼ぶだけ。

## 10. `send_frame` の暗黙的契約

### 場所

`src/connection/mod.rs:1930-1935`

### 内容

`encode` が成功した場合にのみバッファをクリアする、という暗黙の契約に依存している。`encode` の `take` 的な API の方が安全。

## 11. `connection/mod.rs` に `calculate_header_list_size()` と `concatenate_cookies()` の `#[cfg(test)]` がない

これらはプライベート関数であり外部からテストできない。`#[cfg(test)]` を追加する。

## 12. `fuzz/` に `fuzz_flow_control.rs` が欠落

FlowControl モジュールに対するファジングターゲットが存在しない。

## 13. `StreamState::is_idle`、`StateMachine::sent_end_stream`/`received_end_stream`、`Event::stream_id`/`is_connection_level` の必要性検討

これらはプロダクションコードから一切呼ばれておらず、テストのみが使用している。削除し、テストコードを修正する。

## CHANGES.md (実装時に追記)

- `## develop` の `### misc` に以下を追加する:
  - `[UPDATE]` フロー制御エラー型安全性を改善する
    - @voluntas
  - `[FIX]` ポート番号の範囲検証を追加する
    - @voluntas
  - `[UPDATE]` 未使用コード・重複コード・到達不能分岐を整理する
    - @voluntas

## 受け入れ基準

- `cargo test --workspace` が通る
- `cargo clippy --all-targets -- -D warnings` が通る
- `cargo fmt --check` が通る
- `cargo +nightly fuzz` ターゲットがビルドできる
