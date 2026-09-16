//! Localhost web server: replay list, chart-on-click, chart-from-upload.

use std::io::Read as _;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use anyhow::{Result, anyhow};

use crate::{list_page, percent, pipeline, scan};

pub const HTML: &str = "text/html; charset=utf-8";
pub const TEXT: &str = "text/plain; charset=utf-8";
const MAX_UPLOAD: usize = 64 * 1024 * 1024;

pub struct Req<'a> {
    pub method: &'a str,
    pub path: &'a str,
    pub query: &'a str,
    pub body: &'a [u8],
    /// The `Host` request header, e.g. `"127.0.0.1:8321"` (empty if absent).
    /// Checked by `handle` to defend against DNS rebinding: even though the
    /// server only *binds* 127.0.0.1, an attacker's page can still point a
    /// hostname it controls at 127.0.0.1 and have a victim's browser send
    /// same-origin requests there, which the browser will happily let the
    /// page's script read the response to unless the server itself checks
    /// the `Host` header.
    pub host: &'a str,
}

pub struct Resp {
    pub status: u16,
    pub content_type: &'static str,
    pub body: Vec<u8>,
}

pub fn handle(req: &Req, roots: &[PathBuf]) -> Resp {
    if !host_is_allowed(req.host) {
        return text(403, "forbidden host");
    }
    match (req.method, req.path) {
        ("GET", "/") => html(list_page::render(&scan::find_replays(roots), roots)),
        ("GET", "/replay") => replay_route(req.query, roots),
        ("POST", "/open") => open_route(req.body),
        _ => text(404, "not found"),
    }
}

/// Rejects any `Host` header that is not (case-insensitively, ignoring any
/// `:port` suffix) `127.0.0.1`, `localhost`, or `[::1]`, to defend against
/// DNS rebinding: a hostname an attacker controls can be pointed at
/// 127.0.0.1, letting a victim's browser send same-origin requests to this
/// server despite the socket only being bound to loopback.
fn host_is_allowed(host: &str) -> bool {
    // A bracketed IPv6 literal (e.g. "[::1]:8321") has colons of its own, so
    // only strip a ":port" suffix that comes after the closing bracket;
    // anything else (IPv4, a hostname) has at most one colon, the port's.
    let host = if let Some(rest) = host.strip_prefix('[') {
        match rest.find(']') {
            Some(end) => &host[..end + 2], // include the leading '[' and the ']'
            None => host,
        }
    } else {
        host.rsplit_once(':').map_or(host, |(h, _)| h)
    };
    host.eq_ignore_ascii_case("127.0.0.1") || host.eq_ignore_ascii_case("localhost") || host.eq_ignore_ascii_case("[::1]")
}

fn replay_route(query: &str, roots: &[PathBuf]) -> Resp {
    let Some(raw) = query_param(query, "path") else {
        return text(400, "missing path parameter");
    };
    let Some(decoded) = percent::decode(raw) else {
        return text(400, "bad path encoding");
    };
    let path = PathBuf::from(decoded);
    if !path.is_file() {
        return text(404, "replay not found");
    }
    let Some(canon) = scan::is_within(roots, &path) else {
        return text(403, "path is outside the served replay folders");
    };
    match pipeline::chart_html(&canon, true) {
        Ok(page) => html(page),
        Err(e) => text(500, &format!("{e:#}")),
    }
}

fn open_route(body: &[u8]) -> Resp {
    if body.len() > MAX_UPLOAD {
        return text(413, "replay larger than 64 MiB");
    }
    if body.is_empty() {
        return text(400, "empty upload");
    }
    let tmp = std::env::temp_dir().join(format!("arbiter-open-{}-{}.SC2Replay", std::process::id(), next_id()));
    if let Err(e) = std::fs::write(&tmp, body) {
        return text(500, &format!("could not write temp file: {e}"));
    }
    let result = pipeline::chart_html(&tmp, true);
    let _ = std::fs::remove_file(&tmp);
    match result {
        Ok(page) => html(page),
        Err(e) => text(500, &format!("{e:#}")),
    }
}

/// Runs the server until the process is killed.
pub fn run(roots: Vec<PathBuf>, port: u16) -> Result<()> {
    let addr = format!("127.0.0.1:{port}");
    let server = tiny_http::Server::http(&addr).map_err(|e| anyhow!("could not listen on {addr}: {e}"))?;
    println!("Listening on http://{}", server.server_addr());
    for root in &roots {
        println!("  serving {}", root.display());
    }
    for mut request in server.incoming_requests() {
        if request.body_length().is_some_and(|n| n > MAX_UPLOAD) {
            respond(request, text(413, "replay larger than 64 MiB"));
            continue;
        }
        let mut body = Vec::new();
        let _ = request.as_reader().take(MAX_UPLOAD as u64 + 1).read_to_end(&mut body);
        let method = request.method().to_string();
        let url = request.url().to_string();
        let (path, query) = url.split_once('?').unwrap_or((&url, ""));
        let host = request.headers().iter().find(|h| h.field.equiv("Host")).map(|h| h.value.as_str()).unwrap_or("");
        let resp = handle(&Req { method: &method, path, query, body: &body, host }, &roots);
        respond(request, resp);
    }
    Ok(())
}

fn respond(request: tiny_http::Request, resp: Resp) {
    let header = tiny_http::Header::from_bytes("Content-Type", resp.content_type).expect("static header is valid");
    let response = tiny_http::Response::from_data(resp.body).with_status_code(resp.status).with_header(header);
    let _ = request.respond(response);
}

