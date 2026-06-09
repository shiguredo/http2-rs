//! HPACK Huffman 符号化 (RFC 7541 Appendix B)
//!
//! HPACK で使用される Huffman 符号化/復号化を提供する。

use crate::error::{Error, Result};

/// Huffman シンボル (符号語と長さ)
#[derive(Clone, Copy)]
struct HuffmanSym {
    /// ビット長
    bits: u8,
    /// 符号語 (左詰め)
    code: u32,
}

/// Huffman 符号テーブル (RFC 7541 Appendix B)
static HUFFMAN_TABLE: [HuffmanSym; 257] = [
    HuffmanSym {
        bits: 13,
        code: 0xffc0_0000,
    }, // 0
    HuffmanSym {
        bits: 23,
        code: 0xffff_b000,
    }, // 1
    HuffmanSym {
        bits: 28,
        code: 0xffff_fe20,
    }, // 2
    HuffmanSym {
        bits: 28,
        code: 0xffff_fe30,
    }, // 3
    HuffmanSym {
        bits: 28,
        code: 0xffff_fe40,
    }, // 4
    HuffmanSym {
        bits: 28,
        code: 0xffff_fe50,
    }, // 5
    HuffmanSym {
        bits: 28,
        code: 0xffff_fe60,
    }, // 6
    HuffmanSym {
        bits: 28,
        code: 0xffff_fe70,
    }, // 7
    HuffmanSym {
        bits: 28,
        code: 0xffff_fe80,
    }, // 8
    HuffmanSym {
        bits: 24,
        code: 0xffff_ea00,
    }, // 9
    HuffmanSym {
        bits: 30,
        code: 0xffff_fff0,
    }, // 10
    HuffmanSym {
        bits: 28,
        code: 0xffff_fe90,
    }, // 11
    HuffmanSym {
        bits: 28,
        code: 0xffff_fea0,
    }, // 12
    HuffmanSym {
        bits: 30,
        code: 0xffff_fff4,
    }, // 13
    HuffmanSym {
        bits: 28,
        code: 0xffff_feb0,
    }, // 14
    HuffmanSym {
        bits: 28,
        code: 0xffff_fec0,
    }, // 15
    HuffmanSym {
        bits: 28,
        code: 0xffff_fed0,
    }, // 16
    HuffmanSym {
        bits: 28,
        code: 0xffff_fee0,
    }, // 17
    HuffmanSym {
        bits: 28,
        code: 0xffff_fef0,
    }, // 18
    HuffmanSym {
        bits: 28,
        code: 0xffff_ff00,
    }, // 19
    HuffmanSym {
        bits: 28,
        code: 0xffff_ff10,
    }, // 20
    HuffmanSym {
        bits: 28,
        code: 0xffff_ff20,
    }, // 21
    HuffmanSym {
        bits: 30,
        code: 0xffff_fff8,
    }, // 22
    HuffmanSym {
        bits: 28,
        code: 0xffff_ff30,
    }, // 23
    HuffmanSym {
        bits: 28,
        code: 0xffff_ff40,
    }, // 24
    HuffmanSym {
        bits: 28,
        code: 0xffff_ff50,
    }, // 25
    HuffmanSym {
        bits: 28,
        code: 0xffff_ff60,
    }, // 26
    HuffmanSym {
        bits: 28,
        code: 0xffff_ff70,
    }, // 27
    HuffmanSym {
        bits: 28,
        code: 0xffff_ff80,
    }, // 28
    HuffmanSym {
        bits: 28,
        code: 0xffff_ff90,
    }, // 29
    HuffmanSym {
        bits: 28,
        code: 0xffff_ffa0,
    }, // 30
    HuffmanSym {
        bits: 28,
        code: 0xffff_ffb0,
    }, // 31
    HuffmanSym {
        bits: 6,
        code: 0x5000_0000,
    }, // 32 ' '
    HuffmanSym {
        bits: 10,
        code: 0xfe00_0000,
    }, // 33 '!'
    HuffmanSym {
        bits: 10,
        code: 0xfe40_0000,
    }, // 34 '"'
    HuffmanSym {
        bits: 12,
        code: 0xffa0_0000,
    }, // 35 '#'
    HuffmanSym {
        bits: 13,
        code: 0xffc8_0000,
    }, // 36 '$'
    HuffmanSym {
        bits: 6,
        code: 0x5400_0000,
    }, // 37 '%'
    HuffmanSym {
        bits: 8,
        code: 0xf800_0000,
    }, // 38 '&'
    HuffmanSym {
        bits: 11,
        code: 0xff40_0000,
    }, // 39 '\''
    HuffmanSym {
        bits: 10,
        code: 0xfe80_0000,
    }, // 40 '('
    HuffmanSym {
        bits: 10,
        code: 0xfec0_0000,
    }, // 41 ')'
    HuffmanSym {
        bits: 8,
        code: 0xf900_0000,
    }, // 42 '*'
    HuffmanSym {
        bits: 11,
        code: 0xff60_0000,
    }, // 43 '+'
    HuffmanSym {
        bits: 8,
        code: 0xfa00_0000,
    }, // 44 ','
    HuffmanSym {
        bits: 6,
        code: 0x5800_0000,
    }, // 45 '-'
    HuffmanSym {
        bits: 6,
        code: 0x5c00_0000,
    }, // 46 '.'
    HuffmanSym {
        bits: 6,
        code: 0x6000_0000,
    }, // 47 '/'
    HuffmanSym {
        bits: 5,
        code: 0x0000_0000,
    }, // 48 '0'
    HuffmanSym {
        bits: 5,
        code: 0x0800_0000,
    }, // 49 '1'
    HuffmanSym {
        bits: 5,
        code: 0x1000_0000,
    }, // 50 '2'
    HuffmanSym {
        bits: 6,
        code: 0x6400_0000,
    }, // 51 '3'
    HuffmanSym {
        bits: 6,
        code: 0x6800_0000,
    }, // 52 '4'
    HuffmanSym {
        bits: 6,
        code: 0x6c00_0000,
    }, // 53 '5'
    HuffmanSym {
        bits: 6,
        code: 0x7000_0000,
    }, // 54 '6'
    HuffmanSym {
        bits: 6,
        code: 0x7400_0000,
    }, // 55 '7'
    HuffmanSym {
        bits: 6,
        code: 0x7800_0000,
    }, // 56 '8'
    HuffmanSym {
        bits: 6,
        code: 0x7c00_0000,
    }, // 57 '9'
    HuffmanSym {
        bits: 7,
        code: 0xb800_0000,
    }, // 58 ':'
    HuffmanSym {
        bits: 8,
        code: 0xfb00_0000,
    }, // 59 ';'
    HuffmanSym {
        bits: 15,
        code: 0xfff8_0000,
    }, // 60 '<'
    HuffmanSym {
        bits: 6,
        code: 0x8000_0000,
    }, // 61 '='
    HuffmanSym {
        bits: 12,
        code: 0xffb0_0000,
    }, // 62 '>'
    HuffmanSym {
        bits: 10,
        code: 0xff00_0000,
    }, // 63 '?'
    HuffmanSym {
        bits: 13,
        code: 0xffd0_0000,
    }, // 64 '@'
    HuffmanSym {
        bits: 6,
        code: 0x8400_0000,
    }, // 65 'A'
    HuffmanSym {
        bits: 7,
        code: 0xba00_0000,
    }, // 66 'B'
    HuffmanSym {
        bits: 7,
        code: 0xbc00_0000,
    }, // 67 'C'
    HuffmanSym {
        bits: 7,
        code: 0xbe00_0000,
    }, // 68 'D'
    HuffmanSym {
        bits: 7,
        code: 0xc000_0000,
    }, // 69 'E'
    HuffmanSym {
        bits: 7,
        code: 0xc200_0000,
    }, // 70 'F'
    HuffmanSym {
        bits: 7,
        code: 0xc400_0000,
    }, // 71 'G'
    HuffmanSym {
        bits: 7,
        code: 0xc600_0000,
    }, // 72 'H'
    HuffmanSym {
        bits: 7,
        code: 0xc800_0000,
    }, // 73 'I'
    HuffmanSym {
        bits: 7,
        code: 0xca00_0000,
    }, // 74 'J'
    HuffmanSym {
        bits: 7,
        code: 0xcc00_0000,
    }, // 75 'K'
    HuffmanSym {
        bits: 7,
        code: 0xce00_0000,
    }, // 76 'L'
    HuffmanSym {
        bits: 7,
        code: 0xd000_0000,
    }, // 77 'M'
    HuffmanSym {
        bits: 7,
        code: 0xd200_0000,
    }, // 78 'N'
    HuffmanSym {
        bits: 7,
        code: 0xd400_0000,
    }, // 79 'O'
    HuffmanSym {
        bits: 7,
        code: 0xd600_0000,
    }, // 80 'P'
    HuffmanSym {
        bits: 7,
        code: 0xd800_0000,
    }, // 81 'Q'
    HuffmanSym {
        bits: 7,
        code: 0xda00_0000,
    }, // 82 'R'
    HuffmanSym {
        bits: 7,
        code: 0xdc00_0000,
    }, // 83 'S'
    HuffmanSym {
        bits: 7,
        code: 0xde00_0000,
    }, // 84 'T'
    HuffmanSym {
        bits: 7,
        code: 0xe000_0000,
    }, // 85 'U'
    HuffmanSym {
        bits: 7,
        code: 0xe200_0000,
    }, // 86 'V'
    HuffmanSym {
        bits: 7,
        code: 0xe400_0000,
    }, // 87 'W'
    HuffmanSym {
        bits: 8,
        code: 0xfc00_0000,
    }, // 88 'X'
    HuffmanSym {
        bits: 7,
        code: 0xe600_0000,
    }, // 89 'Y'
    HuffmanSym {
        bits: 8,
        code: 0xfd00_0000,
    }, // 90 'Z'
    HuffmanSym {
        bits: 13,
        code: 0xffd8_0000,
    }, // 91 '['
    HuffmanSym {
        bits: 19,
        code: 0xfffe_0000,
    }, // 92 '\\'
    HuffmanSym {
        bits: 13,
        code: 0xffe0_0000,
    }, // 93 ']'
    HuffmanSym {
        bits: 14,
        code: 0xfff0_0000,
    }, // 94 '^'
    HuffmanSym {
        bits: 6,
        code: 0x8800_0000,
    }, // 95 '_'
    HuffmanSym {
        bits: 15,
        code: 0xfffa_0000,
    }, // 96 '`'
    HuffmanSym {
        bits: 5,
        code: 0x1800_0000,
    }, // 97 'a'
    HuffmanSym {
        bits: 6,
        code: 0x8c00_0000,
    }, // 98 'b'
    HuffmanSym {
        bits: 5,
        code: 0x2000_0000,
    }, // 99 'c'
    HuffmanSym {
        bits: 6,
        code: 0x9000_0000,
    }, // 100 'd'
    HuffmanSym {
        bits: 5,
        code: 0x2800_0000,
    }, // 101 'e'
    HuffmanSym {
        bits: 6,
        code: 0x9400_0000,
    }, // 102 'f'
    HuffmanSym {
        bits: 6,
        code: 0x9800_0000,
    }, // 103 'g'
    HuffmanSym {
        bits: 6,
        code: 0x9c00_0000,
    }, // 104 'h'
    HuffmanSym {
        bits: 5,
        code: 0x3000_0000,
    }, // 105 'i'
    HuffmanSym {
        bits: 7,
        code: 0xe800_0000,
    }, // 106 'j'
    HuffmanSym {
        bits: 7,
        code: 0xea00_0000,
    }, // 107 'k'
    HuffmanSym {
        bits: 6,
        code: 0xa000_0000,
    }, // 108 'l'
    HuffmanSym {
        bits: 6,
        code: 0xa400_0000,
    }, // 109 'm'
    HuffmanSym {
        bits: 6,
        code: 0xa800_0000,
    }, // 110 'n'
    HuffmanSym {
        bits: 5,
        code: 0x3800_0000,
    }, // 111 'o'
    HuffmanSym {
        bits: 6,
        code: 0xac00_0000,
    }, // 112 'p'
    HuffmanSym {
        bits: 7,
        code: 0xec00_0000,
    }, // 113 'q'
    HuffmanSym {
        bits: 6,
        code: 0xb000_0000,
    }, // 114 'r'
    HuffmanSym {
        bits: 5,
        code: 0x4000_0000,
    }, // 115 's'
    HuffmanSym {
        bits: 5,
        code: 0x4800_0000,
    }, // 116 't'
    HuffmanSym {
        bits: 6,
        code: 0xb400_0000,
    }, // 117 'u'
    HuffmanSym {
        bits: 7,
        code: 0xee00_0000,
    }, // 118 'v'
    HuffmanSym {
        bits: 7,
        code: 0xf000_0000,
    }, // 119 'w'
    HuffmanSym {
        bits: 7,
        code: 0xf200_0000,
    }, // 120 'x'
    HuffmanSym {
        bits: 7,
        code: 0xf400_0000,
    }, // 121 'y'
    HuffmanSym {
        bits: 7,
        code: 0xf600_0000,
    }, // 122 'z'
    HuffmanSym {
        bits: 15,
        code: 0xfffc_0000,
    }, // 123 '{'
    HuffmanSym {
        bits: 11,
        code: 0xff80_0000,
    }, // 124 '|'
    HuffmanSym {
        bits: 14,
        code: 0xfff4_0000,
    }, // 125 '}'
    HuffmanSym {
        bits: 13,
        code: 0xffe8_0000,
    }, // 126 '~'
    HuffmanSym {
        bits: 28,
        code: 0xffff_ffc0,
    }, // 127
    HuffmanSym {
        bits: 20,
        code: 0xfffe_6000,
    }, // 128
    HuffmanSym {
        bits: 22,
        code: 0xffff_4800,
    }, // 129
    HuffmanSym {
        bits: 20,
        code: 0xfffe_7000,
    }, // 130
    HuffmanSym {
        bits: 20,
        code: 0xfffe_8000,
    }, // 131
    HuffmanSym {
        bits: 22,
        code: 0xffff_4c00,
    }, // 132
    HuffmanSym {
        bits: 22,
        code: 0xffff_5000,
    }, // 133
    HuffmanSym {
        bits: 22,
        code: 0xffff_5400,
    }, // 134
    HuffmanSym {
        bits: 23,
        code: 0xffff_b200,
    }, // 135
    HuffmanSym {
        bits: 22,
        code: 0xffff_5800,
    }, // 136
    HuffmanSym {
        bits: 23,
        code: 0xffff_b400,
    }, // 137
    HuffmanSym {
        bits: 23,
        code: 0xffff_b600,
    }, // 138
    HuffmanSym {
        bits: 23,
        code: 0xffff_b800,
    }, // 139
    HuffmanSym {
        bits: 23,
        code: 0xffff_ba00,
    }, // 140
    HuffmanSym {
        bits: 23,
        code: 0xffff_bc00,
    }, // 141
    HuffmanSym {
        bits: 24,
        code: 0xffff_eb00,
    }, // 142
    HuffmanSym {
        bits: 23,
        code: 0xffff_be00,
    }, // 143
    HuffmanSym {
        bits: 24,
        code: 0xffff_ec00,
    }, // 144
    HuffmanSym {
        bits: 24,
        code: 0xffff_ed00,
    }, // 145
    HuffmanSym {
        bits: 22,
        code: 0xffff_5c00,
    }, // 146
    HuffmanSym {
        bits: 23,
        code: 0xffff_c000,
    }, // 147
    HuffmanSym {
        bits: 24,
        code: 0xffff_ee00,
    }, // 148
    HuffmanSym {
        bits: 23,
        code: 0xffff_c200,
    }, // 149
    HuffmanSym {
        bits: 23,
        code: 0xffff_c400,
    }, // 150
    HuffmanSym {
        bits: 23,
        code: 0xffff_c600,
    }, // 151
    HuffmanSym {
        bits: 23,
        code: 0xffff_c800,
    }, // 152
    HuffmanSym {
        bits: 21,
        code: 0xfffe_e000,
    }, // 153
    HuffmanSym {
        bits: 22,
        code: 0xffff_6000,
    }, // 154
    HuffmanSym {
        bits: 23,
        code: 0xffff_ca00,
    }, // 155
    HuffmanSym {
        bits: 22,
        code: 0xffff_6400,
    }, // 156
    HuffmanSym {
        bits: 23,
        code: 0xffff_cc00,
    }, // 157
    HuffmanSym {
        bits: 23,
        code: 0xffff_ce00,
    }, // 158
    HuffmanSym {
        bits: 24,
        code: 0xffff_ef00,
    }, // 159
    HuffmanSym {
        bits: 22,
        code: 0xffff_6800,
    }, // 160
    HuffmanSym {
        bits: 21,
        code: 0xfffe_e800,
    }, // 161
    HuffmanSym {
        bits: 20,
        code: 0xfffe_9000,
    }, // 162
    HuffmanSym {
        bits: 22,
        code: 0xffff_6c00,
    }, // 163
    HuffmanSym {
        bits: 22,
        code: 0xffff_7000,
    }, // 164
    HuffmanSym {
        bits: 23,
        code: 0xffff_d000,
    }, // 165
    HuffmanSym {
        bits: 23,
        code: 0xffff_d200,
    }, // 166
    HuffmanSym {
        bits: 21,
        code: 0xfffe_f000,
    }, // 167
    HuffmanSym {
        bits: 23,
        code: 0xffff_d400,
    }, // 168
    HuffmanSym {
        bits: 22,
        code: 0xffff_7400,
    }, // 169
    HuffmanSym {
        bits: 22,
        code: 0xffff_7800,
    }, // 170
    HuffmanSym {
        bits: 24,
        code: 0xffff_f000,
    }, // 171
    HuffmanSym {
        bits: 21,
        code: 0xfffe_f800,
    }, // 172
    HuffmanSym {
        bits: 22,
        code: 0xffff_7c00,
    }, // 173
    HuffmanSym {
        bits: 23,
        code: 0xffff_d600,
    }, // 174
    HuffmanSym {
        bits: 23,
        code: 0xffff_d800,
    }, // 175
    HuffmanSym {
        bits: 21,
        code: 0xffff_0000,
    }, // 176
    HuffmanSym {
        bits: 21,
        code: 0xffff_0800,
    }, // 177
    HuffmanSym {
        bits: 22,
        code: 0xffff_8000,
    }, // 178
    HuffmanSym {
        bits: 21,
        code: 0xffff_1000,
    }, // 179
    HuffmanSym {
        bits: 23,
        code: 0xffff_da00,
    }, // 180
    HuffmanSym {
        bits: 22,
        code: 0xffff_8400,
    }, // 181
    HuffmanSym {
        bits: 23,
        code: 0xffff_dc00,
    }, // 182
    HuffmanSym {
        bits: 23,
        code: 0xffff_de00,
    }, // 183
    HuffmanSym {
        bits: 20,
        code: 0xfffe_a000,
    }, // 184
    HuffmanSym {
        bits: 22,
        code: 0xffff_8800,
    }, // 185
    HuffmanSym {
        bits: 22,
        code: 0xffff_8c00,
    }, // 186
    HuffmanSym {
        bits: 22,
        code: 0xffff_9000,
    }, // 187
    HuffmanSym {
        bits: 23,
        code: 0xffff_e000,
    }, // 188
    HuffmanSym {
        bits: 22,
        code: 0xffff_9400,
    }, // 189
    HuffmanSym {
        bits: 22,
        code: 0xffff_9800,
    }, // 190
    HuffmanSym {
        bits: 23,
        code: 0xffff_e200,
    }, // 191
    HuffmanSym {
        bits: 26,
        code: 0xffff_f800,
    }, // 192
    HuffmanSym {
        bits: 26,
        code: 0xffff_f840,
    }, // 193
    HuffmanSym {
        bits: 20,
        code: 0xfffe_b000,
    }, // 194
    HuffmanSym {
        bits: 19,
        code: 0xfffe_2000,
    }, // 195
    HuffmanSym {
        bits: 22,
        code: 0xffff_9c00,
    }, // 196
    HuffmanSym {
        bits: 23,
        code: 0xffff_e400,
    }, // 197
    HuffmanSym {
        bits: 22,
        code: 0xffff_a000,
    }, // 198
    HuffmanSym {
        bits: 25,
        code: 0xffff_f600,
    }, // 199
    HuffmanSym {
        bits: 26,
        code: 0xffff_f880,
    }, // 200
    HuffmanSym {
        bits: 26,
        code: 0xffff_f8c0,
    }, // 201
    HuffmanSym {
        bits: 26,
        code: 0xffff_f900,
    }, // 202
    HuffmanSym {
        bits: 27,
        code: 0xffff_fbc0,
    }, // 203
    HuffmanSym {
        bits: 27,
        code: 0xffff_fbe0,
    }, // 204
    HuffmanSym {
        bits: 26,
        code: 0xffff_f940,
    }, // 205
    HuffmanSym {
        bits: 24,
        code: 0xffff_f100,
    }, // 206
    HuffmanSym {
        bits: 25,
        code: 0xffff_f680,
    }, // 207
    HuffmanSym {
        bits: 19,
        code: 0xfffe_4000,
    }, // 208
    HuffmanSym {
        bits: 21,
        code: 0xffff_1800,
    }, // 209
    HuffmanSym {
        bits: 26,
        code: 0xffff_f980,
    }, // 210
    HuffmanSym {
        bits: 27,
        code: 0xffff_fc00,
    }, // 211
    HuffmanSym {
        bits: 27,
        code: 0xffff_fc20,
    }, // 212
    HuffmanSym {
        bits: 26,
        code: 0xffff_f9c0,
    }, // 213
    HuffmanSym {
        bits: 27,
        code: 0xffff_fc40,
    }, // 214
    HuffmanSym {
        bits: 24,
        code: 0xffff_f200,
    }, // 215
    HuffmanSym {
        bits: 21,
        code: 0xffff_2000,
    }, // 216
    HuffmanSym {
        bits: 21,
        code: 0xffff_2800,
    }, // 217
    HuffmanSym {
        bits: 26,
        code: 0xffff_fa00,
    }, // 218
    HuffmanSym {
        bits: 26,
        code: 0xffff_fa40,
    }, // 219
    HuffmanSym {
        bits: 28,
        code: 0xffff_ffd0,
    }, // 220
    HuffmanSym {
        bits: 27,
        code: 0xffff_fc60,
    }, // 221
    HuffmanSym {
        bits: 27,
        code: 0xffff_fc80,
    }, // 222
    HuffmanSym {
        bits: 27,
        code: 0xffff_fca0,
    }, // 223
    HuffmanSym {
        bits: 20,
        code: 0xfffe_c000,
    }, // 224
    HuffmanSym {
        bits: 24,
        code: 0xffff_f300,
    }, // 225
    HuffmanSym {
        bits: 20,
        code: 0xfffe_d000,
    }, // 226
    HuffmanSym {
        bits: 21,
        code: 0xffff_3000,
    }, // 227
    HuffmanSym {
        bits: 22,
        code: 0xffff_a400,
    }, // 228
    HuffmanSym {
        bits: 21,
        code: 0xffff_3800,
    }, // 229
    HuffmanSym {
        bits: 21,
        code: 0xffff_4000,
    }, // 230
    HuffmanSym {
        bits: 23,
        code: 0xffff_e600,
    }, // 231
    HuffmanSym {
        bits: 22,
        code: 0xffff_a800,
    }, // 232
    HuffmanSym {
        bits: 22,
        code: 0xffff_ac00,
    }, // 233
    HuffmanSym {
        bits: 25,
        code: 0xffff_f700,
    }, // 234
    HuffmanSym {
        bits: 25,
        code: 0xffff_f780,
    }, // 235
    HuffmanSym {
        bits: 24,
        code: 0xffff_f400,
    }, // 236
    HuffmanSym {
        bits: 24,
        code: 0xffff_f500,
    }, // 237
    HuffmanSym {
        bits: 26,
        code: 0xffff_fa80,
    }, // 238
    HuffmanSym {
        bits: 23,
        code: 0xffff_e800,
    }, // 239
    HuffmanSym {
        bits: 26,
        code: 0xffff_fac0,
    }, // 240
    HuffmanSym {
        bits: 27,
        code: 0xffff_fcc0,
    }, // 241
    HuffmanSym {
        bits: 26,
        code: 0xffff_fb00,
    }, // 242
    HuffmanSym {
        bits: 26,
        code: 0xffff_fb40,
    }, // 243
    HuffmanSym {
        bits: 27,
        code: 0xffff_fce0,
    }, // 244
    HuffmanSym {
        bits: 27,
        code: 0xffff_fd00,
    }, // 245
    HuffmanSym {
        bits: 27,
        code: 0xffff_fd20,
    }, // 246
    HuffmanSym {
        bits: 27,
        code: 0xffff_fd40,
    }, // 247
    HuffmanSym {
        bits: 27,
        code: 0xffff_fd60,
    }, // 248
    HuffmanSym {
        bits: 28,
        code: 0xffff_ffe0,
    }, // 249
    HuffmanSym {
        bits: 27,
        code: 0xffff_fd80,
    }, // 250
    HuffmanSym {
        bits: 27,
        code: 0xffff_fda0,
    }, // 251
    HuffmanSym {
        bits: 27,
        code: 0xffff_fdc0,
    }, // 252
    HuffmanSym {
        bits: 27,
        code: 0xffff_fde0,
    }, // 253
    HuffmanSym {
        bits: 27,
        code: 0xffff_fe00,
    }, // 254
    HuffmanSym {
        bits: 26,
        code: 0xffff_fb80,
    }, // 255
    HuffmanSym {
        bits: 30,
        code: 0xffff_fffc,
    }, // 256 EOS
];

