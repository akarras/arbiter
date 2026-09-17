//! Stand-in for the `include_assets` crate.
//!
//! `s2protocol` uses `include_assets` in exactly one place: to embed its
//! StarCraft II balance-data JSON (unit and ability names) and read it back in
//! `read_balance_data_from_included_assets`. Arbiter never calls that path,
//! but the real crate drags in `zstd-sys`, C code that needs `clang` to build
//! for WebAssembly. This crate provides the two items that code touches and
//! yields an empty archive, so every build (native and wasm) skips the C
//! dependency. Selected through `[patch.crates-io]` in the workspace manifest.

/// An archive with no entries.
pub struct NamedArchive;

impl NamedArchive {
    /// Accepts whatever `include_dir!` expands to and returns the empty archive.
    pub fn load<T>(_embedded: T) -> Self {
        NamedArchive
    }

    /// The archive's `(name, bytes)` entries: always none.
    pub fn assets(&self) -> impl Iterator<Item = (&str, &[u8])> {
        std::iter::empty()
    }
}

/// Expands to a unit value; the real macro embeds a directory at compile time.
#[macro_export]
macro_rules! include_dir {
    ($path:expr) => {
        ()
    };
}
