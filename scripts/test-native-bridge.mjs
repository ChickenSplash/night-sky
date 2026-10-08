// Run against an already-open native client with SPACE_UI_DEBUG_PORT=9223.
// Uses CEF DevTools, not OS input. Never takes desktop focus or starts a window.
import { chromium } from '../tools/browser/node_modules/playwright-core/index.mjs';
import assert from 'node:assert/strict';

const browser = await chromium.connectOverCDP(process.env.SPACE_NATIVE_CDP || 'http://127.0.0.1:9223');
try {
  const page = browser.contexts().flatMap(c => c.pages()).find(p => p.url().startsWith('cef://localhost/__inline__/'));
  assert(page, 'No bundled native UI page found.');
  await page.evaluate(() => {
    window.seen = {};
    for (const name of ['galaxy', 'state', 'tick', 'overlay']) window.spaceBridge.on(name, d => { window.seen[name] = d; });
  });
  await page.waitForFunction(() => window.seen.galaxy?.lists.length && window.seen.state && window.seen.tick && window.seen.overlay, null, { timeout: 60_000 });
  const initial = await page.evaluate(() => window.seen.state);
  const send = command => page.evaluate(c => window.spaceBridge.command(c), command);
  await send({ type: 'set_paused', paused: !initial.paused });
  await page.waitForFunction(p => window.seen.state.paused === p, !initial.paused);
  const list = await page.evaluate(() => window.seen.galaxy.lists[0]);
  await send({ type: 'jump', list: list.id });
  await page.waitForFunction(id => window.seen.state.list === id, list.id);
  const task = list.tasks[0];
  await send({ type: 'find_task', list: list.id, task: task.id });
  await page.waitForFunction(id => window.seen.state.task === id, task.id);
  await send({ type: 'set_view', view: 'table' });
  await page.waitForFunction(() => window.seen.state.view === 'table' && window.seen.overlay.bodies.some(b => b.label), null, { timeout: 30_000 });
  await send({ type: 'set_fov', degrees: 75 });
  await page.waitForFunction(() => window.seen.state.fov_deg === 75);
  await send({ type: 'set_display', option: 'realistic', on: false });
  await page.waitForFunction(() => !window.seen.state.display.realistic);
  await send({ type: 'set_display', option: 'realistic', on: true });
  await page.waitForFunction(() => window.seen.state.display.realistic);
  const days = await page.evaluate(() => window.seen.tick.days);
  await send({ type: 'set_paused', paused: true });
  await page.waitForFunction(() => window.seen.state.paused);
  await page.waitForTimeout(300);
  const frozen = await page.evaluate(() => window.seen.tick.days);
  await page.waitForTimeout(300);
  assert.equal(await page.evaluate(() => window.seen.tick.days), frozen);
  assert(frozen >= days);
  assert.equal(await page.locator('select').count(), 0);
  assert.equal(await page.locator('#annotations').count(), 1);
  await send({ type: 'set_paused', paused: initial.paused });
  console.log('Native CEF galaxy/state/tick, commands, table labels, HDR toggle and pause bridge passed. OS input acceptance remains manual.');
} finally { await browser.close(); }
