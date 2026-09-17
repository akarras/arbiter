import { supportsPicker, HandleSource, FileListSource, MockSource, remember, remembered, reopen } from './source.js';

const status = document.getElementById('status');
const list = document.getElementById('list');
const filter = document.getElementById('filter');
const count = document.getElementById('count');
const sourceEl = document.getElementById('source');
const warning = document.getElementById('warning');
const versionEl = document.getElementById('version');
const frame = document.getElementById('chart');
const chooseBtn = document.getElementById('choose');
const reopenBtn = document.getElementById('reopen');
const openBtn = document.getElementById('open');
const dirInput = document.getElementById('dir-input');
const fileInput = document.getElementById('file-input');

let source = null;
let sourceGeneration = 0;
let rows = [];
let filtered = [];
let selected = null;
let selectTimer = null;
let refreshTimer = null;
let lastSignature = '';
let lastNames = '';
let refreshToken = 0;
let refreshing = 0;
let loadToken = 0;

const fmtDate = ms => ms ? new Date(ms).toLocaleString(undefined, {dateStyle: 'medium', timeStyle: 'short'}) : '';
const fmtSize = n => Math.round(n / 1024) + ' KB';

function showStatus(text, isError) { status.textContent = text; status.classList.toggle('error', !!isError); status.hidden = false; }
function showWarning(text) { warning.textContent = text; warning.hidden = !text; }

// ---- parser worker ------------------------------------------------------
let worker = null, nextId = 1, inflight = new Map();
function startWorker() {
  worker = new Worker(new URL('./worker.js', import.meta.url), {type: 'module'});
  worker.onmessage = e => {
    if (e.data.type === 'ready') { versionEl.textContent = 'v' + e.data.version; return; }
    if (e.data.type === 'init-error') {
      const msg = 'The parser could not load: ' + e.data.error;
      showStatus(msg, true);
      showWarning(msg);
      return;
    }
    const p = inflight.get(e.data.id);
    if (e.data.fatal) {
      inflight.delete(e.data.id);
      if (p) p.reject(new Error(e.data.error));
      for (const other of inflight.values()) other.reject(new Error('the parser crashed'));
      inflight.clear();
      worker.terminate();
      startWorker();
      return;
    }
    if (!p) return;
    inflight.delete(e.data.id);
    e.data.error ? p.reject(new Error(e.data.error)) : p.resolve(e.data.html);
  };
  worker.onerror = () => {
    for (const p of inflight.values()) p.reject(new Error('the parser crashed on this file'));
    inflight.clear();
    worker.terminate();
    startWorker();
  };
}
function parse(name, bytes) {
  return new Promise((resolve, reject) => {
    const id = nextId++;
    inflight.set(id, {resolve, reject});
    worker.postMessage({id, name, bytes}, [bytes]);
  });
}
startWorker();

// ---- virtual list (same as the desktop app) ----------------------------
const ROW = 48, BUFFER = 8;
let paintQueued = false, lastQuery = '';
function makeRow(r) {
  const li = document.createElement('li');
  li.dataset.id = r.id;
  if (r.id === selected) li.classList.add('selected');
  const name = document.createElement('span'); name.className = 'name'; name.textContent = r.name;
  const info = document.createElement('span'); info.className = 'info'; info.textContent = r.info;
  li.append(name, info);
  li.addEventListener('click', () => select(r.id));
  return li;
}
function spacer(px) { const li = document.createElement('li'); li.className = 'spacer'; li.style.height = px + 'px'; return li; }
function paint() {
  paintQueued = false;
  const first = Math.max(0, Math.floor(list.scrollTop / ROW) - BUFFER);
  const last = Math.min(filtered.length, Math.ceil((list.scrollTop + list.clientHeight) / ROW) + BUFFER);
  const frag = document.createDocumentFragment();
  frag.appendChild(spacer(first * ROW));
  for (let i = first; i < last; i++) frag.appendChild(makeRow(filtered[i]));
  frag.appendChild(spacer(Math.max(0, filtered.length - last) * ROW));
  list.replaceChildren(frag);
}
function schedulePaint() { if (paintQueued) return; paintQueued = true; requestAnimationFrame(paint); }
function render() {
  const q = filter.value.trim().toLowerCase();
  filtered = q ? rows.filter(r => r.name.toLowerCase().includes(q)) : rows;
  if (q !== lastQuery) list.scrollTop = 0;
  lastQuery = q;
  count.textContent = filtered.length === rows.length ? rows.length + ' replays' : filtered.length + ' of ' + rows.length + ' replays';
  paint();
}
function highlight(id) { selected = id; for (const li of list.children) li.classList.toggle('selected', li.dataset.id === id); }
list.addEventListener('scroll', schedulePaint);
window.addEventListener('resize', schedulePaint);
filter.addEventListener('input', render);

// ---- sources ------------------------------------------------------------
async function useSource(s) {
  source = s;
  sourceGeneration++;
  sourceEl.textContent = 'Folder: ' + s.name;
  showWarning('');
  // A same-named file in a different folder must not keep the old chart or
  // selection, and stale poll signatures must not suppress the first load.
  selected = null;
  frame.srcdoc = '';
  lastSignature = '';
  lastNames = '';
  filter.value = '';
  showStatus('Select a replay');
  if (s.canRemember) await remember(s.handle);
  reopenBtn.hidden = true;
  await refresh(true);
  if (refreshTimer) clearInterval(refreshTimer);
  refreshTimer = setInterval(() => { if (!document.hidden) refresh(false); }, 10000);
}

