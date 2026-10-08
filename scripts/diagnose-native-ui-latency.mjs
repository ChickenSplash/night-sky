// Bounded native diagnostic. No focus changes; always cleans up its own processes.
// Usage: node scripts/diagnose-native-ui-latency.mjs [30|60]
import { chromium } from '../tools/browser/node_modules/playwright-core/index.mjs';
import { execFileSync } from 'node:child_process';
import { existsSync, readFileSync, writeFileSync, readdirSync, statSync, mkdirSync } from 'node:fs';
import { resolve } from 'node:path';
import assert from 'node:assert/strict';

const root = resolve(import.meta.dirname, '..');
const hz = Number(process.argv[2] || 30);
assert([30, 60].includes(hz), 'Use 30 or 60 Hz.');
const monitor = JSON.parse(execFileSync('hyprctl', ['monitors', '-j'])).find(m => m.name === 'HDMI-A-1');
assert(monitor && !monitor.disabled, 'Secondary monitor required.');
assert.equal(monitor.scale, 1, 'This pixel-tag diagnostic currently requires secondary scale 1.');
const id = `latency-${Date.now()}-${hz}`;
const folder = resolve(root, 'data/logs', id);
mkdirSync(folder, { recursive: true });
const quote = s => `'${s.replaceAll("'", "'\\''")}'`;
const command = ['env', `CEF_PATH=${root}/data/cef-sdk`, `LD_LIBRARY_PATH=${root}/data/cef-sdk`,
  'SPACE_UI_DEBUG_PORT=9223', 'SPACE_UI_LATENCY_PROBE=1', `SPACE_UI_PROBE_HZ=${hz}`, `SPACE_UI_PROBE_RUN_ID=${id}`,
  `${root}/target/debug/space-client`].map(quote).join(' ');
