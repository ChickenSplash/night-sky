// Proof-of-concept HTML panels. Open the client with `?ui=html` to use them in
// place of the egui panels.
const RATES = [['Real time', 1], ['1 hr/s', 3600], ['1 day/s', 86400], ['30 days/s', 2592000]];
const VIEWS = [['Orbit', 'orbit'], ['Task table', 'table'], ['Constellation', 'constellation']];
const LY = 9.4607e15;
const AU = 1.495978707e11;

const el = (tag, props = {}, ...children) => {
  const node = Object.assign(document.createElement(tag), props);
  node.append(...children);
  return node;
};

export function mount(bridge, canvas) {
  const lists = el('div', { className: 'lists' });
  const atlas = el('button', { className: 'item', textContent: 'Interstellar atlas' });
  const source = el('p', { className: 'muted' });
  const filter = el('input', { type: 'search', placeholder: 'Filter lists…' });
  const views = el('div', { className: 'row' });
  const previous = el('button', { textContent: '‹', title: 'Previous page' });
  const next = el('button', { textContent: '›', title: 'Next page' });
  const pageLabel = el('span', { className: 'muted' });
  const pause = el('button', { className: 'primary' });
  const rates = el('div', { className: 'row' });
  const clock = el('span', { className: 'readout accent' });
  const range = el('span', { className: 'readout muted' });
  const notice = el('span', { className: 'notice muted' });

  const nav = el('aside', { id: 'nav-panel', className: 'panel' },
    el('h1', { textContent: 'ClickUp / Space' }),
    source,
    el('h2', { textContent: 'Lists' }),
    filter,
    atlas,
    lists,
    el('h2', { textContent: 'View' }),
    views,
    el('div', { className: 'row' }, previous, pageLabel, next),
  );
  const bar = el('footer', { id: 'time-bar', className: 'panel' },
    pause, rates, clock, range, notice);
  document.querySelector('#hud').append(nav, bar);

  let galaxy = { lists: [] };
  let state = {};
  const buttons = new Map();

  const renderLists = () => {
    const query = filter.value.trim().toLowerCase();
    buttons.clear();
    lists.replaceChildren(...galaxy.lists
      .filter(list => !query || list.name.toLowerCase().includes(query))
      .map(list => {
        const button = el('button', { className: 'item' },
          el('span', { textContent: list.name }),
          el('small', { textContent: `${list.tasks} task planets` }));
        button.addEventListener('click', () => bridge.command({ type: 'jump', list: list.id }));
        buttons.set(list.id, button);
        return button;
      }));
    renderState();
  };

  const renderState = () => {
    atlas.classList.toggle('active', !!state.atlas);
    for (const [id, button] of buttons) button.classList.toggle('active', !state.atlas && id === state.list);
    for (const button of views.children) button.classList.toggle('active', !state.atlas && button.dataset.view === state.view);
    for (const button of rates.children) button.classList.toggle('active', Number(button.dataset.rate) === state.rate);
    const pages = state.pages || [];
    const current = Math.max(0, pages.indexOf(state.page));
    pageLabel.textContent = `Page ${current + 1} / ${Math.max(pages.length, 1)}`;
    previous.disabled = current <= 0;
    next.disabled = current + 1 >= pages.length;
    previous.onclick = () => bridge.command({ type: 'show_page', page: pages[current - 1] });
    next.onclick = () => bridge.command({ type: 'show_page', page: pages[current + 1] });
    pause.textContent = state.paused ? '▶ Resume' : 'Ⅱ Pause';
    notice.textContent = state.notice || '';
  };

  for (const [label, view] of VIEWS) {
    const button = el('button', { textContent: label });
    button.dataset.view = view;
    button.addEventListener('click', () => bridge.command({ type: 'set_view', view }));
    views.append(button);
  }
  for (const [label, rate] of RATES) {
    const button = el('button', { textContent: label });
    button.dataset.rate = rate;
    button.addEventListener('click', () => bridge.command({ type: 'set_rate', rate }));
    rates.append(button);
  }
  atlas.addEventListener('click', () => bridge.command({ type: 'atlas' }));
  pause.addEventListener('click', () => bridge.command({ type: 'set_paused', paused: !state.paused }));
  filter.addEventListener('input', renderLists);
  // Esc hands the keyboard back to the 3D view.
  document.querySelector('#hud').addEventListener('keydown', event => {
    if (event.key === 'Escape') {
      event.target.blur?.();
      canvas.focus();
    }
  });

  bridge.on('galaxy', detail => {
    galaxy = detail;
    source.textContent = detail.source;
    renderLists();
  });
  bridge.on('state', detail => {
    state = detail;
    renderState();
  });
  bridge.on('tick', ({ days, range: metres }) => {
    clock.textContent = `T + ${days.toFixed(2)} days`;
    range.textContent = metres > LY * 0.01
      ? `Range ${(metres / LY).toFixed(3)} ly`
      : `Range ${(metres / AU).toFixed(4)} AU`;
  });

  // The 3D view fits the space the panels leave uncovered.
  const report = () => {
    const box = canvas.getBoundingClientRect();
    const left = nav.getBoundingClientRect().right - box.left;
    const bottom = box.bottom - bar.getBoundingClientRect().top;
    bridge.command({ type: 'panels', top: 0, right: 0, bottom, left });
  };
  new ResizeObserver(report).observe(document.querySelector('#hud'));
  new ResizeObserver(report).observe(canvas);
}
