//! The public JSON schema — two layers over one core.
//!
//! **Raw core.** A [`Document`] is a list of [`Page`]s; a page is a list of
//! [`Frame`]s; a frame is a positioned box holding text [`Block`]s, an image,
//! a shape, or a group. Nothing here knows about school materials, exams or
//! books — it is geometry plus styled runs.
//!
//! **Sugar.** [`Resources`] adds named styles, page masters and threaded
//! stories. Every one of them is resolved away before layout, so the engine
//! only ever sees the raw core. Documents that want none of it can omit
//! `resources` entirely.

pub mod chart;
pub mod content;
pub mod document;
pub mod frame;
pub mod style;

pub use content::{
    PanelBlock,
    Block, Inline, InlineImage, InlineRule, Marker, Origin, Paragraph, RuleBlock, SpaceRun,
    SpacerBlock, Tab, TextRun,
};
pub use document::{
    Document, Master, Meta, Page, PageDefaults, PageGeometry, Resources, SCHEMA_VERSION,
};
pub use frame::{
    Border, BorderStyle, Frame, FrameContent, GroupFrame, ImageAlign, ImageFit, ImageFrame,
    ShapeFrame, ShapeKind, Sides, TextFrame,
};
pub use style::{
    FontStyle, FontWeight, LineHeight, Overflow, ResolvedStyle, Style, TextAlign, TextTransform,
    VerticalAlign,
};
