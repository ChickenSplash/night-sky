// Worker transport integration, no app rendering or performance benchmark.
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
const browser = await chromium.launch({ executablePath, headless: true });
try {
  const page = await browser.newPage();
  await page.route('**/craft-worker-test', route => route.fulfill({
    contentType: 'text/html', body: '<!doctype html><title>Worker transport test</title>',
  }));
  await page.goto(`${base}/craft-worker-test`);
  const results = await page.evaluate(() => new Promise((resolve, reject) => {
    const worker = new Worker('/craft-worker.js', { type: 'module' });
    const results = [];
    const timeout = setTimeout(() => { worker.terminate(); reject(new Error('Worker timed out')); }, 30000);
    worker.onerror = event => { clearTimeout(timeout); worker.terminate(); reject(new Error(event.message)); };
    worker.onmessage = ({ data }) => {
      results.push(JSON.parse(data));
      if (results.length === 5) {
        clearTimeout(timeout); worker.terminate(); resolve(results);
      }
    };
    // Sent before WASM initialization: queued replies must keep their tokens.
    const system = id => ({ kind: 'system', id, position_ly: [1, 2, 3] });
    worker.postMessage(JSON.stringify([1, system('g78040000.2.813')]));
    worker.postMessage(JSON.stringify([2, system('g78040000.2.813')]));
    worker.postMessage(JSON.stringify([3, system('invalid')]));
    // The region holding home: seeded, so the same in the browser as natively.
    worker.postMessage(JSON.stringify([4, { kind: 'region', region: [8200, 20, 0] }]));
    worker.postMessage(JSON.stringify([5, { kind: 'nonsense' }]));
  }));
  const replies = new Map(results);
  assert.equal(replies.get(1).kind, 'system');
  assert.equal(replies.get(1).system.id, 'g78040000.2.813');
  assert.deepEqual(replies.get(1).system.position_ly, [1, 2, 3]);
  assert.deepEqual(replies.get(1), replies.get(2));
  assert.equal(replies.get(3).system, null);
  assert.equal(replies.get(4).kind, 'region');
  assert(Array.isArray(replies.get(4).candidates));
  assert.equal(replies.get(5), null);
  console.log('Craft worker: queued requests, deterministic bodies, regions and invalid requests passed.');
} finally {
  await browser.close();
}
