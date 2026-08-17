//! Frames — the geometric primitive of the raw core.
//!
//! A frame is a positioned box that carries content. Everything on a page is a
//! frame: a paragraph of text, a photo, a coloured rectangle, a group. This is
//! the "boxes and runs" layer the sugar layer compiles down to.

use serde::{Deserialize, Serialize};

use super::content::Block;
use super::chart::ChartFrame;
use super::style::{Overflow, Style, VerticalAlign};
use crate::color::Color;
use crate::units::{Corners, Insets, Len, Rect};

// ─────────────────────────────────────────────────────────────────────────────
// Frame
// ─────────────────────────────────────────────────────────────────────────────

/// A positioned box on a page.
///
/// `rect` is in points, measured from the top-left corner of the **page**
/// (not the margin box), with `y` growing downward. Inside a group, it is
/// measured from the group's own top-left corner.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Frame {
    /// Stable identity. Auto-assigned during validation when absent.
    pub id: Option<String>,
    /// Human-facing label shown in the editor's layer list.
    pub name: Option<String>,

    pub rect: Rect,
    /// Clockwise rotation in degrees, about the frame's centre.
    pub rotation: f64,
    pub opacity: f64,

    /// Inner spacing between the frame box and its content.
    pub padding: Insets,
    pub fill: Option<Color>,
    pub border: Option<Border>,
    /// Corner radii in points, clockwise from the top-left.
    ///
    /// A bare number still rounds all four the same, which is what most boxes
    /// want and what every document written before corners were separable
    /// says.
    pub radius: Corners,

    /// Clip content to the frame box.
    pub clip: bool,
    pub visible: bool,
    /// Not selectable in the editor.
    pub locked: bool,

    #[serde(flatten)]
    pub content: FrameContent,
}

impl Default for Frame {
    fn default() -> Self {
        Frame {
            id: None,
            name: None,
            rect: Rect::default(),
            rotation: 0.0,
            opacity: 1.0,
            padding: Insets::ZERO,
            fill: None,
            border: None,
            radius: Corners::ZERO,
            clip: false,
            visible: true,
            locked: false,
            content: FrameContent::default(),
        }
    }
}

impl Frame {
    /// The box available to content, after padding.
    pub fn content_box(&self) -> Rect {
        self.rect.deflate(self.padding)
    }

    pub fn as_text(&self) -> Option<&TextFrame> {
        match &self.content {
            FrameContent::Text(t) => Some(t),
            _ => None,
        }
    }

    pub fn as_image(&self) -> Option<&ImageFrame> {
        match &self.content {
            FrameContent::Image(i) => Some(i),
            _ => None,
        }
    }

    /// The wrap this frame imposes on text around it, if any.
    ///
    /// Lives on `Frame` rather than on `ImageFrame` so that the obstacle pass
    /// can ask every frame the same question. Only images answer today.
    pub fn wrap(&self) -> Option<&Wrap> {
        match &self.content {
            FrameContent::Image(i) => i.wrap.as_ref(),
            _ => None,
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// FrameContent
// ─────────────────────────────────────────────────────────────────────────────

/// What a frame holds. Flattened into the frame object in JSON, so the tag
/// sits alongside the geometry: `{"type":"text","rect":[…],"blocks":[…]}`.
///
/// `Text` dominates the size because it owns its block list and an optional
/// style. Boxing it would save memory on shape-heavy pages at the cost of an
/// indirection on every text frame — the common case. Frames number in the
/// hundreds, so the trade favours directness.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum FrameContent {
    Text(TextFrame),
    Image(ImageFrame),
    Shape(ShapeFrame),
    Group(GroupFrame),
    Chart(ChartFrame),
}

impl Default for FrameContent {
    fn default() -> Self {
        FrameContent::Text(TextFrame::default())
    }
}

/// A frame that lays out blocks of text.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct TextFrame {
    /// Inline content. Ignored when `story` is set.
    pub blocks: Vec<Block>,

    /// Sugar: pull content from `resources.stories` instead of `blocks`.
    pub story: Option<String>,
    /// Sugar: id of the frame that receives content overflowing this one.
    pub thread_next: Option<String>,

    /// Keep generating pages until the content runs out.
    ///
    /// When this frame overflows and no `threadNext` is set, the engine appends
    /// a page modelled on the one this frame sits on — same size, margins and
    /// master — carrying a copy of this frame, and continues the flow there.
    /// This is what turns a story plus one page into a book.
    ///
    /// Ignored when `threadNext` is set: an explicit chain wins.
    pub auto_flow: bool,

    pub columns: u32,
    pub column_gap: Len,
    pub vertical_align: VerticalAlign,
    pub overflow: Overflow,

    /// Lay this text out as if no frame on the page carried a wrap.
    ///
    /// The escape hatch every wrap needs: a caption sitting on top of its own
    /// photograph would otherwise be shoved off it by the very picture it
    /// describes. InDesign spells this "ignore text wrap", and it is a
    /// property of the text, not of the picture — one frame opts out without
    /// changing what the picture does to everything else.
    pub ignore_wrap: bool,

    #[serde(rename = "use")]
    pub use_style: Option<String>,
    pub style: Option<Style>,
}

impl Default for TextFrame {
    fn default() -> Self {
        TextFrame {
            blocks: Vec::new(),
            story: None,
            thread_next: None,
            auto_flow: false,
            columns: 1,
            column_gap: Len(14.0),
            vertical_align: VerticalAlign::Top,
            overflow: Overflow::Clip,
            ignore_wrap: false,
            use_style: None,
            style: None,
        }
    }
}

/// A frame that draws a raster image.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ImageFrame {
    /// Key registered through `add_image`.
    pub src: String,
    pub fit: ImageFit,
    /// Placement of the image inside the frame when `fit` leaves slack.
    pub align: ImageAlign,
    /// How this frame pushes text aside. Absent means it does not.
    pub wrap: Option<Wrap>,
}

