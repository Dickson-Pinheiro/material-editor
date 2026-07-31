//! WebAssembly entry points.
//!
//! Two targets, one engine:
//!
//! - [`browser`] — `wasm-bindgen`, for the editor and in-page PDF export.
//! - [`wasi`] — a plain C ABI over `wasm32-wasip1`, callable from Python, Go,
//!   or any host with a wasm runtime.
//!
//! Both take the same document JSON and produce the same bytes.

#[cfg(feature = "browser")]
pub mod browser;

#[cfg(feature = "wasi-lib")]
pub mod wasi;
