# RFC/仕様参照の節番号誤りを修正する

Created: 2026-05-14
Priority: Low
Model: deepseek-v4-pro

## 致命的な誤り

### 1. `src/connection/mod.rs:1503, 1531` — RFC 9218 Section 5.1 → Section 2.1

コードコメントで `RFC 9218 Section 5.1` と参照しているが、SETTINGS_NO_RFC7540_PRIORITIES の振る舞い（最初の SETTINGS で送信、以後変更禁止）は **Section 2.1** で定義されている。Section 5 は "The Priority HTTP Header Field" であり Section 5.1 は存在しない。

RFC 9218 Section 2.1: "If endpoints use SETTINGS_NO_RFC7540_PRIORITIES, they MUST send it in the first SETTINGS frame. Senders MUST NOT change the SETTINGS_NO_RFC7540_PRIORITIES value after the first SETTINGS frame."

### 2. `src/webtransport/capsule.rs:548` — draft Section 6.1 → Section 6.12、バージョン番号欠落

コメントに `draft-ietf-webtrans-http2 Section 6.1` とあるが:
- バージョン番号 `-14` が欠落している
- 節番号 `6.1` は "PADDING Capsule" であり、WT_CLOSE_SESSION の reason 長制限 (1024 バイト) は **Section 6.12** で定義されている

draft-ietf-webtrans-http2-14 Section 6.12: "The message takes up the remainder of the capsule, and its length MUST NOT exceed 1024 bytes."

## 重要な誤り

### 3. `src/connection/mod.rs:556, 628` — RFC 9113 Section 6.9 → Section 6.9.1

空 DATA + END_STREAM がフロー制御ウィンドウ 0 でも送信許可される規定は **Section 6.9.1** ("The Flow-Control Window") にある。Section 6.9 は複数のサブセクションを持つ上位の節。

### 4. `src/validation.rs:224` — RFC 9110 Section 9 → Section 9.1

`method = token` の ABNF 定義は **Section 9.1** ("Overview") に存在する。Section 9 は "Methods" の大見出し。

### 5. `src/connection/mod.rs:1018-1019` — RFC 9110 Section 6.4.1 → RFC 9113 Section 8.1.1 を併記

DATA フレームと content-length の不一致に関する malformed 規則は **RFC 9113 Section 8.1.1** で定義されている。RFC 9110 Section 6.4.1 は HEAD/204/304 にコンテンツがないことの定義のみ。

### 6. `src/validation.rs:331` — コメントの正確性

コメントに「RFC 9113 Section 8.3.1: :authority の userinfo 禁止は http/https と CONNECT に限定」とあるが、RFC 9113 Section 8.3.1 が禁止しているのは `http` または `https` スキームの URI に限定されており、CONNECT は明示的に言及されていない。

コードの挙動自体は防御的で妥当だが、コメントを修正する。

## 対象ファイル一覧

- `src/connection/mod.rs` (#1, #3, #5)
- `src/webtransport/capsule.rs` (#2)
- `src/validation.rs` (#4, #6)
- `src/settings.rs` (#7)
- `src/limits.rs` (#7)

## CHANGES.md (実装時に追記)

- `## develop` の `### misc` に以下を追加する:
  - `[FIX]` ソースコード内の RFC/仕様参照の節番号誤りを修正する
    - @voluntas

## 受け入れ基準

- `cargo test --workspace` が通る
- `cargo clippy --all-targets -- -D warnings` が通る

## 改善

### 7. `src/settings.rs:92, 183, 185, 322`, `src/limits.rs:27, 128` — 廃止 RFC 7540 への言及

"RFC 7540 の優先度シグナリング" という表現を RFC 9113 Section 5.3.2 の併記に更新する。
