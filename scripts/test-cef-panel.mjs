// Headless DOM/transport-contract test only, not native CEF input acceptance.
import { chromium } from '../tools/browser/node_modules/playwright-core/index.mjs';
import { existsSync, readdirSync } from 'node:fs';
import { homedir } from 'node:os';
import { resolve } from 'node:path';
import { pathToFileURL } from 'node:url';
import assert from 'node:assert/strict';

const root = resolve(import.meta.dirname, '..');
const cache = resolve(homedir(), '.cache/ms-playwright');
const cached = existsSync(cache) ? readdirSync(cache).filter(n => n.startsWith('chromium-'))
  .sort().reverse().map(n => resolve(cache, n, 'chrome-linux64/chrome')).find(existsSync) : undefined;
const executablePath = process.env.SPACE_CHROMIUM || cached;
assert(executablePath, 'Set SPACE_CHROMIUM to a Chromium executable.');
const browser = await chromium.launch({ executablePath, headless: true });
try {
  const page = await browser.newPage({ viewport: { width: 1440, height: 900 } });
  const errors = [];
  page.on('pageerror', e => errors.push(String(e)));
  await page.addInitScript(() => {
    window.messages = [];
    window.listeners = {};
    window.cef = {
      emit: payload => window.messages.push(payload),
      listen: (name, callback) => { window.listeners[name] = callback; },
    };
  });
  await page.goto(pathToFileURL(resolve(root, 'web/cef-overlay.html')).href);
  await page.waitForFunction(() => window.messages.some(m => m.kind === 'layout'));
  const state = paused => page.evaluate(p => window.listeners['space:state'](JSON.stringify({ paused: p })), paused);
  await state(false);
  await page.locator('#pause').click();
  assert.deepEqual(await page.evaluate(() => window.messages.find(m => m.kind === 'command')),
    { kind: 'command', command: { type: 'set_paused', paused: true } });
  assert.equal(await page.locator('#pause').isDisabled(), true);
  assert.equal(await page.locator('#paused').textContent(), 'Running');
  await state(true);
  assert.equal(await page.locator('#paused').textContent(), 'Paused');
  await page.evaluate(() => window.listeners['space:tick'](JSON.stringify({ days: 1.23456 })));
  assert.equal(await page.locator('#days').textContent(), '1.23456');
  await page.locator('[name="position"][value="top-right"]').check();
  await page.waitForFunction(() => window.messages.filter(m => m.kind === 'layout').at(-1).rects[0][0] > 900);
  await page.locator('#text').fill('WASD R/F Q/E pasted text');
  await page.evaluate(() => window.listeners['space:blur'](true));
  assert.equal(await page.locator('#text').evaluate(el => document.activeElement === el), false);
  await page.locator('#close').click();
  assert.equal(await page.locator('#panel').isVisible(), false);
  await page.locator('#show').click();
  assert.equal(await page.locator('#panel').isVisible(), true);
  await page.setViewportSize({ width: 800, height: 600 });
  await page.waitForFunction(() => window.messages.filter(m => m.kind === 'layout').at(-1).rects[0][0] < 800);
  assert.deepEqual(errors, []);
  console.log('CEF panel DOM and transport-contract tests passed (mock CEF, not native input acceptance).');
} finally {
  await browser.close();
}
