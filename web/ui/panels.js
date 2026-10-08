// Shared native/browser HTML panels around the 3D view. Every change goes to Rust as a
// command, and every panel redraws from the state Rust sends back. Open the
// client with `?ui=egui` for the old egui panels.
const YEAR = 31_557_600;
// Simulated seconds per real second, from real time up to a century a second.
const RATES = [
  ['Real time', 1], ['10 s/s', 10], ['1 min/s', 60], ['10 min/s', 600], ['1 hr/s', 3600],
  ['6 hr/s', 21_600], ['1 day/s', 86_400], ['3 days/s', 259_200], ['1 week/s', 604_800],
  ['30 days/s', 2_592_000], ['90 days/s', 7_776_000], ['1 yr/s', YEAR], ['3 yr/s', 3 * YEAR],
  ['10 yr/s', 10 * YEAR], ['30 yr/s', 30 * YEAR], ['100 yr/s', 100 * YEAR],
];
const VIEWS = [['Orbit', 'orbit'], ['Task table', 'table'], ['Constellation', 'constellation']];
const DISPLAY = [['exaggerate', 'Readable planet sizes'], ['orbits', 'Orbital paths'], ['labels', 'Planet labels'], ['markers', 'Body markers'], ['velocity_marker', 'Velocity marker']];
// Physical exposure and wireframes apply only to realistic rendering.
const RENDERING = [['realistic', 'Realistic lighting'], ['galaxy_glow', 'Smooth galaxy glow (prototype)'], ['physical', 'Physical exposure'], ['wireframe', 'Wireframes over bodies']];
const HELP = [['Click', 'Select a star, again to visit'], ['C', 'Select the centre star'], ['Enter', 'Visit the selected star'], ['Left drag', 'Look around'], ['Right drag', 'Orbit'], ['Shift + left drag', 'Field of view'], ['Scroll', 'Zoom'], ['W A S D', 'Move'], ['Q / E', 'Roll left / right'], ['R / F', 'Up / down'],
  ['Shift + scroll', 'Adjust movement speed'], ['Space', 'Pause'], ['Esc', 'Back to the view']];
// Vertical field of view range in degrees, as `FOV_DEG` in Rust.
const FOV = [30, 130];
const LIST_SORTS = [['List order', 'default'], ['Name A–Z', 'name'], ['Most tasks', 'tasks']];
const TASK_SORTS = [['List order', 'default'], ['Name A–Z', 'name'], ['Due date', 'due']];
const STATUS_RANK = { open: 0, custom: 1, done: 2, closed: 3 };
const PREFS = 'space.sidebar';
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
const SVG = 'http://www.w3.org/2000/svg';
const ICONS = {
  play: 'M8 5v14l11-7z',
  pause: 'M7 5h4v14H7zM13 5h4v14h-4z',
  slower: 'M11 6v12l-8.5-6zM20.5 6v12L12 12z',
  faster: 'M13 6v12l8.5-6zM3.5 6v12L12 12z',
  menu: 'M4 6h16v2H4zM4 11h16v2H4zM4 16h16v2H4z',
  close: 'M6.4 5 12 10.6 17.6 5 19 6.4 13.4 12 19 17.6 17.6 19 12 13.4 6.4 19 5 17.6 10.6 12 5 6.4z',
  chevron: 'M9.3 6.7 10.7 5.3 17.4 12l-6.7 6.7-1.4-1.4 5.3-5.3z',
  frame: 'M11 2h2v5h-2zM11 17h2v5h-2zM2 11h5v2H2zM17 11h5v2h-5zM12 10a2 2 0 1 1 0 4 2 2 0 0 1 0-4z',
  back: 'M14.7 6.7 13.3 5.3 6.6 12l6.7 6.7 1.4-1.4L9.4 12z',
  gear: 'M19.14 12.94c.04-.3.06-.61.06-.94 0-.32-.02-.64-.07-.94l2.03-1.58a.49.49 0 0 0 .12-.61l-1.92-3.32a.49.49 0 0 0-.59-.22l-2.39.96c-.5-.38-1.03-.7-1.62-.94l-.36-2.54a.48.48 0 0 0-.48-.41h-3.84c-.24 0-.43.17-.47.41l-.36 2.54c-.59.24-1.13.57-1.62.94l-2.39-.96a.48.48 0 0 0-.59.22L2.74 8.87c-.12.21-.08.47.12.61l2.03 1.58c-.05.3-.09.63-.09.94s.02.64.07.94l-2.03 1.58a.49.49 0 0 0-.12.61l1.92 3.32c.12.22.37.29.59.22l2.39-.96c.5.38 1.03.7 1.62.94l.36 2.54c.05.24.24.41.48.41h3.84c.24 0 .44-.17.47-.41l.36-2.54c.59-.24 1.13-.56 1.62-.94l2.39.96c.22.08.47 0 .59-.22l1.92-3.32c.12-.22.07-.47-.12-.61l-2.01-1.58zM12 15.6A3.6 3.6 0 1 1 12 8.4a3.6 3.6 0 0 1 0 7.2z',
};
const icon = name => {
  const svg = document.createElementNS(SVG, 'svg');
  svg.setAttribute('viewBox', '0 0 24 24');
  svg.setAttribute('aria-hidden', 'true');
  const path = document.createElementNS(SVG, 'path');
  path.setAttribute('d', ICONS[name]);
  svg.append(path);
  return svg;
};

