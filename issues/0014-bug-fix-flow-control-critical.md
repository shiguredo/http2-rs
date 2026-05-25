# FlowControl::with_separate_windows が受信側初期ウィンドウサイズを誤って参照するバグを修正する

- Priority: High
- Created: 2026-05-14
- Model: deepseek-v4-pro
- Branch: feature/fix-flow-control-separate-windows

## 目的

`FlowControl::with_separate_windows(send_initial, recv_initial)` が `initial_window_size` フィールドに `send_initial` を格納するため、`should_send_window_update()` と `window_update_increment()` が受信ウィンドウの閾値計算に送信側の初期値を使ってしまう。`send_initial != recv_initial` の場合に WINDOW_UPDATE の発行タイミングと増分が誤る。

この修正に伴い、公開 API `initial_window_size()` を削除して `send_initial()` / `recv_initial()` に分割する（後方互換のない変更）。

## 優先度根拠

ストリームレベルのフロー制御で `send_initial != recv_initial` になるケース（ローカルとリモートが異なる `SETTINGS_INITIAL_WINDOW_SIZE` を送信した場合）は実運用上一般的である。閾値が誤ると WINDOW_UPDATE の発行が遅れ、受信ウィンドウが枯渇してストリームがストールする可能性がある。

現時点では `should_send_window_update()` / `window_update_increment()` は Sans I/O 層の公開 API として定義されているが、本番コード (`src/`) から直接呼ばれていないため、実害は tokio-http2 ラッパー利用者には及ばない。ただし Sans I/O 層の直接利用者には影響があり、公開 API のセマンティクスが壊れた状態を放置すべきでないため High とする。

## 関連 issue

- `issues/closed/0041-bug-fix-connection-flow-control.md`: 接続レベルのフロー制御バグを修正した issue。スコープ外セクション (L129) で本 issue の問題に言及し「別 issue で対応」と記載している。0041 では接続レベルで `FlowControl::new(DEFAULT_INITIAL_WINDOW_SIZE)` を使う回避策を取ったため、接続レベルは本 issue の修正の影響を受けない（`new()` は `send_initial` と `recv_initial` に同じ値を格納するため）

## 現状

### バグ箇所

`src/flow_control.rs:39-45`:

```rust
pub fn with_separate_windows(send_initial: u32, recv_initial: u32) -> Self {
    Self {
        send_window: i64::from(send_initial),
        recv_window: i64::from(recv_initial),
        initial_window_size: send_initial, // バグ: recv_initial であるべき箇所もある
    }
}
```

`should_send_window_update()` (L172-175) と `window_update_increment()` (L179-187) は `self.initial_window_size` を受信ウィンドウの目標値・閾値として使用するが、格納されているのは `send_initial` である。

さらに `update_initial_window_size(new_size)` (L152-165) が `self.initial_window_size = new_size` で上書きするため、`with_separate_windows` で構築されたインスタンスに対して `update_initial_window_size` を呼ぶと、`should_send_window_update()` / `window_update_increment()` の閾値が送信側の新しい初期値に書き換わり、受信ウィンドウの閾値が二重に汚染される。

### 呼び出し元

`src/stream/mod.rs:84-87` の `Stream::new()`:

```rust
flow_control: FlowControl::with_separate_windows(
    send_initial_window_size,
    recv_initial_window_size,
),
```

### RFC 根拠

RFC 9113 Section 6.5.2 (refs/rfc9113.txt L1702-1704):

> SETTINGS_INITIAL_WINDOW_SIZE (0x04): This setting indicates the sender's initial window size (in units of octets) for stream-level flow control. The initial value is 2^16-1 (65,535) octets.

RFC 9113 Section 6.9.1 (refs/rfc9113.txt L2170-2175):

> The receiver of a frame sends a WINDOW_UPDATE frame as it consumes data and frees up space in flow-control windows. Separate WINDOW_UPDATE frames are sent for the stream- and connection-level flow-control windows.

`should_send_window_update()` は「受信者として WINDOW_UPDATE を送信すべきか」を判断するメソッドであり、基準となるべきは受信側の初期ウィンドウサイズ (`recv_initial`) である。

なお、RFC 9113 Section 5.2.1 #7 (refs/rfc9113.txt L1013-1018) により、WINDOW_UPDATE の送信タイミングと値は RFC で規定されておらず実装の選択である:

