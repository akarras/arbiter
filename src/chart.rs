//! Renders the interactive HTML page: header, legend, one section per panel
//! with a data table, the per-player detail section, and a JSON block the
//! inline JS (in `chart_assets`) draws from.

use std::fmt::Write as _;

use crate::apm::Point;
use crate::chart_assets::{CHART_CSS, JS};
use crate::theme;

pub enum PanelKind {
    Lines,
}

pub struct PanelSeries {
    pub player: usize,
    pub label: String,
    pub points: Vec<Point>,
}

pub struct Panel {
    pub id: String,
    pub title: String,
    pub unit: String,
    pub kind: PanelKind,
    pub series: Vec<PanelSeries>,
}

/// One player's breakdown: APM by kind (stacked), EPM, supply-block bands.
pub struct Detail {
    pub player: usize,
    pub breakdown: Vec<PanelSeries>,
    pub epm: Vec<Point>,
    pub blocks: Vec<(f64, f64)>,
}

pub struct PlayerLegend {
    pub name: String,
    pub race: String,
    pub result: String,
    pub average: f64,
    pub game_apm: Option<f64>,
}

pub struct Chart {
    pub title: String,
    pub map: String,
    pub duration_secs: f64,
    pub players: Vec<PlayerLegend>,
    pub panels: Vec<Panel>,
    pub details: Vec<Detail>,
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
    let mut html = String::new();
    html.push_str("<!doctype html>\n<html lang=\"en\">\n<head>\n<meta charset=\"utf-8\">\n");
    html.push_str("<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n");
    let _ = write!(html, "<title>{}</title>\n<style>{}{}</style>\n</head>\n<body>\n", escape_html(&chart.title), theme::CSS, CHART_CSS);
    html.push_str("<main class=\"viz-root\">\n<header>\n");
    let _ = writeln!(html, "<h1>{}</h1>", escape_html(&chart.map));
    let _ = writeln!(
        html,
        "<p class=\"meta\">Match length {} &middot; {} players &middot; drag to zoom, double-click to reset</p>\n</header>",
        fmt_time(chart.duration_secs),
        chart.players.len()
    );
    render_legend(&mut html, chart);
    for panel in &chart.panels {
        render_panel(&mut html, panel);
    }
    render_detail(&mut html, chart);
    html.push_str("</main>\n");
    let _ = writeln!(html, "<script id=\"data\" type=\"application/json\">{}</script>", render_json(chart));
    let _ = write!(html, "<script>{}</script>\n</body>\n</html>\n", JS);
    html
}

fn render_legend(html: &mut String, chart: &Chart) {
    html.push_str("<ul class=\"legend\">\n");
    for (i, p) in chart.players.iter().enumerate() {
        let game = match p.game_apm {
            Some(g) => format!(" &middot; game says {g:.0}"),
            None => String::new(),
        };
        let _ = writeln!(
            html,
            "<li><span class=\"swatch\" style=\"background:var(--series-{})\"></span><span class=\"name\">{}</span><span class=\"sub\">{} &middot; {} &middot; {:.0} APM{}</span></li>",
            i % 8 + 1,
            escape_html(&p.name),
            escape_html(&p.race),
            escape_html(&p.result),
            p.average,
            game
        );
    }
    html.push_str("</ul>\n");
}

fn render_panel(html: &mut String, panel: &Panel) {
    let has_points = panel.series.iter().any(|s| !s.points.is_empty());
    let _ = writeln!(html, "<section class=\"panel\" id=\"panel-{}\">", escape_html(&panel.id));
    let _ = writeln!(html, "<div class=\"head\"><h2>{}</h2><p class=\"unit\">{}</p></div>", escape_html(&panel.title), escape_html(&panel.unit));
    if has_points {
        let _ = writeln!(
            html,
            "<div class=\"plot\"><svg id=\"svg-{0}\" viewBox=\"0 0 960 260\" role=\"img\" aria-label=\"{1}\"></svg><div id=\"tip-{0}\" class=\"tooltip\" hidden></div></div>",
            escape_html(&panel.id),
            escape_html(&panel.title)
        );
        render_table(html, &panel.series);
    } else {
        html.push_str("<p class=\"empty\">No tracker data in this replay.</p>\n");
    }
    html.push_str("</section>\n");
}

