// Test the actual bundled shared frontend with a mock CEF transport.
// This does not substitute for native keyboard/clipboard/drag acceptance.
import { chromium } from '../tools/browser/node_modules/playwright-core/index.mjs';
import { existsSync, readdirSync, statSync } from 'node:fs';
import { homedir } from 'node:os';
import { resolve } from 'node:path';
import { pathToFileURL } from 'node:url';
import assert from 'node:assert/strict';

const root = resolve(import.meta.dirname, '..');
const build = resolve(root, 'target/debug/build');
const bundle = readdirSync(build).filter(n => n.startsWith('space-client-'))
  .map(n => resolve(build, n, 'out/native-ui.html')).filter(existsSync)
  .sort((a, b) => statSync(b).mtimeMs - statSync(a).mtimeMs)[0];
assert(bundle, 'Build/check space-client first to generate its bundled UI.');
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
    window.messages = []; window.listeners = {};
    window.cef = { emit: p => window.messages.push(p), listen: (n, cb) => { window.listeners[n] = cb; } };
  });
  await page.goto(pathToFileURL(bundle).href);
  await page.waitForFunction(() => window.messages.some(m => m.kind === 'ready') && window.messages.some(m => m.kind === 'layout'));
  const emit = (name, detail) => page.evaluate(([n, d]) => window.listeners[`space:${n}`](JSON.stringify(d)), [name, detail]);
  const state = { paused: false, rate: 1, page: 0, pages: [], view: 'orbit', system: null, selected: null, picked: null, display: {}, fov_deg: 50 };
  await emit('galaxy', { source: 'Built-in demo galaxy', lists: [] });
  await emit('state', state);
  const command = type => page.evaluate(t => window.messages.filter(m => m.kind === 'command' && m.command.type === t).at(-1)?.command, type);
  await page.locator('#time-bar .primary').click();
  assert.deepEqual(await command('set_paused'), { type: 'set_paused', paused: true });
  await emit('state', { ...state, paused: true });
  assert.equal(await page.locator('#time-bar .primary').getAttribute('aria-label'), 'Resume');
  await emit('tick', { days: 1.25, range: 1e16, exposure: 'Physical', set_speed: '1 km/s', velocity: '0 km/s', stream: 'STAR STREAM' });
  assert.match(await page.locator('#time-bar .clock').textContent(), /1.25 days/);
  await page.locator('#settings-button').click();
  await page.locator('input[data-option=labels]').check();
  assert.deepEqual(await command('set_display'), { type: 'set_display', option: 'labels', on: true });
  await page.getByRole('spinbutton').fill('75');
  await page.getByRole('spinbutton').press('Enter');
  assert.deepEqual(await command('set_fov'), { type: 'set_fov', degrees: 75 });
  await page.getByRole('button', { name: 'Save bug report', exact: true }).click();
  assert.deepEqual(await command('save_bug_report'), { type: 'save_bug_report' });
  assert.equal(await page.getByRole('button', { name: 'Run star-stream benchmark' }).count(), 0);
  await page.getByRole('button', { name: 'Close settings', exact: true }).click();
  const search = page.locator('#nav-panel input[type=search]').first();
  await search.fill('WASD R/F Q/E');
  assert.equal(await page.evaluate(() => window.messages.filter(m => m.kind === 'focus').at(-1).keyboard), true);
  await emit('blur', true);
  assert.equal(await search.evaluate(el => document.activeElement === el), false);
  // Custom dropdowns stay in the composed page, never Chromium popup surfaces.
  assert.equal(await page.locator('select').count(), 0);
  await page.getByRole('button', { name: 'Sort lists', exact: true }).click();
  await page.waitForFunction(() => document.querySelector('.menu:not([hidden])'));
  await page.getByRole('option', { name: 'Name A–Z', exact: true }).first().click();
  await emit('overlay', { size: [1440, 900], rect: [0, 0, 1440, 900], bodies: [{ x: 800, y: 400, radius: 9, label: 'Test planet', colour: [255, 0, 0], selected: true, hovered: false, marker: true, table: false, width: 220 }], picked: null, directions: [] });
  assert.equal(await page.locator('#annotations').evaluate(c => c.getContext('2d').getImageData(780, 380, 100, 50).data.some(v => v > 0)), true);
  await emit('overlay', { size: [1440, 900], rect: [0, 0, 1440, 900], bodies: [], picked: null, directions: [], probe_frame: 0x12345678 });
  assert.deepEqual(await page.locator('#annotations').evaluate(c => [...c.getContext('2d').getImageData(0, 0, 6, 1).data]),
    [53, 121, 211, 255, 91, 17, 73, 255, 120, 0, 0, 255, 86, 0, 0, 255, 52, 0, 0, 255, 18, 0, 0, 255]);
  await page.setViewportSize({ width: 800, height: 600 });
  await page.waitForTimeout(350);
  assert.deepEqual(errors, []);
  console.log('Shared native UI bundle, commands, state, focus, dropdowns and annotations passed (mock CEF).');
} finally { await browser.close(); }
