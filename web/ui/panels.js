// The browser UI: HTML panels around the 3D view. Every change goes to Rust as a
// command, and every panel redraws from the state Rust sends back. Open the
// client with `?ui=egui` for the old egui panels.
const RATES = [['Real time', 1], ['1 hr/s', 3600], ['1 day/s', 86400], ['30 days/s', 2592000]];
const VIEWS = [['Orbit', 'orbit'], ['Task table', 'table'], ['Constellation', 'constellation']];
const DISPLAY = [['exaggerate', 'Readable planet sizes'], ['orbits', 'Orbital paths'], ['labels', 'Task labels']];
const STATUS = [['In progress', [92, 213, 242]], ['To do', [169, 154, 238]], ['Complete', [100, 220, 169]]];
const HELP = 'RMB drag: orbit · Scroll: zoom · WASD: move · Q/E: down/up · Shift: boost · Space: pause · Esc: back to the view';
const LY = 9.4607e15;
const AU = 1.495978707e11;
const ROW = 30;

const el = (tag, props = {}, ...children) => {
  const node = Object.assign(document.createElement(tag), props);
  node.append(...children.filter(child => child != null));
  return node;
};
const rgb = ([r, g, b]) => `rgb(${r}, ${g}, ${b})`;
const heading = text => el('h2', { textContent: text });

function caption(state) {
  if (state.atlas) return 'Interstellar atlas / light years';
  if (state.view === 'table') return 'Task table / true sizes · orbits continue';
  if (state.view === 'constellation') return 'List constellation / select a system';
  return 'System view / astronomical units';
}

function dueText(ms) {
  const days = (ms - Date.now()) / 86_400_000;
  return days < 0 ? `Due date: ${(-days).toFixed(1)} days ago` : `Due in ${days.toFixed(1)} days`;
}

// Renders only the rows in view: a live list can hold over a thousand tasks.
function virtualList(container, renderRow) {
  const spacer = el('div', { className: 'spacer' });
  container.append(spacer);
  let items = [];
  const draw = () => {
    const first = Math.max(0, Math.floor(container.scrollTop / ROW) - 4);
    const last = Math.min(items.length, Math.ceil((container.scrollTop + container.clientHeight) / ROW) + 4);
    spacer.style.height = `${items.length * ROW}px`;
    spacer.replaceChildren(...items.slice(first, last).map((item, i) => {
      const row = renderRow(item);
      row.style.top = `${(first + i) * ROW}px`;
      return row;
    }));
  };
  container.addEventListener('scroll', draw);
  new ResizeObserver(draw).observe(container);
  return {
    set(next) { items = next; container.scrollTop = 0; draw(); },
    draw,
  };
}

