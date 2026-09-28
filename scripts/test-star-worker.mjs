// Focused browser integration check. Run with the local server after build-web.sh.
import { chromium } from '../tools/browser/node_modules/playwright-core/index.mjs';
import { existsSync, readdirSync } from 'node:fs';
import { homedir } from 'node:os';
import { resolve } from 'node:path';
import assert from 'node:assert/strict';

const base = process.env.SPACE_URL || 'http://127.0.0.1:4321';
const cache = resolve(homedir(), '.cache/ms-playwright');
const executablePath = process.env.SPACE_CHROMIUM || (existsSync(cache) ? readdirSync(cache)
  .filter(name => name.startsWith('chromium-')).sort().reverse()
  .map(name => resolve(cache, name, 'chrome-linux64/chrome'))
  .find(path => existsSync(path)) : undefined);
assert(executablePath, 'Set SPACE_CHROMIUM to a Chromium executable.');
const browser = await chromium.launch({ executablePath, headless: true,
  args: ['--use-gl=angle', '--use-angle=swiftshader', '--enable-unsafe-swiftshader', '--disable-dev-shm-usage'] });
try {
  const page = await browser.newPage({ viewport: { width: 1600, height: 1000 } });
  const requests = new Set();
  const errors = [];
  page.on('response', response => {
    if (response.url().includes('star_worker') || response.url().includes('star-worker')) {
      assert.equal(response.status(), 200, response.url());
      requests.add(new URL(response.url()).pathname);
    }
  });
  page.on('pageerror', error => errors.push(String(error)));
  await page.goto(`${base}/`, { waitUntil: 'domcontentloaded' });
  await page.waitForFunction(() => {
    const text = document.querySelector('#hud .caption.bottom')?.textContent ?? '';
    return /STAR STREAM \[WEB WORKER\] [1-9]\d* stars/.test(text);
  }, null, { timeout: 90_000 });
  assert.deepEqual(errors.filter(error => !error.includes('Using exceptions for control flow')), []);
  for (const path of ['/star-worker.js', '/star_worker.js', '/star_worker_bg.wasm']) {
    assert(requests.has(path), `Worker asset not requested: ${path}`);
  }
  console.log('Browser star worker generated and delivered cells to the renderer.');
} finally {
  await browser.close();
}