fn render_detail(html: &mut String, chart: &Chart) {
    if chart.details.is_empty() {
        return;
    }
    html.push_str("<section class=\"panel\" id=\"panel-detail\">\n<div class=\"head\"><h2>Player detail</h2><p class=\"unit\">APM by input kind, EPM (dashed), supply blocks (shaded)</p>");
    html.push_str("<select id=\"detail-player\">");
    for d in &chart.details {
        let name = chart.players.get(d.player).map(|p| p.name.as_str()).unwrap_or("?");
        let _ = write!(html, "<option value=\"{}\">{}</option>", d.player, escape_html(name));
    }
    html.push_str("</select></div>\n");
    html.push_str("<div class=\"plot\"><svg id=\"svg-detail\" viewBox=\"0 0 960 260\" role=\"img\" aria-label=\"Player detail\"></svg><div id=\"tip-detail\" class=\"tooltip\" hidden></div></div>\n");
    html.push_str("<ul class=\"klegend\">");
    for (i, label) in crate::metrics::KIND_LABELS.iter().enumerate() {
        let _ = write!(html, "<li><span class=\"swatch\" style=\"background:var(--series-{})\"></span>{}</li>", i + 1, escape_html(label));
    }
    html.push_str("<li><span class=\"swatch\" style=\"background:var(--text-2)\"></span>EPM</li></ul>\n</section>\n");
}

