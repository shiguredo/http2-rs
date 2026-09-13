// WebKit との WebTransport over HTTP/2 相互運用テスト
//
// 検証対象のサーバー (examples/wt_server) を起動し、検証ページを HTTPS で配信し、
// Playwright の webkit を駆動して、ブラウザが公開している WebTransport の機能が
// 動作することを確認する。
//
// 使い方:
//   npm ci
//   npx playwright install webkit
//   npm test
//
// 環境変数:
//   WT_SERVER_BIN        検証対象のサーバーバイナリ (既定は target/debug/wt_server)
//   WT_PORT              検証対象のサーバーのポート (既定は 4443)
//   WT_PAGE_PORT         検証ページのポート (既定は 0 = 自動割り当て)
//   WT_BROWSER_ENGINES   検証するエンジン (カンマ区切り。既定は webkit)
//   WT_FORCE             1 のとき、Playwright やブラウザが無くても失敗させる
//   WT_DUMP_SERVER       1 のとき、終了時にサーバーログを出力する (失敗時は常に出力する)
//   WT_ORIGIN_MISMATCH   1 のとき、不一致の Origin を渡して 403 を確認する

import { spawn, spawnSync } from 'node:child_process';
import { existsSync, mkdirSync, readFileSync } from 'node:fs';
import { createRequire } from 'node:module';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

import { createPageServer } from './serve.mjs';

const here = dirname(fileURLToPath(import.meta.url));
const repoRoot = resolve(here, '..', '..');
const require = createRequire(import.meta.url);

// 検証対象のサーバーが待ち受けるポート
const WT_PORT = Number(process.env.WT_PORT || 4443);

// 不一致の Origin を渡すときに使う値
const MISMATCH_ORIGIN = 'https://127.0.0.1:1';

// Playwright が利用できるかを確認する
//
// 導入されていない環境ではテストを skip する。CI では必ず導入するため、
// skip はローカル開発時の利便のための挙動である。npm ci の失敗は skip の
// 対象外であり、そのままジョブの失敗になる。
function loadPlaywright() {
  try {
    return require('playwright');
  } catch {
    return null;
  }
}

function browserCacheDir() {
  const home = process.env.HOME || '';
  if (process.platform === 'darwin') {
    return join(home, 'Library', 'Caches', 'ms-playwright');
  }
  return join(home, '.cache', 'ms-playwright');
}

// 指定したエンジンのブラウザが導入済みかを確認する
//
// キャッシュディレクトリの有無ではなく、Playwright が解決した実行ファイルの
// 存在を見る (要求したバージョンと一致しないキャッシュを導入済みと判定しない)。
function hasEngine(playwright, engine) {
  const browserType = playwright[engine];
  if (!browserType) {
    return false;
  }
  try {
    return existsSync(browserType.executablePath());
  } catch {
    return false;
  }
}

// 検証ページ用の自己署名証明書を生成する
//
// リポジトリに秘密鍵を置かないため、実行のたびに生成する。ECDSA ではなく RSA を
// 使うのは、http3-rs の実測で ECDSA の証明書が Node.js の TLS 実装に拒否された
// ためである。ページ側の証明書検証は Playwright の ignoreHTTPSErrors で無効化する。
function generatePageCertificate() {
  const certsDir = join(here, 'certs');
  mkdirSync(certsDir, { recursive: true });

  const keyPath = join(certsDir, 'page-key.pem');
  const certPath = join(certsDir, 'page-cert.pem');
  if (existsSync(keyPath) && existsSync(certPath)) {
    return { key: readFileSync(keyPath), cert: readFileSync(certPath) };
  }

  const result = spawnSync(
    'openssl',
    [
      'req',
      '-x509',
      '-newkey',
      'rsa:2048',
      '-nodes',
      '-days',
      '1',
      '-subj',
      '/CN=127.0.0.1',
      '-addext',
      'subjectAltName=IP:127.0.0.1',
      '-keyout',
      keyPath,
      '-out',
      certPath,
    ],
    { stdio: ['ignore', 'ignore', 'pipe'] },
  );
  if (result.status !== 0) {
    throw new Error(`検証ページ用の証明書の生成に失敗した: ${result.stderr}`);
  }

  return { key: readFileSync(keyPath), cert: readFileSync(certPath) };
}

