// Executes in the actual GTK webview, against the live native simulation.
export async function run(bridge) {
  let state, galaxy, tick;
  bridge.on('state', value => { state = value; });
  bridge.on('galaxy', value => { galaxy = value; });
  bridge.on('tick', value => { tick = value; });
  const wait = async predicate => {
    const end = Date.now() + 15000;
    while (!predicate()) {
      if (Date.now() > end) throw new Error('Native UI state update timed out');
      await new Promise(resolve => setTimeout(resolve, 100));
    }
  };
  const report = result => window.webkit.messageHandlers.native.postMessage('smoke:' + JSON.stringify(result));
  try {
    await wait(() => state && galaxy?.lists.length && tick);
    const initial = { paused: state.paused, rate: state.rate, fov: state.fov_deg };
    const pause = document.querySelector('#time-bar button[title="Pause"], #time-bar button[title="Resume"]');
    if (!pause) throw new Error('Shared pause button is missing');
    pause.click();
    await wait(() => state.paused !== initial.paused);
    bridge.command({ type: 'set_rate', rate: 86400 });
    await wait(() => state.rate === 86400);
    bridge.command({ type: 'set_fov', degrees: 75 });
    await wait(() => state.fov_deg === 75);
    bridge.command({ type: 'jump', list: galaxy.lists[0].id });
    await wait(() => state.list === galaxy.lists[0].id && state.system);
    bridge.command({ type: 'set_paused', paused: initial.paused });
    bridge.command({ type: 'set_rate', rate: initial.rate });
    bridge.command({ type: 'set_fov', degrees: initial.fov });
    await wait(() => state.paused === initial.paused && state.rate === initial.rate && state.fov_deg === initial.fov);
    report({ ok: true, lists: galaxy.lists.length, checks: ['shared pause button', 'rate', 'field of view', 'list navigation', 'state round-trip', 'tick telemetry'] });
  } catch (error) {
    report({ ok: false, error: String(error) });
  }
}
