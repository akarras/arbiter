//! The replay list page served at `/`.

use std::fmt::Write as _;
use std::path::PathBuf;
use std::time::UNIX_EPOCH;

use crate::chart::escape_html;
use crate::percent;
use crate::scan::ReplayEntry;
use crate::theme;

pub fn render(entries: &[ReplayEntry], roots: &[PathBuf]) -> String {
    let mut html = String::new();
    html.push_str("<!doctype html>\n<html lang=\"en\">\n<head>\n<meta charset=\"utf-8\">\n");
    html.push_str("<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n");
    let _ = write!(html, "<title>Arbiter replays</title>\n<style>{}{}</style>\n</head>\n<body>\n", theme::CSS, LIST_CSS);
    html.push_str("<main class=\"viz-root\">\n<header>\n<h1>Replays</h1>\n");
    let _ = writeln!(html, "<p class=\"meta\">{} replay{} in:</p>", entries.len(), if entries.len() == 1 { "" } else { "s" });
    html.push_str("<ul class=\"roots\">\n");
    for r in roots {
        let _ = writeln!(html, "<li>{}</li>", escape_html(&r.display().to_string()));
    }
    html.push_str("</ul>\n</header>\n");
    html.push_str("<section class=\"tools\">\n<label>Open a replay from anywhere: <input type=\"file\" id=\"file\" accept=\".SC2Replay\"></label> <button id=\"open\" type=\"button\">Chart it</button> <span id=\"status\" class=\"status\"></span>\n");
    html.push_str("<input id=\"filter\" type=\"search\" placeholder=\"Filter by name\" autocomplete=\"off\">\n</section>\n");
    if entries.is_empty() {
        html.push_str("<p class=\"empty\">No replays found in the folders above. Pass <code>--dir</code> to serve another folder, or use Open above.</p>\n");
    } else {
        html.push_str("<table>\n<thead><tr><th>Replay</th><th>Modified</th><th>Size</th></tr></thead>\n<tbody>\n");
        for e in entries {
            let name = e.path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
            let ts = e.modified.duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
            let _ = writeln!(
                html,
                "<tr data-name=\"{}\"><td><a href=\"/replay?path={}\">{}</a></td><td data-ts=\"{}\"></td><td>{} KB</td></tr>",
                escape_html(&name.to_lowercase()),
                percent::encode(&e.path.to_string_lossy()),
                escape_html(&name),
                ts,
                (e.size + 512) / 1024
            );
        }
        html.push_str("</tbody>\n</table>\n");
    }
    html.push_str("</main>\n");
    let _ = write!(html, "<script>{}</script>\n</body>\n</html>\n", LIST_JS);
    html
}

const LIST_CSS: &str = r#"
.roots{margin:0 0 16px;padding-left:20px;color:var(--text-2);font-size:13px}
.tools{max-width:1040px;margin:0 auto 16px;display:flex;flex-wrap:wrap;gap:12px 16px;align-items:center}
.tools input[type=search]{margin-left:auto;padding:6px 10px;border:1px solid var(--border);border-radius:6px;background:var(--surface);color:var(--text);min-width:240px}
button{padding:6px 12px;border:1px solid var(--border);border-radius:6px;background:var(--surface);color:var(--text);cursor:pointer}
.status{color:var(--text-2);font-size:13px}
table{max-width:1040px;margin:0 auto;width:100%}
th,td{text-align:left}
td:nth-child(3),th:nth-child(3){text-align:right}
.empty{max-width:1040px;margin:32px auto;color:var(--text-2)}
"#;

const LIST_JS: &str = r#"
(() => {
  document.querySelectorAll('[data-ts]').forEach(td => {
    const ts = +td.dataset.ts;
    if (ts) td.textContent = new Date(ts * 1000).toLocaleString(undefined, {dateStyle: 'medium', timeStyle: 'short'});
  });
  const filter = document.getElementById('filter');
  const rows = [...document.querySelectorAll('tbody tr')];
  filter.addEventListener('input', () => {
    const q = filter.value.toLowerCase();
    rows.forEach(r => { r.hidden = !r.dataset.name.includes(q); });
  });
  const file = document.getElementById('file');
  const status = document.getElementById('status');
  document.getElementById('open').addEventListener('click', async () => {
    const f = file.files[0];
    if (!f) { status.textContent = 'Choose a .SC2Replay first.'; return; }
    status.textContent = 'Parsing ' + f.name + '…';
    try {
      const res = await fetch('/open', {method: 'POST', body: f});
      const text = await res.text();
      if (!res.ok) { status.textContent = 'Error: ' + text; return; }
      document.open(); document.write(text); document.close();
    } catch (e) {
      status.textContent = 'Request failed: ' + e;
    }
  });
})();
"#;

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, UNIX_EPOCH};

    fn entry(name: &str, secs: u64, size: u64) -> ReplayEntry {
        ReplayEntry { path: PathBuf::from("C:/r").join(name), modified: UNIX_EPOCH + Duration::from_secs(secs), size }
    }

    #[test]
    fn lists_entries_with_escaped_names_encoded_links_and_timestamps() {
        let html = render(&[entry("Tuonela LE (1).SC2Replay", 1_700_000_000, 227_146), entry("<b>.SC2Replay", 5, 2048)], &[PathBuf::from("C:/r")]);
        assert!(html.contains("2 replays"));
        assert!(html.contains("Tuonela LE (1).SC2Replay"));
        assert!(html.contains("&lt;b&gt;.SC2Replay"));
        assert!(!html.contains("<b>.SC2Replay"));
        assert!(html.contains(r#"href="/replay?path=C%3A%2Fr%2FTuonela%20LE%20%281%29.SC2Replay""#) || html.contains(r#"href="/replay?path=C%3A%2Fr%5CTuonela%20LE%20%281%29.SC2Replay""#));
        assert!(html.contains(r#"data-ts="1700000000""#));
        assert!(html.contains("222 KB"));
        assert!(html.contains(r#"data-name="tuonela le (1).sc2replay""#));
    }

    #[test]
    fn empty_state_names_the_roots() {
        let html = render(&[], &[PathBuf::from("C:/nowhere")]);
        assert!(html.contains("No replays found"));
        assert!(html.contains("C:/nowhere"));
        assert!(!html.contains("<tbody>"));
    }

    #[test]
    fn has_open_file_control_and_filter() {
        let html = render(&[], &[]);
        assert!(html.contains(r#"<input type="file" id="file" accept=".SC2Replay">"#));
        assert!(html.contains(r#"fetch('/open'"#));
        assert!(html.contains(r#"id="filter""#));
    }
}
