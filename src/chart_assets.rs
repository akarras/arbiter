//! Static CSS and JS for the chart page. Kept apart from the Rust assembly in
//! `chart.rs` so each file has one job.

pub const CHART_CSS: &str = r#"
figure{margin:0 auto;max-width:1040px;background:var(--surface);border:1px solid var(--border);border-radius:8px;padding:16px}
.legend{list-style:none;display:flex;flex-wrap:wrap;gap:8px 24px;margin:0 0 4px;padding:0;max-width:1040px;margin-left:auto;margin-right:auto}
.legend li{display:flex;align-items:center;gap:8px}
.swatch{width:12px;height:12px;border-radius:3px;display:inline-block;flex:none}
.legend .name{font-weight:600}
.legend .sub{color:var(--text-2)}
.panel{max-width:1040px;margin:16px auto 0;background:var(--surface);border:1px solid var(--border);border-radius:8px;padding:12px 16px}
.panel h2{font-size:14px;font-weight:600;margin:0}
.panel .unit{color:var(--muted);font-size:12px;margin:0 0 6px}
.panel .head{display:flex;align-items:baseline;gap:12px;flex-wrap:wrap}
.panel select{margin-left:auto;padding:4px 8px;border:1px solid var(--border);border-radius:6px;background:var(--page);color:var(--text)}
.plot{position:relative}
svg{width:100%;height:auto;display:block;user-select:none;cursor:crosshair}
.grid{stroke:var(--grid);stroke-width:1}
.axis{stroke:var(--axis);stroke-width:1}
.tick{fill:var(--muted);font-size:12px;font-variant-numeric:tabular-nums}
.line{fill:none;stroke-width:2;stroke-linejoin:round;stroke-linecap:round}
.area{stroke:none;opacity:.85}
.epm{fill:none;stroke:var(--text-2);stroke-width:2;stroke-dasharray:5 4}
.band{fill:var(--series-8);opacity:.14}
.label{fill:var(--text);font-size:12px}
.xhair{stroke:var(--muted);stroke-dasharray:3 3;pointer-events:none}
.dot{stroke:var(--surface);stroke-width:2;pointer-events:none}
.brush{fill:var(--series-1);opacity:.12;pointer-events:none}
.tooltip{position:absolute;top:8px;pointer-events:none;background:var(--surface);color:var(--text);border:1px solid var(--border);border-radius:6px;padding:6px 10px;font-size:12px;font-variant-numeric:tabular-nums;box-shadow:0 2px 8px rgba(0,0,0,.15);white-space:nowrap}
.tooltip .t{color:var(--text-2);margin-bottom:2px}
.tooltip .row{display:flex;align-items:center;gap:6px}
.klegend{display:flex;flex-wrap:wrap;gap:6px 16px;margin:6px 0 0;padding:0;list-style:none;font-size:12px;color:var(--text-2)}
.klegend li{display:flex;align-items:center;gap:6px}
.hint,.empty{color:var(--muted);font-size:12px;margin:8px 0 0;text-align:center}
.empty{font-size:14px;padding:32px 0}
.table{margin:8px 0 0;color:var(--text-2)}
.table summary{cursor:pointer;font-size:12px}
table{border-collapse:collapse;margin-top:8px;font-variant-numeric:tabular-nums;font-size:12px}
th,td{text-align:right;padding:2px 10px;border-bottom:1px solid var(--grid)}
th:first-child,td:first-child{text-align:left}
"#;

