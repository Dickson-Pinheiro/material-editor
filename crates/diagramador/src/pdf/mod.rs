//! PDF emission from the display list.
//!
//! The display list is already final: positions, glyph ids and advances were
//! all decided by the layout engine. This module only translates them into PDF
//! operators, flipping the y axis on the way (layout measures down from the top
//! of the page, PDF measures up from the bottom).

mod emit;
mod fonts;
mod images;

use pdf_writer::Ref;
use thiserror::Error;

pub use emit::render;

#[derive(Debug, Error)]
pub enum PdfError {
    #[error("font `{family}` could not be subset: {reason}")]
    Subset { family: String, reason: String },
    #[error("image `{key}` could not be embedded: {reason}")]
    Image { key: String, reason: String },
}

/// Hands out PDF object references in sequence.
///
/// Every object is allocated through one counter, so adding an object type
/// never means recomputing anyone else's offsets.
#[derive(Debug)]
pub(crate) struct RefAlloc {
    next: i32,
}

impl RefAlloc {
    pub fn new() -> Self {
        // Reference 0 is reserved by the PDF specification.
        RefAlloc { next: 1 }
    }

    pub fn alloc(&mut self) -> Ref {
        let id = Ref::new(self.next);
        self.next += 1;
        id
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refs_are_sequential_and_start_at_one() {
        let mut alloc = RefAlloc::new();
        assert_eq!(alloc.alloc().get(), 1);
        assert_eq!(alloc.alloc().get(), 2);
        assert_eq!(alloc.alloc().get(), 3);
    }
}
