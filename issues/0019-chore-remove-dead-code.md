# 未使用コード・死亡コードを削除する

Created: 2026-05-14
Model: deepseek-v4-pro

## 内容

コードベース内の呼び出しのない関数・メソッドを削除する。

## 削除候補一覧

### スタンドアロン関数 (呼び出しなし)

| # | ファイル:行番号 | 関数 | 備考 |
|---|---|---|---|
| 1 | `src/frame/encoder.rs:322-340` | `pub fn encode_header` | `FrameEncoder::encode_header` (同名メソッド) が全エンコードに使用されている |
| 2 | `src/frame/encoder.rs:349-356` | `pub fn encode_frame` | 呼び出しなし。PBT 内の同名関数は別実装 |
| 3 | `src/frame/encoder.rs:359-363` | `pub fn encode_frame_to_vec` | 呼び出しなし |

### 上記連鎖で死亡する関数

| # | ファイル:行番号 | 関数 | 備考 |
|---|---|---|---|
| 4 | `src/error.rs:273-280` | `pub fn check_buffer_size` | 上記 1,2 のみが呼び出し |

### 未使用メソッド

| # | ファイル:行番号 | メソッド | 備考 |
|---|---|---|---|
| 5 | `src/settings.rs:434-458` | `WtInitialSettings::apply` | `Settings::apply` が直アクセスで代用 |
| 6 | `src/frame/decoder.rs:40-42` | `FrameDecoder::set_max_frame_size` | 呼び出しなし |
| 7 | `src/settings.rs:427-429` | `WtInitialSettings::new()` | `default()` のラッパー |
| 8 | `src/frame/flags.rs:68-70` | `FrameFlags::clear` | 呼び出しなし |
| 9 | `src/stream/buffer.rs:58-60` | `SendBuffer::remaining` | 呼び出しなし |
| 10 | `src/stream/buffer.rs:63-65` | `SendBuffer::clear` | 呼び出しなし |
| 11 | `src/stream/buffer.rs:123-125` | `RecvBuffer::remaining` | 呼び出しなし |
| 12 | `src/stream/buffer.rs:128-130` | `RecvBuffer::clear` | 呼び出しなし |
| 13 | `src/stream/mod.rs:184-192` | `Stream::is_open`, `is_closed` | connection が StateMachine 経由で直接判定 |
| 14 | `src/stream/mod.rs:144-147` | `Stream::headers` | 呼び出しなし |
| 15 | `src/stream/state.rs:80-83` | `StreamState::is_idle` | 呼び出しなし |
| 16 | `src/stream/state.rs:258-272` | `StateMachine::sent_end_stream`, `received_end_stream` | テストのみから呼び出し |
| 17 | `src/event.rs:116-140` | `Event::stream_id`, `is_connection_level` | テストのみから呼び出し |

## 修正方針

1. 呼び出しのないスタンドアロン関数 (1-3) を削除する
2. 連鎖的に死亡する `check_buffer_size` (4) を削除する
3. 呼び出しのないメソッド (5-12) を削除する
4. テストのみから呼び出されるメソッド (13-17) はテスト内に移動するか、テスト側のコードを修正する
