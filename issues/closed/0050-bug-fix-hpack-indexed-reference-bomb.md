# HPACK インデックス参照爆弾でデコーダがメモリを無制限に確保する

- Priority: High
- Created: 2026-06-03
- Completed: 2026-06-03
- Model: Opus 4.8
- Branch: feature/fix-hpack-indexed-reference-bomb

## 目的

HPACK デコーダがヘッダーブロックをデコードする際に、デコード結果の合計サイズ・ヘッダー数のどちらにも上限を設けていない。このため、小さなワイヤ入力から巨大なメモリ確保を引き起こす DoS (HPACK インデックス参照爆弾) が成立する。これを修正し、デコード途中で上限を超えた時点で確実に中断できるようにする。

## 優先度根拠

High とする。リモートの未認証クライアントが少ないバイト数で大量のサーバーメモリを確保させられるリモート DoS であり、機密性・完全性ではなく可用性に直接影響する。実測で 34KB のワイヤ入力から 120MB のメモリを確保できることを確認済みであり、CONTINUATION フレームの累積と組み合わせると GB 級まで増幅する。HTTP/2 を終端する実装として致命的な欠陥に該当する。

## 現状

### 攻撃の構造

RFC 7541 の HPACK は、動的テーブルに 1 度挿入したヘッダーを、以降は 1 バイトのインデックス参照で何度でも参照できる。受信側は参照ごとにヘッダー全体を複製して展開する。攻撃者は次の手順で増幅する。

1. Literal Header Field with Incremental Indexing で、値の大きいヘッダーを動的テーブルに 1 エントリ seed する (値は `SETTINGS_HEADER_TABLE_SIZE` 既定値 4096 まで)
2. そのエントリを指す 1 バイトの Indexed Header Field を数千〜数万回並べる

### 欠陥 1: デコードに逐次上限がない

`src/hpack/decoder.rs` の `Decoder::decode` (44 行〜) は、ヘッダー数・デコード後合計サイズのどちらにも上限を設けず、全ヘッダーを `Vec<HeaderField>` に完全展開してから返す。インデックス参照ごとに `get_header_by_index().cloned()` で name + value 全体を複製する。

`max_header_list_size` (既定 16384) による検査は `src/connection/headers.rs:317` の `process_headers` で**デコード完全終了後**に走る (`Self::calculate_header_list_size`)。つまり巨大な `Vec` がメモリに乗りきってから初めて拒否されるため、ピークメモリの確保は防げない。

```
decode() で 120MB を展開 ──→ process_headers() でサイズ超過を検知して拒否
        ↑ ここで既にメモリ確保済み
```

`HpackDecoder` 単体の公開 API (`Decoder::new` は `max_table_size` のみを受け取る) にも上限を渡す手段がない。

### 欠陥 2: CONTINUATION フレームの累積に上限がない

`src/connection/headers.rs:605` の `handle_continuation` は、`header_block_fragment.extend_from_slice()` で CONTINUATION フレームを累積上限なしに連結する (CVE-2016-8740 系)。`max_frame_size` はフレーム単体の上限のみを強制する (`src/frame/decoder.rs:63`)。したがって既定 `max_frame_size` (16KB) でも、CONTINUATION を連鎖させて累積ブロックを GB 級にし、上限なしのデコーダに流し込める。

### 再現

`HpackDecoder::new(4096)` に対し、value 4000 バイトのエントリを 1 つ seed し、その 1 バイト参照を 30000 回並べたヘッダーブロックをデコードした結果:

```
ワイヤ入力サイズ:      34006 バイト (約 34KB)
デコード後 raw バイト:  120034001 バイト (120.0 MB)
増幅率 (raw/wire):     3530:1
```

`decode` は `Ok` を返し、120MB の `Vec` が完全展開される。

## 設計方針

「最大デコードサイズ」と「最大フィールド数」は別概念だが、本実装の `HeaderField::size` は name + value + 32 で各フィールドに固定 32 バイトを加算するため、サイズ上限を逐次適用すれば空ヘッダーでもフィールド数が実質的に bound される (16384 なら最大約 512 フィールド)。よってサイズ上限の逐次適用を一次防御とし、CONTINUATION 累積上限を二次防御とする。

### 修正 1: デコーダにデコード後サイズ上限を導入し逐次適用する

- `Decoder` に `max_header_list_size: Option<usize>` フィールドを追加する
- `Decoder::decode` のループ内で、各ヘッダーを `headers` に push する前に走行合計 (各ヘッダーの `HeaderField::size`) を加算し、`max_header_list_size` を超えた時点で即 `Err(Error::hpack_error(...))` を返す
- 上限超過は接続エラー (`CompressionError`) として扱う既存経路に乗せる
- `Connection` 生成時 (`src/connection/mod.rs:171`) に `local_settings.max_header_list_size()` をデコーダへ渡し、SETTINGS 変更時にも同期する
- `process_headers` のデコード後サイズ検査 (`src/connection/headers.rs:317`) はデコーダ側の逐次検査と二重になるため整理する (デコーダ側に一本化、または防御的に残す場合はコメントで意図を明記する)

