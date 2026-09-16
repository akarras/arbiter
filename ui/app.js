(() => {
  const t = window.__TAURI__;
  const bridge = t
    ? { invoke: t.core.invoke, listen: t.event.listen, open: t.dialog.open }
    : window.__MOCK__;

  const list = document.getElementById('list');
  const filter = document.getElementById('filter');
  const count = document.getElementById('count');
  const rootsEl = document.getElementById('roots');
  const frame = document.getElementById('chart');
  const status = document.getElementById('status');

  let rows = [];
  let selected = null;

  const fmtDate = s => s ? new Date(s * 1000).toLocaleString(undefined, {dateStyle: 'medium', timeStyle: 'short'}) : '';
  const fmtSize = n => Math.round(n / 1024) + ' KB';

  function showStatus(text, isError) {
    status.textContent = text;
    status.classList.toggle('error', !!isError);
    status.hidden = false;
  }

  function render() {
    const q = filter.value.trim().toLowerCase();
    list.textContent = '';
    let shown = 0;
    for (const r of rows) {
      if (q && !r.name.toLowerCase().includes(q)) continue;
      shown++;
      const li = document.createElement('li');
      li.dataset.path = r.path;
      if (r.path === selected) li.classList.add('selected');
      const name = document.createElement('span');
      name.className = 'name';
      name.textContent = r.name;
      const info = document.createElement('span');
      info.className = 'info';
      info.textContent = fmtDate(r.modified_secs) + ' · ' + fmtSize(r.size);
      li.append(name, info);
      li.addEventListener('click', () => select(r.path));
      list.appendChild(li);
    }
    count.textContent = shown === rows.length ? rows.length + ' replays' : shown + ' of ' + rows.length + ' replays';
  }

  async function reload() {
    try {
      rows = await bridge.invoke('list_replays');
      const roots = await bridge.invoke('roots');
      rootsEl.textContent = roots.length ? roots.join('\n') : 'No replay folders found. Add one, or open a file.';
      render();
    } catch (e) {
      showStatus('Could not list replays: ' + e, true);
    }
  }

  async function select(path) {
    selected = path;
    render();
    showStatus('Parsing…');
    frame.srcdoc = '';
    try {
      frame.srcdoc = await bridge.invoke('chart_html', {path});
      status.hidden = true;
    } catch (e) {
      showStatus('Could not chart this replay.\n' + e, true);
    }
  }

  filter.addEventListener('input', render);

  document.addEventListener('keydown', e => {
    if (e.key !== 'ArrowDown' && e.key !== 'ArrowUp') return;
    const visible = [...list.children];
    if (!visible.length) return;
    let i = visible.findIndex(li => li.dataset.path === selected);
    i = e.key === 'ArrowDown' ? Math.min(i + 1, visible.length - 1) : Math.max(i - 1, 0);
    e.preventDefault();
    visible[i].scrollIntoView({block: 'nearest'});
    select(visible[i].dataset.path);
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
      showStatus(String(e), true);
    }
  });

  bridge.listen('replays-changed', reload);
  window.addEventListener('focus', reload);
  reload();
})();
