// Install the test driver with: npm install --prefix tools/browser playwright-core
import { chromium } from '../tools/browser/node_modules/playwright-core/index.mjs';
import { existsSync, mkdirSync, readdirSync, writeFileSync } from 'node:fs';
import { homedir } from 'node:os';
import { resolve } from 'node:path';
import assert from 'node:assert/strict';

const root = resolve(import.meta.dirname, '..');
const base = process.env.SPACE_URL || 'http://192.168.1.90:4321';
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
  assert.equal((await health.json()).service, 'task-space');
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
  // Clicks below target the egui panels; test-html-ui.mjs covers the HTML ones.
  await page.goto(`${base}/?ui=egui`, { waitUntil: 'domcontentloaded', timeout: 120_000 });
  await page.waitForFunction(() => document.querySelector('#loading').style.display === 'none', null, { timeout: 120_000 });
  // Allow the WebGL pipelines to compile and Bevy to produce complete frames.
  await page.waitForTimeout(6000);
  await page.keyboard.press('Space');
  // The client opens in the galaxy, relative to home.
  await page.screenshot({ path: resolve(root, 'data/web-galaxy.png') });
  await page.mouse.click(120, 190); // First list.
  await page.waitForTimeout(2500);
  await page.screenshot({ path: resolve(root, 'data/web-list.png') });
  await page.mouse.click(1440, 156); // Task table preset.
  await page.waitForTimeout(2500);
  await page.screenshot({ path: resolve(root, 'data/web-table.png') });
  await page.mouse.click(1470, 239); // Next system page.
  await page.waitForTimeout(2500);
  await page.screenshot({ path: resolve(root, 'data/web-table-next-page.png') });
  await page.mouse.click(1410, 197); // Constellation preset.
  await page.waitForTimeout(2500);
  await page.screenshot({ path: resolve(root, 'data/web-constellation.png') });
  // Exercise event handling and resize, not just the initial WebAssembly boot.
  await page.mouse.move(750, 400);
  await page.mouse.down({ button: 'right' });
  await page.mouse.move(850, 440, { steps: 8 });
  await page.mouse.up({ button: 'right' });
  await page.mouse.wheel(0, -120);
  await page.setViewportSize({ width: 1440, height: 900 });
  await page.waitForTimeout(2000);
  await page.screenshot({ path: resolve(root, 'data/web-resized.png') });
  assert.equal(await page.locator('#loading').evaluate(el => el.style.display), 'none', 'A renderer error overlay appeared');
  assert.deepEqual(errors, [], 'Browser runtime errors');
  console.log('Browser smoke passed: 50-task fixture, WebAssembly boot, galaxy/table/constellation/input/resize, private-file isolation.');
} finally {
  writeFileSync(resolve(root, 'data/browser-console.log'), [...logs, ...errors].join('\n'));
  await browser.close();
}