/// Huffman エンコード後の長さを計算する
#[must_use]
pub fn encoded_len(data: &[u8]) -> usize {
    let bits: usize = data
        .iter()
        .map(|&b| HUFFMAN_TABLE[b as usize].bits as usize)
        .sum();
    bits.div_ceil(8)
}

/// Huffman エンコードする
///
/// 成功時はエンコードしたバイト数を返す。
///
/// # Errors
///
/// バッファが不足している場合は `Err` を返す。
pub fn encode(buf: &mut [u8], data: &[u8]) -> Result<usize> {
    let required = encoded_len(data);
    if buf.len() < required {
        return Err(Error::hpack_error(format!(
            "Huffman encode buffer too short: required {required} bytes, available {} bytes",
            buf.len()
        )));
    }

    let mut acc: u64 = 0;
    let mut acc_bits: u32 = 0;
    let mut offset = 0;

    for &byte in data {
        let sym = &HUFFMAN_TABLE[byte as usize];
        // 符号を 64 ビットの上位に配置してからシフト
        let code_64 = (sym.code as u64) << 32;
        acc |= code_64 >> acc_bits;
        acc_bits += u32::from(sym.bits);

        while acc_bits >= 8 {
            buf[offset] = (acc >> 56) as u8;
            offset += 1;
            acc <<= 8;
            acc_bits -= 8;
        }
    }

    // RFC 7541 Section 5.2: 残りビットは EOS 符号の最上位ビット (すべて 1) でパディングする
    if acc_bits > 0 {
        let padding_bits = 8 - acc_bits;
        let padding_mask = (1u64 << padding_bits) - 1;
        let byte = ((acc >> 56) as u8) | (padding_mask as u8);
        buf[offset] = byte;
        offset += 1;
    }

    Ok(offset)
}