> This document does not stipulate how a receiver decides when to send this frame or the value that it sends, nor does it specify how a sender chooses to send packets.

「初期ウィンドウサイズの半分以下になったら発行」という閾値アルゴリズムは RFC 要件ではなく本実装の設計判断だが、その閾値の基準値が誤っていることがバグである。

### 再現例

`send_initial=65535`, `recv_initial=131072` の場合:

- `should_send_window_update()` は `recv_window < 65535 / 2` (= `recv_window < 32767`) で発火
- 正しくは `recv_window < 131072 / 2` (= `recv_window < 65536`) で発火すべき
- 結果: 受信ウィンドウが 65535 まで低下しても WINDOW_UPDATE が送信されず、32767 未満まで低下して初めて発火する

## 設計方針

### 構造体変更

`initial_window_size: u32` を削除し、`send_initial: u32` と `recv_initial: u32` の 2 フィールドに分割する:

```rust
pub struct FlowControl {
    send_window: i64,
    recv_window: i64,
    send_initial: u32,
    recv_initial: u32,
}
```

### メソッド修正

- `new(initial_window_size)`: `send_initial` と `recv_initial` の両方に `initial_window_size` を格納する（現状維持）
- `with_separate_windows(send_initial, recv_initial)`: それぞれのフィールドに正しく格納する
- `should_send_window_update()`: `self.recv_initial` を基準にする:
  ```rust
  pub fn should_send_window_update(&self) -> bool {
      self.recv_window < i64::from(self.recv_initial / 2)
  }
  ```
- `window_update_increment()`: `self.recv_initial` を基準にする:
  ```rust
  pub fn window_update_increment(&self) -> u32 {
      let target = i64::from(self.recv_initial);
      let increment = target - self.recv_window;
      if increment > 0 && increment <= i64::from(MAX_WINDOW_SIZE) {
          increment as u32
      } else {
          0
      }
  }
  ```
- `update_initial_window_size(new_size)`: **`send_initial` のみを更新する。`recv_initial` は変更しない。** RFC 9113 Section 6.9.2 (L2213-2215) で `SETTINGS_INITIAL_WINDOW_SIZE` 変更時に調整対象となるのは送信ウィンドウのみであり、受信側の初期値には影響しないため:
  ```rust
  pub fn update_initial_window_size(&mut self, new_size: u32) -> Result<(), Error> {
      let delta = i64::from(new_size) - i64::from(self.send_initial);
      let new_window = self.send_window + delta;
      if new_window > i64::from(MAX_WINDOW_SIZE) {
          return Err(Error::connection_error(
              ErrorCode::FlowControlError,
              "window size overflow after SETTINGS update",
          ));
      }
      self.send_window = new_window;
      self.send_initial = new_size;
      Ok(())
  }
  ```

### 公開 API 変更

- `initial_window_size()` getter を削除する
- `pub const fn send_initial(&self) -> u32` を追加する
- `pub const fn recv_initial(&self) -> u32` を追加する

これは後方互換のない変更 (`[CHANGE]`) である。`initial_window_size()` の全呼び出し元 (`pbt/tests/prop_flow_control.rs` L21, L37, L117) を更新する必要がある。

CHANGES.md には以下の 2 エントリを追加する（種別順: CHANGE → FIX）:

- `[CHANGE]` `FlowControl::initial_window_size()` を削除し `send_initial()` / `recv_initial()` に分割する
- `[FIX]` `FlowControl::with_separate_windows` が受信側初期ウィンドウサイズを誤って参照するバグを修正する

### 接続レベルへの影響

接続レベルの `FlowControl` は `FlowControl::new(DEFAULT_INITIAL_WINDOW_SIZE)` で生成される (issue 0041 で修正済み)。`new()` は `send_initial` と `recv_initial` に同じ値を格納するため、本修正の影響を受けない。

### 影響を受けないコード

- `Default for FlowControl`: `Self::new(DEFAULT_INITIAL_WINDOW_SIZE)` を呼ぶため、修正後も `send_initial` と `recv_initial` に同じ値が格納される。動作は変わらない
- `src/stream/mod.rs`: 引数の意味（第 1 引数=送信側、第 2 引数=受信側）が変わらないため変更不要
- `src/lib.rs`: `FlowControl` を型として re-export (`pub use flow_control::{FlowControl, MAX_WINDOW_SIZE}`) しているのみであり、getter の追加・削除は型のメソッド変更として自動的に反映される。`lib.rs` 自体の変更は不要
- `fuzz/fuzz_targets/fuzz_flow_control.rs`: `FlowControl::new()` と各メソッド（シグネチャ変更なし）のみを使用するためコンパイルに影響なし

