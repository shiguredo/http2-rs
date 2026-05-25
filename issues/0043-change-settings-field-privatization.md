# Settings 構造体のフィールドを private 化する

- Priority: High
- Created: 2026-05-24
- Model: Opus 4.7
- Branch: feature/change-settings-field-privatization

## 目的

`Settings` 構造体の全フィールドが `pub` のまま残っている。
`Limits` は issue 0028 で private 化済みだが、`Settings` は未対応。

`initial_window_size: u32` / `max_frame_size: u32` が外部から直接代入可能であるため、
`Setting` enum で `WindowSize` / `MaxFrameSize` 型を使って導入した構築時検査がバイパスできる。

さらに `to_settings_list` で `WindowSize::from_static(self.initial_window_size)` を
呼んでおり、外部から不正値を代入されると runtime panic する。

## 優先度根拠

- `Limits` のフィールド private 化 (issue 0028) と同じ方針の一貫性
- `Setting::from_wire` / `Settings::apply` 経由の型安全性が `Settings` の pub
  フィールドで無効化されている

## 設計方針

### フィールドの private 化と型変更

- `Settings` の全フィールドを private 化する
- `initial_window_size` の型を `u32` → `WindowSize` に変更する
- `max_frame_size` の型を `u32` → `MaxFrameSize` に変更する

### getter の設計

`Limits` の getter パターン（`pub const fn` で型をそのまま返す）に倣う:

- `pub const fn header_table_size(&self) -> u32`
- `pub const fn enable_push(&self) -> bool`
- `pub const fn max_concurrent_streams(&self) -> Option<u32>`
- `pub const fn initial_window_size(&self) -> WindowSize`
- `pub const fn max_frame_size(&self) -> MaxFrameSize`
- `pub const fn max_header_list_size(&self) -> Option<u32>`
- `pub const fn enable_connect_protocol(&self) -> bool`
- `pub const fn no_rfc7540_priorities(&self) -> bool`
- `pub const fn wt_initial_max_data(&self) -> Option<u32>`
- `pub const fn wt_initial_max_stream_data_uni(&self) -> Option<u32>`
- `pub const fn wt_initial_max_stream_data_bidi_local(&self) -> Option<u32>`
- `pub const fn wt_initial_max_streams_uni(&self) -> Option<u32>`
- `pub const fn wt_initial_max_streams_bidi(&self) -> Option<u32>`
- `pub const fn wt_initial_max_stream_data_bidi_remote(&self) -> Option<u32>`

`initial_window_size()` は `WindowSize` を、`max_frame_size()` は `MaxFrameSize` を返す。
呼び出し側で `u32` が必要な箇所は `.get()` を使う。

### setter は提供しない

`Limits` と同じく setter は設けない。ミューテーション経路は以下の 2 つに限定する:

1. `Settings::from_limits(limits: &Limits) -> Self`（新規追加、`pub(crate)`）
2. `Settings::apply(&mut self, setting: Setting)`（既存、`pub`。impl 内なので private フィールドに直接アクセス可能）

### `Connection::new()` の初期化パターン変更

現在の 14 行にわたる直接フィールド代入を `Settings::from_limits(&limits)` 1 行に置き換える:

```rust
let local_settings = Settings::from_limits(&limits);
```

`from_limits` 内で `Limits` の getter から型付き値を受け取り、フィールドに直接代入する。
`Limits::initial_window_size()` が `WindowSize` を返すため、`.get()` なしで直接格納できる。

`Limits` に存在しないフィールド（`enable_push`）はデフォルト値 (`DEFAULT_ENABLE_PUSH = false`) で
初期化する。`Limits` にしか存在しないフィールド（`connection_window_size`）は `from_limits` では
使用しない（`Connection` 側で別途参照する）。

`from_limits` は `pub(crate)` とする。`Settings` は接続の内部状態であり、外部クレートが
`Limits` から直接 `Settings` を構築するユースケースはない（外部は `Limits` → `Connection::new()`
経由で間接的に利用する）。

### `apply()` の変更

`apply()` は `impl Settings` 内のメソッドなので private フィールドに直接アクセス可能。
型変更に伴い以下を修正:

- `Setting::InitialWindowSize(ws) => self.initial_window_size = ws` （`.get()` 削除）
- `Setting::MaxFrameSize(mfs) => self.max_frame_size = mfs` （`.get()` 削除）

### `to_settings_list()` の変更

フィールドが `WindowSize` / `MaxFrameSize` 型になるため、
`WindowSize::from_static(self.initial_window_size)` が `self.initial_window_size` に簡略化される。
`from_static` 呼び出しが不要になり、不正値による runtime panic のリスクが構造的に排除される。

### `Default` 実装の変更

`initial_window_size` と `max_frame_size` の初期値を型付きに変更:

- `initial_window_size: DEFAULT_INITIAL_WINDOW_SIZE` → `initial_window_size: WindowSize::from_static(DEFAULT_INITIAL_WINDOW_SIZE)`
- `max_frame_size: DEFAULT_MAX_FRAME_SIZE` → `max_frame_size: MaxFrameSize::from_static(DEFAULT_MAX_FRAME_SIZE)`

### `connection/mod.rs` の呼び出し側変更

getter 呼び出し + 型変更への対応。主な変更パターン:

- `self.remote_settings.initial_window_size` → `self.remote_settings.initial_window_size().get()`
- `self.remote_settings.max_frame_size as usize` → `self.remote_settings.max_frame_size().get() as usize`
- `self.remote_settings.enable_connect_protocol` → `self.remote_settings.enable_connect_protocol()`
- `self.remote_settings.max_concurrent_streams` → `self.remote_settings.max_concurrent_streams()`
- `self.remote_settings.header_table_size` → `self.remote_settings.header_table_size()`

`update_stream_windows` やフロー制御関連の呼び出しは引き続き `u32` を受け取るため、
getter の戻り値に `.get()` を付けるだけで対応可能。メソッドシグネチャの変更は不要。

## 影響範囲

- `src/settings.rs`: フィールド private 化、型変更、getter 追加、`from_limits` 追加、`apply` / `to_settings_list` / `Default` 修正
- `src/connection/mod.rs`: 全フィールドアクセス箇所を getter 経由に変更（約 38 箇所）
- `pbt/tests/prop_settings.rs`: フィールド直接アクセスを getter 呼び出しに変更。
  型変更に伴い `WindowSize` / `MaxFrameSize` と `u32` 定数の比較は `.get()` で `u32` に戻して行う
- `CHANGES.md`: `[CHANGE]` エントリ追記

## 他 issue との関係

- issue 0042 (`handle_settings` にチェック追加) は `remote_settings.enable_connect_protocol` を参照する。
  本 issue が先にマージされた場合、0042 の実装時は getter 経由のコードを前提とする。
  設計上の競合はない（両方とも `handle_settings` に変更を加えるが、変更箇所は独立）

## 完了条件

- `Settings` の全フィールドが private で getter 経由でのみ読み取り可能
- `initial_window_size` が `WindowSize` 型、`max_frame_size` が `MaxFrameSize` 型
- `to_settings_list` / `apply` で `from_static` / `.get()` による変換が不要
- `Connection::new()` が `Settings::from_limits(&limits)` で初期化
- 既存の全テスト・PBT・fuzz が通る
- CHANGES.md に `[CHANGE]` エントリを 2 件追記（issue 0028 の先例に倣い、private 化と型変更を分割）:
  - `Settings` のフィールド private 化 + getter 追加
  - `initial_window_size` / `max_frame_size` の型変更