// 検証対象のサーバーを起動し、証明書ハッシュがログに出るまで待つ
function startWtServer(serverBin, allowedOrigin) {
  return new Promise((resolvePromise, rejectPromise) => {
    // ブラウザは必ず Origin を送るため、検証ページの Origin を許可する
    // (draft-ietf-webtrans-http2-15 Section 3.2)
    const child = spawn(
      serverBin,
      ['--listen', `127.0.0.1:${WT_PORT}`, '--allow-origin', allowedOrigin],
      {
        cwd: repoRoot,
        // RUST_LOG を指定すればサーバー側の debug ログも取得できる
        env: { ...process.env, RUST_LOG: process.env.RUST_LOG || 'info' },
        stdio: ['ignore', 'pipe', 'pipe'],
      },
    );

    let output = '';
    let settled = false;
    const timer = setTimeout(() => {
      if (!settled) {
        settled = true;
        child.kill('SIGTERM');
        rejectPromise(new Error(`サーバーの起動がタイムアウトした:\n${output}`));
      }
    }, 30000);

    const finish = (certificateHash) => {
      settled = true;
      clearTimeout(timer);
      resolvePromise({ child, certificateHash, getOutput: () => output });
    };

    const onData = (chunk) => {
      output += chunk.toString();
      // サーバーは起動時に証明書ハッシュを base64 で出力する
      const match = output.match(/Certificate SHA-256 \(base64\): ([A-Za-z0-9+/=]+)/);
      if (match && !settled) {
        // 証明書の準備ができてもバインドに失敗することがある
        // (別のプロセスがポートを占有している等)。起動ログで確定させる
        if (output.includes('WebTransport (HTTP/2) server listening on')) {
          finish(match[1]);
        } else if (output.includes('server error:')) {
          settled = true;
          clearTimeout(timer);
          child.kill('SIGTERM');
          rejectPromise(new Error(`サーバーの起動に失敗した:\n${output}`));
        }
      }
    };

    child.stdout.on('data', onData);
    child.stderr.on('data', onData);
    child.on('error', (e) => {
      if (!settled) {
        settled = true;
        clearTimeout(timer);
        rejectPromise(new Error(`サーバーの起動に失敗した: ${e.message}`));
      }
    });
    child.on('exit', (code) => {
      if (!settled) {
        settled = true;
        clearTimeout(timer);
        rejectPromise(new Error(`サーバーが起動前に終了した (code=${code}):\n${output}`));
      }
    });
  });
}

// WebKit を駆動して 1 エンジン分の検証を実行する
async function runEngine(playwright, engineName, config, pagePort) {
  const engine = playwright[engineName];
  if (!engine) {
    throw new Error(`未知のエンジン: ${engineName}`);
  }

  const browser = await engine.launch({ headless: true });
  const context = await browser.newContext({ ignoreHTTPSErrors: true });
  try {
    const page = await context.newPage();

    const results = [];
    page.on('console', (msg) => {
      const text = msg.text();
      if (text.startsWith('RESULT') || text.startsWith('INFO')) {
        results.push(text);
      }
    });
    page.on('pageerror', (err) => {
      results.push(`RESULT FAIL harness pageerror: ${err.message}`);
    });

    await page.addInitScript((cfg) => {
      window.WT_CONFIG = cfg;
    }, config);

    await page.goto(`https://127.0.0.1:${pagePort}/`, { waitUntil: 'load', timeout: 20000 });

    // 全項目の完了 (DONE) を待つ。完了しなかった場合も、それまでに得た
    // RESULT 行を診断のために返す
    let timedOut = false;
    try {
      await page.waitForFunction(() => document.getElementById('log').textContent.includes('DONE'), undefined, {
        timeout: 180000,
      });
    } catch {
      timedOut = true;
    }

    return { results, timedOut };
  } finally {
    await context.close();
    await browser.close();
  }
}

