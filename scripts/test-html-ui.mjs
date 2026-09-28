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
  assert.equal(await pause.getAttribute('aria-label'), 'Resume');

  // Rate ladder: real time is the floor, a century a second the ceiling.
  const rateNow = () => page.evaluate(() => window.seen.state.rate);
  const slower = page.locator('#time-bar button[aria-label="Slow time down"]');
  const faster = page.locator('#time-bar button[aria-label="Speed time up"]');
  const startRate = await rateNow();
  await faster.click();
  await page.waitForFunction(r => window.seen.state.rate > r, startRate);
  await page.locator('#time-bar .real-time').click();
  await page.waitForFunction(() => window.seen.state.rate === 1);
  assert.equal(await slower.isDisabled(), true);
  for (let i = 0; i < 20 && !(await faster.isDisabled()); i++) {
    const before = await rateNow();
    await faster.click();
    await page.waitForFunction(r => window.seen.state.rate > r, before);
  }
  assert.equal(await rateNow(), 100 * 31_557_600);
  await slower.click();
  await page.waitForFunction(() => window.seen.state.rate === 30 * 31_557_600);
  await page.evaluate(r => window.spaceBridge.command({ type: 'set_rate', rate: r }), startRate);
  await page.waitForFunction(r => window.seen.state.rate === r, startRate);
  if (galaxy.lists.length > 1) {
    await lists.nth(1).click();
    await page.waitForFunction(id => window.seen.state.list === id, galaxy.lists[1].id);
    await settle();
    assert.equal(await lists.nth(1).evaluate(b => b.parentElement.classList.contains('active')), true);
  }
  const view = async name => {
    await page.locator('#where button[aria-label="View mode"]').click();
    await page.locator('#where [role=option]', { hasText: name }).click();
  };
  await view('Task table');
  await page.waitForFunction(() => window.seen.state.view === 'table');
  await settle();
  await shot('table');
  const pages = (await seen('state')).pages;
  if (pages.length > 1) {
    await page.locator('#where button[aria-label="Next page"]').click();
    await page.waitForFunction(p => window.seen.state.page === p, pages[1]);
    assert.match(await page.locator('#where .view-pill .meta').textContent(), /System 2 \//);
  }
  await view(/^Orbit$/);
  await page.waitForFunction(() => window.seen.state.view === 'orbit');
  await settle();

  // Lists sort by name.
  await page.locator('#nav-panel button[aria-label="Sort lists"]').click();
  await page.locator('#nav-panel [aria-label="Sort lists"] + [role=listbox] [role=option]', { hasText: 'Name A–Z' }).click();
  const byName = galaxy.lists.map(l => l.name).sort((a, b) => a.localeCompare(b));
  assert.equal(await lists.first().locator('span').textContent(), byName[0]);
  await page.locator('#nav-panel button[aria-label="Sort lists"]').click();
  await page.locator('#nav-panel [aria-label="Sort lists"] + [role=listbox] [role=option]', { hasText: 'ClickUp order' }).click();

  // The chevron opens a list's tasks, grouped by status; groups fold.
  const tasksTab = page.locator('#nav-panel [role=tab]', { hasText: 'Tasks' });
  await page.locator('#nav-panel .open-tasks').first().click();
  assert.equal(await tasksTab.getAttribute('aria-selected'), 'true');
  await page.waitForFunction(id => window.seen.state.list === id, galaxy.lists[0].id);
  const group = page.locator('#nav-panel .group').first();
  const count = page.locator('#nav-panel .tab-body:not([hidden]) > p.muted');
  await group.click();
  assert.match(await count.textContent(), /folded/, 'Folding a status group hid no tasks');
  await group.click();
  assert.doesNotMatch(await count.textContent(), /folded/);

  // Search the list, select a task, open its details and follow it.
  const wanted = { list: galaxy.lists[0].id, task: galaxy.lists[0].tasks.at(-1) };
  const search = page.locator('#nav-panel input[aria-label="Search tasks"]');
  await search.fill(wanted.task.name);
  await page.locator('#nav-panel .task', { hasText: wanted.task.name }).first().click();
  await page.waitForFunction(({ list, id }) => window.seen.state.list === list && window.seen.state.task === id,
    { list: wanted.list, id: wanted.task.id });
  await page.waitForFunction(() => window.seen.state.selected);
  assert.equal(await page.locator('#where .task-pill .name').textContent(), wanted.task.name);
  await page.locator('#where button[aria-label="Frame this planet"]').click();
  await page.waitForFunction(() => window.seen.state.following === true);
  await settle();
  await page.locator('#where .task-pill .details').click();
  assert.equal(await page.locator('#task-modal h3').textContent(), wanted.task.name);
  await shot('selected');
  await page.locator('#task-modal button[aria-label="Close details"]').click();
  assert.equal(await page.locator('#task-modal').isHidden(), true);
  await search.fill('');
  assert.equal(await page.locator('#nav-panel .tasks').evaluate(b => b.querySelectorAll('.task').length > 0), true);

  // Display toggles live in the settings popover and reach Rust.
  await page.locator('#settings-button').click();
  const labels = page.locator('#settings .toggle', { hasText: 'Planet labels' });
  const labelsOn = (await seen('state')).display.labels;
  await labels.click();
  await page.waitForFunction(on => window.seen.state.display.labels === !on, labelsOn);
  await labels.click();
  await page.waitForFunction(on => window.seen.state.display.labels === on, labelsOn);
  const marker = page.locator('#settings .toggle', { hasText: 'Velocity marker' });
  await marker.click();
  await page.waitForFunction(() => window.seen.state.display.velocity_marker === false);
  await marker.click();
  await page.waitForFunction(() => window.seen.state.display.velocity_marker === true);
  await shot('settings');
  await page.mouse.click(900, 450);
  assert.equal(await page.locator('#settings').isHidden(), true, 'Clicking the view did not close settings');
  await page.locator('#where button[aria-label="Frame orbital system"]').click();
  await page.waitForFunction(() => window.seen.state.following === false && window.seen.state.task === null);
  await settle();

  // Typing into an HTML field must not move the camera or toggle pause.
  const target = async () => (await seen('tick')).target;
  const before = await target();
  await page.locator('#nav-panel [role=tab]', { hasText: 'Lists' }).click();
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
  const zoomed = await range();
  const speed = (await seen('tick')).set_speed;
  await page.keyboard.down('Shift');
  await page.mouse.wheel(0, 600);
  await page.waitForTimeout(500);
  await page.keyboard.up('Shift');
  assert.equal(await range(), zoomed, 'Shift + scroll changed camera range');
  assert.notEqual((await seen('tick')).set_speed, speed, 'Shift + scroll did not change travel speed');

  // The sidebar slides away and back.
  await page.locator('#nav-panel button[aria-label="Hide sidebar"]').click();
  await page.waitForFunction(() => document.querySelector('#nav-panel').getBoundingClientRect().right <= 0);
  await settle();
  await shot('sidebar-closed');
  await page.locator('#nav-toggle').click();
  await page.waitForFunction(() => document.querySelector('#nav-panel').getBoundingClientRect().left >= 16);

  // The 3D viewport follows the panel layout after a resize.
  await page.setViewportSize({ width: 1280, height: 800 });
  await settle();
  await shot('resized');
  assert.deepEqual(errors, [], 'Browser runtime errors');
  console.log('HTML UI passed: lists, sorting, tabs, status groups, views, pages, search, selection, follow, settings, sidebar, keyboard focus, wheel routing, resize.');
} finally {
  writeFileSync(resolve(root, 'data/html-ui-console.log'), [...logs, ...errors].join('\n'));
  await browser.close();
}
