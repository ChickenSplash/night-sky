// Fixed galaxy reference views, on/off timing and shader-error checks.
// Headed GPU run is placed on HDMI-A-1's workspace 8 without taking focus.
// SPACE_HEADFUL=1 node scripts/test-galaxy-glow.mjs
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
const headed = process.env.SPACE_HEADFUL === '1';
const base = process.env.SPACE_URL || 'http://127.0.0.1:4321';
mkdirSync(resolve(root, 'data/logs'), { recursive: true });
let browser;
if (headed) {
  const command = `${executable} --ozone-platform=wayland --user-data-dir=${root}/data/glow-browser --remote-debugging-port=9223 --no-first-run --no-default-browser-check --window-size=1600,1000 about:blank`;
  execFileSync('hyprctl', ['dispatch', `hl.dsp.exec_cmd(${JSON.stringify(command)}, { workspace = '8 silent', no_focus = true, float = true, size = '1600 1000', center = true })`]);
  for (let attempt = 0; attempt < 100; attempt++) {
    try { browser = await chromium.connectOverCDP('http://127.0.0.1:9223'); break; }
    catch { await new Promise(resolve => setTimeout(resolve, 100)); }
  }
  assert(browser, 'Chromium CDP did not start');
} else {
  browser = await chromium.launch({ executablePath: executable, headless: true,
    args: ['--use-gl=angle', '--use-angle=swiftshader', '--enable-unsafe-swiftshader', '--disable-dev-shm-usage'] });
}
const errors = [], logs = [], results = {};
try {
  const context = headed ? browser.contexts()[0] : await browser.newContext({ viewport: { width: 1600, height: 1000 } });
  const page = headed ? context.pages().at(-1) : await context.newPage();
  // This is a project-local test profile, never the user's browser profile.
  if (headed) for (const other of context.pages().filter(p => p !== page)) await other.close();
  page.on('console', message => {
    logs.push(`${message.type()}: ${message.text()}`);
    if (/shader|pipeline|validation|panicked/i.test(message.text()) && (message.type() === 'error' || /%cERROR|failed to process shader/i.test(message.text()))) errors.push(message.text());
  });
  page.on('pageerror', error => { if (!String(error).includes('Using exceptions for control flow')) errors.push(String(error)); });
  await page.goto(base, { waitUntil: 'domcontentloaded', timeout: 120000 });
  await page.waitForFunction(() => document.querySelector('#loading').style.display === 'none', null, { timeout: 120000 });
  await page.evaluate(() => {
    window.glowState = null;
    window.spaceBridge.on('state', state => { window.glowState = state; });
  });
  await page.waitForFunction(() => window.glowState?.display, null, { timeout: 60000 });
  await page.getByRole('button', { name: 'Settings', exact: true }).click();
  const glowToggle = page.locator('#settings input[data-option="galaxy_glow"]');
  assert(await glowToggle.isDisabled(), 'Glow control should be disabled outside realistic lighting');
  await page.getByRole('button', { name: 'Galaxy reference: Inside', exact: true }).click();
  await page.waitForFunction(() => window.glowState.display.realistic);
  assert(!(await glowToggle.isDisabled()), 'Reference view should enable realistic lighting');
  await page.getByRole('button', { name: 'Close settings', exact: true }).click();
  const command = cmd => page.evaluate(cmd => window.spaceBridge.command(cmd), cmd);
  const timings = () => page.evaluate(async () => {
    const samples = []; let previous = await new Promise(requestAnimationFrame);
    for (let i = 0; i < 180; i++) {
      const now = await new Promise(requestAnimationFrame);
      samples.push(now - previous); previous = now;
    }
    samples.sort((a, b) => a - b);
    return { median_ms: samples[90], p95_ms: samples[171], p99_ms: samples[178] };
  });
  for (const view of (process.env.SPACE_GLOW_QUICK === '1' ? ['inside'] : ['inside', 'face_on', 'edge_on'])) {
    await command({ type: 'galaxy_reference', view });
    await page.waitForTimeout(8000);
    results[view] = {};
    const images = [];
    for (const on of [false, true]) {
      await command({ type: 'set_display', option: 'galaxy_glow', on });
      await page.waitForFunction(on => window.glowState.display.galaxy_glow === on, on);
      await page.waitForTimeout(2500);
      results[view][on ? 'on' : 'off'] = await timings();
      images.push((await page.screenshot()).toString('base64'));
      if (headed) {
        const directory = resolve(root, `data/logs/glow-${view}-${on ? 'on' : 'off'}`);
        mkdirSync(directory, { recursive: true });
        execFileSync('dms', ['screenshot', 'output', '-o', 'HDMI-A-1', '--no-clipboard', '-d', directory]);
      } else await page.screenshot({ path: resolve(root, `data/logs/glow-${view}-${on ? 'on' : 'off'}.png`) });
    }
    // A successful command or frame-time reading does not prove a shader drew.
    // Check central canvas pixels, excluding panels and telemetry.
    const difference = JSON.parse(execFileSync('python', ['-c', `
import base64, io, json, sys
from PIL import Image, ImageChops, ImageStat
a,b=[Image.open(io.BytesIO(base64.b64decode(s))).convert('RGB') for s in json.load(sys.stdin)]
w,h=a.size
box=(int(w*.38),int(h*.25),int(w*.9),int(h*.75))
a,b=a.crop(box),b.crop(box)
print(json.dumps({'mean_pixel_difference':sum(ImageStat.Stat(ImageChops.difference(a,b)).mean)/3,'off_mean':sum(ImageStat.Stat(a).mean)/3,'on_mean':sum(ImageStat.Stat(b).mean)/3,'viewport':[w,h]}))
`], { input: JSON.stringify(images), encoding: 'utf8' }));
    results[view].image = difference;
    assert(difference.mean_pixel_difference > 0.1, `Glow did not visibly draw in ${view}: ${JSON.stringify(difference)}`);
    assert.deepEqual(errors, [], `Browser/shader errors in ${view}`);
  }
  assert.deepEqual(errors, [], 'Browser/shader errors');
  console.log(JSON.stringify(results, null, 2));
} finally {
  writeFileSync(resolve(root, 'data/logs/galaxy-glow-test.json'), JSON.stringify({ environment: headed ? 'Chromium / Wayland / GPU' : 'SwiftShader (not a GPU benchmark)', results, errors, logs }, null, 2));
  if (headed) await browser.contexts()[0].pages()[0].evaluate(() => window.spaceBridge.command({ type: 'galaxy_reference', view: 'inside' })).catch(() => {});
  if (headed) {
    const session = await browser.newBrowserCDPSession();
    await session.send('Browser.close').catch(() => {});
    await browser.close();
  }
  else await browser.close();
}
