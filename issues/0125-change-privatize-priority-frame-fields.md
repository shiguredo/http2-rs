# PriorityFrame のフィールドを pub(crate) に変更する

- Created: 2026-08-16
- Completed: {YYYY-MM-DD}
- Branch: feature/change-privatize-priority-frame
- Polished: 2026-11-16

## 目的

`PriorityFrame` の全フィールドを `pub` から `pub(crate)` に変更し、外部クレートが構造体リテラルで構築できなくする。あわせて、外部からフィールドを読み取るための getter を追加し、`Settings` / `Limits` / `Error` で確立済みの「フィールドを外部非公開にして getter を提供する」パターンに揃える。

## 現状

`src/frame.rs` の `PriorityFrame` は、doc コメントで「公開コンストラクタは提供しない」と明記されているが、`stream_id` / `exclusive` / `stream_dependency` / `weight` の全フィールドが `pub` である。このため外部クレートから構造体リテラルで構築でき、doc コメントの意図と一致していない。

`PriorityFrame` の利用箇所は次のとおり。

- `src/frame/decoder.rs` の `decode_priority` が `PriorityFrame` を構築する。
- `src/frame/encoder.rs` の `encode_priority` は `&PriorityFrame` を受け取るが、フィールドを読まずに `Error::protocol_error` を返す。PRIORITY フレームは送信しない。
- `src/connection.rs` の `Frame::Priority(_)` は受信したフレームを無視する。RFC 9113 Section 6.3 の非推奨に伴う相互運用性のための受信処理である。
- `src/frame.rs` の `Frame::stream_id` は `f.stream_id` を読む。`Debug` / `PartialEq` の derive も全フィールドを読む。
- `pbt/tests/prop_frame/main.rs` の `prop_priority_frame_decode` は、デコード結果の `pf.stream_id` / `pf.stream_dependency` / `pf.weight` / `pf.exclusive` を直接読む。
- `fuzz/fuzz_targets/fuzz_frame_encoder.rs` の `FuzzFrame::Priority` は、`shiguredo_http2::frame::PriorityFrame` を構造体リテラルで構築する。fuzz クレートは workspace 外の別クレートであり、CI の `cargo check --manifest-path fuzz/Cargo.toml` で検査される。
- `tests/` / `examples/` / `crates/` には `PriorityFrame` の利用はない。

closed issue 0027 では、`PriorityFrame` の `pub` フィールド制限が「deprecated であり、PBT テストでの利用に影響が大きいため据え置き」とされていた。本 issue は getter を追加して PBT を getter 経由に更新することで、この据え置きを解消する。

RFC 9113 Section 6.3 で PRIORITY フレームは非推奨であり、本実装でも受信のみを処理する。外部利用者は `Frame::Priority` variant から受け取った `PriorityFrame` のフィールドを getter 経由で読むことができる必要がある。

## 設計方針

- `PriorityFrame` の `stream_id` / `exclusive` / `stream_dependency` / `weight` を `pub` から `pub(crate)` に変更する。
- 外部から読み取るための getter を追加する。`PriorityFrame` は `Copy` であり、`Settings` の getter と同様に `&self` を受ける `pub const fn` とする。

  ```rust
  impl PriorityFrame {
      /// ストリーム ID (非ゼロ) を取得する
      #[must_use]
      pub const fn stream_id(&self) -> NonZeroStreamId { ... }

      /// Exclusive フラグを取得する
      #[must_use]
      pub const fn exclusive(&self) -> bool { ... }

      /// 依存するストリーム ID を取得する
      #[must_use]
      pub const fn stream_dependency(&self) -> StreamId { ... }

      /// 重みを取得する
      #[must_use]
      pub const fn weight(&self) -> Weight { ... }
  }
  ```

- `PriorityFrame` の doc コメントは「公開コンストラクタは提供しない」を維持し、フィールドは getter 経由で読み取る旨を追記する。フィールド側の doc コメントは内部実装の説明として維持し、getter 側には公開 API の説明を書く (closed issue 0070 の方針に倣う)。
- 公開コンストラクタは追加しない。`PriorityFrame` は受信専用であり、外部からの構築を許可しない方針を維持する。
- `pbt/tests/prop_frame/main.rs` の `prop_priority_frame_decode` を getter 経由の読み取りに変更する。getter の動作はこの既存 PBT で検証するため、新規 PBT は追加しない。
- `fuzz/fuzz_targets/fuzz_frame_encoder.rs` の `FuzzFrame::Priority` は、検証済みの `NonZeroStreamId` / `StreamId` / `Weight` から 5 バイトの PRIORITY ペイロードと 9 バイトのフレームヘッダーを組み立て、`FrameDecoder` でデコードして `Frame::Priority` を得る形に変更する。デコードに失敗した場合は `None` を返す。これにより公開コンストラクタを追加せずに fuzz ターゲットのコンパイルを維持する。
- getter は公開 API の追加のため、`skills/shiguredo-http2/SKILL.md` のフレーム型一覧に `PriorityFrame` を追加し、フィールドは getter 経由で読み取る旨を記載する。
- `src/frame.rs` の `Frame::stream_id` と `Debug` / `PartialEq` の derive、`src/frame/decoder.rs` / `src/frame/encoder.rs` / `src/connection.rs` は、`pub(crate)` 化後も同一クレート内のアクセスであるため変更不要。`src/frame/encoder.rs` は引き続き `PriorityFrame` を受け取るが、フィールドは読まない。
- `PriorityFields` (`HeadersFrame` の優先度フィールド) は本 issue の対象外とし、現状の `pub` フィールドを維持する。
- `src/lib.rs` は変更しない。`PriorityFrame` は既に `pub mod frame` 経由で公開されており、getter は型のメソッドとして追加するため re-export の追加は不要である。
- `CHANGES.md` は `CODEBASE.md` の指示「この指示がなくなるまでは変更履歴を `CHANGES.md` に残さないこと」に従い、変更しない。

## 完了条件

- `PriorityFrame` の全フィールドが `pub(crate)` に変更されていること
- `stream_id()` / `exclusive()` / `stream_dependency()` / `weight()` getter が追加されていること
- `PriorityFrame` の doc コメントに getter 経由で読み取る旨が追記されていること
- `pbt/tests/prop_frame/main.rs` の `prop_priority_frame_decode` が getter 経由でフィールドを読み取っていること
- `fuzz/fuzz_targets/fuzz_frame_encoder.rs` の `FuzzFrame::Priority` が `FrameDecoder` 経由で `Frame::Priority` を構築し、公開コンストラクタを追加していないこと
- `skills/shiguredo-http2/SKILL.md` のフレーム型一覧に `PriorityFrame` が追加され、フィールドは getter 経由で読み取る旨が記載されていること
- `CHANGES.md` を変更していないこと
- `cargo fmt --all -- --check` が通過すること
- `cargo test --workspace` が全件通過すること
- `cargo clippy --workspace --all-targets -- -D warnings` が通過すること
- `cargo check --manifest-path fuzz/Cargo.toml` が通過すること
