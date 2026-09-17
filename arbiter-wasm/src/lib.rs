//! WebAssembly exports for the website: parse replay bytes, return chart HTML.

use wasm_bindgen::prelude::*;

/// Parses a replay held in memory and returns the self-contained chart page.
/// Errors carry the same message the CLI prints.
#[wasm_bindgen]
pub fn chart_html(name: &str, bytes: &[u8]) -> Result<String, JsError> {
    arbiter::pipeline::chart_html_bytes(name, bytes).map_err(|e| JsError::new(&format!("{e:#}")))
}

/// The crate version, shown in the page footer.
#[wasm_bindgen]
pub fn version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}