async function refresh(force) {
  // Snapshot the source and its generation so a switch that happens while
  // this call is awaiting can't mix results from two different folders.
  const src = source;
  const gen = sourceGeneration;
  if (!src) return;
  // The reentrancy guard only throttles timer ticks; a forced refresh (from
  // useSource) must always run, and a stale in-flight tick for the old
  // source bails on the generation check below instead of being dropped.
  if (!force && refreshing) return;
  const mine = ++refreshToken;
  refreshing = mine;
  try {
    // Cheap check first: on an unchanged folder this avoids reading every
    // file's metadata on every 10s poll tick.
    let names;
    try { names = (await src.names()).join('|'); }
    catch (e) { if (gen === sourceGeneration) showWarning('Could not read the folder: ' + e.message); return; }
    if (gen !== sourceGeneration) return;
    if (!force && names === lastNames) return;
    lastNames = names;

    let entries;
    try { entries = await src.list(); }
    catch (e) {
      // A failed list() must not permanently wedge lastNames at a value that
      // matches next tick's cheap names() check, or the retry above would
      // never re-run list() again.
      if (gen === sourceGeneration) { showWarning('Could not read the folder: ' + e.message); lastNames = ''; }
      return;
    }
    if (gen !== sourceGeneration) return;
    entries.sort((a, b) => b.modified - a.modified);
    const signature = entries.map(e => e.id + ':' + e.modified + ':' + e.size).join('|');
    if (!force && signature === lastSignature) return;
    lastSignature = signature;
    rows = entries.map(e => ({...e, info: fmtDate(e.modified) + ' · ' + fmtSize(e.size)}));
    if (selected && !rows.some(r => r.id === selected)) { selected = null; frame.srcdoc = ''; showStatus('Select a replay'); }
    render();
    if (!rows.length) showStatus('No .SC2Replay files found in ' + src.name + '.');
    else if (!selected && !frame.srcdoc) showStatus('Select a replay');
  } finally {
    if (refreshing === mine) refreshing = 0;
  }
}

async function loadEntry(entry) {
  // Two quick selections can both be in flight; only the most recent one may
  // touch the DOM, or a slow first parse could clobber a faster second pick.
  const mine = ++loadToken;
  showStatus('Parsing ' + entry.name + '…');
  frame.srcdoc = '';
  try {
    const bytes = await entry.bytes();
    const html = await parse(entry.name, bytes);
    if (mine !== loadToken) return;
    frame.srcdoc = html;
    status.hidden = true;
  } catch (e) {
    if (mine !== loadToken) return;
    showStatus('Could not chart this replay.\n' + e.message, true);
  }
}

function select(id) {
  highlight(id);
  if (selectTimer) { clearTimeout(selectTimer); selectTimer = null; }
  const entry = rows.find(r => r.id === id);
  if (entry) loadEntry(entry);
}

document.addEventListener('keydown', e => {
  if (e.key !== 'ArrowDown' && e.key !== 'ArrowUp') return;
  if (document.activeElement === filter) return;
  if (!filtered.length) return;
  let i = filtered.findIndex(r => r.id === selected);
  i = e.key === 'ArrowDown' ? Math.min(i + 1, filtered.length - 1) : Math.max(i - 1, 0);
  e.preventDefault();
  if (i * ROW < list.scrollTop) list.scrollTop = i * ROW;
  else if ((i + 1) * ROW > list.scrollTop + list.clientHeight) list.scrollTop = (i + 1) * ROW - list.clientHeight;
  paint();
  highlight(filtered[i].id);
  if (selectTimer) clearTimeout(selectTimer);
  const entry = filtered[i];
  selectTimer = setTimeout(() => { selectTimer = null; loadEntry(entry); }, 150);
});

chooseBtn.addEventListener('click', async () => {
  if (supportsPicker) {
    try {
      const handle = await window.showDirectoryPicker({mode: 'read'});
      await useSource(new HandleSource(handle));
    } catch (e) {
      if (e.name !== 'AbortError') showWarning('Could not open the folder: ' + e.message);
    }
  } else {
    dirInput.click();
  }
});
dirInput.addEventListener('change', async () => {
  if (!dirInput.files.length) return;
  const name = (dirInput.files[0].webkitRelativePath || '').split('/')[0] || 'chosen folder';
  await useSource(new FileListSource(dirInput.files, name));
  dirInput.value = '';
});
reopenBtn.addEventListener('click', async () => {
  const handle = await remembered();
  if (!handle) { reopenBtn.hidden = true; return; }
  const s = await reopen(handle);
  if (s) await useSource(s); else showWarning('Permission to read ' + handle.name + ' was not granted. Choose the folder again.');
});
openBtn.addEventListener('click', () => fileInput.click());
fileInput.addEventListener('change', async () => {
  const f = fileInput.files[0];
  if (!f) return;
  selected = null; highlight(null);
  await loadEntry({name: f.name, bytes: () => f.arrayBuffer()});
  fileInput.value = '';
});

// ---- startup ------------------------------------------------------------
(async () => {
  if (new URLSearchParams(location.search).get('mock') === '1') { await useSource(new MockSource()); return; }
  if (!supportsPicker) sourceEl.textContent = 'This browser cannot remember a folder; you will pick it each visit.';
  const handle = supportsPicker ? await remembered() : null;
  if (handle) { reopenBtn.textContent = 'Reopen ' + handle.name; reopenBtn.hidden = false; }
})();