fn query_param<'a>(query: &'a str, key: &str) -> Option<&'a str> {
    query.split('&').find_map(|pair| {
        let (k, v) = pair.split_once('=')?;
        (k == key).then_some(v)
    })
}

fn next_id() -> u64 {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    COUNTER.fetch_add(1, Ordering::Relaxed)
}

fn html(body: String) -> Resp {
    Resp { status: 200, content_type: HTML, body: body.into_bytes() }
}

fn text(status: u16, body: &str) -> Resp {
    Resp { status, content_type: TEXT, body: body.as_bytes().to_vec() }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    const FIXTURE: &str = r"the local fixture replay";

    fn get(path: &str, query: &str, roots: &[PathBuf]) -> Resp {
        handle(&Req { method: "GET", path, query, body: &[], host: "127.0.0.1:8321" }, roots)
    }

    fn temp_root(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("arbiter-serve-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn index_lists_replays_in_roots() {
        let root = temp_root("index");
        std::fs::write(root.join("game one.SC2Replay"), b"x").unwrap();
        let resp = get("/", "", std::slice::from_ref(&root));
        assert_eq!(resp.status, 200);
        assert_eq!(resp.content_type, HTML);
        let body = String::from_utf8(resp.body).unwrap();
        assert!(body.contains("game one.SC2Replay"));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn replay_route_rejects_bad_requests() {
        let root = temp_root("reject");
        let outside = std::env::temp_dir().join(format!("arbiter-serve-outside-{}.SC2Replay", std::process::id()));
        std::fs::write(&outside, b"x").unwrap();
        let roots = vec![root.clone()];
        assert_eq!(get("/replay", "", &roots).status, 400);
        assert_eq!(get("/replay", "path=%zz", &roots).status, 400);
        assert_eq!(get("/replay", &format!("path={}", crate::percent::encode(&root.join("missing.SC2Replay").to_string_lossy())), &roots).status, 404);
        assert_eq!(get("/replay", &format!("path={}", crate::percent::encode(&outside.to_string_lossy())), &roots).status, 403);
        assert_eq!(get("/nope", "", &roots).status, 404);
        assert_eq!(handle(&Req { method: "DELETE", path: "/", query: "", body: &[], host: "127.0.0.1:8321" }, &roots).status, 404);
        let _ = std::fs::remove_file(&outside);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn replay_route_charts_the_fixture() {
        let fixture = Path::new(FIXTURE);
        if !fixture.is_file() {
            eprintln!("skipping: fixture absent");
            return;
        }
        let roots = vec![fixture.parent().unwrap().to_path_buf()];
        let resp = get("/replay", &format!("path={}", crate::percent::encode(FIXTURE)), &roots);
        assert_eq!(resp.status, 200, "{}", String::from_utf8_lossy(&resp.body));
        let body = String::from_utf8(resp.body).unwrap();
        assert!(body.contains("All replays"), "chart pages served by the server carry the back link");
        assert!(body.contains("Tuonela LE"));
    }

    #[test]
    fn open_route_handles_garbage_and_leaves_no_temp_file() {
        let resp = handle(&Req { method: "POST", path: "/open", query: "", body: b"definitely not a replay", host: "127.0.0.1:8321" }, &[]);
        assert_eq!(resp.status, 500);
        assert!(String::from_utf8(resp.body).unwrap().contains("not a StarCraft II replay"));
        let leftovers = std::fs::read_dir(std::env::temp_dir()).unwrap().flatten().filter(|e| e.file_name().to_string_lossy().starts_with(&format!("arbiter-open-{}-", std::process::id()))).count();
        assert_eq!(leftovers, 0);
        assert_eq!(handle(&Req { method: "POST", path: "/open", query: "", body: &[], host: "127.0.0.1:8321" }, &[]).status, 400);
    }

    #[test]
    fn open_route_returns_413_for_a_body_over_the_upload_limit() {
        let body = vec![0u8; MAX_UPLOAD + 1];
        let resp = handle(&Req { method: "POST", path: "/open", query: "", body: &body, host: "127.0.0.1:8321" }, &[]);
        assert_eq!(resp.status, 413);
    }

    #[test]
    fn host_header_is_checked_against_loopback_names() {
        let roots: Vec<PathBuf> = vec![];
        let req = |host: &'static str| Req { method: "GET", path: "/", query: "", body: &[], host };
        assert_eq!(handle(&req("evil.example"), &roots).status, 403);
        assert_eq!(handle(&req(""), &roots).status, 403);
        assert_eq!(handle(&req("localhost"), &roots).status, 200);
        assert_eq!(handle(&req("LOCALHOST:8321"), &roots).status, 200);
        assert_eq!(handle(&req("127.0.0.1"), &roots).status, 200);
        assert_eq!(handle(&req("127.0.0.1:8321"), &roots).status, 200);
        assert_eq!(handle(&req("[::1]"), &roots).status, 200);
        assert_eq!(handle(&req("[::1]:8321"), &roots).status, 200);
    }

    #[test]
    fn query_param_finds_the_named_key() {
        assert_eq!(query_param("a=1&path=x%20y&b=2", "path"), Some("x%20y"));
        assert_eq!(query_param("a=1", "path"), None);
        assert_eq!(query_param("path", "path"), None);
    }
}
