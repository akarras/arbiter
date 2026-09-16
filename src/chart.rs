//! Renders the interactive HTML page. Rust emits the frame, legend, data table,
//! and a JSON block; the inline JS draws the SVG plot from that JSON so that the
//! initial draw and every zoom share one code path.

use std::fmt::Write as _;

use crate::apm::{Point, STEP_SECS};
use crate::theme;

pub struct Series {
    pub name: String,
    pub race: String,
    pub result: String,
    pub average: f64,
    pub game_apm: Option<f64>,
    pub points: Vec<Point>,
}

pub struct Chart {
    pub title: String,
    pub map: String,
    pub duration_secs: f64,
    pub series: Vec<Series>,
}

pub fn escape_html(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(c),
        }
    }
    out
}

/// Escapes a string for use inside a JSON string literal that lives in a
/// `<script>` block. Every `<` becomes `<` unconditionally, so no raw
/// `<` ever reaches the HTML parser and a name cannot close the block (this
/// covers `</script>` and any other tag, not just the `</` case).
/// `JSON.parse` decodes `<` back to the identical `<` character, so the
/// parsed string value is unchanged.
pub fn escape_json(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '<' => out.push_str("\\u003c"),
            c if (c as u32) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            _ => out.push(c),
        }
    }
    out
}

pub fn fmt_time(secs: f64) -> String {
    let total = secs.round() as i64;
    format!("{}:{:02}", total / 60, total % 60)
}

pub fn render(chart: &Chart) -> String {
    let has_points = chart.series.iter().any(|s| !s.points.is_empty());
    let mut html = String::new();
    html.push_str("<!doctype html>\n<html lang=\"en\">\n<head>\n<meta charset=\"utf-8\">\n");
    html.push_str("<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n");
    let _ = write!(
        html,
        "<title>{}</title>\n<style>{}{}</style>\n</head>\n<body>\n",
        escape_html(&chart.title),
        theme::CSS,
        CHART_CSS
    );
    html.push_str("<main class=\"viz-root\">\n<header>\n");
    let _ = writeln!(html, "<h1>{}</h1>", escape_html(&chart.map));
    let _ = writeln!(
        html,
        "<p class=\"meta\">Match length {} &middot; {} players &middot; trailing 60&#8201;s window</p>\n</header>",
        fmt_time(chart.duration_secs),
        chart.series.len()
    );
    html.push_str("<figure>\n<ul class=\"legend\">\n");
    for (i, s) in chart.series.iter().enumerate() {
        let game = match s.game_apm {
            Some(g) => format!(" &middot; game says {g:.0}"),
            None => String::new(),
        };
        let _ = writeln!(
            html,
            "<li><span class=\"swatch\" style=\"background:var(--series-{})\"></span><span class=\"name\">{}</span><span class=\"sub\">{} &middot; {} &middot; {:.0} APM{}</span></li>",
            i % 8 + 1,
            escape_html(&s.name),
            escape_html(&s.race),
            escape_html(&s.result),
            s.average,
            game
        );
    }
    html.push_str("</ul>\n");
    if has_points {
        html.push_str("<div class=\"plot\"><svg id=\"chart\" viewBox=\"0 0 960 420\" role=\"img\" aria-label=\"APM over time\"></svg><div id=\"tooltip\" class=\"tooltip\" hidden></div></div>\n");
        html.push_str("<p class=\"hint\">Drag to zoom &middot; double-click to reset</p>\n");
    } else {
        html.push_str("<p class=\"empty\">No player actions found in this replay.</p>\n");
    }
    html.push_str("</figure>\n");
    render_table(&mut html, chart);
    html.push_str("</main>\n");
    let _ = writeln!(
        html,
        "<script id=\"data\" type=\"application/json\">{}</script>",
        render_json(chart)
    );
    let _ = write!(html, "<script>{}</script>\n</body>\n</html>\n", JS);
    html
}