pub const JS: &str = r#"
(() => {
  const dataEl = document.getElementById('data');
  if (!dataEl) return;
  const data = JSON.parse(dataEl.textContent);
  const NS = 'http://www.w3.org/2000/svg';
  const W = 960, H = 260, M = {top: 14, right: 110, bottom: 32, left: 60};
  const pw = W - M.left - M.right, ph = H - M.top - M.bottom;
  const colour = i => 'var(--series-' + (i % 8 + 1) + ')';
  const KIND_COLOURS = [1, 2, 3, 4, 5].map(colour);
  let view = {x0: 0, x1: data.duration};
  let detailPlayer = 0;
  const escapeHtml = s => s.replace(/[&<>"']/g, c => ({'&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;'}[c]));
  const fmtTime = s => { s = Math.round(s); return Math.floor(s / 60) + ':' + String(s % 60).padStart(2, '0'); };
  const niceStep = (range, target) => {
    const raw = Math.max(range, 1e-9) / target, pow = Math.pow(10, Math.floor(Math.log10(raw)));
    for (const m of [1, 2, 5, 10]) if (m * pow >= raw) return m * pow;
    return 10 * pow;
  };
  const timeStep = range => { for (const s of [5, 10, 15, 30, 60, 120, 300, 600, 900]) if (range / s <= 8) return s; return 1800; };
  const nearest = (pts, t) => {
    let best = null, bd = Infinity;
    for (const p of pts) { const d = Math.abs(p[0] - t); if (d < bd) { bd = d; best = p; } }
    return best;
  };

  // One drawable per <svg>: panels are line charts, the detail is stacked.
  const drawables = [];
  for (const p of data.panels) {
    const svg = document.getElementById('svg-' + p.id);
    if (svg) drawables.push({svg, tip: document.getElementById('tip-' + p.id), panel: p, kind: 'lines'});
  }
  const detailSvg = document.getElementById('svg-detail');
  if (detailSvg && data.details.length) drawables.push({svg: detailSvg, tip: document.getElementById('tip-detail'), kind: 'detail'});

  function el(svg, tag, attrs, text) {
    const e = document.createElementNS(NS, tag);
    for (const k in attrs) e.setAttribute(k, attrs[k]);
    if (text !== undefined) e.textContent = text;
    svg.appendChild(e);
    return e;
  }

  function frame(d, ymax) {
    const {svg} = d, {x0, x1} = view;
    const ystep = niceStep(ymax, 4), ytop = Math.max(ystep, Math.ceil(ymax / ystep) * ystep);
    const sx = t => M.left + (t - x0) / (x1 - x0) * pw;
    const sy = v => M.top + ph - v / ytop * ph;
    for (let v = 0; v <= ytop + 1e-9; v += ystep) {
      el(svg, 'line', {x1: M.left, x2: M.left + pw, y1: sy(v), y2: sy(v), class: 'grid'});
      el(svg, 'text', {x: M.left - 8, y: sy(v) + 4, class: 'tick', 'text-anchor': 'end'}, Math.round(v));
    }
    const ts = timeStep(x1 - x0);
    for (let t = Math.ceil(x0 / ts) * ts; t <= x1 + 1e-9; t += ts) {
      el(svg, 'line', {x1: sx(t), x2: sx(t), y1: M.top + ph, y2: M.top + ph + 5, class: 'axis'});
      el(svg, 'text', {x: sx(t), y: M.top + ph + 18, class: 'tick', 'text-anchor': 'middle'}, fmtTime(t));
    }
    el(svg, 'line', {x1: M.left, x2: M.left + pw, y1: M.top + ph, y2: M.top + ph, class: 'axis'});
    d.sx = sx; d.sy = sy;
  }

  function pathOf(pts, sx, sy) {
    return pts.map((p, j) => (j ? 'L' : 'M') + sx(p[0]).toFixed(1) + ' ' + sy(p[1]).toFixed(1)).join(' ');
  }

  function drawLines(d) {
    const {svg, panel} = d, {x0, x1} = view;
    const visible = panel.series.map(s => s.points.filter(p => p[0] >= x0 && p[0] <= x1));
    const ymax = Math.max(1, ...visible.flat().map(p => p[1]));
    frame(d, ymax);
    const labelYs = [];
    panel.series.forEach((s, i) => {
      const pts = visible[i];
      if (!pts.length) return;
      el(svg, 'path', {d: pathOf(pts, d.sx, d.sy), class: 'line', style: 'stroke:' + colour(s.player)});
      if (data.players.length <= 4) {
        const last = pts[pts.length - 1];
        let y = d.sy(last[1]) + 4;
        while (labelYs.some(o => Math.abs(o - y) < 14)) y += 14;
        labelYs.push(y);
        el(svg, 'text', {x: d.sx(last[0]) + 8, y, class: 'label'}, s.label);
      }
    });
    d.visible = visible;
  }

  function drawDetail(d) {
    const {svg} = d, {x0, x1} = view;
    const det = data.details[detailPlayer];
    const n = det.breakdown.length ? det.breakdown[0].points.length : 0;
    // cumulative stacks share the breakdown's sample times
    const cum = det.breakdown.map(() => []);
    for (let i = 0; i < n; i++) {
      let acc = 0;
      det.breakdown.forEach((s, k) => { acc += s.points[i][1]; cum[k].push([s.points[i][0], acc]); });
    }
    const top = cum.length ? cum[cum.length - 1] : [];
    const inView = p => p[0] >= x0 && p[0] <= x1;
    const ymax = Math.max(1, ...top.filter(inView).map(p => p[1]), ...det.epm.filter(inView).map(p => p[1]));
    frame(d, ymax);
    for (const [from, to] of det.blocks) {
      const a = Math.max(from, x0), b = Math.min(to, x1);
      if (b <= a) continue;
      el(svg, 'rect', {x: d.sx(a), y: M.top, width: d.sx(b) - d.sx(a), height: ph, class: 'band'});
    }
    for (let k = cum.length - 1; k >= 0; k--) {
      const upper = cum[k].filter(inView);
      if (!upper.length) continue;
      const lower = k ? cum[k - 1].filter(inView) : upper.map(p => [p[0], 0]);
      const dpath = pathOf(upper, d.sx, d.sy) + ' ' + lower.slice().reverse().map(p => 'L' + d.sx(p[0]).toFixed(1) + ' ' + d.sy(p[1]).toFixed(1)).join(' ') + ' Z';
      el(svg, 'path', {d: dpath, class: 'area', style: 'fill:' + KIND_COLOURS[k]});
    }
    const epm = det.epm.filter(inView);
    if (epm.length) el(svg, 'path', {d: pathOf(epm, d.sx, d.sy), class: 'epm'});
    d.visible = det.breakdown.map((s, k) => cum[k]).concat([det.epm]);
    d.labels = det.breakdown.map(s => s.label).concat(['EPM']);
  }

  function draw(d) {
    d.svg.innerHTML = '';
    if (d.kind === 'lines') drawLines(d); else drawDetail(d);
    el(d.svg, 'line', {class: 'xhair', y1: M.top, y2: M.top + ph, x1: 0, x2: 0, visibility: 'hidden'});
    el(d.svg, 'rect', {class: 'brush', y: M.top, height: ph, x: 0, width: 0});
    const hit = el(d.svg, 'rect', {x: M.left, y: M.top, width: pw, height: ph, fill: 'transparent'});
    hit.addEventListener('mousemove', e => onMove(d, e));
    hit.addEventListener('mouseleave', hideHover);
    hit.addEventListener('mousedown', e => onDown(d, e));
    d.svg.addEventListener('dblclick', () => { view = {x0: 0, x1: data.duration}; hideHover(); drawAll(); });
  }
  function drawAll() { drawables.forEach(draw); }

  const toSvgX = (svg, e) => {
    const pt = svg.createSVGPoint();
    pt.x = e.clientX; pt.y = e.clientY;
    return pt.matrixTransform(svg.getScreenCTM().inverse()).x;
  };
  const xToTime = x => view.x0 + (x - M.left) / pw * (view.x1 - view.x0);

  function onMove(d, e) {
    const t = xToTime(toSvgX(d.svg, e));
    const x = d.sx(t);
    for (const o of drawables) {
      const xh = o.svg.querySelector('.xhair');
      xh.setAttribute('x1', x); xh.setAttribute('x2', x); xh.setAttribute('visibility', 'visible');
      if (o !== d) o.tip.hidden = true;
    }
    let rows = '<div class="t">' + fmtTime(t) + '</div>';
    if (d.kind === 'lines') {
      d.panel.series.forEach((s, i) => {
        const p = nearest(d.visible[i], t);
        rows += '<div class="row"><span class="swatch" style="background:' + colour(s.player) + '"></span><span>' + escapeHtml(s.label) + '</span><span style="margin-left:auto;padding-left:12px">' + (p ? Math.round(p[1]) : '-') + '</span></div>';
      });
    } else {
      const det = data.details[detailPlayer];
      det.breakdown.forEach((s, k) => {
        const p = nearest(s.points, t);
        rows += '<div class="row"><span class="swatch" style="background:' + KIND_COLOURS[k] + '"></span><span>' + escapeHtml(s.label) + '</span><span style="margin-left:auto;padding-left:12px">' + (p ? Math.round(p[1]) : '-') + '</span></div>';
      });
      const p = nearest(det.epm, t);
      rows += '<div class="row"><span>EPM</span><span style="margin-left:auto;padding-left:12px">' + (p ? Math.round(p[1]) : '-') + '</span></div>';
    }
    d.tip.innerHTML = rows;
    d.tip.hidden = false;
    const rect = d.svg.getBoundingClientRect(), px = x / W * rect.width;
    d.tip.style.left = (px + 12 + d.tip.offsetWidth > rect.width ? px - 12 - d.tip.offsetWidth : px + 12) + 'px';
  }
  function hideHover() {
    for (const o of drawables) {
      o.tip.hidden = true;
      const xh = o.svg.querySelector('.xhair');
      if (xh) xh.setAttribute('visibility', 'hidden');
    }
  }
  function onDown(d, e) {
    if (e.button !== 0) return;
    e.preventDefault();
    const clamp = x => Math.min(Math.max(x, M.left), M.left + pw);
    const startX = clamp(toSvgX(d.svg, e));
    const brush = d.svg.querySelector('.brush');
    const move = ev => {
      const x = clamp(toSvgX(d.svg, ev));
      brush.setAttribute('x', Math.min(startX, x)); brush.setAttribute('width', Math.abs(x - startX));
    };
    const up = ev => {
      window.removeEventListener('mousemove', move); window.removeEventListener('mouseup', up);
      const x = clamp(toSvgX(d.svg, ev));
      brush.setAttribute('width', 0);
      if (Math.abs(x - startX) < 6) return;
      const a = xToTime(Math.min(startX, x)), b = xToTime(Math.max(startX, x));
      if (b - a < 10) return;
      view = {x0: a, x1: b};
      hideHover();
      drawAll();
    };
    window.addEventListener('mousemove', move);
    window.addEventListener('mouseup', up);
  }

  const sel = document.getElementById('detail-player');
  if (sel) sel.addEventListener('change', () => { detailPlayer = +sel.value; const d = drawables.find(o => o.kind === 'detail'); if (d) draw(d); });
  drawAll();
})();
"#;
