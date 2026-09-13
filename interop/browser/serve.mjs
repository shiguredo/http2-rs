// 検証ページを HTTPS で配信する
//
// WebTransport は secure context を要求するため、検証ページも HTTPS で配信する
// 必要がある。ここで使う証明書はページ配信専用であり、WebTransport サーバーの
// 証明書とは別物である (接続先の検証はページ側の serverCertificateHashes が担う)。

import { createServer } from 'node:https';
import { readFileSync } from 'node:fs';
import { extname, join, resolve, sep } from 'node:path';

// 検証ページを配信するサーバーを起動する
export function createPageServer(port, dir, key, cert) {
  const types = {
    '.html': 'text/html; charset=utf-8',
    '.js': 'text/javascript; charset=utf-8',
  };

  const server = createServer({ key, cert }, (req, res) => {
    // クエリ文字列を除いてファイルを解決する
    const pathOnly = (req.url || '/').split('?')[0];
    const path = pathOnly === '/' ? '/index.html' : pathOnly;

    // 配信するのは検証ページとそのスクリプトだけにする。配信ディレクトリには
    // 実行時に生成した証明書と秘密鍵 (certs/) があるため、外に出るパスと
    // 許可しない拡張子は配信しない
    const resolved = resolve(dir, `.${path}`);
    const allowed = new Set(['.html', '.js']);
    if (!resolved.startsWith(resolve(dir) + sep) || !allowed.has(extname(resolved))) {
      res.writeHead(404);
      res.end('not found');
      return;
    }

    try {
      const body = readFileSync(join(dir, path));
      res.writeHead(200, {
        'content-type': types[extname(path)] || 'application/octet-stream',
      });
      res.end(body);
    } catch {
      res.writeHead(404);
      res.end('not found');
    }
  });

  return new Promise((resolve, reject) => {
    server.on('error', reject);
    server.listen(port, '127.0.0.1', () => resolve(server));
  });
}
