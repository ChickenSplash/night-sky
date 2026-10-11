// Manual-exposure WebGL regression, not a performance benchmark.
// Run after build-web.sh, with the local server running. No desktop windows.
import { chromium } from '../tools/browser/node_modules/playwright-core/index.mjs';
import { existsSync, mkdirSync, readdirSync, writeFileSync } from 'node:fs';
import { homedir } from 'node:os';
import { resolve } from 'node:path';
import { execFileSync } from 'node:child_process';
import assert from 'node:assert/strict';

const root = resolve(import.meta.dirname, '..');
const cache = resolve(homedir(), '.cache/ms-playwright');
const executable = process.env.SPACE_CHROMIUM || readdirSync(cache).filter(n => n.startsWith('chromium-')).sort().reverse()
  .map(n => resolve(cache, n, 'chrome-linux64/chrome')).find(existsSync);
assert(executable, 'Chromium is required');
const directory = resolve(root, 'data/logs');
mkdirSync(directory, { recursive: true });
const browser = await chromium.launch({ executablePath: executable, headless: true,
  args: ['--use-gl=angle', '--use-angle=swiftshader', '--enable-unsafe-swiftshader', '--disable-dev-shm-usage'] });
const errors = [], logs = [], results = {};
try {
  const page = await browser.newPage({ viewport: { width: 1600, height: 1000 } });
  page.on('console', message => {
    logs.push(`${message.type()}: ${message.text()}`);
    if ((message.type() === 'error' && !message.text().includes('404 (Not Found)')) || /%cERROR|failed to process shader|panicked/i.test(message.text())) errors.push(message.text());
  });
  page.on('pageerror', error => { if (!String(error).includes('Using exceptions for control flow')) errors.push(String(error)); });
  await page.goto(process.env.SPACE_URL || 'http://127.0.0.1:4321', { waitUntil: 'domcontentloaded', timeout: 120000 });
  await page.waitForFunction(() => document.querySelector('#loading').style.display === 'none', null, { timeout: 120000 });
  await page.waitForFunction(() => window.space?.state()?.display, null, { timeout: 120000 });
  const command = cmd => page.evaluate(cmd => window.space.command(cmd), cmd);
  const state = () => page.evaluate(() => window.space.state());
  await command({ type: 'set_paused', paused: true });
  await command({ type: 'set_debug', switch: 'bloom', on: false });
  await page.waitForTimeout(6000);
  await command({ type: 'set_debug', switch: 'freeze_stream', on: true });
  await page.waitForTimeout(3000);
  assert(!Object.hasOwn((await state()).display, 'physical'), 'Old physical/assisted toggle remains');
  const capture = async (name, stops, amount) => {
    await command({ type: 'set_exposure', stops });
    await command({ type: 'set_compression', amount });
    await page.waitForFunction(({ stops, amount }) => {
      const display = window.space.state().display;
      return display.exposure === stops && display.compression === amount;
    }, { stops, amount });
    await page.waitForTimeout(1500);
    const path = resolve(directory, `exposure-${name}.png`);
    await page.screenshot({ path });
    return path;
  };
  const images = [await capture('linear', 0, 0), await capture('dim', -6, 0), await capture('compressed', -6, 1)];
  results.images = JSON.parse(execFileSync('python', ['-c', `
import json, sys
from PIL import Image, ImageStat
out=[]
for path in json.load(sys.stdin):
 im=Image.open(path).convert('RGB'); w,h=im.size
 im=im.crop((int(w*.3),int(h*.2),int(w*.75),int(h*.7)))
 out.append(sum(ImageStat.Stat(im).mean)/3)
print(json.dumps(out))
`], { input: JSON.stringify(images), encoding: 'utf8' }));
  assert(results.images[0] > results.images[1], 'Lower exposure did not darken the scene');
  assert(results.images[2] > results.images[1], 'Compression did not recover visible shadow detail');

  // These are the free-floating text drags, not slider tracks. A press alone
  // does nothing; moving while held applies before release.
  for (const [field, x, y] of [['exposure', 1500, 927], ['compression', 1470, 950]]) {
    await capture('hud', -6, 0.5);
    const before = (await state()).display[field];
    await page.mouse.move(x, y);
    await page.mouse.down();
    await page.waitForTimeout(300);
    assert.equal((await state()).display[field], before, 'A click must not change manual controls');
    await page.mouse.move(x + 30, y, { steps: 5 });
    await page.waitForTimeout(300);
    const after = (await state()).display[field];
    assert(after > before, `${field} text drag did not apply immediately`);
    await page.mouse.move(700, 400, { steps: 5 });
    await page.mouse.up();
    results[field] = { before, after };
  }
  await command({ type: 'set_display', option: 'realistic', on: false });
  await page.waitForFunction(() => !window.space.state().display.realistic);
  await page.waitForTimeout(500);
  await command({ type: 'set_display', option: 'realistic', on: true });
  await page.waitForTimeout(1000);
  assert.deepEqual(errors, [], 'Browser or shader errors');
  console.log(JSON.stringify(results, null, 2));
} finally {
  writeFileSync(resolve(directory, 'exposure-test.json'), JSON.stringify({ results, errors, logs }, null, 2));
  await browser.close();
}
