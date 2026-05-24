# nghttp2-sys

## 概要

[nghttp2](https://nghttp2.org/) C ライブラリへの低レベル FFI バインディングです。

ビルド時に nghttp2 のソースコードを Git で取得し、cmake で静的ライブラリとしてビルドします。バインディングは bindgen で生成済みのものを使用します。

## nghttp2 バージョン

1.69.0

## ビルド

### 必要なツール

- cmake
- C コンパイラ (cc)
- Git (nghttp2 ソースコードの取得)

### バインディングの再生成

通常は生成済みの `src/bindings.rs` を使用します。 nghttp2 のバージョンを更新した場合は `overwrite` feature で再生成します。

```bash
cargo build -p nghttp2-sys --features overwrite
```

再生成には libclang が必要です。

## nghttp2 ライセンス

<https://github.com/nghttp2/nghttp2/blob/master/COPYING>

```text
The MIT License

Copyright (c) 2012, 2014, 2015, 2016 Tatsuhiro Tsujikawa
Copyright (c) 2012, 2014, 2015, 2016 nghttp2 contributors

Permission is hereby granted, free of charge, to any person obtaining
a copy of this software and associated documentation files (the
"Software"), to deal in the Software without restriction, including
without limitation the rights to use, copy, modify, merge, publish,
distribute, sublicense, and/or sell copies of the Software, and to
permit persons to whom the Software is furnished to do so, subject to
the following conditions:

The above copyright notice and this permission notice shall be
included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND,
EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF
MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND
NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE
LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION
OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION
WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
```

## ライセンス

Apache License 2.0

```text
Copyright 2026-2026, Shiguredo Inc.

Licensed under the Apache License, Version 2.0 (the "License");
you may not use this file except in compliance with the License.
You may obtain a copy of the License at

    http://www.apache.org/licenses/LICENSE-2.0

Unless required by applicable law or agreed to in writing, software
distributed under the License is distributed on an "AS IS" BASIS,
WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
See the License for the specific language governing permissions and
limitations under the License.
```
