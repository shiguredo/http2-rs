# prop_window_update_increases_window の Strategy がフロー制御ウィンドウ上限を考慮しない

Created: 2026-05-24
Model: Opus 4.7

## 概要

`pbt/tests/prop_connection.rs:729-737` の `prop_window_update_increases_window` PBT は
`increment` を `1u32..=0x7FFF_FFFF` で生成しているが、受信側の接続レベル送信ウィンドウは
初期値 `DEFAULT_INITIAL_WINDOW_SIZE = 65535` から始まるため、`increment` が大きいと
ウィンドウが 2^31 - 1 を超えて `FLOW_CONTROL_ERROR` (RFC 9113 §6.9.1) になる。

実装側は正しく `Err(FLOW_CONTROL_ERROR)` を返しているのに、テスト側が `.unwrap()` で
成功を期待してパニックする。proptest の seed 依存で macOS / Windows の CI が落ちている。

## 背景

CI run https://github.com/shiguredo/http2-rs/actions/runs/26338536122 で
`prop_window_update_increases_window` が macos-26 で失敗。

```
thread 'prop_window_update_increases_window' panicked at pbt/tests/prop_connection.rs:737:26:
called `Result::unwrap()` on an `Err` value:
  ConnectionError(FLOW_CONTROL_ERROR): window size overflow (at src/flow_control.rs:120)
minimal failing input: increment = 2147418113
```

- 失敗入力 `increment = 2147418113` (= 0x7FFF_0001)
- 接続レベル送信ウィンドウ初期値 = `DEFAULT_INITIAL_WINDOW_SIZE = 65535`
- 合計: `2147418113 + 65535 = 2147483648 = 2^31` → 上限超過

ubuntu-24.04 / ubuntu-24.04-arm では pass しているが、これは proptest が振る seed が
プラットフォーム間で偶然違っていただけで、本質的にはどの環境でも再現しうる。

## 根拠

RFC 9113 §6.9.1:

> A sender MUST NOT allow a flow-control window to exceed 2^31 - 1 octets.
> If a sender receives a WINDOW_UPDATE that causes a flow-control window to exceed
> this maximum, it MUST terminate either the stream or the connection, as appropriate.
> For streams, the sender sends a RST_STREAM with an error code of FLOW_CONTROL_ERROR;
> for the connection, a GOAWAY frame with an error code of FLOW_CONTROL_ERROR is sent.

`src/flow_control.rs:120` (`FlowControl::increase`) はこの仕様に従い `FLOW_CONTROL_ERROR`
を返している。一方 PBT 側はその挙動を考慮していない。

## 設計

### 修正方針

`prop_window_update_increases_window` の意図は「正常な WINDOW_UPDATE で受信側の送信
ウィンドウが増えること」を確認することで、overflow ケースの挙動検証は別 PBT の責務。

`pbt/tests/prop_connection.rs:730` の Strategy を以下に変更する:

```rust
// 接続レベル送信ウィンドウの初期値は DEFAULT_INITIAL_WINDOW_SIZE で、
// increment + 初期値 が 2^31 - 1 を超えないようにする (RFC 9113 §6.9.1)
fn prop_window_update_increases_window(
    increment in 1u32..=(0x7FFF_FFFF - shiguredo_http2::settings::DEFAULT_INITIAL_WINDOW_SIZE)
)
```

### overflow ケースの独立 PBT 追加

`prop_window_update_overflow_is_flow_control_error` を別途追加し、`increment` の上限を
overflow が必ず起きる範囲 (`0x7FFF_0000..=0x7FFF_FFFF`) に限定して
`Err(FLOW_CONTROL_ERROR)` を期待する PBT を書く。

### proptest-regressions ファイル

`pbt/tests/prop_connection.proptest-regressions` (なければ新規作成) に以下を追加し、
将来同じ seed で再現できるようにする:

```
cc 6b38191bdd70a8b9be1392ae93e08c5db28efc8edc7d15784eba1ba7d3de7d3b
```

## 影響範囲

- `pbt/tests/prop_connection.rs:729-737`: Strategy 修正
- `pbt/tests/prop_connection.rs`: overflow 専用 PBT 追加
- `pbt/tests/prop_connection.proptest-regressions`: 再現 seed 登録

実装コード (`src/flow_control.rs`) は変更しない。実装は RFC 準拠で正しい。

## CHANGES.md エントリ

```
- [FIX] PBT `prop_window_update_increases_window` の Strategy がフロー制御ウィンドウの
  上限 (RFC 9113 §6.9.1) を考慮していなかったため、proptest の seed 依存で
  FLOW_CONTROL_ERROR が発生して CI が断続的に失敗する問題を修正する (issue 0040)
  - @voluntas
```

`### misc` ではなく `[FIX]` (バグ修正) として登録する。CI 失敗の修正であり、
利用者向けの挙動には影響しないが、テスト基盤の堅牢性に関わるため。

## 受け入れ条件

- `cargo test --workspace` が macOS / Linux / Windows の全環境で安定して pass する
- proptest を 1000 ケース以上回しても (デフォルト) FLOW_CONTROL_ERROR で落ちない
- 再現用 seed が `pbt/tests/prop_connection.proptest-regressions` に登録されている
- overflow を検証する独立 PBT が追加されている

## 依存

なし (独立した PBT バグ修正)。
