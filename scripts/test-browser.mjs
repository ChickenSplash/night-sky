// Install the test driver with: npm install --prefix tools/browser playwright-core
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
try {
  const page = await browser.newPage({ viewport: { width: 1600, height: 1000 } });
  page.on('console', msg => logs.push(`${msg.type()}: ${msg.text()}`));
  page.on('pageerror', error => {
    if (!String(error).includes('Using exceptions for control flow')) errors.push(String(error));
  });
  const health = await page.request.get(`${base}/health`);
  assert.equal(health.status(), 200);
  assert.equal((await health.json()).service, 'night-sky');
  const galaxy = await (await page.request.get(`${base}/galaxy`)).json();
  assert(galaxy.systems.length > 0, 'Expected cached or synced systems.');
  for (const path of ['/.env', '/data/space.sqlite3', '/compose.yaml']) {
    assert.equal((await page.request.get(base + path)).status(), 404, `${path} must not be served`);
  }
  // Deterministic pagination coverage, independent of the connected account.
  const template = galaxy.systems.flatMap(system => system.planets).find(planet => planet.task);
  assert(template, 'Expected at least one task for the rendering fixture.');
  // The client rebuilds every body from task slots, so the planets only carry tasks.
  const fixture = {
    source: 'Demo · browser pagination fixture',
    systems: [{ ...galaxy.systems[0], name: 'Browser fixture', planets: Array.from({ length: 50 }, (_, slot) => ({
      ...template,
      task: { ...template.task, id: `browser-task-${slot}`, slot, name: `Task ${slot + 1}: review the navigation checklist` },
    })) }],
  };
  await page.route('**/galaxy', route => route.fulfill({ json: fixture }));
  await page.goto(`${base}/`, { waitUntil: 'domcontentloaded', timeout: 120_000 });
  await page.waitForFunction(() => document.querySelector('#loading').style.display === 'none', null, { timeout: 120_000 });
  // Allow the WebGL pipelines to compile and Bevy to produce complete frames.
  await page.waitForTimeout(6000);
  const state = () => page.evaluate(() => window.space.state());
  assert.equal((await state()).presentation_mode, 'simulation');
  assert.equal(Object.hasOwn(await state(), 'view'), false);
  await page.keyboard.press('Space');
  await page.waitForFunction(() => window.space.state().paused, null, { timeout: 5000 });
  // The client opens in the galaxy, relative to home.
  await page.screenshot({ path: resolve(root, 'data/web-galaxy.png') });
  // Clicks target the egui windows and HUD icons at 1600 x 1000. Windows open
  // below the three icons (⚙ 31, ✔ 63, 🔍 95).
  await page.mouse.click(1570, 63); // ✔ Tasks icon, top right.
  await page.waitForTimeout(1500);
  await page.screenshot({ path: resolve(root, 'data/web-tasks.png') });
  await page.mouse.click(1431, 308); // First list, in the Tasks window.
  await page.waitForTimeout(2500);
  assert.equal((await state()).list, fixture.systems[0].id);
  assert.equal((await state()).presentation_mode, 'constellation');
  await page.screenshot({ path: resolve(root, 'data/web-list.png') });
  await page.mouse.click(1570, 63); // Close Tasks.
  await page.waitForTimeout(1000);
  // 🔍 opens the object browser, which explains that searching is native-only for now.
  await page.mouse.click(1570, 95);
  await page.waitForFunction(() => window.space.state().search.browser_open, null, { timeout: 5000 });
  await page.waitForTimeout(1500);
  await page.screenshot({ path: resolve(root, 'data/web-browser.png') });
  await page.mouse.click(1570, 95); // Close it.
  await page.waitForFunction(() => !window.space.state().search.browser_open, null, { timeout: 5000 });
  // Presentation and paging live in the canvas inspector's actions, sent here as commands.
  const command = cmd => page.evaluate(cmd => window.space.command(cmd), cmd);
  await command({ type: 'set_presentation_mode', presentation_mode: 'table' });
  await page.waitForFunction(() => window.space.state().presentation_mode === 'table', null, { timeout: 30_000 });
  await page.waitForTimeout(2500);
  assert.equal((await state()).presentation_mode, 'table');
  await page.screenshot({ path: resolve(root, 'data/web-table.png') });
  await command({ type: 'show_page', page: 1 });
  await page.waitForFunction(() => window.space.state().page === 1, null, { timeout: 30_000 });
  await page.waitForTimeout(2500);
  assert.equal((await state()).page, 1);
  await page.screenshot({ path: resolve(root, 'data/web-table-next-page.png') });
  await command({ type: 'set_presentation_mode', presentation_mode: 'constellation' });
  await page.waitForFunction(() => window.space.state().presentation_mode === 'constellation', null, { timeout: 30_000 });
  await page.waitForTimeout(2500);
  assert.equal((await state()).presentation_mode, 'constellation');
  await page.screenshot({ path: resolve(root, 'data/web-constellation.png') });
   // ⚙ opens the unified debug console below the icons.
   assert.equal('exaggerate' in (await state()).display, false);
   const orbits = (await state()).display.orbits;
   await page.mouse.click(1570, 31);
   await page.waitForTimeout(3000); // Its fade-in is slow under SwiftShader.
   await page.mouse.click(1176, 180); // render tab.
   await page.waitForTimeout(1500);
   await page.mouse.click(1048, 250); // Orbital paths.
   await page.waitForFunction(on => window.space.state().display.orbits === on, !orbits, { timeout: 5000 });
   const realistic = (await state()).display.realistic;
   await page.mouse.click(1048, 362); // Realistic lighting.
   await page.waitForFunction(on => window.space.state().display.realistic === on, !realistic, { timeout: 5000 });
   await page.screenshot({ path: resolve(root, 'data/web-debug.png') });
   await page.mouse.click(1248, 180); // controls tab.
   await page.waitForTimeout(1500);
   await page.screenshot({ path: resolve(root, 'data/web-controls.png') });
  // The terminal tab focuses its command line, which runs typed commands.
  await page.mouse.click(1028, 180); // terminal tab.
  await page.waitForTimeout(1500);
  await page.keyboard.type('set_display labels false');
  await page.keyboard.press('Enter');
  await page.waitForFunction(() => window.space.state().display.labels === false, null, { timeout: 5000 });
  await page.screenshot({ path: resolve(root, 'data/web-terminal.png') });
  await page.keyboard.press('Escape'); // Leave the command line so ` closes the window.
  await page.keyboard.press('Backquote');
  await page.waitForTimeout(1500);
  // Exercise event handling and resize, not just the initial WebAssembly boot.
  await page.mouse.move(750, 400);
  await page.mouse.down({ button: 'right' });
  await page.mouse.move(850, 440, { steps: 8 });
  await page.mouse.up({ button: 'right' });
  await page.mouse.wheel(0, -120);
  await page.keyboard.down('Shift');
  await page.mouse.wheel(0, -120);
  await page.keyboard.up('Shift');
  await page.setViewportSize({ width: 1440, height: 900 });
  await page.waitForTimeout(2000);
  await page.screenshot({ path: resolve(root, 'data/web-resized.png') });
  assert.equal(await page.locator('#loading').evaluate(el => el.style.display), 'none', 'A renderer error overlay appeared');
  assert.deepEqual(errors, [], 'Browser runtime errors');
  console.log('Browser smoke passed: 50-task fixture, WebAssembly boot, galaxy/tasks/browser/table/constellation/settings/debug/terminal/input/resize, private-file isolation.');
} finally {
  writeFileSync(resolve(root, 'data/browser-console.log'), [...logs, ...errors].join('\n'));
  await browser.close();
}
