// CEF-only transport/input adapter. Panels and annotations have no CEF dependency.
const bridge = createBridge();
bridge.native = true;
window.spaceBridge = bridge;
const canvas = document.querySelector('#space-canvas');
mount(bridge, canvas);
mountAnnotations(bridge);
const emit = payload => window.cef.emit(payload);
let connected = false;
let previous = '';
function layout() {
  // Include menus outside panel bounds and the whole modal backdrop. CSS
  // transitions change hit bounds without resize, so observe them each frame.
  const rects = [...document.querySelectorAll('#hud .panel, #hud .menu, #task-modal')]
    .filter(node => {
      const style = getComputedStyle(node);
      return style.display !== 'none' && style.visibility !== 'hidden' && Number(style.opacity) > 0 && style.pointerEvents !== 'none';
    }).map(node => { const r = node.getBoundingClientRect(); return [r.x, r.y, r.width, r.height]; })
    .filter(([x, y, w, h]) => w > 0 && h > 0 && x < innerWidth && y < innerHeight && x + w > 0 && y + h > 0);
  const json = JSON.stringify(rects);
  if (json !== previous) { emit({ kind: 'layout', rects }); previous = json; }
}
function initialise() {
  if (!window.cef?.emit) { requestAnimationFrame(initialise); return; }
  for (const name of ['galaxy', 'state', 'tick', 'overlay', 'blur']) {
    window.cef.listen(`space:${name}`, detail => window.dispatchEvent(new CustomEvent(`space:${name}`, { detail })));
  }
  bridge.connect(json => emit({ kind: 'command', command: JSON.parse(json) }));
  emit({ kind: 'ready' });
  connected = true;
}
window.addEventListener('space:blur', () => { document.activeElement?.blur(); canvas.focus(); });
document.addEventListener('contextmenu', event => event.preventDefault());
document.addEventListener('click', event => {
  const link = event.target.closest('a[href]');
  if (link) { event.preventDefault(); bridge.command({ type: 'open_external', url: link.href }); }
});
document.addEventListener('focusin', () => { if (connected) emit({ kind: 'focus', keyboard: document.activeElement !== canvas && document.activeElement !== document.body }); });
document.addEventListener('pointerup', () => { if (connected) emit({ kind: 'focus', keyboard: document.activeElement !== canvas && document.activeElement !== document.body }); });
// Native benchmark controls remain separate from ordinary UI behaviour.
const benchmarkMode = document.body.dataset.benchmark;
function frame(now) {
  if (connected) {
    layout();
    if (benchmarkMode === 'active') {
      const field = document.querySelector('input[type=search]');
      if (field && Math.floor(now / 100) !== frame.step) { frame.step = Math.floor(now / 100); field.value = frame.step % 2 ? 'demo' : ''; field.dispatchEvent(new Event('input')); }
    } else if (benchmarkMode === 'animation') {
      document.querySelector('#time-bar').style.transform = `translateY(${Math.sin(now / 200) * 3}px)`;
    }
  }
  requestAnimationFrame(frame);
}
initialise(); requestAnimationFrame(frame);