fn render_table(html: &mut String, chart: &Chart) {
    let rows = chart.series.iter().map(|s| s.points.len()).max().unwrap_or(0);
    html.push_str("<details class=\"table\">\n<summary>Data table</summary>\n<table>\n<tr><th>Time</th>");
    for s in &chart.series {
        let _ = write!(html, "<th>{}</th>", escape_html(&s.name));
    }
    html.push_str("</tr>\n");
    for row in 0..rows {
        let _ = write!(html, "<tr><td>{}</td>", fmt_time((row as f64 + 1.0) * STEP_SECS));
        for s in &chart.series {
            match s.points.get(row) {
                Some(p) => {
                    let _ = write!(html, "<td>{:.0}</td>", p.value);
                }
                None => html.push_str("<td></td>"),
            }
        }
        html.push_str("</tr>\n");
    }
    html.push_str("</table>\n</details>\n");
}

fn render_json(chart: &Chart) -> String {
    let mut json = String::new();
    let _ = write!(
        json,
        "{{\"duration\":{:.1},\"step\":{:.1},\"series\":[",
        chart.duration_secs, STEP_SECS
    );
    for (i, s) in chart.series.iter().enumerate() {
        if i > 0 {
            json.push(',');
        }
        let _ = write!(json, "{{\"name\":\"{}\",\"points\":[", escape_json(&s.name));
        for (j, p) in s.points.iter().enumerate() {
            if j > 0 {
                json.push(',');
            }
            let _ = write!(json, "[{:.1},{:.1}]", p.secs, p.value);
        }
        json.push_str("]}");
    }
    json.push_str("]}");
    json
}

const CHART_CSS: &str = r#"
figure{margin:0 auto;max-width:1040px;background:var(--surface);border:1px solid var(--border);border-radius:8px;padding:16px}
.legend{list-style:none;display:flex;flex-wrap:wrap;gap:8px 24px;margin:0 0 12px;padding:0}
.legend li{display:flex;align-items:center;gap:8px}
.swatch{width:12px;height:12px;border-radius:3px;display:inline-block;flex:none}
.legend .name{font-weight:600}
.legend .sub{color:var(--text-2)}
.plot{position:relative}
svg{width:100%;height:auto;display:block;user-select:none;cursor:crosshair}
.grid{stroke:var(--grid);stroke-width:1}
.axis{stroke:var(--axis);stroke-width:1}
.tick{fill:var(--muted);font-size:12px;font-variant-numeric:tabular-nums}
.line{fill:none;stroke-width:2;stroke-linejoin:round;stroke-linecap:round}
.label{fill:var(--text);font-size:12px}
.xhair{stroke:var(--muted);stroke-dasharray:3 3;pointer-events:none}
.dot{stroke:var(--surface);stroke-width:2;pointer-events:none}
.brush{fill:var(--series-1);opacity:.12;pointer-events:none}
.tooltip{position:absolute;top:8px;pointer-events:none;background:var(--surface);color:var(--text);border:1px solid var(--border);border-radius:6px;padding:6px 10px;font-size:12px;font-variant-numeric:tabular-nums;box-shadow:0 2px 8px rgba(0,0,0,.15);white-space:nowrap}
.tooltip .t{color:var(--text-2);margin-bottom:2px}
.tooltip .row{display:flex;align-items:center;gap:6px}
.hint,.empty{color:var(--muted);font-size:12px;margin:8px 0 0;text-align:center}
.empty{font-size:14px;padding:48px 0}
"#;

