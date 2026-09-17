// Runs the wasm parser off the main thread. Messages in: {id, name, bytes};
// out: {type:'ready', version} once, then {id, html} or {id, error}.
import init, { chart_html, version } from './pkg/arbiter_wasm.js';

const ready = init().then(() => { postMessage({type: 'ready', version: version()}); });

onmessage = async e => {
  await ready;
  const {id, name, bytes} = e.data;
  try {
    postMessage({id, html: chart_html(name, new Uint8Array(bytes))});
  } catch (err) {
    postMessage({id, error: String(err && err.message ? err.message : err)});
  }
};