// The step on the rate ladder nearest to a rate, measured on a log scale.
function rateStep(rate) {
  let best = 0;
  RATES.forEach(([, r], i) => {
    if (Math.abs(Math.log(r / rate)) < Math.abs(Math.log(RATES[best][1] / rate))) best = i;
  });
  return best;
}

function elapsed(days) {
  return days >= 365.25 ? `T + ${(days / 365.25).toFixed(2)} yr` : `T + ${days.toFixed(2)} days`;
}

function caption(state) {
  if (!state.system) return 'Galaxy / light years';
  if (state.view === 'table') return 'Task table / true sizes · orbits continue';
  if (state.view === 'constellation') return 'List constellation / select a system';
  return 'System view / astronomical units';
}

// Status groups follow the board: open, then custom, then done and closed.
function statusRank(task) {
  return STATUS_RANK[task.status_type] ?? (/complete|done|closed/i.test(task.status) ? 2 : 1);
}

function dueShort(ms) {
  const days = (ms - Date.now()) / 86_400_000;
  if (Math.abs(days) < 1) return ['Today', ''];
  if (days < 0) return [`${Math.round(-days)}d late`, 'late'];
  if (days > 45) return [new Date(ms).toLocaleDateString('en-GB', { day: 'numeric', month: 'short' }), ''];
  return [`in ${Math.round(days)}d`, ''];
}

function loadPrefs() {
  const defaults = { open: true, tab: 'lists', listSort: 'default', taskSort: 'default', group: true };
  try {
    return { ...defaults, ...JSON.parse(localStorage.getItem(PREFS) || '{}') };
  } catch {
    return defaults;
  }
}

function dueText(ms) {
  const date = new Date(ms).toLocaleDateString('en-GB', { day: 'numeric', month: 'short', year: 'numeric' });
  return `${date} · ${dueShort(ms)[0].toLowerCase()}`;
}

const JUPITER_MASSES = 317.83;

function massText(earths) {
  return earths >= 0.1 * JUPITER_MASSES
    ? `${(earths / JUPITER_MASSES).toFixed(2)} Jupiter masses`
    : `${earths < 0.1 ? earths.toFixed(4) : earths.toFixed(2)} Earth masses`;
}

function rotationText(hours) {
  return hours >= 48 ? periodText(hours / 24) : hours >= 2 ? `${hours.toFixed(1)} hours` : `${(hours * 60).toFixed(0)} minutes`;
}

function tiltText(rad) {
  const deg = rad * 180 / Math.PI;
  return `${deg.toFixed(1)}°${deg > 90 ? ' · retrograde' : ''}`;
}

function starRadiusText(solar) {
  return solar < 0.05 ? `${Math.round(solar * 695700).toLocaleString('en-GB')} km` : `${solar.toFixed(2)} solar radii`;
}

function luminosityText(solar) {
  return solar < 0.01 ? solar.toExponential(1) : solar < 10 ? solar.toFixed(3) : Math.round(solar).toLocaleString('en-GB');
}

function periodText(days) {
  return days >= 730 ? `${(days / 365.25).toFixed(1)} years` : `${days.toFixed(1)} days`;
}

