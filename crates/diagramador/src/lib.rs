//! # diagramador
//!
//! A generic document diagramming engine. One JSON document in, two outputs
//! that agree by construction: a **display list** for the browser editor and a
//! **PDF** for print.
//!
//! ```text
//!                    Document (JSON)
//!                          │
//!            ┌─────────────▼──────────────┐
//!            │  resolve  (sugar → core)   │
//!            │  cascade  (styles)         │
//!            │  layout   (shape + break)  │
//!            └─────────────┬──────────────┘
//!                          │
//!                    DisplayList
//!                  (positioned, final)
//!                    ╱          ╲
//!            pdf::emit          bindings::browser
//!            (pdf-writer)       (Canvas2D / Path2D)
//! ```
//!
//! The layout engine is the single authority on where every glyph sits. The
//! browser never re-lays-out anything; it paints coordinates the engine already
//! decided. That is what makes the on-screen editor and the printed PDF match.

#[cfg(any(feature = "browser", feature = "wasi-lib"))]
pub mod bindings;
pub mod color;
pub mod display;
pub mod engine;
pub mod fonts;
pub mod images;
pub mod layout;
pub mod pdf;
pub mod spec;
pub mod units;

pub use color::Color;
pub use display::DisplayList;
pub use engine::{Engine, EngineError};
pub use fonts::FontRegistry;
pub use images::ImageStore;
pub use layout::LayoutEngine;
pub use spec::Document;
pub use units::{Insets, Len, PageSize, Rect};
