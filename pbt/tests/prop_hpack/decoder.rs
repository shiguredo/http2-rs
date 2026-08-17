//! HPACK デコーダーの PBT (RFC 7541 / RFC 9113 Section 6.5.2)
//!
//! デコード後ヘッダーリストサイズの上限が、インデックス参照爆弾を含む任意の入力に対して
//! 常に守られることを検証する。

use shiguredo_http2::hpack::{huffman, integer};
use shiguredo_http2::{HeaderField, HpackDecoder};

/// 各 PBT 共通のシード取得用環境変数名
const SEED_ENV: &str = "HTTP2_PBT_SEED";

/// デフォルトのケースバジェット
///
/// 爆弾構築とデコードが case ごとに大きくなりうるため標準の半分にする。
const CASES: usize = 128;

/// HPACK 文字列リテラルを書き込む (RFC 7541 Section 5.2)
///
/// `huffman` が true のときは Huffman 符号化し、長さプレフィックスの H ビットを立てる。
fn push_string(block: &mut Vec<u8>, data: &[u8], huffman: bool) {
    let encoded = if huffman {
        huffman::encode_to_vec(data)
    } else {
        data.to_vec()
    };
    let mut len_buf = [0u8; 8];
    let n = integer::encode(&mut len_buf, encoded.len() as u64, 7, 0).expect("length encodes");
    if huffman {
        // H ビット (最上位) を立てる
        len_buf[0] |= 0x80;
    }
    block.extend_from_slice(&len_buf[..n]);
    block.extend_from_slice(&encoded);
}

/// インデックス参照爆弾のヘッダーブロックを組み立てる
///
/// 1) Literal Header Field with Incremental Indexing で name="x"・value=`'a' * value_len` を
///    動的テーブルへ seed する (動的テーブル絶対インデックス 62)
/// 2) そのエントリを指す 1 バイトの Indexed Header Field を `ref_count` 回並べる
///
/// `huffman_value` が true のとき seed の value を Huffman 符号化し、デコーダの Huffman 経路を
/// 通す。デコード後の値は符号化方式に関わらず `value_len` バイトなので、出力サイズは不変。
fn build_bomb(value_len: usize, ref_count: usize, huffman_value: bool) -> Vec<u8> {
    let mut block = Vec::new();

    // Literal Header Field with Incremental Indexing, name index = 0 (新規名前)
    block.push(0x40);
    push_string(&mut block, b"x", false);
    let value = vec![b'a'; value_len];
    push_string(&mut block, &value, huffman_value);

    // Indexed Header Field (0x80 | 62) を ref_count 回
    for _ in 0..ref_count {
        block.push(0x80 | 62);
    }

    block
}

/// デコード後ヘッダーリストサイズ (RFC 9113 Section 6.5.2: name + value + 32 の総和)
fn header_list_size(headers: &[HeaderField]) -> usize {
    headers.iter().map(HeaderField::size).sum()
}

/// 上限を設定したデコーダーは、爆弾の総サイズが上限を超えるなら必ず Err、
/// 上限以下なら必ず Ok かつデコード後サイズが厳密に一致する
///
/// seed (name "x" 1 + value value_len + 32) と各参照は同一エントリを指すため、
/// デコード後サイズは `(ref_count + 1) * (value_len + 33)` で決定論的に計算できる。
/// これにより「上限超過 -> 必ず中断」と「上限以下 -> 正確なサイズ」の双方向を固定する。
#[test]
fn prop_bomb_is_exactly_bounded() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    let mut runner = noprop::Runner::new(seed);
    runner.run(CASES, |ctx| {
        let value_len = noprop::sample_usize_in(ctx, 0..=300);
        let ref_count = noprop::sample_usize_in(ctx, 0..=2000);
        let max_size = noprop::sample_usize_in(ctx, 0..=65535);
        let huffman_value = noprop::sample_bool(ctx);
        let block = build_bomb(value_len, ref_count, huffman_value);

        let mut decoder = HpackDecoder::new(4096);
        decoder.set_max_header_list_size(Some(max_size));

        // 1 ヘッダーあたりのサイズ = name("x")=1 + value=value_len + 32
        let per_header = value_len + 33;
        // seed 1 個 + 参照 ref_count 個
        let total = (ref_count + 1) * per_header;

        let result = decoder.decode(&block);
        if total > max_size {
            assert!(
                result.is_err(),
                "総サイズ {total} が上限 {max_size} を超えるなら必ず Err",
            );
        } else {
            let headers = result.expect("上限以下なら成功する");
            assert_eq!(headers.len(), ref_count + 1);
            assert_eq!(header_list_size(&headers), total);
            assert!(total <= max_size);
        }
        Ok(())
    })?;
    Ok(())
}

/// 上限なし (None) のデコーダーは爆弾を最後まで展開する (従来挙動の保持)
#[test]
fn prop_unlimited_decodes_fully() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    let mut runner = noprop::Runner::new(seed);
    runner.run(CASES, |ctx| {
        let value_len = noprop::sample_usize_in(ctx, 0..=200);
        let ref_count = noprop::sample_usize_in(ctx, 0..=200);
        let huffman_value = noprop::sample_bool(ctx);
        let block = build_bomb(value_len, ref_count, huffman_value);

        let mut decoder = HpackDecoder::new(4096);
        // 上限を明示的に無制限にする
        decoder.set_max_header_list_size(None);

        let headers = decoder.decode(&block).expect("無制限なら成功する");
        // seed 1 個 + 参照 ref_count 個
        assert_eq!(headers.len(), ref_count + 1);
        Ok(())
    })?;
    Ok(())
}