// Renders only the rows in view: a list can hold over a thousand tasks.
// `keyOf` names everything a row shows, so unchanged rows are reused rather
// than rebuilt under the pointer.
function virtualList(container, renderRow, keyOf) {
  const spacer = el('div', { className: 'spacer' });
  container.append(spacer);
  let items = [];
  let cache = new Map();
  const draw = () => {
    const first = Math.max(0, Math.floor(container.scrollTop / ROW) - 4);
    const last = Math.min(items.length, Math.ceil((container.scrollTop + container.clientHeight) / ROW) + 4);
    spacer.style.height = `${items.length * ROW}px`;
    const next = new Map();
    const rows = items.slice(first, last).map((item, i) => {
      const key = keyOf(item);
      const row = cache.get(key) ?? renderRow(item);
      row.style.top = `${(first + i) * ROW}px`;
      next.set(key, row);
      return row;
    });
    cache = next;
    if (rows.length !== spacer.children.length || rows.some((row, i) => spacer.children[i] !== row)) {
      spacer.replaceChildren(...rows);
    }
  };
  container.addEventListener('scroll', draw);
  new ResizeObserver(draw).observe(container);
  return {
    set(next, keepScroll) {
      items = next;
      if (!keepScroll) container.scrollTop = 0;
      draw();
    },
    draw,
  };
}