/// Huffman エンコードして Vec<u8> として返す
#[must_use]
pub fn encode_to_vec(data: &[u8]) -> Vec<u8> {
    let len = encoded_len(data);
    let mut buf = vec![0u8; len];
    let _ = encode(&mut buf, data);
    buf
}

/// Huffman デコードする
///
/// # Errors
///
/// 不正な Huffman データの場合は `Err` を返す。
pub fn decode(data: &[u8]) -> Result<Vec<u8>> {
    let mut result = Vec::new();
    let mut acc: u64 = 0;
    let mut acc_bits: u32 = 0;

    for &byte in data {
        acc = (acc << 8) | (byte as u64);
        acc_bits += 8;

        // 十分なビットがある間、デコードを続ける
        'decode: while acc_bits >= 5 {
            // 現在のビットを左詰めで 32 ビットに配置
            let current = if acc_bits >= 32 {
                (acc >> (acc_bits - 32)) as u32
            } else {
                (acc << (32 - acc_bits)) as u32
            };

            // 符号テーブルを検索
            for (sym_idx, sym) in HUFFMAN_TABLE.iter().enumerate() {
                if u32::from(sym.bits) <= acc_bits {
                    // 符号のマスクを作成 (左詰め)
                    let mask = if sym.bits >= 32 {
                        0xffff_ffff_u32
                    } else {
                        !((1u32 << (32 - sym.bits)) - 1)
                    };

                    // 符号が一致するかチェック
                    if (current & mask) == sym.code {
                        if sym_idx == 256 {
                            // RFC 7541 Section 5.2:
                            // EOS シンボル (256) を含む Huffman エンコードされた文字列リテラルは
                            // デコードエラーとして扱わなければならない (MUST)。
                            return Err(Error::hpack_error(
                                "EOS symbol in Huffman-encoded string literal is not allowed",
                            ));
                        }
                        result.push(sym_idx as u8);
                        acc_bits -= u32::from(sym.bits);
                        // 使用したビットをクリア
                        if acc_bits > 0 {
                            acc &= (1u64 << acc_bits) - 1;
                        } else {
                            acc = 0;
                        }
                        continue 'decode;
                    }
                }
            }

            // マッチしない = まだ符号が完成していない
            break;
        }
    }

    // RFC 7541 Section 5.2: パディングは EOS 符号の最上位ビット (すべて 1) と一致しなければならず、
    // 一致しないパディングはデコードエラーとして扱わなければならない (MUST)
    if acc_bits > 0 && acc_bits <= 7 {
        let mask = (1u64 << acc_bits) - 1;
        if (acc & mask) == mask {
            return Ok(result);
        }
        // パディングが不正
        return Err(Error::hpack_error("invalid Huffman padding"));
    }

    if acc_bits > 7 {
        // RFC 7541 Section 5.2: 7 ビットより長いパディングはデコードエラーとして扱わなければならない (MUST)
        return Err(Error::hpack_error("incomplete Huffman data"));
    }

    Ok(result)
}
