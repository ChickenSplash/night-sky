// Headless regression coverage for remote selection and inspector-only crumbs.
import { chromium } from '../tools/browser/node_modules/playwright-core/index.mjs';
import { existsSync, mkdirSync, readdirSync } from 'node:fs';
import { homedir } from 'node:os';
import { resolve } from 'node:path';
import assert from 'node:assert/strict';

const root = resolve(import.meta.dirname, '..');
const base = process.env.SPACE_URL || 'http://127.0.0.1:4322';
const cache = resolve(homedir(), '.cache/ms-playwright');
const executablePath = process.env.SPACE_CHROMIUM || (existsSync(cache) && readdirSync(cache)
  .filter(name => name.startsWith('chromium-')).sort().reverse()
  .map(name => resolve(cache, name, 'chrome-linux64/chrome')).find(existsSync));
assert(executablePath, 'Set SPACE_CHROMIUM to a Chromium executable.');
const output = resolve(root, 'data/logs');
mkdirSync(output, { recursive: true });
const browser = await chromium.launch({ executablePath, headless: true,
  args: ['--use-gl=angle', '--use-angle=swiftshader', '--enable-unsafe-swiftshader', '--disable-dev-shm-usage'] });
try {
  const page = await browser.newPage({ viewport: { width: 1600, height: 1000 } });
  const errors = [];
  page.on('pageerror', error => {
    if (!String(error).includes('Using exceptions for control flow')) errors.push(String(error));
  });
  const galaxy = await (await page.request.get(`${base}/galaxy`)).json();
  const planet = galaxy.systems[1].planets[0];
  const star = planet.id.slice(0, planet.id.lastIndexOf('/'));
  await page.goto(base, { waitUntil: 'domcontentloaded', timeout: 120_000 });
  await page.waitForFunction(() => document.querySelector('#loading').style.display === 'none', null, { timeout: 120_000 });
  const command = cmd => page.evaluate(cmd => window.space.command(cmd), cmd);
  const state = () => page.evaluate(() => window.space.state());
  await command({ type: 'set_paused', paused: true });
  await command({ type: 'inspect', planet: planet.id });
  await page.waitForFunction(id => window.space.state().selection?.id === id, planet.id, { timeout: 30_000 });
  assert.equal((await state()).list, null);
  assert.deepEqual((await state()).trail, ['Milky Way']);
  assert.equal((await state()).inspection.object.id, planet.id);
  assert.equal((await state()).selection_trail.at(-1), planet.designation);
  await page.waitForTimeout(2000);
  await page.screenshot({ path: resolve(output, 'breadcrumbs-remote-planet.png') });
  // Actual floating text hit, not a command: Location › Milky Way.
  await page.mouse.click(110, 27);
  await page.waitForFunction(() => window.space.state().inspection.source === 'location', null, { timeout: 30_000 });
  assert.equal((await state()).inspection.object.kind, 'galaxy');
  assert.equal((await state()).selection.id, planet.id);
  assert.equal((await state()).list, null);
  await page.screenshot({ path: resolve(output, 'breadcrumbs-galaxy-tab.png') });
  for (const object of [{ kind: 'star', id: star }, { kind: 'planet', id: planet.id }]) {
    await command({ type: 'inspect_object', inspection: { source: 'selection', object } });
    await page.waitForFunction(id => window.space.state().inspection.object.id === id, object.id, { timeout: 30_000 });
    assert.equal((await state()).selection.id, planet.id);
    assert.equal((await state()).list, null);
  }
  await page.setViewportSize({ width: 760, height: 600 });
  await page.waitForTimeout(2000);
  await page.screenshot({ path: resolve(output, 'breadcrumbs-narrow.png') });
  assert.deepEqual(errors, []);
  console.log('Breadcrumbs passed: remote planet, default inspector, actual galaxy crumb click, parent tabs, no visits, narrow layout.');
} finally {
  await browser.close();
}