export function mount(bridge, canvas) {
  const hud = document.querySelector('#hud');
  const send = command => bridge.command(command);
  let galaxy = { source: '', lists: [] };
  let state = {};

  // Sidebar: a floating panel with a tab of lists and a tab of the current
  // list's tasks. It slides away, and its choices persist between visits.
  const prefs = loadPrefs();
  const save = () => { try { localStorage.setItem(PREFS, JSON.stringify(prefs)); } catch { /* Inline CEF pages may have no persistent origin. */ } };
  const round = (name, label, className = 'round') => el('button', { className, title: label, ariaLabel: label }, icon(name));
  // A styled dropdown: the browser's own <select> menu cannot be themed.
  let openMenu = null;
  const dropdown = (label, options, onChange) => {
    const text = el('span');
    const button = el('button', { className: 'dropdown', title: label, ariaLabel: label }, text, icon('chevron'));
    button.setAttribute('aria-haspopup', 'listbox');
    const menu = el('div', { className: 'menu floating', role: 'listbox', hidden: true });
    const node = el('div', { className: 'dropdown-wrap' }, button, menu);
    let value;
    const items = options.map(([label, option]) => {
      const item = el('button', { role: 'option', textContent: label });
      item.addEventListener('click', () => {
        close();
        if (option !== value) onChange(option);
      });
      menu.append(item);
      return [option, label, item];
    });
    const close = () => {
      menu.hidden = true;
      button.setAttribute('aria-expanded', false);
      if (openMenu === close) openMenu = null;
    };
    button.addEventListener('click', () => {
      if (!menu.hidden) return close();
      openMenu?.();
      menu.hidden = false;
      button.setAttribute('aria-expanded', true);
      openMenu = close;
    });
    node.set = next => {
      value = next;
      for (const [option, label, item] of items) {
        item.setAttribute('aria-selected', option === next);
        if (option === next) text.textContent = label;
      }
    };
    return node;
  };
  document.addEventListener('pointerdown', event => {
    if (openMenu && !event.target.closest?.('.dropdown-wrap')) openMenu();
  }, true);
  document.addEventListener('keydown', event => {
    if (event.key === 'Escape') {
      openMenu?.();
      if (!modal.hidden) setModal(false);
    }
  });
  const select = (label, options, key, onChange) => {
    const node = dropdown(label, options, value => {
      prefs[key] = value;
      node.set(value);
      save();
      onChange();
    });
    node.set(prefs[key]);
    return node;
  };
  const chip = (label, key) => {
    const node = el('button', { className: 'chip', textContent: label });
    node.setAttribute('aria-pressed', prefs[key]);
    node.addEventListener('click', () => {
      prefs[key] = !prefs[key];
      node.setAttribute('aria-pressed', prefs[key]);
      save();
      renderIndex(true);
    });
    return node;
  };
  const tabButton = (tab, label) => {
    const node = el('button', { role: 'tab' }, el('span', { textContent: label }), el('small'));
    node.dataset.tab = tab;
    node.addEventListener('click', () => showTab(tab));
    return node;
  };

  const source = el('p', { className: 'muted source' });
  const hide = round('close', 'Hide sidebar', 'round small');
  const opener = round('menu', 'Show sidebar', 'panel floating round');
  opener.id = 'nav-toggle';
  const listsTab = tabButton('lists', 'Lists');
  const tasksTab = tabButton('tasks', 'Tasks');
  const listFilter = el('input', { type: 'search', placeholder: 'Filter lists…', ariaLabel: 'Filter lists' });
  const listSort = select('Sort lists', LIST_SORTS, 'listSort', () => renderLists());
  const lists = el('div', { className: 'lists' });
  const taskTitle = el('p', { className: 'tab-title' });
  const taskSearch = el('input', { type: 'search', placeholder: 'Filter tasks…', ariaLabel: 'Search tasks' });
  const taskSort = select('Sort tasks', TASK_SORTS, 'taskSort', () => renderIndex(true));
  const groupChip = chip('Group by status', 'group');
  const taskCount = el('p', { className: 'muted small' });
  const taskBox = el('div', { className: 'tasks' });
  const notice = el('p', { className: 'notice small' });
  const listsBody = el('section', { className: 'tab-body' },
    el('div', { className: 'toolbar' }, listFilter, listSort), lists);
  const tasksBody = el('section', { className: 'tab-body' },
    taskTitle,
    el('div', { className: 'toolbar' }, taskSearch, taskSort),
    el('div', { className: 'row' }, groupChip),
    taskCount, taskBox);
  const nav = el('aside', { id: 'nav-panel', className: 'panel floating' },
    el('header', {}, el('div', {}, el('h1', { textContent: 'Night Sky' }), source), hide),
    el('div', { className: 'tabs', role: 'tablist' }, listsTab, tasksTab),
    listsBody, tasksBody, notice);

  const showTab = tab => {
    prefs.tab = tab;
    save();
    listsBody.hidden = tab !== 'lists';
    tasksBody.hidden = tab !== 'tasks';
    listsTab.setAttribute('aria-selected', tab === 'lists');
    tasksTab.setAttribute('aria-selected', tab === 'tasks');
  };
  const setOpen = open => {
    prefs.open = open;
    save();
    hud.classList.toggle('nav-open', open);
    nav.inert = !open;
  };

  // Settings: a popover under the gear.
  const gear = round('gear', 'Settings', 'panel floating round');
  gear.id = 'settings-button';
  const closeSettings = round('close', 'Close settings', 'round small');
  const benchmarkButton = el('button', { type: 'button', className: 'benchmark-button', textContent: 'Run star-stream benchmark' });
  benchmarkButton.addEventListener('click', () => {
    const url = new URL(location.href);
    url.searchParams.set('benchmark', 'star-flight');
    location.assign(url);
  });
  const toggle = ([option, label]) => {
    const input = el('input', { type: 'checkbox' });
    input.dataset.option = option;
    input.addEventListener('change', () => send({ type: 'set_display', option, on: input.checked }));
    return el('label', { className: 'toggle' }, input, el('span', { textContent: label }));
  };
  // Field of view: a slider and a typed value, kept in step with Rust's state.
  const fovRange = el('input', { type: 'range', min: FOV[0], max: FOV[1], step: 1, ariaLabel: 'Field of view' });
  const fovNumber = el('input', { type: 'number', min: FOV[0], max: FOV[1], step: 1, ariaLabel: 'Field of view in degrees' });
  const sendFov = input => {
    const degrees = Number(input.value);
    if (input.value !== '' && Number.isFinite(degrees)) send({ type: 'set_fov', degrees: Math.min(FOV[1], Math.max(FOV[0], degrees)) });
  };
  fovRange.addEventListener('input', () => sendFov(fovRange));
  fovNumber.addEventListener('change', () => sendFov(fovNumber));
  fovNumber.addEventListener('keydown', event => {
    if (event.key === 'Enter') sendFov(fovNumber);
  });
  // Not a <label>: the HUD keeps focus off labels, and the number must be typable.
  const fov = el('div', { className: 'fov' }, el('span', { textContent: 'Field of view' }), fovRange,
    el('span', { className: 'fov-value' }, fovNumber, el('span', { className: 'muted', textContent: '°' })));
  const settings = el('div', { id: 'settings', className: 'panel floating', hidden: true, role: 'dialog', ariaLabel: 'Settings' },
    el('header', {}, el('h3', { textContent: 'Settings' }), closeSettings),
    heading('Display'), ...DISPLAY.map(toggle), fov,
    el('p', { className: 'muted small', textContent: 'Orbit view uses physical distances. The task table keeps true body sizes with schematic spacing. Orbits keep running in every view.' }),
    heading('Rendering'), ...RENDERING.map(toggle),
    el('div', { className: 'galaxy-reference' }, ...[['inside', 'Inside'], ['face_on', 'Face-on'], ['edge_on', 'Edge-on']].map(([view, label]) => {
      const button = el('button', { type: 'button', textContent: label, ariaLabel: `Galaxy reference: ${label}` });
      button.addEventListener('click', () => send({ type: 'galaxy_reference', view }));
      return button;
    })),
    el('p', { className: 'muted small', textContent: 'Reference views for the smooth galaxy prototype. No dust or spiral detail yet.' }),
    el('p', { className: 'muted small', textContent: 'Realistic lighting draws stars by temperature and true brightness, and lights planets by their star. Physical exposure shows true contrast, as one camera exposure adapting like an eye; assisted exposure keeps faint planets and stars readable.' }),
    heading('Controls'),
    el('dl', { className: 'keys' }, ...HELP.flatMap(([key, action]) => [el('dt', {}, el('kbd', { textContent: key })), el('dd', { textContent: action })])),
    ...(!bridge.native ? [heading('Benchmark'), benchmarkButton,
      el('p', { className: 'muted small', textContent: 'Reloads with no stars loaded, then flies 15 seconds from home towards the galactic centre at 1,000 ly/s. Results appear on screen and in the browser console.' })] : []));
  if (bridge.native) {
    const report = el('button', { type: 'button', textContent: 'Save bug report' });
    report.addEventListener('click', () => send({ type: 'save_bug_report' }));
    settings.append(heading('Diagnostics'), report);
  }
  const setSettings = open => {
    settings.hidden = !open;
    gear.classList.toggle('active', open);
    gear.setAttribute('aria-expanded', open);
  };

  // Task details: a modal opened from the task pill.
  const closeModal = round('close', 'Close details', 'round small');
  const modalBody = el('div', { className: 'modal-body' });
  const modal = el('div', { id: 'task-modal', hidden: true },
    el('div', { className: 'panel floating card', role: 'dialog', ariaLabel: 'Task details' }, closeModal, modalBody));
  let modalKey = null;
  const setModal = open => {
    modal.hidden = !open;
    modalKey = null;
    if (open) renderModal();
  };

  // Time controls: a pill floating over the bottom of the 3D view.
  const control = (name, label) => el('button', { className: 'round', title: label, ariaLabel: label }, icon(name));
  const slower = control('slower', 'Slow time down');
  const pause = control('pause', 'Pause');
  pause.classList.add('primary');
  const faster = control('faster', 'Speed time up');
  const realTime = el('button', { className: 'real-time', textContent: 'Real time', title: 'Run at real time' });
  const clock = el('span', { className: 'clock' });
  const speed = el('span', { className: 'speed' });
  const bar = el('footer', { id: 'time-bar', className: 'panel floating' },
    el('div', { className: 'readout' }, clock, speed), slower, pause, faster, realTime);
  // Where you are: list and page, view mode with page arrows, selected task.
  const wherePage = el('span', { className: 'meta' });
  const viewSelect = dropdown('View mode', VIEWS, view => send({ type: 'set_view', view }));
  const pageBack = round('back', 'Previous page', 'round small');
  const pageOn = round('chevron', 'Next page', 'round small');
  const frame = round('frame', 'Frame orbital system', 'round small');
  const whereTask = el('span', { className: 'name' });
  const framePlanet = round('frame', 'Frame this planet', 'round small');
  const taskDot = el('span', { className: 'dot' });
  const where = el('div', { id: 'where' },
    el('div', { className: 'panel floating pill view-pill' }, pageBack, viewSelect, wherePage, pageOn, frame),
    el('div', { className: 'panel floating pill task-pill' },
      el('button', { className: 'details', title: 'Show task details' }, taskDot, whereTask), framePlanet));
  const renderWhere = () => {
    const pages = state.pages || [];
    const current = Math.max(0, pages.indexOf(state.page));
    // A picked star is any galactic star; only a list page has a system number.
    const picked = state.picked;
    const system = picked?.page != null ? `System ${picked.page + 1} / ${picked.pages}` : '';
    wherePage.textContent = picked ? system
      : state.system ? `System ${current + 1} / ${Math.max(pages.length, 1)}` : '';
    // The view menu is labelled with where you are; the menu marks the view.
    viewSelect.set(state.view);
    const name = picked?.name ?? state.system?.name ?? 'Galaxy';
    wherePage.closest('.view-pill').classList.toggle('picked', !!picked);
    frame.title = picked ? `Visit ${picked.name}${system ? `, ${system.toLowerCase()}` : ''}` : 'Frame orbital system';
    frame.disabled = !picked && !state.system;
    viewSelect.querySelector('.dropdown span').textContent = name;
    viewSelect.querySelector('.dropdown').title = `${name} · change view`;
    pageBack.disabled = current <= 0;
    pageOn.disabled = current + 1 >= pages.length;
    pageBack.onclick = () => send({ type: 'show_page', page: pages[current - 1] });
    pageOn.onclick = () => send({ type: 'show_page', page: pages[current + 1] });
    const task = state.selected;
    whereTask.textContent = task?.name ?? 'No planet selected';
    whereTask.closest('.task-pill').classList.toggle('none', !task);
    whereTask.parentElement.disabled = !task;
    framePlanet.hidden = !task;
    taskDot.style.color = task ? rgb(task.colour) : '';
  };
  const viewCaption = el('div', { className: 'caption top' });
  const rangeCaption = el('div', { className: 'caption bottom' });
  const empty = el('div', { id: 'empty-universe', hidden: true },
    el('h3', { textContent: 'Loading your universe' }),
    el('p', { className: 'muted', textContent: 'Systems appear as each list finishes loading.' }));
  hud.append(nav, opener, gear, settings, where, modal, bar, viewCaption, rangeCaption, empty);

  // Rows are reused while their list is unchanged: lists may arrive one by
  // one, and a rebuilt row would swallow a click in progress.
  const listRows = new Map();
  const listRow = list => {
    const key = `${list.name}\n${list.tasks.length}`;
    const cached = listRows.get(list.id);
    if (cached?.key === key) return cached.row;
    const jump = el('button', { className: 'item', title: list.name },
      el('span', { textContent: list.name }),
      el('small', { textContent: `${list.tasks.length} task planets` }));
    jump.addEventListener('click', () => send({ type: 'jump', list: list.id }));
    const open = round('chevron', `Show tasks in ${list.name}`, 'round small open-tasks');
    open.addEventListener('click', () => {
      if (list.id !== state.list) send({ type: 'jump', list: list.id });
      showTab('tasks');
    });
    const row = el('div', { className: 'list-row' }, jump, open);
    listRows.set(list.id, { key, row });
    return row;
  };
  const renderLists = () => {
    const query = listFilter.value.trim().toLowerCase();
    const shown = galaxy.lists.filter(list => !query || list.name.toLowerCase().includes(query));
    if (prefs.listSort === 'name') shown.sort((a, b) => a.name.localeCompare(b.name));
    if (prefs.listSort === 'tasks') shown.sort((a, b) => b.tasks.length - a.tasks.length);
    const rows = shown.map(listRow);
    if (rows.length !== lists.children.length || rows.some((row, i) => lists.children[i] !== row)) {
      lists.replaceChildren(...rows);
    }
    listsTab.querySelector('small').textContent = galaxy.lists.length;
    highlightLists();
  };
  const highlightLists = () => {
    for (const [id, { row }] of listRows) row.classList.toggle('active', id === state.list);
  };

  // Tasks of the current list, optionally grouped by status.
  const collapsed = new Set();
  const taskRow = ({ list, task }) => {
    const [due, dueClass] = task.due_date_ms != null ? dueShort(task.due_date_ms) : [null, ''];
    const row = el('button', { className: 'task', title: task.name },
      el('span', { className: 'dot' }), el('span', { className: 'name', textContent: task.name }),
      due ? el('span', { className: `meta ${dueClass}`, textContent: due }) : null);
    row.style.color = rgb(task.colour);
    row.dataset.list = list;
    row.dataset.task = task.id;
    row.classList.toggle('active', list === state.list && task.id === state.task);
    row.addEventListener('click', () => send({ type: 'find_task', list, task: task.id }));
    return row;
  };
  const groupRow = group => {
    const row = el('button', { className: 'group' }, icon('chevron'),
      el('span', { className: 'name', textContent: group.name }),
      el('span', { className: 'meta', textContent: group.tasks.length }));
    row.style.color = rgb(group.colour);
    row.classList.toggle('collapsed', collapsed.has(group.key));
    row.setAttribute('aria-expanded', !collapsed.has(group.key));
    row.addEventListener('click', () => {
      if (!collapsed.delete(group.key)) collapsed.add(group.key);
      renderIndex(true, true);
    });
    return row;
  };
  const rowKey = item => (item.group
    ? `group\n${item.key}\n${item.name}\n${item.colour}\n${item.tasks.length}\n${collapsed.has(item.key)}`
    : [item.list, item.task.id, item.task.name, item.task.colour, item.task.due_date_ms].join('\n'));
  const index = virtualList(taskBox, item => (item.group ? groupRow(item) : taskRow(item)), rowKey);
  let matches = [];
  let indexKey = null;
  const renderIndex = (force, keepScroll) => {
    const query = taskSearch.value.trim().toLowerCase();
    const key = [query, state.list, prefs.taskSort, prefs.group].join('\n');
    // Leave the rows in place when only the selection changed, so a click in
    // progress is not lost to a rebuilt row.
    if (!force && key === indexKey) {
      markActive();
      return;
    }
    indexKey = key;
    const current = galaxy.lists.find(list => list.id === state.list);
    matches = (current?.tasks ?? [])
      .filter(task => !query || task.name.toLowerCase().includes(query))
      .map(task => ({ list: current.id, task }));
    const due = item => item.task.due_date_ms ?? Number.MAX_SAFE_INTEGER;
    if (prefs.taskSort === 'name') matches.sort((a, b) => a.task.name.localeCompare(b.task.name));
    if (prefs.taskSort === 'due') matches.sort((a, b) => due(a) - due(b));
    const total = matches.length;
    let items = matches;
    if (prefs.group) {
      const groups = new Map();
      for (const item of matches) {
        const k = item.task.status.toLowerCase();
        if (!groups.has(k)) {
          groups.set(k, { group: true, key: k, name: item.task.status, colour: item.task.colour,
            rank: statusRank(item.task), order: item.task.status_order ?? 0, tasks: [] });
        }
        groups.get(k).tasks.push(item);
      }
      items = [...groups.values()]
        .sort((a, b) => a.rank - b.rank || a.order - b.order || a.name.localeCompare(b.name))
        .flatMap(group => [group, ...(collapsed.has(group.key) ? [] : group.tasks)]);
      // Enter picks the first task the user can see.
      matches = items.filter(item => !item.group);
    }
    taskTitle.textContent = current?.name ?? 'No list selected';
    tasksTab.querySelector('small').textContent = current?.tasks.length ?? '';
    const folded = total - matches.length;
    taskCount.textContent = `${total} task${total === 1 ? '' : 's'}${query ? ' matching' : ''}${folded ? ` · ${folded} folded` : ''}`;
    index.set(items, keepScroll);
    markActive();
  };
  const markActive = () => {
    for (const row of taskBox.querySelectorAll('.task')) {
      row.classList.toggle('active', row.dataset.list === state.list && row.dataset.task === state.task);
    }
  };

  const facts = rows => el('dl', {}, ...rows.filter(([, value]) => value != null)
    .flatMap(([label, value]) => [el('dt', { textContent: label }), el('dd', { textContent: value })]));
  const renderModal = () => {
    const task = state.selected;
    if (!task) return setModal(false);
    const system = state.system;
    const key = JSON.stringify([task, system, state.page]);
    if (modal.hidden || key === modalKey) return;
    modalKey = key;
    const star = system?.star_ly.map(v => (v >= 0 ? '+' : '') + v.toFixed(2)).join('  ');
    // A planet without a task has no status, description or task facts.
    const tasked = task.status != null;
    modalBody.replaceChildren(...[
      tasked ? el('p', { className: 'status', textContent: task.status.toUpperCase(), style: `color: ${rgb(task.colour)}` }) : null,
      el('h3', { textContent: task.name }),
      tasked ? el('p', { className: task.description ? 'description' : 'muted', textContent: task.description || 'No description.' })
        : el('p', { className: 'muted', textContent: 'No task on this planet.' }),
      el('div', { className: 'columns' },
        tasked ? el('section', {}, heading('Task'), facts([
          ['Assigned', task.assignees.length ? task.assignees.join(', ') : 'Nobody'],
          ['Due', task.due_date_ms != null ? dueText(task.due_date_ms) : 'No due date'],
          ['List', system?.name],
        ])) : null,
        el('section', {}, heading('Planet'), facts([
          ['Designation', tasked ? task.designation : null],
          ['Type', task.class],
          ['Mass', massText(task.mass_earth)],
          ['Physical radius', `${task.radius_km.toFixed(0)} km`],
          ['Semi-major axis', `${task.semi_major_au.toFixed(3)} AU`],
          ['Eccentricity', task.eccentricity.toFixed(3)],
          ['Orbital period', periodText(task.period_days)],
          ['Rotation', task.spin.locked ? `Tidally locked · ${periodText(task.period_days)}` : rotationText(task.spin.period_hours)],
          ['Axial tilt', tiltText(task.spin.tilt_rad)],
          ['Resonance', task.resonance ? `${task.resonance[0]}:${task.resonance[1]} with inner neighbour` : 'None'],
        ])),
        system?.star ? el('section', {}, heading('Star'), facts([
          ['Type', `${system.star.kind} · ${system.star.spectral}`],
          ['Mass', `${system.star.mass_solar.toFixed(2)} solar masses`],
          ['Radius', `${starRadiusText(system.star.radius_solar)}`],
          ['Temperature', `${Math.round(system.star.temperature_k).toLocaleString('en-GB')} K`],
          ['Luminosity', `${luminosityText(system.star.luminosity_solar)} solar`],
          ['Rotation', rotationText(system.star.spin.period_hours)],
          ['Axial tilt', tiltText(system.star.spin.tilt_rad)],
        ])) : null,
        system ? el('section', {}, heading('Host system'), facts([
          ['Page', `System ${state.page + 1} · ${system.page_tasks} of ${system.tasks} tasks`],
          ['Star position', `${star} ly`],
          ['Orbital plane', `tilt ${system.tilt_deg.toFixed(0)}°, node ${system.node_deg.toFixed(0)}°`],
        ])) : null),
      task.url ? el('div', { className: 'row actions' },
        el('a', { href: task.url, target: '_blank', rel: 'noopener noreferrer', textContent: 'Open task link ↗' })) : null,
    ].filter(Boolean));
  };

  const renderState = () => {
    highlightLists();
    renderIndex(false);
    renderWhere();
    renderModal();
    for (const input of settings.querySelectorAll('.toggle input')) {
      input.checked = !!state.display?.[input.dataset.option];
      input.disabled = ['galaxy_glow', 'physical', 'wireframe'].includes(input.dataset.option) && !state.display?.realistic;
    }
    // Leave a value being typed alone until it is committed.
    const degrees = Math.round(state.fov_deg ?? 50);
    fovRange.value = degrees;
    if (document.activeElement !== fovNumber) fovNumber.value = degrees;
    const step = rateStep(state.rate);
    slower.disabled = step === 0;
    faster.disabled = step === RATES.length - 1;
    realTime.classList.toggle('active', state.rate === 1);
    speed.textContent = state.paused ? 'Paused' : RATES[step][0];
    const label = state.paused ? 'Resume' : 'Pause';
    pause.replaceChildren(icon(state.paused ? 'play' : 'pause'));
    pause.title = label;
    pause.ariaLabel = label;
    notice.textContent = state.notice || '';
    viewCaption.textContent = caption(state);
  };

  frame.addEventListener('click', () => send({ type: 'frame_system' }));
  whereTask.parentElement.addEventListener('click', () => setModal(true));
  framePlanet.addEventListener('click', () => send({ type: 'follow' }));
  closeModal.addEventListener('click', () => setModal(false));
  modal.addEventListener('pointerdown', event => {
    if (event.target === modal) setModal(false);
  });
  pause.addEventListener('click', () => send({ type: 'set_paused', paused: !state.paused }));
  const stepRate = by => send({ type: 'set_rate', rate: RATES[rateStep(state.rate) + by][1] });
  slower.addEventListener('click', () => stepRate(-1));
  faster.addEventListener('click', () => stepRate(1));
  realTime.addEventListener('click', () => send({ type: 'set_rate', rate: 1 }));
  listFilter.addEventListener('input', renderLists);
  hide.addEventListener('click', () => setOpen(false));
  opener.addEventListener('click', () => setOpen(true));
  gear.addEventListener('click', () => setSettings(settings.hidden));
  closeSettings.addEventListener('click', () => setSettings(false));
  // Capture, because the canvas swallows pointer events for the 3D controls.
  document.addEventListener('pointerdown', event => {
    if (!settings.hidden && !settings.contains(event.target) && !gear.contains(event.target)) setSettings(false);
  }, true);
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
      setSettings(false);
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
  bridge.on('tick', ({ days, range: metres, exposure, set_speed, velocity, stream }) => {
    clock.textContent = elapsed(days);
    const range = metres > LY * 0.01 ? `${(metres / LY).toFixed(3)} ly` : `${(metres / AU).toFixed(4)} AU`;
    const mode = state.following ? 'tracking' : state.view !== 'orbit' ? 'auto-framed preset' : 'free navigation';
    rangeCaption.textContent = `Set speed ${set_speed} / Velocity ${velocity}\n${exposure}\nCamera range ${range} / ${mode}\n${stream}`;
  });

  // Every panel floats over the full-window 3D view. Reporting the insets also
  // tells Rust to skip its egui panels.
  send({ type: 'panels', top: 0, right: 0, bottom: 0, left: 0 });
  showTab(prefs.tab);
  setOpen(prefs.open);
}
