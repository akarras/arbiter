//! Shared page chrome: color tokens, body/header/table styles, and link
//! styling. Used by both the chart page (`chart::render`) and the server's
//! replay-listing page (`list_page::render`). Chart-only rules live alongside
//! `chart::render`; list-page-only rules alongside `list_page::render`.

pub const CSS: &str = r#"
.viz-root{color-scheme:light;
--surface:#fcfcfb;--page:#f9f9f7;--text:#0b0b0b;--text-2:#52514e;--muted:#898781;--grid:#e1e0d9;--axis:#c3c2b7;--border:rgba(11,11,11,.10);
--series-1:#2a78d6;--series-2:#eb6834;--series-3:#1baf7a;--series-4:#eda100;--series-5:#e87ba4;--series-6:#008300;--series-7:#4a3aa7;--series-8:#e34948}
@media (prefers-color-scheme:dark){:root:where(:not([data-theme="light"])) .viz-root{color-scheme:dark;
--surface:#1a1a19;--page:#0d0d0d;--text:#fff;--text-2:#c3c2b7;--muted:#898781;--grid:#2c2c2a;--axis:#383835;--border:rgba(255,255,255,.10);
--series-1:#3987e5;--series-2:#d95926;--series-3:#199e70;--series-4:#c98500;--series-5:#d55181;--series-6:#008300;--series-7:#9085e9;--series-8:#e66767}}
:root[data-theme="dark"] .viz-root{color-scheme:dark;
--surface:#1a1a19;--page:#0d0d0d;--text:#fff;--text-2:#c3c2b7;--muted:#898781;--grid:#2c2c2a;--axis:#383835;--border:rgba(255,255,255,.10);
--series-1:#3987e5;--series-2:#d95926;--series-3:#199e70;--series-4:#c98500;--series-5:#d55181;--series-6:#008300;--series-7:#9085e9;--series-8:#e66767}
html,body{margin:0}
body{font-family:system-ui,-apple-system,"Segoe UI",sans-serif;font-size:14px}
.viz-root{background:var(--page);color:var(--text);min-height:100vh;padding:24px 16px;box-sizing:border-box}
header{max-width:1040px;margin:0 auto}
header h1{font-size:20px;font-weight:600;margin:0 0 4px}
.meta{color:var(--text-2);margin:0 0 16px}
a{color:var(--series-1)}
.back{margin:0 0 8px;font-size:13px}
.table{max-width:1040px;margin:16px auto 0;color:var(--text-2)}
.table summary{cursor:pointer}
table{border-collapse:collapse;margin-top:8px;font-variant-numeric:tabular-nums}
th,td{text-align:right;padding:2px 10px;border-bottom:1px solid var(--grid)}
th:first-child,td:first-child{text-align:left}
"#;
