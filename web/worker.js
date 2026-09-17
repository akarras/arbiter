// Runs the wasm parser off the main thread. Messages in: {id, name, bytes};
// out: {type:'ready', version} once, {type:'init-error', error} if the wasm
// module fails to load, then {id, html} or {id, error[, fatal]}.
import init, { chart_html, version } from './pkg/arbiter_wasm.js';

const ready = init().then(() => { postMessage({type: 'ready', version: version()}); }).catch(e => {
  postMessage({type: 'init-error', error: String(e && e.message ? e.message : e)});
  throw e;
});

onmessage = async e => {
  const {id, name, bytes} = e.data;
  try {
    await ready;
    postMessage({id, html: chart_html(name, new Uint8Array(bytes))});
  } catch (err) {
    // On wasm32 a panic traps the instance into an unusable state, surfacing
    // here as a WebAssembly.RuntimeError ("unreachable"). Flag it as fatal so
    // the caller restarts the worker instead of reusing a crashed instance.
    const fatal = err instanceof WebAssembly.RuntimeError;
    postMessage({id, error: fatal ? 'the parser crashed on this file' : String(err && err.message ? err.message : err), fatal});
    if (fatal) close();
  }
};