const shell = `exec ${command} > ${quote(resolve(folder, 'client.log'))} 2>&1`;
const lua = `hl.dsp.exec_cmd(${JSON.stringify(`bash -c ${quote(shell)}`)}, { workspace = '${monitor.activeWorkspace.id} silent', no_focus = true, float = true, size = '1440 900', center = true })`;
const launched = Date.now();
let browser;
const delay = ms => new Promise(resolve => setTimeout(resolve, ms));
const owns = pid => {
  try { return readFileSync(`/proc/${pid}/environ`).toString().split('\0').includes(`SPACE_UI_PROBE_RUN_ID=${id}`); }
  catch { return false; }
};
try {
  execFileSync('hyprctl', ['dispatch', lua]);
  while (Date.now() - launched < 12_000) {
    try { browser = await chromium.connectOverCDP('http://127.0.0.1:9223', { timeout: 1000 }); break; }
    catch { await delay(250); }
  }
  assert(browser, 'Native diagnostic did not start.');
  const pid = Number(readFileSync(resolve(root, 'data/logs/ui-latency-probe.pid'), 'utf8'));
  assert(owns(pid), 'Diagnostic PID belongs to another process.');
  const client = JSON.parse(execFileSync('hyprctl', ['clients', '-j'])).find(c => c.pid === pid);
  assert(client && client.monitor === monitor.id && client.workspace.id === monitor.activeWorkspace.id,
    'Diagnostic window is not visible on the requested secondary workspace.');
  const page = browser.contexts().flatMap(c => c.pages()).find(p => p.url().startsWith('cef://localhost/__inline__/'));
  assert(page, 'No native UI page.');
  await page.evaluate(() => {
    window.probeState = {};
    for (const name of ['galaxy', 'state', 'tick']) window.spaceBridge.on(name, value => { window.probeState[name] = value; });
  });
  await page.waitForFunction(() => window.probeState.galaxy?.lists.length && window.probeState.state, null, { timeout: 8000 });
  const task = await page.evaluate(() => {
    const list = window.probeState.galaxy.lists[0];
    return { type: 'find_task', list: list.id, task: list.tasks[0].id };
  });
  await page.evaluate(command => window.spaceBridge.command(command), task);
  await page.waitForFunction(id => window.probeState.state.task === id, task.task);
  // Rust's opt-in probe moves the camera without per-frame panel-state changes.
  await page.evaluate(() => {
    window.spaceBridge.command({ type: 'set_paused', paused: true });
    document.querySelector('#nav-panel .tabs button:last-child').click();
    const list = document.querySelector('#nav-panel .tasks');
    const tag = document.createElement('canvas'); tag.width = 6; tag.height = 1;
    Object.assign(tag.style, { position: 'fixed', left: '8px', top: '0', width: '6px', height: '1px', zIndex: 1000, pointerEvents: 'none' });
    document.body.append(tag);
    const ctx = tag.getContext('2d');
    window.scrollProbeSequence = 0;
    list.addEventListener('scroll', () => {
      ctx.fillStyle = 'rgb(169,203,51)'; ctx.fillRect(0, 0, 1, 1);
      ctx.fillStyle = 'rgb(81,43,121)'; ctx.fillRect(1, 0, 1, 1);
      for (let i = 0; i < 4; i++) { ctx.fillStyle = `rgb(${(window.scrollProbeSequence >>> (i * 8)) & 255},0,0)`; ctx.fillRect(i + 2, 0, 1, 1); }
    });
  });
  await delay(1500);
  const session = await browser.contexts()[0].newCDPSession(page);
  const point = await page.locator('#nav-panel .tasks').evaluate(el => { const r = el.getBoundingClientRect(); return { x: r.x + r.width / 2, y: r.y + r.height / 2 }; });
  const wheels = [];
  for (let sequence = 1; sequence <= 20 && owns(pid); sequence++) {
    await page.evaluate(s => { window.scrollProbeSequence = s; }, sequence);
    const issued_wall_ms = Date.now();
    await session.send('Input.dispatchMouseEvent', { type: 'mouseWheel', ...point, deltaX: 0, deltaY: sequence % 2 ? 240 : -240 });
    wheels.push({ sequence, issued_wall_ms });
    await delay(600);
  }
  while (owns(pid) && Date.now() - launched < 32_000) await delay(250);
  const report = resolve(root, 'data/logs/ui-latency-probe.json');
  assert(statSync(report).mtimeMs > launched, 'No fresh painted-pixel samples.');
  const samples = JSON.parse(readFileSync(report, 'utf8')).filter(s => s.seconds >= 5);
  assert(samples.length >= 100, 'Too few steady-state samples.');
  const percentile = (values, p) => { const v = values.toSorted((a, b) => a - b); return v[Math.round((v.length - 1) * p)]; };
  const summary = {
    requested_cef_hz: hz, samples: samples.length,
    received_paints_hz: (samples.length - 1) / (samples.at(-1).seconds - samples[0].seconds),
    native_source_frames_hz: (samples.at(-1).latest_source_sequence - samples[0].latest_source_sequence) / (samples.at(-1).seconds - samples[0].seconds),
    source_to_received_pixels_ms: { median: percentile(samples.map(s => s.source_to_received_pixels_ms), .5), p95: percentile(samples.map(s => s.source_to_received_pixels_ms), .95) },
    source_frames_behind: { median: percentile(samples.map(s => s.source_frames_behind), .5), p95: percentile(samples.map(s => s.source_frames_behind), .95) },
    limitations: ['Measures originating annotation frame to CPU pixel receipt, not screen presentation.', 'Render upload/pipelining/presentation can add further latency.', 'Opt-in pixel tag forces annotation repaint.', 'Wheel injection uses CEF DevTools, excluding OS/Bevy input delivery; scroll tags mark the first DOM scroll update, not the end of smooth scrolling.'],
  };
  const allSamples = JSON.parse(readFileSync(report, 'utf8'));
  const scroll = wheels.map(w => {
    const paint = allSamples.find(s => s.scroll_sequence === w.sequence && s.received_wall_ms >= w.issued_wall_ms);
    return paint ? { ...w, wheel_to_received_pixels_ms: paint.received_wall_ms - w.issued_wall_ms } : null;
  }).filter(Boolean);
  summary.scroll_samples = scroll.length;
  if (scroll.length) summary.wheel_to_received_pixels_ms = { median: percentile(scroll.map(s => s.wheel_to_received_pixels_ms), .5), p95: percentile(scroll.map(s => s.wheel_to_received_pixels_ms), .95) };
  writeFileSync(resolve(folder, 'scroll-samples.json'), JSON.stringify(scroll, null, 2));
  writeFileSync(resolve(folder, 'samples.json'), JSON.stringify(samples, null, 2));
  writeFileSync(resolve(folder, 'summary.json'), JSON.stringify(summary, null, 2));
  console.log(JSON.stringify({ folder, ...summary }, null, 2));
} finally {
  if (browser) await browser.close().catch(() => {});
  for (const pid of readdirSync('/proc').filter(p => /^\d+$/.test(p))) {
    if (owns(pid)) { try { process.kill(Number(pid), 'SIGTERM'); } catch {} }
  }
}