/// Rows follow the sorted union of every series' sample times (deduplicated
/// within 0.05 s), matching the JS tooltip's merged time axis rather than
/// aligning series by index (which prints the wrong time, and `0:00`, for
/// any series shorter than the first). Each cell is the series' nearest
/// sample by time, left empty when that nearest sample is further away
/// than half the series' own median sample spacing.
fn render_table(html: &mut String, series: &[PanelSeries]) {
    let times = merged_times(series);
    html.push_str("<details class=\"table\">\n<summary>Data table</summary>\n<table>\n<tr><th>Time</th>");
    for s in series {
        let _ = write!(html, "<th>{}</th>", escape_html(&s.label));
    }
    html.push_str("</tr>\n");
    let half_spacings: Vec<f64> = series.iter().map(|s| median_spacing(&s.points) / 2.0).collect();
    for &t in &times {
        let _ = write!(html, "<tr><td>{}</td>", fmt_time(t));
        for (s, &half_spacing) in series.iter().zip(&half_spacings) {
            match nearest_within(&s.points, t, half_spacing) {
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

/// Sorted union of every series' sample times, deduplicated within 0.05 s.
fn merged_times(series: &[PanelSeries]) -> Vec<f64> {
    let mut times: Vec<f64> = series.iter().flat_map(|s| s.points.iter().map(|p| p.secs)).collect();
    times.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let mut out: Vec<f64> = Vec::with_capacity(times.len());
    for t in times {
        if out.last().is_none_or(|&last| (t - last).abs() > 0.05) {
            out.push(t);
        }
    }
    out
}

/// Median gap between consecutive sample times. A series with fewer than
/// two points has no gap to measure, so it falls back to 5 s, the panels'
/// own sample interval (`apm::STEP_SECS`).
fn median_spacing(points: &[Point]) -> f64 {
    if points.len() < 2 {
        return 5.0;
    }
    let mut gaps: Vec<f64> = points.windows(2).map(|w| w[1].secs - w[0].secs).collect();
    gaps.sort_by(|a, b| a.partial_cmp(b).unwrap());
    gaps[gaps.len() / 2]
}

/// The point in `points` nearest `t` by time, or `None` when the nearest one
/// is still further than `max_dist` away (or `points` is empty).
fn nearest_within(points: &[Point], t: f64, max_dist: f64) -> Option<&Point> {
    points.iter().min_by(|a, b| (a.secs - t).abs().partial_cmp(&(b.secs - t).abs()).unwrap()).filter(|p| (p.secs - t).abs() <= max_dist)
}

fn write_points(json: &mut String, points: &[Point]) {
    json.push('[');
    for (j, p) in points.iter().enumerate() {
        if j > 0 {
            json.push(',');
        }
        let _ = write!(json, "[{:.1},{:.1}]", p.secs, p.value);
    }
    json.push(']');
}

fn write_series(json: &mut String, series: &[PanelSeries]) {
    json.push('[');
    for (i, s) in series.iter().enumerate() {
        if i > 0 {
            json.push(',');
        }
        let _ = write!(json, "{{\"player\":{},\"label\":\"{}\",\"points\":", s.player, escape_json(&s.label));
        write_points(json, &s.points);
        json.push('}');
    }
    json.push(']');
}

fn render_json(chart: &Chart) -> String {
    let mut json = String::new();
    let _ = write!(json, "{{\"duration\":{:.1},\"players\":[", chart.duration_secs);
    for (i, p) in chart.players.iter().enumerate() {
        if i > 0 {
            json.push(',');
        }
        let _ = write!(json, "{{\"name\":\"{}\"}}", escape_json(&p.name));
    }
    json.push_str("],\"panels\":[");
    for (i, p) in chart.panels.iter().enumerate() {
        if i > 0 {
            json.push(',');
        }
        let kind = match p.kind {
            PanelKind::Lines => "lines",
        };
        let _ = write!(json, "{{\"id\":\"{}\",\"title\":\"{}\",\"unit\":\"{}\",\"kind\":\"{}\",\"series\":", escape_json(&p.id), escape_json(&p.title), escape_json(&p.unit), kind);
        write_series(&mut json, &p.series);
        json.push('}');
    }
    json.push_str("],\"details\":[");
    for (i, d) in chart.details.iter().enumerate() {
        if i > 0 {
            json.push(',');
        }
        let _ = write!(json, "{{\"player\":{},\"breakdown\":", d.player);
        write_series(&mut json, &d.breakdown);
        json.push_str(",\"epm\":");
        write_points(&mut json, &d.epm);
        json.push_str(",\"blocks\":[");
        for (j, (a, b)) in d.blocks.iter().enumerate() {
            if j > 0 {
                json.push(',');
            }
            let _ = write!(json, "[{a:.1},{b:.1}]");
        }
        json.push_str("]}");
    }
    json.push_str("]}");
    json
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::apm::Point;

    fn pts(vals: &[f64]) -> Vec<Point> {
        vals.iter().enumerate().map(|(i, &v)| Point { secs: (i as f64 + 1.0) * 5.0, value: v }).collect()
    }

    fn legend(name: &str) -> PlayerLegend {
        PlayerLegend { name: name.to_string(), race: "Zerg".to_string(), result: "Win".to_string(), average: 123.4, game_apm: None }
    }

    fn chart(players: Vec<&str>, panels: Vec<Panel>, details: Vec<Detail>) -> Chart {
        Chart {
            title: "APM".to_string(),
            map: "Tuonela LE".to_string(),
            duration_secs: 15.0,
            players: players.into_iter().map(legend).collect(),
            panels,
            details,
        }
    }

    fn panel(id: &str, series: Vec<(usize, &str, Vec<Point>)>) -> Panel {
        Panel {
            id: id.to_string(),
            title: id.to_uppercase(),
            unit: "per min".to_string(),
            kind: PanelKind::Lines,
            series: series.into_iter().map(|(player, label, points)| PanelSeries { player, label: label.to_string(), points }).collect(),
        }
    }

    #[test]
    fn escapes_html_special_characters() {
        assert_eq!(escape_html("<b>&\"'"), "&lt;b&gt;&amp;&quot;&#39;");
    }

    #[test]
    fn escapes_json_strings_and_angle_brackets() {
        assert_eq!(escape_json("a\"b\\c\n</script>"), "a\\\"b\\\\c\\n\\u003c/script>");
    }

    #[test]
    fn formats_time_as_minutes_and_seconds() {
        assert_eq!(fmt_time(65.0), "1:05");
    }

    #[test]
    fn renders_one_section_and_table_per_panel() {
        let c = chart(
            vec!["Bob", "Ann"],
            vec![
                panel("apm", vec![(0, "Bob", pts(&[60.0, 72.0])), (1, "Ann", pts(&[30.0, 36.0]))]),
                panel("income", vec![(0, "Bob", pts(&[800.0, 900.0])), (1, "Ann", pts(&[700.0, 750.0]))]),
            ],
            vec![],
        );
        let html = render(&c);
        assert!(html.starts_with("<!doctype html>"));
        assert_eq!(html.matches("<section class=\"panel\"").count(), 2);
        assert!(html.contains(r#"id="panel-apm""#));
        assert!(html.contains(r#"id="panel-income""#));
        assert_eq!(html.matches("<details class=\"table\">").count(), 2);
        assert!(html.contains("<td>0:05</td><td>60</td><td>30</td>"));
        assert!(html.contains("<td>0:05</td><td>800</td><td>700</td>"));
    }

    #[test]
    fn table_rows_follow_the_merged_time_axis_with_empty_cells_for_far_samples() {
        let series = vec![
            PanelSeries { player: 0, label: "Bob".to_string(), points: pts(&[10.0, 20.0, 30.0]) }, // secs 5, 10, 15
            PanelSeries { player: 1, label: "Ann".to_string(), points: pts(&[40.0, 50.0]) },        // secs 5, 10
        ];
        let p = Panel { id: "x".to_string(), title: "X".to_string(), unit: "u".to_string(), kind: PanelKind::Lines, series };
        let c = chart(vec!["Bob", "Ann"], vec![p], vec![]);
        let html = render(&c);
        assert_eq!(html.matches("<tr><td>").count(), 3, "three rows, one per merged time");
        assert!(html.contains("<tr><td>0:05</td><td>10</td><td>40</td></tr>"));
        assert!(html.contains("<tr><td>0:10</td><td>20</td><td>50</td></tr>"));
        assert!(html.contains("<tr><td>0:15</td><td>30</td><td></td></tr>"), "Ann has no sample near 0:15");
        assert!(!html.contains("<td>0:00</td>"), "no spurious row at time zero");
    }

    #[test]
    fn player_name_cannot_close_the_json_script_block() {
        let name = "x</script><script>alert(1)";
        let c = chart(vec![name], vec![], vec![]);
        let html = render(&c);
        let marker = r#"<script id="data" type="application/json">"#;
        let start = html.find(marker).unwrap() + marker.len();
        let end = start + html[start..].find("</script>").unwrap();
        let json: serde_json::Value =
            serde_json::from_str(&html[start..end]).expect("JSON block must parse; a real </script> in the name would truncate it early");
        assert_eq!(json["players"][0]["name"], name);
        assert!(!html[start..end].contains("</script>"), "raw </script> must never appear inside the JSON block");
    }

    #[test]
    fn player_name_with_bare_angle_brackets_leaves_no_raw_lt_in_the_json_block() {
        let name = "<!--<script";
        let c = chart(vec![name], vec![], vec![]);
        let html = render(&c);
        let marker = r#"<script id="data" type="application/json">"#;
        let start = html.find(marker).unwrap() + marker.len();
        let end = start + html[start..].find("</script>").unwrap();
        let json: serde_json::Value = serde_json::from_str(&html[start..end]).expect("JSON block must parse");
        assert_eq!(json["players"][0]["name"], name);
        assert!(!html[start..end].contains('<'), "no raw < inside the JSON block");
    }

    #[test]
    fn json_carries_panels_players_and_details() {
        let c = chart(
            vec!["<b>Bob"],
            vec![panel("apm", vec![(0, "<b>Bob", pts(&[60.0]))])],
            vec![Detail {
                player: 0,
                breakdown: vec![PanelSeries { player: 0, label: "Commands".to_string(), points: pts(&[40.0]) }],
                epm: pts(&[50.0]),
                blocks: vec![(5.0, 10.0)],
            }],
        );
        let html = render(&c);
        let start = html.find(r#"<script id="data" type="application/json">"#).unwrap();
        let end = start + html[start..].find("</script>").unwrap();
        let json: serde_json::Value = serde_json::from_str(&html[start + r#"<script id="data" type="application/json">"#.len()..end]).unwrap();
        assert_eq!(json["players"][0]["name"], "<b>Bob");
        assert_eq!(json["panels"][0]["id"], "apm");
        assert_eq!(json["panels"][0]["series"][0]["player"], 0);
        assert_eq!(json["panels"][0]["series"][0]["points"][0][1], 60.0);
        assert_eq!(json["details"][0]["blocks"][0][0], 5.0);
        assert_eq!(json["details"][0]["breakdown"][0]["label"], "Commands");
        assert_eq!(json["details"][0]["epm"][0][1], 50.0);
        assert!(!html[..start].contains("<b>Bob"), "raw name never in rendered HTML");
        assert!(!html[start..end].contains("<b>"), "JSON escapes every <");
    }

    #[test]
    fn detail_section_has_one_option_per_player() {
        let c = chart(vec!["Bob", "Ann"], vec![], vec![
            Detail { player: 0, breakdown: vec![], epm: vec![], blocks: vec![] },
            Detail { player: 1, breakdown: vec![], epm: vec![], blocks: vec![] },
        ]);
        let html = render(&c);
        assert!(html.contains(r#"<select id="detail-player">"#));
        assert_eq!(html.matches("<option value=").count(), 2);
        assert!(html.contains(r#"<option value="1">Ann</option>"#));
    }

    #[test]
    fn legend_shows_race_result_average_and_game_apm() {
        let mut l = legend("Bob");
        l.game_apm = Some(61.4);
        let c = Chart { title: "APM".to_string(), map: "M".to_string(), duration_secs: 1.0, players: vec![l], panels: vec![], details: vec![] };
        let html = render(&c);
        assert!(html.contains("Zerg &middot; Win &middot; 123 APM &middot; game says 61"));
        assert!(html.contains("--series-1"));
    }

    #[test]
    fn panel_without_points_renders_empty_note() {
        let c = chart(vec!["Bob"], vec![panel("income", vec![(0, "Bob", vec![])])], vec![]);
        let html = render(&c);
        assert!(html.contains("No tracker data in this replay."));
    }
}