### 修正 2: CONTINUATION 累積バイト数に上限を設ける

- `handle_continuation` (`src/connection/headers.rs`) で `header_block_fragment` の累積バイト数を監視し、上限を超えたら接続エラーにする
- 上限値は `local_settings.max_header_list_size()` を基準とする (圧縮後ワイヤサイズはデコード後サイズ以下のため、同値で十分な保護になる)。`max_header_list_size` が `None` の場合の扱いを定める

## 完了条件

- 上限を設定したデコーダに対し、本 issue の爆弾パターンを入力すると、巨大な `Vec` を展開する前に `Err` を返す
- CONTINUATION を連鎖させた累積ブロックが上限を超えると接続エラーになる
- 既存の HPACK デコード/ラウンドトリップのテストが引き続き通る
- 後述のテストが追加され通る

## テスト戦略

- PBT (`pbt/tests/prop_hpack/`): 上限 L を設定したデコーダについて、爆弾パターンのデコード後サイズを決定論的に計算し「サイズ > L なら必ず `Err`、サイズ <= L なら必ず `Ok` かつ厳密一致」を検証する。常に `Err` を返す実装が通る抜け穴を避けるため、Ok/Err 双方向を固定する
- Fuzzing (`fuzz/fuzz_targets/fuzz_hpack_decoder.rs` の拡張または新規ターゲット): 上限を設定したデコーダで、`Ok` 時に出力サイズが上限以下であることを `assert!` で保証する。`-rss_limit_mb` を絞って実行すれば上限欠落の回帰を検知できる
- 単体テスト: CONTINUATION 累積上限の境界 (上限ちょうど / 上限 +1) を `tests/` で検証する

## 解決方法

### 修正 1: HPACK デコーダにデコード後サイズ上限を導入し逐次適用する (`src/hpack/decoder.rs`)

- `Decoder` に `max_header_list_size: Option<usize>` フィールドと `set_max_header_list_size` を追加した (`new` の既定は `None` = 無制限で後方互換を維持)。
- `Decoder::decode` を、各分岐がデコードしたヘッダー (Dynamic Table Size Update は `None`) と消費バイト数を返す形に整理し、ループ末尾でヘッダーを push する前に走行合計 (`HeaderField::size` の累積) を加算して上限を逐次検査するようにした。超過時点で `Err` を返し、全体を展開しきる前に中断する。

### 修正 2: 接続生成時にローカル設定の上限をデコーダへ渡す (`src/connection/mod.rs`)

- `Connection::new` で `local_settings.max_header_list_size()` を `hpack_decoder.set_max_header_list_size` に渡すようにした。`local_settings` は構築後に変更されないため、デコーダ側の上限と常に一致する。
- これに伴い `process_headers` (`src/connection/headers.rs`) のデコード完了後サイズ検査は、デコーダの逐次検査で先に `COMPRESSION_ERROR` になり到達不能なデッドコードとなったため削除した。サイズ計算式は RFC 9113 §6.5.2 で定義され、展開を打ち切るため §4.3 が COMPRESSION_ERROR を MUST とする。従来の STREAM エラー PROTOCOL_ERROR から接続エラー COMPRESSION_ERROR への変更は CHANGES.md に `[CHANGE]` として記載した。

### 修正 3: CONTINUATION 累積フラグメントのサイズ上限 (`src/connection/headers.rs`)

- `check_header_block_fragment_size` を追加し、HEADERS 継続開始時と各 CONTINUATION 受信時に累積フラグメントサイズを `SETTINGS_MAX_HEADER_LIST_SIZE` (ローカル設定) と比較するようにした。field block を展開せず打ち切るため、超過時は RFC 9113 §4.3 (MUST) に従い COMPRESSION_ERROR の接続エラーにする。`None` (無制限) の場合は検査しない。

### テスト

- PBT (`pbt/tests/prop_hpack/decoder.rs`): 爆弾のデコード後サイズを `(ref_count + 1) * (value_len + 33)` で決定論的に計算し、「上限超過なら必ず Err」「上限以下なら必ず Ok かつサイズが厳密一致」の双方向を検証する。seed の value を Huffman 符号化する経路も含む。無制限なら従来どおり全展開することも確認する。
- 単体テスト (`tests/test_connection.rs`): 単一 HEADERS 内の参照爆弾が COMPRESSION_ERROR に、CONTINUATION 累積超過が COMPRESSION_ERROR になること、累積が上限ちょうどではエラーにならないこと (off-by-one 境界) を検証する。
- Fuzzing (`fuzz/fuzz_targets/fuzz_hpack_decoder.rs`): 上限を設定したデコーダで、デコード成功時に出力サイズが上限以下であることを `assert!` で保証する。

### 実測 (修正前)

`HpackDecoder::new(4096)` に value 4000 バイトのエントリを seed し 1 バイト参照を 30000 回並べた約 34KB の入力で、デコード後 120MB (増幅率 3530:1) を確認した。修正後は上限を設定したデコーダで展開前に `Err` を返すことを確認した。