async function main() {
  const engines = (process.env.WT_BROWSER_ENGINES || 'webkit')
    .split(',')
    .map((s) => s.trim())
    .filter(Boolean);

  const playwright = loadPlaywright();
  // エンジン名の誤りは「未導入」と区別して報告する
  const unknownEngines = playwright ? engines.filter((name) => !playwright[name]) : [];
  if (unknownEngines.length > 0) {
    console.error(`NG 未知のエンジン: ${unknownEngines.join(', ')}`);
    process.exit(1);
  }
  const missingEngines = playwright ? engines.filter((name) => !hasEngine(playwright, name)) : [];
  if (!playwright || missingEngines.length > 0) {
    const reason = !playwright
      ? 'playwright が未導入'
      : `ブラウザが未導入 (${missingEngines.join(', ')} in ${browserCacheDir()})`;
    if (process.env.WT_FORCE === '1') {
      console.error(`NG ${reason}。WT_FORCE=1 のため失敗として扱う`);
      process.exit(1);
    }
    console.log(`SKIP ${reason}`);
    console.log('実行するには npm ci && npx playwright install webkit');
    process.exit(0);
  }

  const serverBin = process.env.WT_SERVER_BIN || join(repoRoot, 'target', 'debug', 'wt_server');
  if (!existsSync(serverBin)) {
    console.error(`NG 検証対象のサーバーが無い: ${serverBin}`);
    console.error('先に cargo build -p wt_server を実行すること');
    process.exit(1);
  }

  const expectOriginRejected = process.env.WT_ORIGIN_MISMATCH === '1';
  const pageCertificate = generatePageCertificate();
  // 検証ページのポートは自動割り当てにする (固定ポートの衝突を避ける)
  const pageServer = await createPageServer(
    Number(process.env.WT_PAGE_PORT || 0),
    here,
    pageCertificate.key,
    pageCertificate.cert,
  );
  const pagePort = pageServer.address().port;

  const allowedOrigin = expectOriginRejected ? MISMATCH_ORIGIN : `https://127.0.0.1:${pagePort}`;

  let server;
  let failed = false;
  const summary = [];

  try {
    server = await startWtServer(serverBin, allowedOrigin);

    for (const engineName of engines) {
      const config = {
        url: `https://127.0.0.1:${WT_PORT}/wt`,
        certificateHash: server.certificateHash,
        expectOriginRejected,
      };

      let runResult;
      try {
        runResult = await runEngine(playwright, engineName, config, pagePort);
      } catch (e) {
        failed = true;
        summary.push({ engine: engineName, result: `NG 実行エラー: ${e.message}` });
        continue;
      }

      const { results, timedOut } = runResult;
      const resultLines = results.filter((r) => r.startsWith('RESULT'));
      const failures = resultLines.filter((r) => r.startsWith('RESULT FAIL'));
      const passes = resultLines.filter((r) => r.startsWith('RESULT PASS'));
      // 必須の検証項目が実行されたかを照合する (項目が消えたら気づけるようにする)
      const expectedNames = expectOriginRejected
        ? ['originRejected']
        : ['session', 'reliability', 'bidiEcho', 'uniSend'];
      const passedNames = passes.map((line) => line.split(' ')[2]);
      const missingNames = expectedNames.filter((name) => !passedNames.includes(name));

      console.log(`=== ${engineName} ===`);
      for (const line of results) {
        console.log(`  ${line}`);
      }

      if (failures.length > 0 || missingNames.length > 0 || timedOut) {
        failed = true;
        const reasons = [];
        if (failures.length > 0) {
          reasons.push(`${failures.length} 件失敗`);
        }
        if (missingNames.length > 0) {
          reasons.push(`検証項目が実行されていない (${missingNames.join(', ')})`);
        }
        const reason = timedOut ? '実行が完了しなかった' : reasons.join('、');
        summary.push({ engine: engineName, result: `NG ${reason}` });
      } else {
        summary.push({ engine: engineName, result: `OK ${passes.length} 件成功` });
      }
    }
  } finally {
    if (server) {
      // 失敗したときは原因切り分けのため常にサーバーログを出す
      if (failed || process.env.WT_DUMP_SERVER === '1') {
        console.log('=== サーバーログ ===');
        console.log(server.getOutput());
      }
      server.child.kill('SIGTERM');
    }
    pageServer.close();
  }

  // Origin 拒否の確認では、接続失敗の理由が Origin 検証であることをログで確かめる
  if (!failed && expectOriginRejected && server && !server.getOutput().includes('origin rejected')) {
    failed = true;
    for (const line of summary) {
      line.result = 'NG Origin 検証による拒否をサーバーログで確認できない';
    }
  }

  console.log('=== 結果 ===');
  for (const line of summary) {
    console.log(`  ${line.engine}: ${line.result}`);
  }
  console.log(`  許可した Origin: ${allowedOrigin}`);

  process.exit(failed ? 1 : 0);
}

main().catch((e) => {
  console.error(`NG 想定外のエラー: ${e.stack || e.message}`);
  process.exit(1);
});
