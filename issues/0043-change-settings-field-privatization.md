# Settings 構造体のフィールドを private 化する

Created: 2026-05-24
Priority: High
Model: Opus 4.7

## 概要

`Settings` 構造体の全フィールドが `pub` のまま残っている。
`Limits` は issue 0028 で private 化済みだが、`Settings` は未対応。

`initial_window_size: u32` / `max_frame_size: u32` が外部から直接代入可能であるため、
`Setting` enum で `WindowSize` / `MaxFrameSize` 型を使って導入した構築時検査がバイパスできる。

さらに `to_settings_list` で `WindowSize::from_static(self.initial_window_size)` を
呼んでおり、外部から不正値を代入されると runtime panic する。

## 根拠

- `Limits` のフィールド private 化 (issue 0028) と同じ方針の一貫性
- `Setting::from_wire` / `Settings::apply` 経由の型安全性が `Settings` の pub
  フィールドで無効化されている

## 設計

- `Settings` の全フィールドを private 化し getter を提供する
- `initial_window_size` を `WindowSize` 型に、`max_frame_size` を `MaxFrameSize` 型に変更する
- `connection/mod.rs` 内の直接フィールドアクセスを getter/setter 経由に変更する
- `to_settings_list` 内の `from_static` を不要にする (フィールドが型付きになるため)

## 影響範囲

- `src/settings.rs`: フィールド private 化、getter/setter 追加、型変更
- `src/connection/mod.rs`: フィールドアクセスを getter/setter 経由に変更 (多数箇所)
- `crates/tokio-http2/`: `Settings` を参照する箇所の追従

## 受け入れ条件

- `Settings` の全フィールドが private で getter/setter 経由でのみアクセス可能
- `initial_window_size` が `WindowSize` 型、`max_frame_size` が `MaxFrameSize` 型
- `to_settings_list` で `from_static` を使用していない
- 既存の全テスト・PBT・fuzz が通る