impl Default for ImageFrame {
    fn default() -> Self {
        ImageFrame {
            src: String::new(),
            fit: ImageFit::Contain,
            align: ImageAlign::Center,
            wrap: None,
        }
    }
}

/// Text wrap: the space a frame denies to the text around it.
///
/// The shape lives in the document rather than being read back from the
/// image's pixels at layout time. Tracing a silhouette is authoring work — it
/// belongs to the editor, where a person can see and correct it. An engine
/// that decoded pixels would stop being deterministic across platforms, and
/// the PDF would stop matching the canvas.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Wrap {
    pub mode: WrapMode,
    /// Clearance in points, held between the text and the shape.
    pub padding: Insets,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum WrapMode {
    /// The frame's own box blocks the text.
    #[default]
    Box,
    /// A silhouette blocks the text.
    ///
    /// `points` is a closed ring in `0..1` coordinates relative to the
    /// frame's rect, so it survives resizing the frame. It is stored as a
    /// ring rather than a pixel mask because a document is authored, edited
    /// and diffed by people.
    Contour { points: Vec<[f64; 2]> },
}

impl WrapMode {
    /// Whether the ring is usable. A degenerate ring falls back to the box.
    ///
    /// Points outside `0..1` are allowed on purpose: a wrap contour that sits
    /// looser than the image it belongs to is a legitimate thing to author.
    /// Only a ring that cannot enclose area, or one carrying a non-finite
    /// number, is rejected.
    pub fn usable(&self) -> bool {
        match self {
            WrapMode::Box => true,
            WrapMode::Contour { points } => {
                points.len() >= 3 && points.iter().flatten().all(|v| v.is_finite())
            }
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ImageFit {
    /// Scale to fit entirely inside, preserving the aspect ratio.
    #[default]
    Contain,
    /// Scale to cover the whole frame, preserving the ratio, cropping the rest.
    Cover,
    /// Ignore the aspect ratio and fill the frame exactly.
    Stretch,
    /// Draw at natural size.
    None,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ImageAlign {
    TopLeft,
    Top,
    TopRight,
    Left,
    #[default]
    Center,
    Right,
    BottomLeft,
    Bottom,
    BottomRight,
}

impl ImageAlign {
    /// Fractions of the leftover space to place before the image, 0–1.
    pub fn factors(self) -> (f64, f64) {
        let fx = match self {
            ImageAlign::TopLeft | ImageAlign::Left | ImageAlign::BottomLeft => 0.0,
            ImageAlign::Top | ImageAlign::Center | ImageAlign::Bottom => 0.5,
            ImageAlign::TopRight | ImageAlign::Right | ImageAlign::BottomRight => 1.0,
        };
        let fy = match self {
            ImageAlign::TopLeft | ImageAlign::Top | ImageAlign::TopRight => 0.0,
            ImageAlign::Left | ImageAlign::Center | ImageAlign::Right => 0.5,
            ImageAlign::BottomLeft | ImageAlign::Bottom | ImageAlign::BottomRight => 1.0,
        };
        (fx, fy)
    }
}

/// A frame that draws a vector primitive using the frame's own fill and border.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ShapeFrame {
    pub shape: ShapeKind,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ShapeKind {
    #[default]
    Rect,
    Ellipse,
    /// A straight line from the top-left to the bottom-right of the frame box.
    Line,
}

/// A frame whose children are positioned relative to its own origin.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct GroupFrame {
    pub children: Vec<Frame>,
}

// ─────────────────────────────────────────────────────────────────────────────
// Border
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Border {
    pub width: Len,
    pub color: Color,
    pub style: BorderStyle,
    /// Which edges to draw. All four by default.
    pub sides: Sides,
}

impl Default for Border {
    fn default() -> Self {
        Border {
            width: Len(1.0),
            color: Color::BLACK,
            style: BorderStyle::Solid,
            sides: Sides::default(),
        }
    }
}

impl Border {
    /// Dash pattern in points for the stroke, or `None` when solid.
    pub fn dash_pattern(&self) -> Option<[f64; 2]> {
        let w = self.width.get().max(0.1);
        match self.style {
            BorderStyle::Solid => None,
            BorderStyle::Dashed => Some([w * 3.0, w * 2.0]),
            BorderStyle::Dotted => Some([w, w * 2.0]),
        }
    }

    #[inline]
    pub fn is_uniform(&self) -> bool {
        self.sides.all()
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum BorderStyle {
    #[default]
    Solid,
    Dashed,
    Dotted,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Sides {
    pub top: bool,
    pub right: bool,
    pub bottom: bool,
    pub left: bool,
}

impl Default for Sides {
    fn default() -> Self {
        Sides {
            top: true,
            right: true,
            bottom: true,
            left: true,
        }
    }
}

impl Sides {
    #[inline]
    pub fn all(&self) -> bool {
        self.top && self.right && self.bottom && self.left
    }
    #[inline]
    pub fn none(&self) -> bool {
        !self.top && !self.right && !self.bottom && !self.left
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_frame_flattens_its_tag_next_to_the_geometry() {
        let json = r#"{
            "id": "f1",
            "type": "text",
            "rect": [56, 56, 483, 200],
            "blocks": ["Olá"]
        }"#;
        let f: Frame = serde_json::from_str(json).unwrap();
        assert_eq!(f.id.as_deref(), Some("f1"));
        assert_eq!(f.rect, Rect::new(56.0, 56.0, 483.0, 200.0));
        assert_eq!(f.as_text().unwrap().blocks.len(), 1);
        // Defaults survive the flatten.
        assert_eq!(f.opacity, 1.0);
        assert!(f.visible);
        assert_eq!(f.as_text().unwrap().columns, 1);
    }

#[test]
    fn image_frame_parses() {

        let json = r#"{"type":"image","rect":[0,0,100,100],"src":"foto.png","fit":"cover"}"#;
        let f: Frame = serde_json::from_str(json).unwrap();
        match &f.content {
            FrameContent::Image(i) => {
                assert_eq!(i.src, "foto.png");
                assert_eq!(i.fit, ImageFit::Cover);
                assert_eq!(i.align, ImageAlign::Center);
            }
            other => panic!("expected image, got {other:?}"),
        }
    }

    #[test]
    fn an_image_without_wrap_stays_without_wrap() {
        let json = r#"{"type":"image","rect":[0,0,100,100],"src":"foto.png"}"#;
        let f: Frame = serde_json::from_str(json).unwrap();
        assert_eq!(f.as_image().unwrap().wrap, None);
        // The field must not appear from nowhere on the way back out.
        let back = serde_json::to_value(&f).unwrap();
        assert_eq!(back["wrap"], serde_json::Value::Null);
    }

    #[test]
    fn wrap_by_box_parses_with_shorthand_padding() {
        let json = r#"{
            "type": "image",
            "rect": [0, 0, 100, 100],
            "src": "foto.png",
            "wrap": { "mode": { "kind": "box" }, "padding": 6 }
        }"#;
        let f: Frame = serde_json::from_str(json).unwrap();
        let wrap = f.as_image().unwrap().wrap.as_ref().unwrap();
        assert_eq!(wrap.mode, WrapMode::Box);
        assert_eq!(wrap.padding, Insets::all(6.0));
        assert!(wrap.mode.usable());
    }

    #[test]
    fn wrap_by_contour_keeps_its_ring_through_a_round_trip() {
        let json = r#"{
            "type": "image",
            "rect": [0, 0, 100, 100],
            "src": "foto.png",
            "wrap": {
                "mode": { "kind": "contour", "points": [[0, 0], [1, 0.5], [0, 1]] },
                "padding": [4, 8]
            }
        }"#;
        let f: Frame = serde_json::from_str(json).unwrap();
        let wrap = f.as_image().unwrap().wrap.clone().unwrap();
        assert_eq!(wrap.padding, Insets::symmetric(4.0, 8.0));
        match &wrap.mode {
            WrapMode::Contour { points } => assert_eq!(points.len(), 3),
            other => panic!("expected a contour, got {other:?}"),
        }

        let again: Frame = serde_json::from_str(&serde_json::to_string(&f).unwrap()).unwrap();
        assert_eq!(again.as_image().unwrap().wrap, Some(wrap));
    }

    #[test]
    fn a_ring_that_encloses_nothing_is_not_usable() {
        let two = WrapMode::Contour {
            points: vec![[0.0, 0.0], [1.0, 1.0]],
        };
        assert!(!two.usable());

        let broken = WrapMode::Contour {
            points: vec![[0.0, 0.0], [f64::NAN, 1.0], [1.0, 1.0]],
        };
        assert!(!broken.usable());

        // Outside 0..1 is allowed: a contour looser than its image is valid.
        let loose = WrapMode::Contour {
            points: vec![[-0.2, 0.0], [1.2, 0.5], [0.0, 1.0]],
        };
        assert!(loose.usable());
    }

    #[test]
    fn group_children_parse_recursively() {
        let json = r#"{
            "type": "group",
            "rect": [10, 10, 200, 200],
            "children": [
                {"type": "shape", "rect": [0, 0, 50, 50], "shape": "ellipse"}
            ]
        }"#;
        let f: Frame = serde_json::from_str(json).unwrap();
        match &f.content {
            FrameContent::Group(g) => {
                assert_eq!(g.children.len(), 1);
                assert!(matches!(
                    g.children[0].content,
                    FrameContent::Shape(ShapeFrame { shape: ShapeKind::Ellipse })
                ));
            }
            other => panic!("expected group, got {other:?}"),
        }
    }

    #[test]
    fn border_defaults_to_all_four_solid_sides() {
        let b: Border = serde_json::from_str(r##"{"width":2,"color":"#f00"}"##).unwrap();
        assert_eq!(b.width, Len(2.0));
        assert!(b.is_uniform());
        assert_eq!(b.dash_pattern(), None);

        let dotted: Border =
            serde_json::from_str(r#"{"style":"dotted","sides":{"top":false}}"#).unwrap();
        assert!(!dotted.is_uniform());
        assert!(!dotted.sides.top && dotted.sides.bottom);
        assert!(dotted.dash_pattern().is_some());
    }

    #[test]
    fn a_frame_rounds_its_corners_one_by_one() {
        let json = r#"{"type":"shape","rect":[0,0,100,100],"radius":[8,0,0,8]}"#;
        let f: Frame = serde_json::from_str(json).unwrap();
        assert_eq!(f.radius, Corners::new(8.0, 0.0, 0.0, 8.0));

        // The old spelling has not changed meaning.
        let flat: Frame =
            serde_json::from_str(r#"{"type":"shape","rect":[0,0,100,100],"radius":6}"#).unwrap();
        assert_eq!(flat.radius, Corners::all(6.0));
        assert_eq!(
            serde_json::to_value(&flat).unwrap()["radius"],
            serde_json::json!(6.0),
            "a uniform radius stays a bare number on the way out"
        );

        // A frame that never mentioned corners still has none.
        let bare: Frame = serde_json::from_str(r#"{"type":"shape","rect":[0,0,10,10]}"#).unwrap();
        assert!(bare.radius.is_zero());
    }

    #[test]
    fn padding_shrinks_the_content_box() {
        let json = r#"{"type":"text","rect":[0,0,100,100],"padding":[10,5]}"#;
        let f: Frame = serde_json::from_str(json).unwrap();
        assert_eq!(f.content_box(), Rect::new(5.0, 10.0, 90.0, 80.0));
    }

    #[test]
    fn frame_roundtrips_through_json() {
        let original: Frame = serde_json::from_str(
            r#"{"id":"a","type":"text","rect":[1,2,3,4],"columns":2,"verticalAlign":"middle"}"#,
        )
        .unwrap();
        let json = serde_json::to_string(&original).unwrap();
        let back: Frame = serde_json::from_str(&json).unwrap();
        assert_eq!(original, back);
    }

    #[test]
    fn image_align_factors() {
        assert_eq!(ImageAlign::TopLeft.factors(), (0.0, 0.0));
        assert_eq!(ImageAlign::Center.factors(), (0.5, 0.5));
        assert_eq!(ImageAlign::BottomRight.factors(), (1.0, 1.0));
    }
}
