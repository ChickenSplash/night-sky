// Cold, deterministic browser flight: benchmark the currently built web/dist.
// Usage: node scripts/benchmark-star-flight.mjs main (or worker)
import { chromium } from '../tools/browser/node_modules/playwright-core/index.mjs';
import { existsSync, mkdirSync, readdirSync, writeFileSync } from 'node:fs';
import { homedir } from 'node:os';
import { resolve } from 'node:path';
import { execFileSync } from 'node:child_process';
import assert from 'node:assert/strict';

const root = resolve(import.meta.dirname, '..');
const label = process.argv[2];
assert(label && /^[a-z0-9-]+$/.test(label), 'Usage: node scripts/benchmark-star-flight.mjs <label>');
const base = process.env.SPACE_URL || 'http://127.0.0.1:4321';
const cache = resolve(homedir(), '.cache/ms-playwright');
const executablePath = process.env.SPACE_CHROMIUM || (existsSync(cache) ? readdirSync(cache)
  .filter(name => name.startsWith('chromium-')).sort().reverse()
  .map(name => resolve(cache, name, 'chrome-linux64/chrome'))
  .find(path => existsSync(path)) : undefined);
assert(executablePath, 'Set SPACE_CHROMIUM to a Chromium executable.');

const headful = process.env.SPACE_HEADFUL === '1';
const browser = await chromium.launch({ executablePath, headless: !headful,
  args: headful ? ['--ozone-platform=wayland'] :
    ['--use-gl=angle', '--use-angle=swiftshader', '--enable-unsafe-swiftshader', '--disable-dev-shm-usage'] });
try {
  // Each invocation gets a new context/page and fresh WASM state. Inter-run
  // disk cache must not preserve generated cells (none are saved to disk).
  const context = await browser.newContext({ viewport: { width: 1600, height: 1000 } });
  const page = await context.newPage();
  const errors = [];
  page.on('pageerror', error => {
    if (!String(error).includes('Using exceptions for control flow')) errors.push(String(error));
  });
  await page.goto(`${base}/?benchmark=star-flight`, { waitUntil: 'domcontentloaded', timeout: 120_000 });
  await page.waitForFunction(() => window.starBenchmark?.done, null, { timeout: 180_000 });
  const results = await page.evaluate(() => window.starBenchmark);
  assert.deepEqual(errors, [], 'Browser runtime errors');
  assert.equal(results.initial_loaded_stars, 0);
  assert(results.final && results.samples.length >= 10,
    `Benchmark did not collect enough data: ${JSON.stringify({ final: results.final, samples: results.samples.length })}`);
  results.branch = execFileSync('git', ['branch', '--show-current'], { cwd: root, encoding: 'utf8' }).trim();
  results.commit = execFileSync('git', ['rev-parse', '--short', 'HEAD'], { cwd: root, encoding: 'utf8' }).trim();
  results.environment = headful ? 'headed Chromium / Wayland' :
    'headless Chromium / SwiftShader (not representative of native GPU FPS)';
  mkdirSync(resolve(root, 'data/logs'), { recursive: true });
  const stamp = new Date().toISOString().replaceAll(':', '-');
  const path = resolve(root, `data/logs/star-flight-${label}-${stamp}.json`);
  writeFileSync(path, JSON.stringify(results, null, 2) + '\n');
  assert(results.wall_seconds < 22 && results.final.stars > 0,
    `Throttled or incomplete run (${results.wall_seconds.toFixed(1)} s wall time, ${results.final.stars} stars). Saved diagnostics at ${path}`);
  console.log(`${results.final.mode}: ${results.final.stars} stars, ${results.final.pending} pending, ${results.final.mesh_chunks} mesh chunks, P99 ${results.final.frame_p99_ms} ms`);
  console.log(`Saved ${path}`);
} finally {
  await browser.close();
}
