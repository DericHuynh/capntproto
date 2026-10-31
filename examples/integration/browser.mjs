import assert from 'node:assert/strict';
import { createServer } from 'node:http';
import { readFile, realpath } from 'node:fs/promises';
import path from 'node:path';
import { chromium } from '@playwright/test';
import { X509Certificate, createHash } from 'node:crypto';
import { ready, registerCleanup } from './process.mjs';
export async function browserTests({ root, target, credentials }) {
  const source = path.join(root, 'examples/wasip3');
  const http = createServer(async (request, response) => {
    try {
      const url = new URL(request.url, 'http://localhost'); const pathname = url.pathname === '/' ? '/web/index.html' : decodeURIComponent(url.pathname);
      if (!pathname.startsWith('/web/') && !pathname.startsWith('/target/browser/')) throw new Error('not a public asset');
      const file = await realpath(path.join(source, pathname)); if (!file.startsWith(`${source}${path.sep}`)) throw new Error('invalid path');
      const content = await readFile(file); response.setHeader('Content-Type', file.endsWith('.wasm') ? 'application/wasm' : file.endsWith('.js') ? 'text/javascript' : 'text/html'); response.end(content);
    } catch { response.writeHead(404); response.end('Not found'); }
  });
  await new Promise((resolve, reject) => { http.once('error', reject); http.listen(0, '127.0.0.1', resolve); });
  const origin = `http://127.0.0.1:${http.address().port}`;
  const pin = createHash('sha256').update(new X509Certificate(await readFile(path.join(credentials, 'server.pem'))).raw).digest('hex');
  let browser; const cleanup = async () => { await browser?.close(); http.closeAllConnections(); await new Promise(resolve => http.close(resolve)); }; const unregister = registerCleanup(cleanup);
  try {
    browser = await chromium.launch({ executablePath: process.env.PLAYWRIGHT_CHROMIUM_EXECUTABLE_PATH || undefined, args: ['--no-sandbox'] });
    for (const driver of ['legacy', 'portable']) {
      const backend = await ready(path.join(target, `debug/rpc-server${process.platform === 'win32' ? '.exe' : ''}`), ['--listen', '127.0.0.1:0', '--transport', 'webtransport', '--driver', driver, '--cert', path.join(credentials, 'server.pem'), '--key', path.join(credentials, 'server-key.pem'), '--origin', origin], { cwd: root });
      const page = await browser.newPage(); const errors = []; page.on('pageerror', error => errors.push(String(error)));
      try {
        await page.goto(`${origin}/web/index.html`); await page.waitForSelector('form[data-ready="true"]', { timeout: 30_000 });
        await page.locator('[name=url]').fill(`https://${backend.address}/rpc`); await page.locator('[name=pin]').fill(pin); await page.locator('[name=prefix]').fill('Hello, '); await page.locator('[name=value]').fill('browser 🦀'); await page.locator('button').click();
        await page.waitForFunction(() => document.querySelector('output').dataset.state !== undefined, undefined, { timeout: 20_000 });
        assert.equal(await page.locator('output').getAttribute('data-state'), 'success', await page.locator('output').textContent()); assert.equal(await page.locator('output').textContent(), 'Hello, browser 🦀');
        const checks = await page.evaluate(async ({ address, pin }) => {
          const { frontend } = await import('/web/app.js'); const hash = Uint8Array.from(pin.match(/../g), h => parseInt(h, 16)); const url = `https://${address}/rpc`;
          const repeated = await Promise.all([0,1,2,3].map(i => frontend.greet(url, hash, `${i}:`, `value-${i}`))); const large = '🦀'.repeat(16_384); const echoed = await frontend.greet(url, hash, 'large:', large);
          const rejected = async (url, pin) => { try { await frontend.greet(url, pin, '', 'rejected'); return false; } catch { return true; } };
          return { repeated, large: echoed === `large:${large}`, badPin: await rejected(url, new Uint8Array(32)), badLength: await rejected(url, new Uint8Array(3)), badPath: await rejected(`https://${address}/denied`, hash), afterFailure: await frontend.greet(url, hash, 'after:', 'failure') };
        }, { address: backend.address, pin });
        assert.deepEqual(checks, { repeated: ['0:value-0','1:value-1','2:value-2','3:value-3'], large: true, badPin: true, badLength: true, badPath: true, afterFailure: 'after:failure' }); assert.deepEqual(errors, []);
        const blocked = await browser.newPage();
        try {
          await blocked.goto(`http://localhost:${http.address().port}/web/index.html`); await blocked.waitForSelector('form[data-ready="true"]');
          const denied = await blocked.evaluate(async ({ address, pin }) => { const { frontend } = await import('/web/app.js'); try { await frontend.greet(`https://${address}/rpc`, Uint8Array.from(pin.match(/../g), h => parseInt(h,16)), '', 'denied'); return false; } catch { return true; } }, { address: backend.address, pin }); assert.equal(denied, true);
        } finally { await blocked.close(); }
        console.log(`PASS browser WASI Preview 3 → ${driver}: UI, pipelines, concurrency, large frames, pin/path/origin rejection and recovery`);
      } finally { await page.close(); await backend.stop(); }
    }
  } finally { unregister(); await cleanup(); }
}
