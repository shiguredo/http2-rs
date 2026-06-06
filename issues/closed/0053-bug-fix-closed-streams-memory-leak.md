# closed_streams HashSet が永不変に増殖するメモリ枯渇問題を修正する

- Priority: Medium
- Created: 2026-06-06
- Completed: 2026-06-06
- Model: DeepSeek V4 Pro
- Branch: feature/fix-closed-streams-memory-limit
- Polished: 2026-06-06

## 目的

`src/connection/mod.rs:68` の `closed_streams: HashSet<u32>` はクローズ済みストリーム ID を追跡するが、一度追加されたエントリが一切削除されない。長期間稼働する接続でストリーム ID 空間（最大約 21 億）のエントリが蓄積されることでメモリが単調増加する。

## 優先度根拠

- 攻撃者が短命のストリームを大量に開閉することで、意図的にメモリを枯渇させられる
- RFC 9113 §10.5 は「An endpoint that doesn't monitor use of these features exposes itself to a risk of denial of service」と DoS リスクを指摘している
- 特にサーバー側で長時間稼働する接続では、全クローズ済みストリーム ID が HashSet に残り続ける

ただし、closed_streams には削除できない制約がある（後述）。そのため優先度を Medium に下げる。

## 現状

`closed_streams` は以下の 6 箇所でエントリが追加され、2 箇所で読み取られる:

### 挿入箇所

| ファイル | 行 | コンテキスト |
|----------|-----|------------|
| `mod.rs` | 767 | `try_remove_closed_stream()` 正常クローズ時 |
| `mod.rs` | 1010 | `handle_data()` DATA 受信でクローズ時 |
| `mod.rs` | 1031 | `handle_rst_stream()` RST_STREAM 受信時 |
| `headers.rs` | 121 | `send_response()` レスポンス送信でクローズ時 |
| `headers.rs` | 196 | `send_trailers()` トレーラー送信でクローズ時 |
| `headers.rs` | 540 | `process_headers()` ヘッダー処理でクローズ時 |

### 読み取り箇所

| ファイル | 行 | 用途 |
|----------|-----|------|
| `headers.rs` | 236 | クローズ済みストリームへの遅延 HEADERS を HPACK 状態更新後に破棄 |
| `headers.rs` | 614 | 同上（CONTINUATION 経路） |

### 削除できない理由

`headers.rs:236, 614` では `closed_streams.contains(&sid)` を用いて、遅延到着した HEADERS フレームのストリーム ID が「以前にクローズされたストリーム」か「単調増加違反の未開設ストリーム」かを区別している。RFC 9113 §5.1 はクローズ後にフレームが到着しうることを明示的に認めている（"frames might be received for some time after closing"）。単純にエントリを削除すると、削除後に到着した遅延 HEADERS が PROTOCOL_ERROR の接続エラーとして誤って処理される。

## 設計方針

閉塞的な削除は行わず、**上限付きデータ構造**で対応する。`HashSet` の代わりに以下のいずれかを使う:

1. **上限付き BTreeSet + 古いエントリ削除**: 挿入時にサイズが上限（例: 10000）を超えたら、最も小さいエントリを削除する。ストリーム ID は単調増加するため、小さい値ほど古い。遅延フレームが問題になるのは直近のクローズ済みストリームのみであり、上限を十分大きく取れば実用上の問題はない
2. **上限付きリングバッファ**: 最後の N 個のクローズ済みストリーム ID のみを保持する

上限値の根拠: 同時ストリーム数 (`max_concurrent_streams`) の 100 倍程度（デフォルトで 10000）を上限とすれば、遅延フレームの検出は実用上十分。RFC 9113 §5.1 の遅延フレームは「some time after closing」であり、次のストリームが 10000 個も開かれた後まで frames が滞留することはない。

## 対応手順

1. 作業ブランチ `feature/fix-closed-streams-memory-limit` を作成する
2. `closed_streams` の型を `HashSet<u32>` から上限付きのデータ構造に変更する
3. `headers.rs:236, 614` の読み取りロジックが新しいデータ構造でも正しく動作することを確認する
4. `tests/test_connection.rs` に上限到達時のエントリ削除を検証する単体テストを追加する
5. `CHANGES.md` の `## develop` に `[FIX]` エントリを追加する
6. `cargo test --workspace` で全テスト通過を確認する
7. `cargo clippy --workspace --all-targets -- -D warnings` で警告がないことを確認する

## 完了条件

- `closed_streams` にサイズ上限が設定され、古いエントリが自動的に削除される
- クローズ直後のストリームへの遅延 HEADERS フレームが正しく処理される
- 上限を超えたエントリ削除のテストが追加されている
- 長時間稼働接続でも `closed_streams` のメモリ消費が上限に留まる

## 解決方法

1. `closed_streams` の型を `HashSet<u32>` から上限付きの `BoundedClosedStreams` (内部は `BTreeSet<u32>`、上限 10000 件) に変更した。
2. 上限を超えて挿入された場合、最も小さいストリーム ID（最も古いエントリ）を自動削除するロジックを実装した。
3. `closed_streams` の 6 箇所の insert 呼び出しと 2 箇所の contains 呼び出しはすべて新しい型と互換性があり、変更不要だった。
4. `src/connection/mod.rs` に `BoundedClosedStreams` の単体テスト（上限到達時のエントリ削除、連続削除）を追加した。
5. `CHANGES.md` のメイン `[FIX]` セクションにエントリを追加した。
6. `cargo test --workspace` と `cargo clippy --workspace --all-targets -- -D warnings` の通過を確認した。