### 既知の制限事項

`recv_initial = 1` の場合、`recv_initial / 2` の整数除算結果が 0 になるため `should_send_window_update()` は決して `true` を返さない（`recv_window` は 0 未満にならないため `recv_window < 0` が常に偽）。RFC 9113 は `SETTINGS_INITIAL_WINDOW_SIZE` に 1 以上の値を許容しているため、これは有効な設定値である。本 issue のスコープでは修正しない（閾値アルゴリズム自体の見直しは別 issue の範囲）。

### 変更対象ファイル

- `src/flow_control.rs`: 構造体・メソッド修正
- `pbt/tests/prop_flow_control.rs`: PBT 修正・追加
- `CHANGES.md`: `[CHANGE]` + `[FIX]` エントリ追加

`tests/test_flow_control.rs` の既存テストは `initial_window_size()` を使用しておらず、`FlowControl::new()` (send_initial == recv_initial) のみを使用しているため変更不要。新たな単体テストも不要（テスト戦略の全項目を PBT でカバーできるため、AGENTS.md の「PBT でカバーできるものを単体テストで書かない」に従う）。

## テスト戦略

AGENTS.md のテスト役割分担に従い、PBT でカバーできるものを単体テストで書かない。

### PBT 修正 (`pbt/tests/prop_flow_control.rs`)

1. `prop_flow_control_init` (L21): `fc.initial_window_size()` を `fc.send_initial()` に変更し、`prop_assert_eq!(fc.recv_initial(), initial_window)` を追加
2. `prop_separate_windows_init` (L37): `fc.initial_window_size()` を `fc.send_initial()` に変更し、`prop_assert_eq!(fc.recv_initial(), recv_initial)` を追加
3. `prop_update_initial_window_size` (L117): `fc.initial_window_size()` を `fc.send_initial()` に変更

### PBT 追加 (`pbt/tests/prop_flow_control.rs`)

4. `should_send_window_update` / `window_update_increment` が `send_initial` に依存しないことを検証するプロパティ: 任意の `(send_initial_a, send_initial_b, recv_initial, consume_amount)` に対して、`with_separate_windows(send_initial_a, recv_initial)` と `with_separate_windows(send_initial_b, recv_initial)` で同じ量を `consume_recv` した後の `should_send_window_update()` と `window_update_increment()` の結果が一致することを検証する。これにより「recv 系メソッドが send_initial に影響されない」ことを回帰テストとして保証する
5. `update_initial_window_size` が `recv_initial` を変更しないことを検証するプロパティ: 任意の `(send_initial, recv_initial, new_size)` に対して、`update_initial_window_size(new_size)` 前後で `recv_initial()` が不変であることを検証
6. `recv_window > recv_initial` のケースを含むプロパティ: `with_separate_windows` で構築後に `add_recv_window` で `recv_window` を `recv_initial` 超に増加させた場合、`window_update_increment()` が 0 を返し、`should_send_window_update()` が `false` を返すことを検証

### fuzz

`fuzz/fuzz_targets/fuzz_flow_control.rs` は現在 `FlowControl::new()` のみを使用しており、`with_separate_windows` 経路がカバーされていない。本 issue の完了後に fuzz target 拡張の issue を起票すること（忘れ防止のため完了条件に含めない代わりに、ここで明記する）。

## 完了条件

- `FlowControl::with_separate_windows` で `send_initial` と `recv_initial` がそれぞれ正しいフィールドに格納される
- `should_send_window_update()` が `recv_initial` を基準に判定する
- `window_update_increment()` が `recv_initial` を基準に計算する
- `update_initial_window_size()` が `send_initial` のみを更新し `recv_initial` は不変
- `initial_window_size()` getter が削除され、`send_initial()` / `recv_initial()` が追加されている
- CHANGES.md に `[CHANGE]` と `[FIX]` の 2 エントリが追加されている
- 全 PBT・単体テストが通る
- `cargo test --workspace` が通る
- `cargo clippy --all-targets -- -D warnings` が通る
- `cargo fmt --check` が通る