const JS: &str = r#"
(() => {
  const dataEl = document.getElementById('data');
  const svg = document.getElementById('chart');
  if (!dataEl || !svg) return;
  const data = JSON.parse(dataEl.textContent);
  const tip = document.getElementById('tooltip');
  const NS = 'http://www.w3.org/2000/svg';
  const W = 960, H = 420, M = {top: 16, right: 110, bottom: 36, left: 52};
  const pw = W - M.left - M.right, ph = H - M.top - M.bottom;
  const colour = i => 'var(--series-' + (i % 8 + 1) + ')';
  let view = {x0: 0, x1: data.duration};
  let scale = null;

  const fmtTime = s => { s = Math.round(s); return Math.floor(s / 60) + ':' + String(s % 60).padStart(2, '0'); };
  const niceStep = (range, target) => {
    const raw = range / target, pow = Math.pow(10, Math.floor(Math.log10(raw)));
    for (const m of [1, 2, 5, 10]) if (m * pow >= raw) return m * pow;
    return 10 * pow;
  };
  const timeStep = range => { for (const s of [5, 10, 15, 30, 60, 120, 300, 600, 900]) if (range / s <= 8) return s; return 1800; };
  const escapeHtml = s => s.replace(/[&<>"']/g, c => ({'&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;'}[c]));
  const el = (tag, attrs, text) => {
    const e = document.createElementNS(NS, tag);
    for (const k in attrs) e.setAttribute(k, attrs[k]);
    if (text !== undefined) e.textContent = text;
    svg.appendChild(e);
    return e;
  };

  function draw() {
    svg.innerHTML = '';
    const {x0, x1} = view;
    const visible = data.series.map(s => s.points.filter(p => p[0] >= x0 && p[0] <= x1));
    const ymax = Math.max(10, ...visible.flat().map(p => p[1]));
    const ystep = niceStep(ymax, 5), ytop = Math.ceil(ymax / ystep) * ystep;
    const sx = t => M.left + (t - x0) / (x1 - x0) * pw;
    const sy = v => M.top + ph - v / ytop * ph;
    scale = {sx, sy};
    for (let v = 0; v <= ytop + 1e-9; v += ystep) {
      el('line', {x1: M.left, x2: M.left + pw, y1: sy(v), y2: sy(v), class: 'grid'});
      el('text', {x: M.left - 8, y: sy(v) + 4, class: 'tick', 'text-anchor': 'end'}, v);
    }
    const ts = timeStep(x1 - x0);
    for (let t = Math.ceil(x0 / ts) * ts; t <= x1 + 1e-9; t += ts) {
      el('line', {x1: sx(t), x2: sx(t), y1: M.top + ph, y2: M.top + ph + 5, class: 'axis'});
      el('text', {x: sx(t), y: M.top + ph + 20, class: 'tick', 'text-anchor': 'middle'}, fmtTime(t));
    }
    el('line', {x1: M.left, x2: M.left + pw, y1: M.top + ph, y2: M.top + ph, class: 'axis'});
    const labelYs = [];
    data.series.forEach((s, i) => {
      const pts = visible[i];
      if (!pts.length) return;
      const d = pts.map((p, j) => (j ? 'L' : 'M') + sx(p[0]).toFixed(1) + ' ' + sy(p[1]).toFixed(1)).join(' ');
      el('path', {d, class: 'line', style: 'stroke:' + colour(i)});
      if (data.series.length <= 4) {
        const last = pts[pts.length - 1];
        let y = sy(last[1]) + 4;
        while (labelYs.some(o => Math.abs(o - y) < 14)) y += 14;
        labelYs.push(y);
        el('text', {x: sx(last[0]) + 8, y, class: 'label'}, s.name);
      }
    });
    el('line', {id: 'xhair', class: 'xhair', y1: M.top, y2: M.top + ph, x1: 0, x2: 0, visibility: 'hidden'});
    data.series.forEach((s, i) => el('circle', {id: 'dot' + i, class: 'dot', r: 4, fill: colour(i), visibility: 'hidden'}));
    el('rect', {id: 'brush', class: 'brush', y: M.top, height: ph, x: 0, width: 0});
    const hit = el('rect', {x: M.left, y: M.top, width: pw, height: ph, fill: 'transparent'});
    hit.addEventListener('mousemove', onMove);
    hit.addEventListener('mouseleave', hideHover);
    hit.addEventListener('mousedown', onDown);
  }

  const toSvgX = e => {
    const pt = svg.createSVGPoint();
    pt.x = e.clientX; pt.y = e.clientY;
    return pt.matrixTransform(svg.getScreenCTM().inverse()).x;
  };
  const xToTime = x => view.x0 + (x - M.left) / pw * (view.x1 - view.x0);

  function onMove(e) {
    const t = xToTime(toSvgX(e));
    const idx = Math.max(0, Math.round(t / data.step) - 1);
    const st = (idx + 1) * data.step;
    if (st < view.x0 || st > view.x1) return hideHover();
    const x = scale.sx(st);
    const xhair = document.getElementById('xhair');
    xhair.setAttribute('x1', x); xhair.setAttribute('x2', x); xhair.setAttribute('visibility', 'visible');
    let rows = '<div class="t">' + fmtTime(st) + '</div>';
    data.series.forEach((s, i) => {
      const p = s.points[idx];
      const dot = document.getElementById('dot' + i);
      if (p) {
        dot.setAttribute('cx', x); dot.setAttribute('cy', scale.sy(p[1])); dot.setAttribute('visibility', 'visible');
      } else dot.setAttribute('visibility', 'hidden');
      rows += '<div class="row"><span class="swatch" style="background:' + colour(i) + '"></span><span>' + escapeHtml(s.name) + '</span><span style="margin-left:auto;padding-left:12px">' + (p ? Math.round(p[1]) : '-') + '</span></div>';
    });
    tip.innerHTML = rows;
    tip.hidden = false;
    const rect = svg.getBoundingClientRect(), px = x / W * rect.width;
    tip.style.left = (px + 12 + tip.offsetWidth > rect.width ? px - 12 - tip.offsetWidth : px + 12) + 'px';
  }
  function hideHover() {
    tip.hidden = true;
    document.getElementById('xhair').setAttribute('visibility', 'hidden');
    data.series.forEach((s, i) => document.getElementById('dot' + i).setAttribute('visibility', 'hidden'));
  }

  function onDown(e) {
    if (e.button !== 0) return;
    e.preventDefault();
    const clamp = x => Math.min(Math.max(x, M.left), M.left + pw);
    const startX = clamp(toSvgX(e));
    const brush = document.getElementById('brush');
    const move = ev => {
      const x = clamp(toSvgX(ev));
      brush.setAttribute('x', Math.min(startX, x)); brush.setAttribute('width', Math.abs(x - startX));
    };
    const up = ev => {
      window.removeEventListener('mousemove', move); window.removeEventListener('mouseup', up);
      const x = clamp(toSvgX(ev));
      brush.setAttribute('width', 0);
      if (Math.abs(x - startX) < 6) return;
      const a = xToTime(Math.min(startX, x)), b = xToTime(Math.max(startX, x));
      if (b - a < data.step * 2) return;
      view = {x0: a, x1: b};
      hideHover();
      draw();
    };
    window.addEventListener('mousemove', move);
    window.addEventListener('mouseup', up);
  }
  svg.addEventListener('dblclick', () => { view = {x0: 0, x1: data.duration}; hideHover(); draw(); });
  draw();
})();
"#;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::apm::Point;

    fn series(name: &str, apms: &[f64]) -> Series {
        Series {
            name: name.to_string(),
            race: "Zerg".to_string(),
            result: "Win".to_string(),
            average: 123.4,
            game_apm: None,
            points: apms
                .iter()
                .enumerate()
                .map(|(i, &apm)| Point { secs: (i as f64 + 1.0) * 5.0, value: apm })
                .collect(),
        }
    }

    fn chart(series: Vec<Series>) -> Chart {
        Chart { title: "APM".to_string(), map: "Tuonela LE".to_string(), duration_secs: 15.0, series }
    }

    #[test]
    fn escapes_html_special_characters() {
        assert_eq!(escape_html("<b>&\"'"), "&lt;b&gt;&amp;&quot;&#39;");
    }

    #[test]
    fn escapes_json_strings_and_script_closers() {
        assert_eq!(escape_json("a\"b\\c\n</script>"), "a\\\"b\\\\c\\n\\u003c/script>");
    }

    #[test]
    fn formats_time_as_minutes_and_seconds() {
        assert_eq!(fmt_time(0.0), "0:00");
        assert_eq!(fmt_time(65.0), "1:05");
        assert_eq!(fmt_time(3599.6), "60:00");
    }

    #[test]
    fn output_is_a_full_document_with_escaped_names() {
        let html = render(&chart(vec![
            series("<b>Bob", &[60.0, 72.0, 84.0]),
            series("Ann", &[30.0, 36.0, 42.0]),
        ]));
        assert!(html.starts_with("<!doctype html>"));
        assert!(html.contains("<title>APM</title>"));
        assert!(html.contains("&lt;b&gt;Bob"), "legend must escape the name");
        let json_start = html.find(r#"<script id="data""#).unwrap();
        assert!(!html[..json_start].contains("<b>Bob"), "raw name must never appear in rendered HTML");
        assert!(html.contains(r#"<script id="data" type="application/json">"#));
        assert!(html.contains(r#""name":"Ann""#));
        assert!(html.contains("[5.0,60.0]"), "points are [secs, apm] pairs");
    }

    /// Slices out just the JSON payload text between the `<script>` tags,
    /// excluding the tags themselves (which legitimately contain `<`).
    fn json_payload(html: &str) -> &str {
        let open_tag_end = html.find(r#"<script id="data" type="application/json">"#).unwrap()
            + r#"<script id="data" type="application/json">"#.len();
        let close_tag_start = html[open_tag_end..].find("</script>").unwrap() + open_tag_end;
        &html[open_tag_end..close_tag_start]
    }

    #[test]
    fn json_block_cannot_be_closed_by_a_player_name() {
        let html = render(&chart(vec![series("x</script><script>alert(1)", &[1.0])]));
        let payload = json_payload(&html);
        assert!(payload.contains("x\\u003c/script>\\u003cscript>alert(1)"));
        assert!(!payload.contains('<'), "no raw < may survive in the JSON block: {payload}");
    }

    #[test]
    fn no_raw_angle_bracket_survives_html_comment_and_script_tricks() {
        let html = render(&chart(vec![series("<!--<script", &[1.0])]));
        let payload = json_payload(&html);
        assert!(!payload.contains('<'), "no raw < may survive in the JSON block: {payload}");
        assert!(payload.contains("\\u003c!--\\u003cscript"));
    }

    #[test]
    fn data_table_has_one_row_per_sample_and_one_column_per_player() {
        let html = render(&chart(vec![
            series("Bob", &[60.0, 72.0, 84.0]),
            series("Ann", &[30.0, 36.0, 42.0]),
        ]));
        assert_eq!(html.matches("<tr>").count(), 1 + 3, "header + 3 samples");
        assert!(html.contains("<th>Bob</th>"));
        assert!(html.contains("<th>Ann</th>"));
        assert!(html.contains("<td>0:05</td><td>60</td><td>30</td>"));
    }

    #[test]
    fn legend_shows_race_result_and_average() {
        let html = render(&chart(vec![series("Bob", &[60.0])]));
        assert!(html.contains("Zerg"));
        assert!(html.contains("Win"));
        assert!(html.contains("123 APM"));
        assert!(html.contains("--series-1"));
    }

    #[test]
    fn legend_shows_game_apm_when_present() {
        let mut s = series("Bob", &[60.0]);
        s.game_apm = Some(61.4);
        let html = render(&chart(vec![s]));
        assert!(html.contains("123 APM &middot; game says 61"));
        let html = render(&chart(vec![series("Ann", &[60.0])]));
        assert!(!html.contains("game says"));
    }

    #[test]
    fn empty_series_render_a_note_instead_of_the_hint() {
        let html = render(&chart(vec![series("Bob", &[]), series("Ann", &[])]));
        assert!(html.contains("No player actions found in this replay."));
        assert!(!html.contains("Drag to zoom"));
    }
}

