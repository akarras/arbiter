// Folder sources: where the replay list comes from. Every source exposes
// { kind, name, canRemember, list() } where list() resolves to entries of
// { id, name, size, modified, bytes() }.

export const supportsPicker = typeof window.showDirectoryPicker === 'function';
const isReplay = name => /\.sc2replay$/i.test(name);
const MAX_DEPTH = 32;

async function walk(dir, prefix, depth, out) {
  if (depth > MAX_DEPTH) return;
  for await (const [name, handle] of dir.entries()) {
    if (handle.kind === 'directory') await walk(handle, prefix + name + '/', depth + 1, out);
    else if (isReplay(name)) out.push({handle, id: prefix + name, name});
  }
}

export class HandleSource {
  constructor(handle) { this.kind = 'handle'; this.handle = handle; this.name = handle.name; this.canRemember = true; }
  async list() {
    const raw = [];
    await walk(this.handle, '', 0, raw);
    const entries = [];
    for (const r of raw) {
      const f = await r.handle.getFile();
      entries.push({id: r.id, name: r.name, size: f.size, modified: f.lastModified, bytes: () => r.handle.getFile().then(x => x.arrayBuffer())});
    }
    return entries;
  }
}

export class FileListSource {
  constructor(files, name) { this.kind = 'files'; this.files = [...files]; this.name = name; this.canRemember = false; }
  async list() {
    return this.files.filter(f => isReplay(f.name)).map(f => ({
      id: f.webkitRelativePath || f.name, name: f.name, size: f.size, modified: f.lastModified, bytes: () => f.arrayBuffer(),
    }));
  }
}

// Automation only (?mock=1): entries come from mock/manifest.json on the same origin.
export class MockSource {
  constructor() { this.kind = 'mock'; this.name = 'mock folder'; this.canRemember = false; }
  async list() {
    const manifest = await (await fetch('mock/manifest.json', {cache: 'no-store'})).json();
    return manifest.map(e => ({id: e.file, name: e.name, size: e.size, modified: e.modified, bytes: async () => (await fetch('mock/' + e.file)).arrayBuffer()}));
  }
}

const DB = 'arbiter', STORE = 'handles', KEY = 'replays';
function openDb() {
  return new Promise((res, rej) => {
    const r = indexedDB.open(DB, 1);
    r.onupgradeneeded = () => r.result.createObjectStore(STORE);
    r.onsuccess = () => res(r.result);
    r.onerror = () => rej(r.error);
  });
}
export async function remember(handle) {
  try {
    const db = await openDb();
    await new Promise((res, rej) => { const tx = db.transaction(STORE, 'readwrite'); tx.objectStore(STORE).put(handle, KEY); tx.oncomplete = res; tx.onerror = () => rej(tx.error); });
  } catch (e) { console.warn('could not remember folder', e); }
}
export async function remembered() {
  try {
    const db = await openDb();
    return await new Promise((res, rej) => { const q = db.transaction(STORE).objectStore(STORE).get(KEY); q.onsuccess = () => res(q.result || null); q.onerror = () => rej(q.error); });
  } catch { return null; }
}
export async function reopen(handle) {
  const opts = {mode: 'read'};
  if (await handle.queryPermission(opts) === 'granted' || await handle.requestPermission(opts) === 'granted') return new HandleSource(handle);
  return null;
}
