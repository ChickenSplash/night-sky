// Checks the HTML panel bridge: commands reach the scene, state comes back, and
// keyboard and mouse input stay with whichever layer the user is using.
// Run against a local server: SPACE_URL=http://127.0.0.1:4321 node scripts/test-html-ui.mjs
import { chromium } from '../tools/browser/node_modules/playwright-core/index.mjs';
import { existsSync, mkdirSync, readdirSync, writeFileSync } from 'node:fs';
import { homedir } from 'node:os';
import { resolve } from 'node:path';
import assert from 'node:assert/strict';

const root = resolve(import.meta.dirname, '..');
const base = process.env.SPACE_URL || 'http://127.0.0.1:4321';
const cache = resolve(homedir(), '.cache/ms-playwright');
const cachedChrome = existsSync(cache) ? readdirSync(cache)
  .filter(name => name.startsWith('chromium-')).sort().reverse()
  .map(name => resolve(cache, name, 'chrome-linux64/chrome'))
  .find(path => existsSync(path)) : undefined;
const executablePath = process.env.SPACE_CHROMIUM || cachedChrome;
assert(executablePath, 'Set SPACE_CHROMIUM to a Chromium executable.');
mkdirSync(resolve(root, 'data'), { recursive: true });
const browser = await chromium.launch({
  executablePath,
  headless: true,
  args: ['--use-gl=angle', '--use-angle=swiftshader', '--enable-unsafe-swiftshader', '--disable-dev-shm-usage'],
});
const logs = [];
const errors = [];
const shot = name => page.screenshot({ path: resolve(root, `data/html-${name}.png`) });
let page;
try {
  page = await browser.newPage({ viewport: { width: 1600, height: 1000 } });
  page.on('console', msg => logs.push(`${msg.type()}: ${msg.text()}`));
  page.on('pageerror', error => {
    if (!String(error).includes('Using exceptions for control flow')) errors.push(String(error));
  });
  await page.goto(`${base}/?ui=html`, { waitUntil: 'domcontentloaded', timeout: 120_000 });
  await page.waitForFunction(() => document.querySelector('#loading').style.display === 'none', null, { timeout: 120_000 });
  await page.evaluate(() => {
    window.seen = {};
    for (const name of ['galaxy', 'state', 'tick']) window.spaceBridge.on(name, d => { window.seen[name] = d; });
  });
  await page.waitForFunction(() => window.seen.galaxy?.lists.length && window.seen.state && window.seen.tick, null, { timeout: 60_000 });
  const seen = name => page.evaluate(n => window.seen[n], name);
  const settle = () => page.waitForTimeout(2500);
  await settle();
  const galaxy = await seen('galaxy');
  const lists = page.locator('#nav-panel .lists .item');
  assert.equal(await lists.count(), galaxy.lists.length, 'Every list has a button');
  await shot('start');

  // Rust → JS and JS → Rust.
  const pause = page.locator('#time-bar .primary');
  const wasPaused = (await seen('state')).paused;
  for (const paused of [!wasPaused, wasPaused]) {
    await pause.click();
    await page.waitForFunction(p => window.seen.state.paused === p, paused);
  }
  if (!wasPaused) await pause.click();
  await page.waitForFunction(() => window.seen.state.paused === true);
  assert.match(await pause.textContent(), /Resume/);
  if (galaxy.lists.length > 1) {
    await lists.nth(1).click();
    await page.waitForFunction(id => window.seen.state.list === id, galaxy.lists[1].id);
    await settle();
    assert.equal(await lists.nth(1).evaluate(b => b.classList.contains('active')), true);
  }
  await page.locator('#nav-panel button', { hasText: 'Task table' }).click();
  await page.waitForFunction(() => window.seen.state.view === 'table');
  await settle();
  await shot('table');
  await page.locator('#nav-panel button', { hasText: 'Orbit' }).click();
  await page.waitForFunction(() => window.seen.state.view === 'orbit');
  await settle();

  // Typing into an HTML field must not move the camera or toggle pause.
  const target = async () => (await seen('tick')).target;
  const before = await target();
  const filter = page.locator('#nav-panel input');
  await filter.click();
  await page.keyboard.down('w');
  await page.waitForTimeout(600);
  await page.keyboard.up('w');
  await page.keyboard.type('as d');
  await page.waitForTimeout(300);
  assert.deepEqual(await target(), before, 'Typing in the filter moved the camera');
  assert.equal((await seen('state')).paused, true, 'Space in the filter toggled pause');
  await filter.fill('');

  // Esc hands focus back to the canvas, where WASD moves the camera again.
  await filter.focus();
  await page.keyboard.press('Escape');
  assert.equal(await page.evaluate(() => document.activeElement.id), 'space-canvas');
  await page.keyboard.down('w');
  await page.waitForTimeout(600);
  await page.keyboard.up('w');
  await page.waitForTimeout(300);
  assert.notDeepEqual(await target(), before, 'WASD on the canvas did not move the camera');

  // Scrolling a panel must not zoom the 3D view; scrolling the canvas must.
  const range = async () => (await seen('tick')).range;
  const steady = await range();
  await page.mouse.move(140, 500);
  await page.mouse.wheel(0, -600);
  await page.waitForTimeout(500);
  assert.equal(await range(), steady, 'Scrolling the panel zoomed the scene');
  await page.mouse.move(900, 450);
  await page.mouse.wheel(0, -600);
  await page.waitForTimeout(1200);
  assert.notEqual(await range(), steady, 'Scrolling the canvas did not zoom');

  // The 3D viewport follows the panel layout after a resize.
  await page.setViewportSize({ width: 1280, height: 800 });
  await settle();
  await shot('resized');
  assert.deepEqual(errors, [], 'Browser runtime errors');
  console.log('HTML UI bridge passed: commands, state events, keyboard focus, wheel routing, resize.');
} finally {
  writeFileSync(resolve(root, 'data/html-ui-console.log'), [...logs, ...errors].join('\n'));
  await browser.close();
}
