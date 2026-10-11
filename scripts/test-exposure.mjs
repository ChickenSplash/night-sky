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
  // Compare captures only once every star is loaded and uploaded.
  await page.waitForFunction(() => {
    const s = window.space.state();
    return s.stream_stars > 0 && / 0 pending {2}· {2}0 mesh chunks/.test(s.stream);
  }, null, { timeout: 300000, polling: 500 });
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
  // Compares a reference capture with another: mean brightness, mean change
  // where the reference is faint (the galaxy, which only Exposure sets), white
  // pixels (bright stars' size) and the spread of a lit patch.
  // `around` ([x, y, r]) leaves a body and its streaks out of the faint check.
  const compare = (reference, other, lit, around) => JSON.parse(execFileSync('python', ['-c', `
import json, sys
from PIL import Image, ImageStat
a_path, b_path, lit, around = json.load(sys.stdin)
box = (480, 200, 1200, 700)
a, b = [Image.open(p).convert('L').crop(box) for p in (a_path, b_path)]
pa, pb = list(a.get_flattened_data()), list(b.get_flattened_data())
width = box[2] - box[0]
def outside(i):
    if not around:
        return True
    x, y = box[0] + i % width, box[1] + i // width
    return (x - around[0]) ** 2 + (y - around[1]) ** 2 > around[2] ** 2
faint = [abs(x - y) for i, (x, y) in enumerate(zip(pa, pb)) if x < 150 and outside(i)]
def stats(path):
    im = Image.open(path).convert('L')
    return ImageStat.Stat(im.crop(tuple(lit))).stddev[0] if lit else 0
print(json.dumps(dict(
    mean=[sum(pa) / len(pa), sum(pb) / len(pb)],
    faintChange=sum(faint) / max(len(faint), 1),
    white=[sum(x >= 245 for x in pa), sum(x >= 245 for x in pb)],
    litSpread=[stats(a_path), stats(b_path)])))
`], { input: JSON.stringify([reference, other, lit, around]), encoding: 'utf8' }));
  const linear = await capture('linear', 0, 0);
  assert.deepEqual(errors, [], 'Browser or shader errors');
  const dim = compare(linear, await capture('dim', -6, 0));
  assert(dim.mean[0] > dim.mean[1], 'Lower exposure did not darken the scene');
  const slight = compare(linear, await capture('slight', 0, 0.01));
  assert(slight.faintChange < 0.5, 'Compression changed the galaxy');
  assert(slight.white[1] >= 0.8 * slight.white[0], '1% compression shrank bright stars');
  const full = compare(linear, await capture('compressed', 0, 1));
  assert(full.faintChange < 0.5, 'Full compression changed the galaxy');
  assert(full.white[1] >= 0.8 * full.white[0], 'Full compression shrank galactic stars');
  results.sky = { dim, slight, full };

  const planet = await page.evaluate(async () => {
    const galaxy = await (await fetch('/galaxy')).json();
    return galaxy.systems[0].planets[0].id;
  });
  await command({ type: 'fly_to_planet', planet });
  await command({ type: 'set_display', option: 'wireframe', on: false });
  await page.waitForTimeout(5000);
  const lit = [805, 455, 865, 545];
  const planetLinear = await capture('planet-linear', 0, 0);
  // The overexposed planet streaks over the galaxy at 0% and not at 100%.
  results.planet = compare(planetLinear, await capture('planet-compressed', 0, 1), lit, [800, 498, 320]);
  assert(results.planet.faintChange < 0.5, 'Planet compression changed the faint galaxy');
  assert(results.planet.litSpread[1] > results.planet.litSpread[0] + 5, 'Exposing the planet for itself did not show its shading');

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