export function mount(bridge, canvas) {
  const hud = document.querySelector('#hud');
  const send = command => bridge.command(command);
  let galaxy = { source: '', lists: [] };
  let state = {};

  // Navigation panel: lists, task index and display options.
  const source = el('p', { className: 'muted source' });
  const listFilter = el('input', { type: 'search', placeholder: 'Filter lists…', ariaLabel: 'Filter lists' });
  const atlas = el('button', { className: 'item', textContent: 'Interstellar atlas' });
  const lists = el('div', { className: 'lists' });
  const taskSearch = el('input', { type: 'search', placeholder: 'Search all task planets…', ariaLabel: 'Search tasks' });
  const taskCount = el('p', { className: 'muted small' });
  const taskBox = el('div', { className: 'tasks' });
  const toggles = DISPLAY.map(([option, label]) => {
    const input = el('input', { type: 'checkbox' });
    input.dataset.option = option;
    input.addEventListener('change', () => send({ type: 'set_display', option, on: input.checked }));
    return el('label', { className: 'toggle' }, input, el('span', { textContent: label }));
  });
  const nav = el('aside', { id: 'nav-panel', className: 'panel' },
    el('header', {},
      el('h1', { textContent: 'ClickUp / Space' }),
      source),
    el('section', { className: 'grow-1' },
      heading('01 / Lists'), listFilter, atlas, lists),
    el('section', { className: 'grow-2' },
      heading('02 / Task index'), taskSearch, taskCount, taskBox),
    el('section', {},
      heading('Display'), ...toggles,
      el('p', { className: 'muted small', textContent: 'Orbit view uses physical distances. The task table keeps true body sizes with schematic spacing. Orbits keep running in every view.' })),
  );

  // Inspector: telemetry for the current page and the selected task.
  const systemName = el('h3');
  const views = el('div', { className: 'row' }, ...VIEWS.map(([label, view]) => {
    const button = el('button', { textContent: label });
    button.dataset.view = view;
    button.addEventListener('click', () => send({ type: 'set_view', view }));
    return button;
  }));
  const previous = el('button', { textContent: '‹', title: 'Previous page', ariaLabel: 'Previous page' });
  const next = el('button', { textContent: '›', title: 'Next page', ariaLabel: 'Next page' });
  const pageLabel = el('span');
  const stats = el('div', { className: 'stats muted' });
  const frame = el('button', { textContent: 'Frame orbital system' });
  const detail = el('div', { className: 'detail' });
  const inspector = el('aside', { id: 'inspector', className: 'panel' },
    heading('03 / Telemetry'), systemName, views,
    el('div', { className: 'row pager' }, previous, pageLabel, next),
    stats, frame, el('hr'), detail);

  // Time bar and the captions over the 3D view.
  const pause = el('button', { className: 'primary' });
  const rates = el('div', { className: 'row' }, ...RATES.map(([label, rate]) => {
    const button = el('button', { textContent: label });
    button.dataset.rate = rate;
    button.addEventListener('click', () => send({ type: 'set_rate', rate }));
    return button;
  }));
  const clock = el('span', { className: 'readout accent' });
  const notice = el('span', { className: 'notice muted' });
  const bar = el('footer', { id: 'time-bar', className: 'panel' },
    pause, rates, clock, notice, el('span', { className: 'help muted', textContent: HELP, title: HELP }));
  const viewCaption = el('div', { className: 'caption top' });
  const rangeCaption = el('div', { className: 'caption bottom' });
  const empty = el('div', { id: 'empty-universe', hidden: true },
    el('h3', { textContent: 'Loading your universe' }),
    el('p', { className: 'muted', textContent: 'Systems appear as each list finishes loading.' }));
  hud.append(nav, inspector, bar, viewCaption, rangeCaption, empty);

  const listButtons = new Map();
  const renderLists = () => {
    const query = listFilter.value.trim().toLowerCase();
    listButtons.clear();
    lists.replaceChildren(...galaxy.lists
      .filter(list => !query || list.name.toLowerCase().includes(query))
      .map(list => {
        const button = el('button', { className: 'item' },
          el('span', { textContent: list.name }),
          el('small', { textContent: `${list.tasks.length} task planets` }));
        button.addEventListener('click', () => send({ type: 'jump', list: list.id }));
        listButtons.set(list.id, button);
        return button;
      }));
    highlightLists();
  };
  const highlightLists = () => {
    atlas.classList.toggle('active', !!state.atlas);
    for (const [id, button] of listButtons) button.classList.toggle('active', !state.atlas && id === state.list);
  };

  // With no query the index lists the current list's tasks, like egui did.
  const index = virtualList(taskBox, ({ list, task }) => {
    const row = el('button', { className: 'task', title: task.name },
      el('span', { className: 'dot' }), el('span', { textContent: task.name }));
    row.style.color = rgb(task.colour);
    row.classList.toggle('active', list === state.list && task.id === state.task);
    row.addEventListener('click', () => send({ type: 'find_task', list, task: task.id }));
    return row;
  });
  let matches = [];
  let indexKey = null;
  const renderIndex = force => {
    const query = taskSearch.value.trim().toLowerCase();
    const key = `${query}\n${query ? '' : state.list}`;
    if (!force && key === indexKey) {
      index.draw();
      return;
    }
    indexKey = key;
    matches = galaxy.lists
      .filter(list => query || list.id === state.list)
      .flatMap(list => list.tasks
        .filter(task => !query || task.name.toLowerCase().includes(query))
        .map(task => ({ list: list.id, task })));
    const current = galaxy.lists.find(list => list.id === state.list);
    taskCount.textContent = query
      ? `${matches.length} matching task${matches.length === 1 ? '' : 's'} in all lists`
      : `${matches.length} tasks in ${current?.name ?? 'this list'}`;
    index.set(matches);
  };

  const renderInspector = () => {
    const system = state.system;
    systemName.textContent = system?.name ?? '';
    for (const button of views.children) button.classList.toggle('active', !state.atlas && button.dataset.view === state.view);
    const pages = state.pages || [];
    const current = Math.max(0, pages.indexOf(state.page));
    pageLabel.textContent = `Page ${current + 1} / ${Math.max(pages.length, 1)}`;
    previous.disabled = current <= 0;
    next.disabled = current + 1 >= pages.length;
    previous.onclick = () => send({ type: 'show_page', page: pages[current - 1] });
    next.onclick = () => send({ type: 'show_page', page: pages[current + 1] });
    if (system) {
      const [x, y, z] = system.star_ly.map(v => (v >= 0 ? '+' : '') + v.toFixed(2));
      stats.replaceChildren(
        el('p', { textContent: `System ${state.page + 1} · ${system.page_tasks} of ${system.tasks} tasks` }),
        el('p', { textContent: `${system.mass_solar.toFixed(2)} solar masses` }),
        el('p', { className: 'mono', textContent: `Page star position (ly)\n${x}  ${y}  ${z}` }),
        el('p', { className: 'mono', textContent: `Page ${state.page + 1} plane tilt ${system.tilt_deg.toFixed(0)}°, node ${system.node_deg.toFixed(0)}°` }));
    }
    const task = state.selected;
    if (!task) {
      detail.replaceChildren(
        el('h3', { textContent: 'Select a planet' }),
        el('p', { className: 'muted', textContent: 'Click a wireframe, its marker, or a task in the index to inspect it.' }),
        heading('Status signals'),
        ...STATUS.map(([text, colour]) => el('p', { textContent: text, style: `color: ${rgb(colour)}` })),
        el('p', { className: 'muted small', textContent: 'Live lists use their ClickUp status colours.' }));
      return;
    }
    const follow = el('button', { textContent: state.following ? '◎ Following' : '◎ Follow this planet' });
    follow.classList.toggle('active', !!state.following);
    follow.addEventListener('click', () => send({ type: 'follow' }));
    const elements = [
      ['Semi-major axis', `${task.semi_major_au.toFixed(3)} AU`],
      ['Eccentricity', task.eccentricity.toFixed(3)],
      ['Orbital period', `${task.period_days.toFixed(1)} days`],
      ['Physical radius', `${task.radius_km.toFixed(0)} km`],
    ];
    detail.replaceChildren(...[
      heading('Selected task'),
      el('h3', { textContent: task.name }),
      el('p', { className: 'status', textContent: task.status.toUpperCase(), style: `color: ${rgb(task.colour)}` }),
      task.description ? el('p', { className: 'description', textContent: task.description }) : null,
      el('hr'),
      el('p', { textContent: `Assigned: ${task.assignees.length ? task.assignees.join(', ') : 'Nobody'}` }),
      task.due_date_ms != null ? el('p', { textContent: dueText(task.due_date_ms) }) : null,
      heading('Orbital elements'),
      el('dl', {}, ...elements.flatMap(([label, value]) => [el('dt', { textContent: label }), el('dd', { textContent: value })])),
      follow,
      task.url ? el('a', { href: task.url, target: '_blank', rel: 'noopener noreferrer', textContent: 'Open task in ClickUp ↗' }) : null,
    ].filter(Boolean));
  };

  const renderState = () => {
    highlightLists();
    renderIndex(false);
    renderInspector();
    for (const input of nav.querySelectorAll('.toggle input')) input.checked = !!state.display?.[input.dataset.option];
    for (const button of rates.children) button.classList.toggle('active', Number(button.dataset.rate) === state.rate);
    pause.textContent = state.paused ? '▶ Resume' : 'Ⅱ Pause';
    notice.textContent = state.notice || '';
    viewCaption.textContent = caption(state);
  };

  atlas.addEventListener('click', () => send({ type: 'atlas' }));
  frame.addEventListener('click', () => send({ type: 'frame_system' }));
  pause.addEventListener('click', () => send({ type: 'set_paused', paused: !state.paused }));
  listFilter.addEventListener('input', renderLists);
  taskSearch.addEventListener('input', () => renderIndex(false));
  taskSearch.addEventListener('keydown', event => {
    if (event.key === 'Enter' && matches.length) {
      send({ type: 'find_task', list: matches[0].list, task: matches[0].task.id });
    }
  });
  // Clicking a control leaves the keyboard with the 3D view, so Space and WASD
  // keep working and Space cannot re-press the last button.
  hud.addEventListener('mousedown', event => {
    if (event.target.closest('button, label, a')) event.preventDefault();
  });
  // Esc hands the keyboard back to the 3D view.
  hud.addEventListener('keydown', event => {
    if (event.key === 'Escape') {
      event.target.blur?.();
      canvas.focus();
    }
  });

  bridge.on('galaxy', detail => {
    galaxy = detail;
    source.textContent = detail.source;
    empty.hidden = detail.lists.length > 0;
    renderLists();
    renderIndex(true);
  });
  bridge.on('state', detail => {
    state = detail;
    renderState();
  });
  bridge.on('tick', ({ days, range: metres }) => {
    clock.textContent = `T + ${days.toFixed(2)} days`;
    const range = metres > LY * 0.01 ? `${(metres / LY).toFixed(3)} ly` : `${(metres / AU).toFixed(4)} AU`;
    const mode = state.following ? 'tracking' : state.view !== 'orbit' ? 'auto-framed preset' : 'free navigation';
    rangeCaption.textContent = `Camera range ${range} / ${mode}`;
  });

  // The 3D view fits the space the panels leave uncovered.
  const report = () => {
    const box = canvas.getBoundingClientRect();
    send({
      type: 'panels',
      top: 0,
      left: Math.max(0, nav.getBoundingClientRect().right - box.left),
      right: Math.max(0, box.right - inspector.getBoundingClientRect().left),
      bottom: Math.max(0, box.bottom - bar.getBoundingClientRect().top),
    });
  };
  new ResizeObserver(report).observe(hud);
  new ResizeObserver(report).observe(canvas);
}
