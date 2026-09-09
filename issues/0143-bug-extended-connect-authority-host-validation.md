# Extended CONNECT の :authority 検証が host 部 (uri-host) を検査しない

- Created: 2026-09-09
- Completed: {YYYY-MM-DD}
- Branch: feature/fix-extended-connect-authority-host-validation
- Polished: 2026-09-10

## 目的

HTTP/2 の Extended CONNECT (`:protocol` 付き CONNECT) の `:authority` 検証が host 部 (uri-host) を検査せず、SP や制御文字を含む不正な値を許容する問題を修正する。通常の CONNECT は `is_valid_connect_authority` で host 部を検査するようになったが、Extended CONNECT はその検査を通らないため検証強度が非対称になっている。

## 現状

`src/validation.rs` の `validate_request_headers` は、`:protocol` がある Extended CONNECT では `is_valid_connect_authority` を呼ばず、`:authority` の存在のみを検証する。そのため `:method=CONNECT` / `:protocol=webtransport` / `:scheme=https` / `:path=/` / `:authority="foo bar:443"` のような SP 入り host が通過する。`:authority` は `HeaderField::new` の疑似ヘッダー検査でも値の文字検査をされず、`validate_field_value` も内部 SP を許容する。

## 設計方針

- Extended CONNECT の `:authority` に対しても host 部 (uri-host) の文字検査を行う。通常の CONNECT と同じ `is_valid_reg_name` / `is_valid_ipv6_literal_chars` を再利用する
- Extended CONNECT の `:authority` はポートを必須としないため、host 部と任意のポートに分解して検証する (`host[:port]`)。ポートがある場合は既存の `is_valid_port` を適用する
- 不正な host を拒否し、正常な host (`example.com`、`example.com:443`、`[::1]`、`[::1]:443`) を受理するテストを追加する

## 完了条件

- Extended CONNECT の `:authority` に SP・制御文字・不正文字を含む host が拒否されること
- 正常な host (`example.com`、`example.com:443`、`[::1]`、`[::1]:443`) が受理されること
- テストが追加され、`cargo test --all` が通過すること
