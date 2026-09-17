(() => {
  const t = window.__TAURI__;
  const bridge = t
    ? { invoke: t.core.invoke, listen: t.event.listen, open: t.dialog.open }
    : window.__MOCK__;

  const status = document.getElementById('status');

  if (!bridge) {
    status.textContent = 'Tauri bridge unavailable';
    status.hidden = false;
    return;
  }

  const list = document.getElementById('list');
  const filter = document.getElementById('filter');
  const count = document.getElementById('count');
  const rootsEl = document.getElementById('roots');
  const warning = document.getElementById('warning');
  const frame = document.getElementById('chart');

  let rows = [];
  let selected = null;
  let selectTimer = null;

  const fmtDate = s => s ? new Date(s * 1000).toLocaleString(undefined, {dateStyle: 'medium', timeStyle: 'short'}) : '';
  const fmtSize = n => Math.round(n / 1024) + ' KB';

  function showStatus(text, isError) {
    status.textContent = text;
    status.classList.toggle('error', !!isError);
    status.hidden = false;
  }

  function showWarning(text) {
    warning.textContent = text;
    warning.hidden = false;
  }

  // The list is virtualised: only the rows inside the scroll viewport (plus a
  // small buffer) exist in the DOM, with spacer elements holding the rest of
  // the scroll height. Thousands of live rows made the WebView's layer large
  // enough to stall the compositor while the window moved.
  const ROW = 48; // px, must match #list li height in app.css
  const BUFFER = 8;
  let filtered = [];
  let paintQueued = false;

  function makeRow(r) {
    const li = document.createElement('li');
    li.dataset.path = r.path;
    if (r.path === selected) li.classList.add('selected');
    const name = document.createElement('span');
    name.className = 'name';
    name.textContent = r.name;
    const info = document.createElement('span');
    info.className = 'info';
    info.textContent = r.info;
    li.append(name, info);
    li.addEventListener('click', () => select(r.path));
    return li;
  }

  function spacer(px) {
    const li = document.createElement('li');
    li.className = 'spacer';
    li.style.height = px + 'px';
    return li;
  }

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

  function schedulePaint() {
    if (paintQueued) return;
    paintQueued = true;
    requestAnimationFrame(paint);
  }

  let lastQuery = '';

  function render() {
    const q = filter.value.trim().toLowerCase();
    filtered = q ? rows.filter(r => r.name.toLowerCase().includes(q)) : rows;
    // A changed filter starts from the top; a reload keeps the scroll position.
    if (q !== lastQuery) list.scrollTop = 0;
    lastQuery = q;
    count.textContent = filtered.length === rows.length ? rows.length + ' replays' : filtered.length + ' of ' + rows.length + ' replays';
    paint();
  }

  list.addEventListener('scroll', schedulePaint);
  window.addEventListener('resize', schedulePaint);

  async function reload() {
    try {
      rows = await bridge.invoke('list_replays');
      // Format once per reload: toLocaleString costs ~0.04 ms per call, which
      // adds up over thousands of rows if done on every render.
      for (const r of rows) r.info = fmtDate(r.modified_secs) + ' · ' + fmtSize(r.size);
      const roots = await bridge.invoke('roots');
      rootsEl.textContent = roots.length ? roots.join('\n') : 'No replay folders found. Add one, or open a file.';
      if (selected && !rows.some(r => r.path === selected)) {
        selected = null;
        frame.srcdoc = '';
        showStatus('Select a replay');
      }
      render();
    } catch (e) {
      showWarning('Could not list replays: ' + e);
    }
  }

  async function loadSelected(path) {
    showStatus('Parsing…');
    frame.srcdoc = '';
    try {
      frame.srcdoc = await bridge.invoke('chart_html', {path});
      status.hidden = true;
    } catch (e) {
      showStatus('Could not chart this replay.\n' + e, true);
    }
  }

  // Move the highlight without rebuilding the list: with thousands of rows a
  // full render is a visible hitch on every click.
  function highlight(path) {
    selected = path;
    for (const li of list.children) li.classList.toggle('selected', li.dataset.path === path);
  }

  function select(path) {
    highlight(path);
    if (selectTimer) {
      clearTimeout(selectTimer);
      selectTimer = null;
    }
    loadSelected(path);
  }

  filter.addEventListener('input', render);

  document.addEventListener('keydown', e => {
    if (e.key !== 'ArrowDown' && e.key !== 'ArrowUp') return;
    if (document.activeElement === filter) return;
    if (!filtered.length) return;
    let i = filtered.findIndex(r => r.path === selected);
    i = e.key === 'ArrowDown' ? Math.min(i + 1, filtered.length - 1) : Math.max(i - 1, 0);
    e.preventDefault();
    // Keep the row inside the viewport, then repaint so it exists in the DOM.
    if (i * ROW < list.scrollTop) list.scrollTop = i * ROW;
    else if ((i + 1) * ROW > list.scrollTop + list.clientHeight) list.scrollTop = (i + 1) * ROW - list.clientHeight;
    paint();

    // Move the highlight instantly, but only fetch and render the chart
    // once the user has paused on a row for a bit, so holding the arrow
    // key down does not fire a `chart_html` parse per row.
    highlight(filtered[i].path);
    if (selectTimer) clearTimeout(selectTimer);
    const path = selected;
    selectTimer = setTimeout(() => { selectTimer = null; loadSelected(path); }, 150);
  });

  document.getElementById('open').addEventListener('click', async () => {
    const p = await bridge.open({multiple: false, filters: [{name: 'StarCraft II replay', extensions: ['SC2Replay']}]});
    if (typeof p === 'string') select(p);
  });

  document.getElementById('add').addEventListener('click', async () => {
    const p = await bridge.open({directory: true});
    if (typeof p !== 'string') return;
    try {
      await bridge.invoke('add_root', {path: p});
      await reload();
    } catch (e) {
      showWarning(String(e));
    }
  });

  bridge.listen('replays-changed', reload);
  bridge.listen('watcher-degraded', e => showWarning(e.payload));
  window.addEventListener('focus', reload);
  reload();
})();
