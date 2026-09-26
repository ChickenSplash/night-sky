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
  await page.goto(`${base}/`, { waitUntil: 'domcontentloaded', timeout: 120_000 });
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
  await page.locator('#inspector button', { hasText: 'Task table' }).click();
  await page.waitForFunction(() => window.seen.state.view === 'table');
  await settle();
  await shot('table');
  const pages = (await seen('state')).pages;
  if (pages.length > 1) {
    await page.locator('#inspector button[aria-label="Next page"]').click();
    await page.waitForFunction(p => window.seen.state.page === p, pages[1]);
    assert.match(await page.locator('#inspector .pager span').textContent(), /Page 2 \//);
  }
  await page.locator('#inspector button', { hasText: /^Orbit$/ }).click();
  await page.waitForFunction(() => window.seen.state.view === 'orbit');
  await settle();

  // Search every list, select a task and follow it.
  const wanted = galaxy.lists.flatMap(l => l.tasks.map(t => ({ list: l.id, task: t }))).at(-1);
  const search = page.locator('#nav-panel input[aria-label="Search tasks"]');
  await search.fill(wanted.task.name);
  await page.locator('#nav-panel .task', { hasText: wanted.task.name }).first().click();
  await page.waitForFunction(({ list, id }) => window.seen.state.list === list && window.seen.state.task === id,
    { list: wanted.list, id: wanted.task.id });
  await page.waitForFunction(() => window.seen.state.selected);
  assert.equal(await page.locator('#inspector .detail h3').textContent(), wanted.task.name);
  await settle();
  await page.locator('#inspector button', { hasText: 'Follow this planet' }).click();
  await page.waitForFunction(() => window.seen.state.following === true);
  await settle();
  await shot('selected');
  await search.fill('');
  assert.equal(await page.locator('#nav-panel .tasks').evaluate(b => b.querySelectorAll('.task').length > 0), true);

  // Display toggles reach Rust.
  const labels = page.locator('#nav-panel .toggle', { hasText: 'Task labels' });
  const labelsOn = (await seen('state')).display.labels;
  await labels.click();
  await page.waitForFunction(on => window.seen.state.display.labels === !on, labelsOn);
  await labels.click();
  await page.waitForFunction(on => window.seen.state.display.labels === on, labelsOn);
  await page.locator('#inspector button', { hasText: 'Frame orbital system' }).click();
  await page.waitForFunction(() => window.seen.state.following === false && window.seen.state.task === null);
  await settle();

  // Typing into an HTML field must not move the camera or toggle pause.
  const target = async () => (await seen('tick')).target;
  const before = await target();
  const filter = page.locator('#nav-panel input[aria-label="Filter lists"]');
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
  console.log('HTML UI passed: lists, views, pages, search, selection, follow, display, keyboard focus, wheel routing, resize.');
} finally {
  writeFileSync(resolve(root, 'data/html-ui-console.log'), [...logs, ...errors].join('\n'));
  await browser.close();
}
