//! Paragraph layout: inlines → shaped spans → break opportunities → lines.
//!
//! # How a paragraph becomes lines
//!
//! 1. Every inline is turned into a **span** and appended to one flat string.
//!    Non-text inlines contribute `U+FFFC OBJECT REPLACEMENT CHARACTER`, so the
//!    Unicode line-breaking algorithm sees them as ordinary objects.
//! 2. Text spans are shaped **whole**, once. Glyph clusters are rebased to the
//!    paragraph string, so measuring any byte range is a scan over advances —
//!    never a re-shape. Measuring and drawing therefore cannot disagree.
//! 3. `unicode-linebreak` yields the legal break positions. The text between
//!    two of them is a **piece**: the smallest unit a line can be built from.
//! 4. Pieces are packed greedily into lines, measured without their trailing
//!    whitespace so a space at the margin never pushes a word down.
//! 5. Each line is emitted as display items with final coordinates.
//!
//! Line boxes follow the CSS model: the box is `line-height` tall and the
//! baseline sits at half-leading plus the ascent, so text stays optically
//! centred when the leading is loosened.

use std::borrow::Cow;
use std::collections::BTreeMap;

use unicode_linebreak::{BreakOpportunity, linebreaks};

use crate::color::Color;
use crate::display::{
    DisplayItem, Glyph, GlyphRun, ImageItem, LineItem, RectItem, SourceRef, Stroke,
};
use crate::fonts::{FaceMetrics, FontId, FontRegistry};
use crate::images::ImageStore;
use crate::spec::{Inline, Marker, Origin, Paragraph, ResolvedStyle, Style, TextAlign, TextRun};
use crate::units::{Len, PT_PER_PX, Rect};

use super::cascade;
use super::shape::{ShapedGlyph, shape_text};
use super::wrap::{BandMode, Interval, LineSpace};

/// Stands in for a non-text inline while computing break opportunities.
const OBJECT_REPLACEMENT: &str = "\u{FFFC}";
/// Never a break opportunity, so an explicit fixed space holds words together.
const FIXED_SPACE: &str = "\u{00A0}";

/// Slack allowed when deciding whether a piece still fits on the line.
const FIT_EPSILON: f64 = 0.01;

/// Consecutive bands a paragraph may skip before giving up on the frame.
///
/// A picture that covers a column edge to edge leaves nowhere to put a line.
/// Stepping down past it is right; stepping down forever is not, so the
/// paragraph hands the rest on as overset instead of hanging.
const MAX_BLOCKED_BANDS: u32 = 512;

/// Which vertical extent of a line asks the wrap where it may sit.
///
/// Set by measurement, not by taste — see `docs/contorno/medicao-faixa.md`.
/// Flip it, rebuild, and rerun `examples/faixa.rs` to redo the comparison.
const BAND: BandMode = BandMode::LineBox;

// ─────────────────────────────────────────────────────────────────────────────
// Spans
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
struct Span {
    /// Byte range within the paragraph string.
    start: usize,
    end: usize,
    inline_index: u32,
    style: ResolvedStyle,
    font: Option<FontId>,
    kind: SpanKind,
}

#[derive(Debug, Clone)]
enum SpanKind {
    Text {
        /// Text as laid out, after `textTransform`.
        text: String,
        /// Character length of the source text, for mapping offsets back.
        source_chars: usize,
        /// Clusters are absolute offsets into the paragraph string.
        glyphs: Vec<ShapedGlyph>,
    },
    Image {
        src: String,
        width: f64,
        height: f64,
        /// How far the image bottom sits below the baseline.
        baseline: f64,
    },
    Rule {
        /// `None` means "fill the rest of the line".
        width: Option<f64>,
        thickness: f64,
        color: Color,
        offset: f64,
    },
    Space {
        width: f64,
    },
    Tab {
        to: Option<f64>,
    },
    Break,
}

impl Span {
    fn covers(&self, a: usize, b: usize) -> bool {
        self.start < b && self.end > a
    }

    /// Width contributed by the byte range `[a, b)`.
    fn width_in(&self, a: usize, b: usize) -> f64 {
        match &self.kind {
            SpanKind::Text { glyphs, .. } => glyphs
                .iter()
                .filter(|g| (g.cluster as usize) >= a && (g.cluster as usize) < b)
                .map(|g| g.x_advance)
                .sum(),
            SpanKind::Image { width, .. } | SpanKind::Space { width } => {
                if self.start >= a && self.start < b { *width } else { 0.0 }
            }
            SpanKind::Rule { width, .. } => {
                if self.start >= a && self.start < b {
                    width.unwrap_or(0.0)
                } else {
                    0.0
                }
            }
            // Tabs resolve against the pen position; breaks have no width.
            SpanKind::Tab { .. } | SpanKind::Break => 0.0,
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Pieces and lines
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy)]
struct Piece {
    start: usize,
    end: usize,
    width: f64,
    /// Width without the trailing whitespace, used for the fit test.
    trimmed_width: f64,
    mandatory: bool,
}

#[derive(Debug, Clone, Copy)]
struct LineRange {
    start: usize,
    end: usize,
    /// The line ends because of a hard break or the end of the paragraph.
    hard_end: bool,
}

/// Vertical metrics of one line box.
#[derive(Debug, Clone, Copy)]
struct LineMetrics {
    height: f64,
    baseline: f64,
    /// Ascender to descender, without the leading. The line's actual ink.
    ink: f64,
}

/// One run of text on one line, inside one gap of the band.
#[derive(Debug, Clone, Copy)]
struct Segment {
    slot: Interval,
    /// Usable width once the indents are out — what justification divides.
    limit: f64,
    line: LineRange,
    /// Piece the next segment starts at.
    next: usize,
    first_in_band: bool,
}

/// One visual line: everything sharing a baseline across the band's gaps.
///
/// The height is the tallest segment's, so a picture between two columns of
/// text cannot make the halves drift apart.
#[derive(Debug, Clone)]
struct Band {
    segments: Vec<Segment>,
    height: f64,
    baseline: f64,
    ink: f64,
}

// ─────────────────────────────────────────────────────────────────────────────
// Result
// ─────────────────────────────────────────────────────────────────────────────

/// A laid-out paragraph, positioned relative to the column's top-left corner.
#[derive(Debug, Clone)]
pub(crate) struct ParagraphLayout {
    pub items: Vec<DisplayItem>,
    pub height: f64,
    /// Lines actually placed. Read by the tests and by callers that need to
    /// know whether a frame received anything at all.
    #[allow(dead_code)]
    pub line_count: usize,
    /// What did not fit, ready to flow into the next frame. `None` when the
    /// whole paragraph was placed.
    pub remainder: Option<Paragraph>,
    /// The wrap, rather than the height budget, is what stopped the text.
    ///
    /// Reported so the author is told the real cause: "does not fit" reads as
    /// "the frame is too small", when the frame may be ample and the
    /// photograph simply standing on all of it.
    pub walled_in: bool,
}

impl ParagraphLayout {
    fn empty() -> Self {
        ParagraphLayout {
            items: Vec::new(),
            height: 0.0,
            line_count: 0,
            remainder: None,
            walled_in: false,
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Layouter
// ─────────────────────────────────────────────────────────────────────────────

pub(crate) struct TextLayouter<'a> {
    pub registry: &'a FontRegistry,
    pub images: &'a ImageStore,
    pub styles: &'a BTreeMap<String, Style>,
    pub variables: Variables,
}

/// Values substituted into text as it is laid out.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct Variables {
    /// One-based number of the page being laid out.
    pub page: u32,
    /// Total pages in the finished document.
    pub pages: u32,
}

/// Replace `{page}` and `{pages}` in a run's text.
///
/// A field is *computed* text: substitution changes the run's byte offsets, so
/// the editor cannot map a caret into one. That is the right trade — nobody
/// wants to type inside a page number — but it is why the check is exact rather
/// than a general template language.
fn substitute(text: &str, variables: Variables) -> Cow<'_, str> {
    if !text.contains('{') {
        return Cow::Borrowed(text);
    }
    Cow::Owned(
        text.replace("{page}", &variables.page.to_string())
            .replace("{pages}", &variables.pages.to_string()),
    )
}

/// Whether any run in the document asks for the total page count.
///
/// Knowing this up front is what decides between one layout pass and the
/// fixed-point loop: the total is only known after auto-flow has run.
pub(crate) fn uses_total_pages(blocks: &[crate::spec::Block]) -> bool {
    blocks.iter().any(|block| match block {
        crate::spec::Block::Paragraph(paragraph) => paragraph.content.iter().any(|inline| {
            matches!(inline, Inline::Text(run) if run.text.contains("{pages}"))
        }),
        _ => false,
    })
}

impl TextLayouter<'_> {
    /// Lay out one paragraph into a column `avail_width` wide.
    ///
    /// `max_height` caps how much vertical space may be used; lines beyond it
    /// are left unplaced and returned as [`ParagraphLayout::remainder`].
    pub fn layout_paragraph(
        &self,
        para: &Paragraph,
        parent: &ResolvedStyle,
        space: &dyn LineSpace,
        max_height: Option<f64>,
        block_index: u32,
        frame_source: &SourceRef,
    ) -> ParagraphLayout {
        let style = cascade::resolve(
            parent,
            self.styles,
            para.use_style.as_deref(),
            para.style.as_ref(),
        );

        // Provenance is expressed in the coordinates of whoever owns this text.
        // For a continuation those differ from the local indices, which is what
        // lets the editor write back through a chain of threaded frames.
        let origin = para.origin.unwrap_or(Origin {
            block: block_index,
            inline: 0,
            offset: 0,
        });

        let (text, spans) = self.build_spans(para, &style);
        let pieces = build_pieces(&text, &spans);

        // Marker geometry has to be known before the first line's indent.
        let marker = para.marker.as_ref().filter(|m| !m.text.is_empty());
        let marker_shape = marker.map(|m| self.shape_marker(m, &style));
        let marker_column = marker_shape.as_ref().map_or(0.0, |s| s.column);
        let hanging = marker.is_some_and(|m| m.hanging);

        let indent_left = style.indent_left + if hanging { marker_column } else { 0.0 };
        let first_extra = style.indent_first + if hanging { 0.0 } else { marker_column };
        let indent_right = style.indent_right;

        let boundaries: Vec<usize> = pieces.iter().map(|p| p.start).collect();

        // ── Break and place, one line at a time ───────────────────────────────
        //
        // These used to be two loops: fill every line, then walk them placing
        // each. They cannot be, once a picture is in the way — the width a
        // line may use depends on where the line sits, and where it sits
        // depends on how tall the lines above it turned out.
        //
        // The knot: the band needs a height, the height comes from the line,
        // the line needs the band. It is untied with the style's nominal
        // leading and *one* retry — a line that measures taller than nominal
        // asks again with its real height and takes that answer. No loop.
        let mut items = Vec::new();
        let mut y = style.space_before;
        let mut placed = 0usize;
        let budget = max_height.unwrap_or(f64::INFINITY);

        let nominal = style.leading();
        let nominal_ink = {
            let face = self.metrics_for(&style);
            (face.ascender - face.descender) * style.font_size
        };
        let mut start = 0usize;
        let mut index = 0usize;
        let mut skipped = 0u32;
        let mut cut_short = false;
        // Reused every line: the answer is usually one interval and one
        // segment, and a fresh allocation per line showed up in the benchmark.
        let mut slots: Vec<Interval> = Vec::with_capacity(4);
        let mut band = Band {
            segments: Vec::with_capacity(4),
            height: 0.0,
            baseline: 0.0,
            ink: 0.0,
        };
        // Set when the wrap, not the height budget, is what stopped the text.
        let mut walled_in = false;

        // A paragraph with nothing in it still occupies one line.
        let empty = pieces.is_empty();

        while empty || index < pieces.len() || start < text.len() {
            let first = placed == 0;

            // Every gap on this band, left to right. More than one means a
            // picture sits inside the column with room on both sides, and the
            // line runs through all of them before moving down.
            self.slots_for(space, y, nominal, nominal_ink, first, indent_left, first_extra, &mut slots);
            if slots.is_empty() {
                // Nothing on this band. Move down and try again rather than
                // wedge the text inside the picture.
                skipped += 1;
                if skipped > MAX_BLOCKED_BANDS || y + nominal > budget + FIT_EPSILON {
                    cut_short = true;
                    walled_in = true;
                    break;
                }
                y += nominal;
                continue;
            }
            skipped = 0;

            // Fill the band once at the nominal height. A line that measures
            // taller has to ask again, because a taller band can meet a shape
            // the shorter one missed. Once, never in a loop.
            self.fill_band(
                &text, &pieces, &spans, &style, &slots, index, start, first, indent_left,
                indent_right, first_extra, &mut band,
            );

            if band.height > nominal + FIT_EPSILON {
                let mut taller: Vec<Interval> = Vec::with_capacity(slots.len());
                self.slots_for(space, y, band.height, band.ink, first, indent_left, first_extra, &mut taller);
                if !taller.is_empty() && taller != slots {
                    slots.clear();
                    slots.extend_from_slice(&taller);
                    self.fill_band(
                        &text, &pieces, &spans, &style, &slots, index, start, first, indent_left,
                        indent_right, first_extra, &mut band,
                    );
                }
            }

            if band.segments.is_empty() {
                // Every gap was too narrow for the next word. Try lower down.
                skipped += 1;
                if skipped > MAX_BLOCKED_BANDS || y + nominal > budget + FIT_EPSILON {
                    cut_short = true;
                    walled_in = true;
                    break;
                }
                y += nominal;
                continue;
            }

            if y + band.height > budget + FIT_EPSILON {
                cut_short = true;
                break;
            }

            // The marker is painted before the line it belongs to, against the
            // first gap that line uses.
            if first && let Some(shape) = &marker_shape {
                self.emit_marker(
                    &mut items,
                    shape,
                    band.segments[0].slot.left + style.indent_left,
                    y + band.baseline,
                    origin.block,
                    frame_source,
                );
            }

            for segment in &band.segments {
                let at_start = first && segment.first_in_band;
                let left = segment.slot.left
                    + indent_left
                    + if at_start { first_extra } else { 0.0 };

                self.emit_line(
                    &mut items,
                    &text,
                    &spans,
                    segment.line,
                    LineMetrics {
                        height: band.height,
                        baseline: band.baseline,
                        ink: band.ink,
                    },
                    y,
                    left,
                    segment.limit,
                    &style,
                    &boundaries,
                    // Only the segment that reaches the paragraph's end escapes
                    // justification; the others are full lines like any other.
                    segment.line.end >= text.len(),
                    origin,
                    frame_source,
                );
            }

            let last = band.segments.last().expect("banda com segmentos");
            let finished = last.line.end >= text.len();

            y += band.height;
            placed += 1;
            start = last.line.end;
            index = last.next;

            if empty || finished {
                break;
            }
        }

        if placed == 0 {
            // Not even one line fits; the caller must move the whole paragraph.
            return ParagraphLayout {
                remainder: Some(para.clone()),
                walled_in,
                ..ParagraphLayout::empty()
            };
        }

        let remainder = if cut_short && start < text.len() {
            self.remainder_of(para, &spans, start, origin)
        } else {
            None
        };

        ParagraphLayout {
            items,
            height: y + if remainder.is_none() { style.space_after } else { 0.0 },
            line_count: placed,
            remainder,
            walled_in,
        }
    }

    // ── Span construction ────────────────────────────────────────────────────

    fn build_spans(&self, para: &Paragraph, style: &ResolvedStyle) -> (String, Vec<Span>) {
        let mut text = String::new();
        let mut spans = Vec::new();

        for (index, inline) in para.content.iter().enumerate() {
            let start = text.len();
            let inline_index = index as u32;

            match inline {
                Inline::Text(run) => {
                    if let Some(span) =
                        self.text_span(run, style, start, inline_index, &mut text)
                    {
                        spans.push(span);
                    }
                }
                Inline::Break => {
                    text.push('\n');
                    spans.push(Span {
                        start,
                        end: text.len(),
                        inline_index,
                        style: style.clone(),
                        font: None,
                        kind: SpanKind::Break,
                    });
                }
                Inline::Space(space) => {
                    text.push_str(FIXED_SPACE);
                    spans.push(Span {
                        start,
                        end: text.len(),
                        inline_index,
                        style: style.clone(),
                        font: None,
                        kind: SpanKind::Space {
                            width: space.width.get(),
                        },
                    });
                }
                Inline::Tab(tab) => {
                    text.push('\t');
                    spans.push(Span {
                        start,
                        end: text.len(),
                        inline_index,
                        style: style.clone(),
                        font: None,
                        kind: SpanKind::Tab {
                            to: tab.to.map(Len::get),
                        },
                    });
                }
                Inline::Image(image) => {
                    let (width, height) = self.image_size(
                        &image.src,
                        image.width.map(Len::get),
                        image.height.map(Len::get),
                        style.font_size,
                    );
                    text.push_str(OBJECT_REPLACEMENT);
                    spans.push(Span {
                        start,
                        end: text.len(),
                        inline_index,
                        style: style.clone(),
                        font: None,
                        kind: SpanKind::Image {
                            src: image.src.clone(),
                            width,
                            height,
                            baseline: image.baseline.map_or(0.0, Len::get),
                        },
                    });
                }
                Inline::Rule(rule) => {
                    let metrics = self.metrics_for(style);
                    text.push_str(OBJECT_REPLACEMENT);
                    spans.push(Span {
                        start,
                        end: text.len(),
                        inline_index,
                        style: style.clone(),
                        font: None,
                        kind: SpanKind::Rule {
                            width: rule.width.map(Len::get),
                            thickness: rule
                                .thickness
                                .map_or(metrics.underline_thickness * style.font_size, Len::get),
                            color: rule.color.unwrap_or(style.color),
                            offset: rule.offset.map_or(
                                -metrics.underline_position * style.font_size,
                                Len::get,
                            ),
                        },
                    });
                }
            }
        }

        (text, spans)
    }

    fn text_span(
        &self,
        run: &TextRun,
        parent: &ResolvedStyle,
        start: usize,
        inline_index: u32,
        text: &mut String,
    ) -> Option<Span> {
        let style = cascade::resolve(
            parent,
            self.styles,
            run.use_style.as_deref(),
            run.style.as_ref(),
        );
        let content = style
            .text_transform
            .apply(&substitute(&run.text, self.variables));
        if content.is_empty() {
            return None;
        }

        let font = self
            .registry
            .select(style.font_family.as_deref(), style.font_weight, style.font_style);

        let glyphs = font
            .and_then(|id| self.registry.face(id))
            .map(|face| {
                shape_text(
                    face,
                    &content,
                    style.font_size,
                    style.letter_spacing,
                    style.word_spacing,
                )
            })
            .unwrap_or_default()
            .into_iter()
            .map(|mut glyph| {
                glyph.cluster += start as u32;
                glyph
            })
            .collect();

        text.push_str(&content);

        Some(Span {
            start,
            end: text.len(),
            inline_index,
            style,
            font,
            kind: SpanKind::Text {
                text: content,
                source_chars: run.text.chars().count(),
                glyphs,
            },
        })
    }

    fn image_size(
        &self,
        src: &str,
        width: Option<f64>,
        height: Option<f64>,
        font_size: f64,
    ) -> (f64, f64) {
        let entry = self.images.get(src);
        let aspect = entry.map_or(1.0, |e| e.aspect());

        match (width, height) {
            (Some(w), Some(h)) => (w, h),
            (Some(w), None) => (w, w / aspect),
            (None, Some(h)) => (h * aspect, h),
            (None, None) => match entry.filter(|e| e.width > 0) {
                // Natural size, treating image pixels as CSS pixels.
                Some(e) => (e.width as f64 * PT_PER_PX, e.height as f64 * PT_PER_PX),
                None => (font_size, font_size),
            },
        }
    }

    // ── Metrics ──────────────────────────────────────────────────────────────

    fn metrics_for(&self, style: &ResolvedStyle) -> FaceMetrics {
        self.registry
            .select(style.font_family.as_deref(), style.font_weight, style.font_style)
            .and_then(|id| self.registry.face(id))
            .map(|face| face.metrics)
            .unwrap_or(FALLBACK_METRICS)
    }

    /// Every stretch a line of `height` may use at `top`, left to right.
    ///
    /// Empty means the band is blocked from edge to edge, or that what is
    /// left of it cannot even hold the indent.
    #[allow(clippy::too_many_arguments)]
    fn slots_for(
        &self,
        space: &dyn LineSpace,
        top: f64,
        height: f64,
        ink: f64,
        first: bool,
        indent_left: f64,
        first_extra: f64,
        out: &mut Vec<Interval>,
    ) {
        let needed = indent_left + if first { first_extra } else { 0.0 };
        let (from, to) = band_of(BAND, top, height, ink);
        out.clear();
        space.slots(from, to, out);
        out.retain(|slot| slot.width() > needed);
    }

    /// Run the text through every gap on one band, left to right.
    ///
    /// This is the part the reference library never did: `pretext` computes
    /// all the gaps and then keeps only the widest, so its text never runs
    /// down both sides of a picture. Ours does, which is the whole difference
    /// between a column that narrows and a text that wraps.
    ///
    /// A gap too narrow for the next word is stepped over rather than made to
    /// hold it — but only when there is another gap to try. With a single gap
    /// the word is forced in, exactly as it always was, which is what keeps
    /// every existing document laying out to the same bytes.
    #[allow(clippy::too_many_arguments)]
    fn fill_band(
        &self,
        text: &str,
        pieces: &[Piece],
        spans: &[Span],
        style: &ResolvedStyle,
        slots: &[Interval],
        from: usize,
        start: usize,
        first_line: bool,
        indent_left: f64,
        indent_right: f64,
        first_extra: f64,
        band: &mut Band,
    ) {
        band.segments.clear();
        band.height = 0.0;
        band.baseline = 0.0;
        band.ink = 0.0;

        let mut index = from;
        let mut at = start;
        let single = slots.len() == 1;

        for (position, slot) in slots.iter().enumerate() {
            if at >= text.len() && !band.segments.is_empty() {
                break;
            }

            let at_start = first_line && position == 0;
            let limit = usable(*slot, indent_left, indent_right, first_extra, at_start);

            // Stepping over a gap is only safe when another one follows.
            if !single
                && let Some(piece) = pieces.get(index)
                && piece.trimmed_width > limit + FIT_EPSILON
            {
                continue;
            }

            let (line, next) = break_one_line(text, pieces, index, at, limit);
            let metrics = self.line_metrics(spans, &line, style);
            band.height = band.height.max(metrics.height);
            band.baseline = band.baseline.max(metrics.baseline);
            band.ink = band.ink.max(metrics.ink);

            band.segments.push(Segment {
                slot: *slot,
                limit,
                line,
                next,
                first_in_band: position == 0,
            });

            index = next;
            at = line.end;

            // A hard break ends the whole line, not just this gap.
            if line.hard_end {
                break;
            }
        }
    }

    fn line_metrics(&self, spans: &[Span], line: &LineRange, style: &ResolvedStyle) -> LineMetrics {
        let mut ascent: f64 = 0.0;
        let mut descent: f64 = 0.0;
        let mut leading = style.leading();

        let mut saw_span = false;
        for span in spans.iter().filter(|s| s.covers(line.start, line.end)) {
            saw_span = true;
            leading = leading.max(span.style.leading());

            match &span.kind {
                SpanKind::Image { height, baseline, .. } => {
                    ascent = ascent.max(height - baseline);
                    descent = descent.max(*baseline);
                }
                _ => {
                    let metrics = self.metrics_for(&span.style);
                    let size = span.style.font_size;
                    ascent = ascent.max(metrics.ascender * size - span.style.baseline_shift);
                    descent = descent.max(-metrics.descender * size + span.style.baseline_shift);
                }
            }
        }

        if !saw_span {
            let metrics = self.metrics_for(style);
            ascent = metrics.ascender * style.font_size;
            descent = -metrics.descender * style.font_size;
        }

        // CSS half-leading: the extra space is split above and below the text.
        let half_leading = (leading - (ascent + descent)) / 2.0;
        LineMetrics {
            height: leading,
            baseline: half_leading + ascent,
            ink: ascent + descent,
        }
    }

    // ── Emission ─────────────────────────────────────────────────────────────

    #[allow(clippy::too_many_arguments)]
    fn emit_line(
        &self,
        items: &mut Vec<DisplayItem>,
        text: &str,
        spans: &[Span],
        line: LineRange,
        metrics: LineMetrics,
        line_top: f64,
        left: f64,
        limit: f64,
        style: &ResolvedStyle,
        boundaries: &[usize],
        is_last: bool,
        origin: Origin,
        frame_source: &SourceRef,
    ) {
        let trimmed_end = trim_end_of(text, line.start, line.end);
        let natural = width_of(spans, line.start, trimmed_end);
        let baseline = line_top + metrics.baseline;

        // Horizontal placement of the whole line.
        let mut pen = left
            + match style.text_align {
                TextAlign::Left | TextAlign::Justify => 0.0,
                TextAlign::Center => (limit - natural) / 2.0,
                TextAlign::Right => limit - natural,
            };

        // Extra width shared between the gaps, for justified lines.
        let inner: Vec<usize> = boundaries
            .iter()
            .copied()
            .filter(|b| *b > line.start && *b < trimmed_end)
            .collect();
        let justify_delta = if style.text_align == TextAlign::Justify
            && !is_last
            && !line.hard_end
            && !inner.is_empty()
            && natural < limit
        {
            (limit - natural) / inner.len() as f64
        } else {
            0.0
        };

        let mut next_gap = 0usize;

        for span in spans.iter().filter(|s| s.covers(line.start, line.end)) {
            let slice_start = span.start.max(line.start);
            let slice_end = span.end.min(line.end);

            match &span.kind {
                SpanKind::Text { text: span_text, glyphs, .. } => {
                    let local_start = slice_start - span.start;
                    let local_end = slice_end - span.start;
                    let slice = &span_text[local_start..local_end];

                    let run_origin = pen;
                    let mut out = Vec::new();

                    for glyph in glyphs
                        .iter()
                        .filter(|g| (g.cluster as usize) >= slice_start && (g.cluster as usize) < slice_end)
                    {
                        while next_gap < inner.len() && inner[next_gap] <= glyph.cluster as usize {
                            pen += justify_delta;
                            next_gap += 1;
                        }
                        out.push(Glyph {
                            id: glyph.id,
                            x: pen - run_origin + glyph.x_offset,
                            y: glyph.y_offset,
                            advance: glyph.x_advance,
                            cluster: glyph.cluster - slice_start as u32,
                        });
                        pen += glyph.x_advance;
                    }

                    if out.is_empty() {
                        continue;
                    }

                    let source = source_for(frame_source, origin, span, local_start);
                    let run_width = pen - run_origin;

                    self.emit_text_run(
                        items, span, slice, out, run_origin, baseline, run_width, source,
                    );
                }

                SpanKind::Space { width } => pen += width,

                SpanKind::Tab { to } => {
                    let target = to.map_or_else(
                        || next_tab_stop(pen - left, style.font_size) + left,
                        |t| left + t,
                    );
                    pen = pen.max(target);
                }

                SpanKind::Image { src, width, height, baseline: shift } => {
                    items.push(DisplayItem::Image(ImageItem {
                        src: src.clone(),
                        rect: Rect::new(pen, baseline + shift - height, *width, *height),
                        crop: None,
                        source: Some(source_for(frame_source, origin, span, 0)),
                    }));
                    pen += width;
                }

                SpanKind::Rule { width, thickness, color, offset } => {
                    // A rule with no width stretches to the end of the line.
                    let w = width.unwrap_or_else(|| (left + limit - pen).max(0.0));
                    let y = baseline + offset;
                    items.push(DisplayItem::Line(LineItem {
                        x1: pen,
                        y1: y,
                        x2: pen + w,
                        y2: y,
                        stroke: Stroke {
                            color: *color,
                            width: *thickness,
                            dash: None,
                        },
                        source: Some(source_for(frame_source, origin, span, 0)),
                    }));
                    pen += w;
                }

                SpanKind::Break => {}
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn emit_text_run(
        &self,
        items: &mut Vec<DisplayItem>,
        span: &Span,
        slice: &str,
        glyphs: Vec<Glyph>,
        x: f64,
        baseline: f64,
        width: f64,
        source: SourceRef,
    ) {
        let (glyphs, x, width) = normalise_run(glyphs, x, width);
        let style = &span.style;
        let metrics = self.metrics_for(style);
        let size = style.font_size;

        // Highlight sits behind the glyphs, so it is emitted first.
        if let Some(background) = style.background {
            items.push(DisplayItem::Rect(RectItem {
                rect: Rect::new(
                    x,
                    baseline - metrics.ascender * size,
                    width,
                    (metrics.ascender - metrics.descender) * size,
                ),
                fill: Some(background),
                source: Some(source.clone()),
                ..Default::default()
            }));
        }

        items.push(DisplayItem::Glyphs(GlyphRun {
            font: span.font.map_or(0, |f| f.0),
            size,
            fill: style.color,
            x,
            y: baseline - style.baseline_shift,
            width,
            glyphs,
            text: slice.to_string(),
            source: Some(source.clone()),
        }));

        let mut decoration = |offset: f64, thickness: f64| {
            items.push(DisplayItem::Line(LineItem {
                x1: x,
                y1: baseline + offset,
                x2: x + width,
                y2: baseline + offset,
                stroke: Stroke {
                    color: style.color,
                    width: thickness,
                    dash: None,
                },
                source: Some(source.clone()),
            }));
        };

        if style.underline {
            decoration(
                -metrics.underline_position * size - style.baseline_shift,
                metrics.underline_thickness * size,
            );
        }
        if style.strikethrough {
            decoration(
                -metrics.strikeout_position * size - style.baseline_shift,
                metrics.strikeout_thickness * size,
            );
        }
    }

    fn emit_marker(
        &self,
        items: &mut Vec<DisplayItem>,
        shape: &MarkerShape,
        x: f64,
        baseline: f64,
        block_index: u32,
        frame_source: &SourceRef,
    ) {
        if shape.glyphs.is_empty() {
            return;
        }

        let mut pen = 0.0;
        let glyphs = shape
            .glyphs
            .iter()
            .map(|glyph| {
                let out = Glyph {
                    id: glyph.id,
                    x: pen + glyph.x_offset,
                    y: glyph.y_offset,
                    advance: glyph.x_advance,
                    cluster: glyph.cluster,
                };
                pen += glyph.x_advance;
                out
            })
            .collect();

        items.push(DisplayItem::Glyphs(GlyphRun {
            font: shape.font.map_or(0, |f| f.0),
            size: shape.style.font_size,
            fill: shape.style.color,
            x,
            y: baseline,
            width: shape.width,
            glyphs,
            text: shape.text.clone(),
            source: Some(SourceRef {
                block: Some(block_index),
                ..frame_source.clone()
            }),
        }));
    }

    fn shape_marker(&self, marker: &Marker, parent: &ResolvedStyle) -> MarkerShape {
        let style = cascade::resolve(parent, self.styles, None, marker.style.as_ref());
        let font = self
            .registry
            .select(style.font_family.as_deref(), style.font_weight, style.font_style);

        let glyphs = font
            .and_then(|id| self.registry.face(id))
            .map(|face| {
                shape_text(
                    face,
                    &marker.text,
                    style.font_size,
                    style.letter_spacing,
                    0.0,
                )
            })
            .unwrap_or_default();

        let width: f64 = glyphs.iter().map(|g| g.x_advance).sum();
        let gap = marker.gap.map_or(style.font_size * 0.35, Len::get);
        let column = marker.width.map_or(width + gap, Len::get);

        MarkerShape {
            text: marker.text.clone(),
            glyphs,
            width,
            column,
            font,
            style,
        }
    }

    // ── Continuation ─────────────────────────────────────────────────────────

    /// Build the paragraph that carries whatever starts at byte `offset`.
    ///
    /// The result records where it came from, so a run painted three frames
    /// down the thread still reports the original block, inline and offset.
    fn remainder_of(
        &self,
        para: &Paragraph,
        spans: &[Span],
        offset: usize,
        origin: Origin,
    ) -> Option<Paragraph> {
        let mut content = Vec::new();
        // The inline each kept element came from, so trimming stays honest.
        let mut sources: Vec<u32> = Vec::new();
        // Byte offset into the first kept inline where the remainder starts.
        let mut cut_of_first = 0u32;

        for span in spans {
            if span.end <= offset {
                continue;
            }

            let original = para.content.get(span.inline_index as usize)?;

            if span.start >= offset {
                content.push(original.clone());
                sources.push(span.inline_index);
                continue;
            }

            // The break falls inside this span: keep only its tail.
            match (&span.kind, original) {
                (SpanKind::Text { text, source_chars, .. }, Inline::Text(run)) => {
                    let chars_before = text[..offset - span.start].chars().count();
                    if chars_before >= *source_chars {
                        continue;
                    }
                    let cut = run
                        .text
                        .char_indices()
                        .nth(chars_before)
                        .map_or(run.text.len(), |(index, _)| index);

                    if content.is_empty() {
                        cut_of_first = cut as u32;
                    }
                    content.push(Inline::Text(TextRun {
                        text: run.text[cut..].to_string(),
                        ..run.clone()
                    }));
                    sources.push(span.inline_index);
                }
                _ => {
                    content.push(original.clone());
                    sources.push(span.inline_index);
                }
            }
        }

        // Drop leading whitespace so the continued line does not start indented.
        if let Some(Inline::Text(run)) = content.first_mut() {
            let dropped = run.text.len() - run.text.trim_start().len();
            run.text = run.text[dropped..].to_string();
            cut_of_first += dropped as u32;

            if run.text.is_empty() {
                content.remove(0);
                sources.remove(0);
                cut_of_first = 0;
            }
        }

        let first_inline = *sources.first()?;

        let mut style = para.style.clone().unwrap_or_default();
        // A continuation must not repeat the first-line indent.
        style.indent_first = Some(Len::ZERO);
        style.space_before = Some(Len::ZERO);

        Some(Paragraph {
            marker: None,
            style: Some(style),
            content,
            origin: Some(Origin {
                block: origin.block,
                inline: origin.inline + first_inline,
                // Only the paragraph's very first inline inherits a byte shift.
                offset: if first_inline == 0 {
                    origin.offset + cut_of_first
                } else {
                    cut_of_first
                },
            }),
            ..para.clone()
        })
    }
}

/// Make a run's numbers self-consistent before it is published.
///
/// Justification widens the gaps at break opportunities by advancing the pen,
/// which would otherwise leave `advance` meaning something different from
/// "distance to the next glyph". Both consumers rely on that equivalence — the
/// PDF emitter turns it into `TJ` offsets, the editor scans it to place a
/// caret — so the extra space is folded back into the glyph it follows.
///
/// Returns the corrected glyphs together with the run's adjusted origin and
/// width: a gap landing before the first glyph simply moves the run right.
fn normalise_run(mut glyphs: Vec<Glyph>, mut x: f64, mut width: f64) -> (Vec<Glyph>, f64, f64) {
    let Some(first) = glyphs.first().copied() else {
        return (glyphs, x, width);
    };

    if first.x != 0.0 {
        x += first.x;
        width -= first.x;
        for glyph in &mut glyphs {
            glyph.x -= first.x;
        }
    }

    for index in 0..glyphs.len().saturating_sub(1) {
        glyphs[index].advance = glyphs[index + 1].x - glyphs[index].x;
    }

    if let Some(last) = glyphs.last() {
        width = last.x + last.advance;
    }

    (glyphs, x, width)
}

/// Provenance for a span, expressed in the owning document's coordinates.
fn source_for(frame: &SourceRef, origin: Origin, span: &Span, local_offset: usize) -> SourceRef {
    let shift = if span.inline_index == 0 {
        origin.offset
    } else {
        0
    };
    frame.clone().at(
        origin.block,
        origin.inline + span.inline_index,
        shift + local_offset as u32,
    )
}

struct MarkerShape {
    text: String,
    glyphs: Vec<ShapedGlyph>,
    width: f64,
    /// Total horizontal space the marker reserves, including its gap.
    column: f64,
    font: Option<FontId>,
    style: ResolvedStyle,
}

/// Metrics used when no font is available, so layout degrades instead of
/// panicking. Roughly Helvetica's proportions.
const FALLBACK_METRICS: FaceMetrics = FaceMetrics {
    units_per_em: 1000.0,
    ascender: 0.75,
    descender: -0.25,
    line_gap: 0.0,
    cap_height: 0.7,
    x_height: 0.52,
    underline_position: -0.1,
    underline_thickness: 0.05,
    strikeout_position: 0.26,
    strikeout_thickness: 0.05,
    italic_angle: 0.0,
};

// ─────────────────────────────────────────────────────────────────────────────
// Free functions
// ─────────────────────────────────────────────────────────────────────────────

fn width_of(spans: &[Span], a: usize, b: usize) -> f64 {
    spans.iter().map(|span| span.width_in(a, b)).sum()
}

/// Byte offset where the trailing whitespace of `[start, end)` begins.
fn trim_end_of(text: &str, start: usize, end: usize) -> usize {
    start + text[start..end].trim_end().len()
}

fn build_pieces(text: &str, spans: &[Span]) -> Vec<Piece> {
    if text.is_empty() {
        return Vec::new();
    }

    let mut pieces = Vec::new();
    let mut start = 0usize;

    for (end, opportunity) in linebreaks(text) {
        if end <= start {
            continue;
        }
        let trimmed = trim_end_of(text, start, end);
        pieces.push(Piece {
            start,
            end,
            width: width_of(spans, start, end),
            trimmed_width: width_of(spans, start, trimmed),
            mandatory: opportunity == BreakOpportunity::Mandatory && end < text.len(),
        });
        start = end;
    }

    pieces
}

/// The vertical extent a line asks the wrap about.
///
/// `LineBox` is the whole line box, leading included; `InkBox` is only as far
/// as the glyphs actually rise and fall, centred inside it. The tighter band
/// lets text sit closer to a shape, and risks a tall accent meeting a part of
/// the shape the band never consulted.
fn band_of(mode: BandMode, top: f64, height: f64, ink: f64) -> (f64, f64) {
    match mode {
        BandMode::LineBox => (top, top + height),
        BandMode::InkBox => {
            let half_leading = ((height - ink) / 2.0).max(0.0);
            (top + half_leading, top + height - half_leading)
        }
    }
}

/// Usable text width inside a slot, once the indents are taken out.
fn usable(slot: Interval, left: f64, right: f64, first_extra: f64, first: bool) -> f64 {
    (slot.width() - left - right - if first { first_extra } else { 0.0 }).max(1.0)
}

/// Fill one line, greedily, starting at piece `from` and byte `start`.
///
/// Returns the line and the piece the next line begins at. Pulled out of
/// `break_into_lines` so that a caller which needs the line's vertical
/// position before choosing the next width — text flowing around a picture —
/// can drive the same filling one line at a time.
fn break_one_line(
    text: &str,
    pieces: &[Piece],
    from: usize,
    start: usize,
    limit: f64,
) -> (LineRange, usize) {
    let mut width = 0.0f64;
    let mut index = from;

    while index < pieces.len() {
        let piece = &pieces[index];

        if width > 0.0 && width + piece.trimmed_width > limit + FIT_EPSILON {
            return (
                LineRange {
                    start,
                    end: piece.start,
                    hard_end: false,
                },
                index,
            );
        }

        width += piece.width;

        if piece.mandatory {
            return (
                LineRange {
                    start,
                    end: piece.end,
                    hard_end: true,
                },
                index + 1,
            );
        }

        index += 1;
    }

    (
        LineRange {
            start,
            end: text.len(),
            hard_end: true,
        },
        pieces.len(),
    )
}

/// Next default tab stop, every 4 em from the column's left edge.
fn next_tab_stop(x: f64, font_size: f64) -> f64 {
    let stop = font_size * 4.0;
    ((x / stop).floor() + 1.0) * stop
}

#[cfg(test)]
mod band_tests {
    use super::*;

    #[test]
    fn the_line_box_band_is_the_whole_line() {
        let (top, bottom) = band_of(BandMode::LineBox, 100.0, 14.0, 10.0);
        assert_eq!((top, bottom), (100.0, 114.0));
    }

    #[test]
    fn the_ink_box_band_drops_the_leading_evenly() {
        // 14 tall, 10 of ink: 2 of leading above and 2 below.
        let (top, bottom) = band_of(BandMode::InkBox, 100.0, 14.0, 10.0);
        assert_eq!((top, bottom), (102.0, 112.0));
    }

    #[test]
    fn ink_taller_than_the_line_never_inverts_the_band() {
        // An inline image can out-measure the leading. The band must not fold.
        let (top, bottom) = band_of(BandMode::InkBox, 100.0, 10.0, 30.0);
        assert!(bottom >= top, "faixa invertida: {top}..{bottom}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fonts::{FontRegistry, test_fonts};
    use crate::spec::{Block, FontWeight};

    struct Harness {
        registry: FontRegistry,
        images: ImageStore,
        styles: BTreeMap<String, Style>,
    }

    impl Harness {
        fn new() -> Option<Harness> {
            let mut registry = FontRegistry::new();
            registry.add("body", test_fonts::dejavu()?.to_vec(), None, None).ok()?;
            if let Some(bold) = test_fonts::dejavu_bold() {
                registry
                    .add("body", bold.to_vec(), Some(FontWeight::BOLD), Some(false))
                    .ok()?;
            }
            Some(Harness {
                registry,
                images: ImageStore::new(),
                styles: BTreeMap::new(),
            })
        }

        fn layouter(&self) -> TextLayouter<'_> {
            TextLayouter {
                registry: &self.registry,
                images: &self.images,
                styles: &self.styles,
                variables: Variables { page: 1, pages: 1 },
            }
        }

        fn run(&self, json: &str, width: f64) -> ParagraphLayout {
            self.run_capped(json, width, None)
        }

        fn run_capped(&self, json: &str, width: f64, max: Option<f64>) -> ParagraphLayout {
            let block: Block = serde_json::from_str(json).unwrap();
            let para = block.as_paragraph().unwrap().clone();
            self.layouter().layout_paragraph(
                &para,
                &ResolvedStyle::default(),
                &super::super::wrap::WholeColumn { width },
                max,
                0,
                &SourceRef::frame(0, "f1"),
            )
        }
    }

    fn runs(layout: &ParagraphLayout) -> Vec<&GlyphRun> {
        layout
            .items
            .iter()
            .filter_map(|item| match item {
                DisplayItem::Glyphs(run) => Some(run),
                _ => None,
            })
            .collect()
    }

    /// Right edge of a run ignoring trailing whitespace. A space at a line
    /// break is kept in the run (the caret must be able to sit after it) but is
    /// allowed to hang past the margin, exactly as it does in print.
    fn visible_right(run: &GlyphRun) -> f64 {
        let trimmed = run.text.trim_end().len();
        run.glyphs
            .iter()
            .filter(|g| (g.cluster as usize) < trimmed)
            .map(|g| run.x + g.x + g.advance)
            .fold(run.x, f64::max)
    }

    /// Distinct baselines, i.e. how many lines were actually drawn.
    fn baselines(layout: &ParagraphLayout) -> Vec<f64> {
        let mut ys: Vec<f64> = runs(layout).iter().map(|r| r.y).collect();
        ys.dedup();
        ys
    }

    #[test]
    fn short_text_stays_on_one_line() {
        let Some(h) = Harness::new() else { return };
        let layout = h.run(r#""Olá mundo""#, 400.0);
        assert_eq!(layout.line_count, 1);
        assert_eq!(baselines(&layout).len(), 1);
        assert!(layout.remainder.is_none());
        assert!(layout.height > 0.0);
    }

    #[test]
    fn long_text_wraps_within_the_column() {
        let Some(h) = Harness::new() else { return };
        let json = r#""As plantas convertem a luz solar em energia química através da fotossíntese, um processo essencial para a vida no planeta.""#;
        let layout = h.run(json, 150.0);
        assert!(layout.line_count > 2, "expected wrapping, got {}", layout.line_count);

        // No visible glyph may extend past the column.
        for run in runs(&layout) {
            assert!(visible_right(run) <= 150.0 + 1.0, "run overflowed: {run:?}");
        }
    }

    #[test]
    fn narrower_columns_produce_more_lines() {
        let Some(h) = Harness::new() else { return };
        let json = r#""palavra outra terceira quarta quinta sexta sétima oitava nona""#;
        let wide = h.run(json, 400.0).line_count;
        let narrow = h.run(json, 120.0).line_count;
        assert!(narrow > wide);
    }

    #[test]
    fn explicit_break_forces_a_new_line() {
        let Some(h) = Harness::new() else { return };
        let layout = h.run(
            r#"{"type":"paragraph","content":["um",{"type":"break"},"dois"]}"#,
            400.0,
        );
        assert_eq!(layout.line_count, 2);
        let ys = baselines(&layout);
        assert_eq!(ys.len(), 2);
        assert!(ys[1] > ys[0]);
    }

    #[test]
    fn empty_paragraph_still_occupies_a_line() {
        let Some(h) = Harness::new() else { return };
        let layout = h.run(r#"{"type":"paragraph","content":[]}"#, 400.0);
        assert_eq!(layout.line_count, 1);
        assert!(layout.height > 0.0);
        assert!(runs(&layout).is_empty());
    }

    #[test]
    fn alignment_moves_the_line_horizontally() {
        let Some(h) = Harness::new() else { return };
        let text = "curto";
        let left = h.run(&format!(r#"{{"type":"paragraph","content":["{text}"]}}"#), 300.0);
        let centre = h.run(
            &format!(r#"{{"type":"paragraph","style":{{"textAlign":"center"}},"content":["{text}"]}}"#),
            300.0,
        );
        let right = h.run(
            &format!(r#"{{"type":"paragraph","style":{{"textAlign":"right"}},"content":["{text}"]}}"#),
            300.0,
        );

        let x = |l: &ParagraphLayout| runs(l)[0].x;
        assert!((x(&left) - 0.0).abs() < 0.01);
        assert!(x(&centre) > x(&left));
        assert!(x(&right) > x(&centre));

        let width = runs(&left)[0].width;
        assert!((x(&right) + width - 300.0).abs() < 0.5);
        assert!((x(&centre) - (300.0 - width) / 2.0).abs() < 0.5);
    }

    #[test]
    fn justification_stretches_every_line_but_the_last() {
        let Some(h) = Harness::new() else { return };
        let json = r#"{"type":"paragraph","style":{"textAlign":"justify"},"content":["As plantas convertem a luz solar em energia química através da fotossíntese, essencial para a vida."]}"#;
        let layout = h.run(json, 180.0);
        assert!(layout.line_count >= 3);

        let by_line = group_by_baseline(&layout);
        // Every line except the last must reach the right edge.
        for line in &by_line[..by_line.len() - 1] {
            let right = line.iter().map(visible_right).fold(0.0, f64::max);
            assert!((right - 180.0).abs() < 1.0, "line not justified: {right}");
        }
        let last = by_line.last().unwrap();
        let last_right = last.iter().map(visible_right).fold(0.0, f64::max);
        assert!(last_right < 180.0 - 1.0, "last line should stay ragged");
    }

    fn group_by_baseline(layout: &ParagraphLayout) -> Vec<Vec<GlyphRun>> {
        let mut out: Vec<Vec<GlyphRun>> = Vec::new();
        for run in runs(layout) {
            match out.last_mut() {
                Some(line) if (line[0].y - run.y).abs() < 0.01 => line.push(run.clone()),
                _ => out.push(vec![run.clone()]),
            }
        }
        out
    }

    #[test]
    fn a_style_change_splits_the_run_but_not_the_line() {
        let Some(h) = Harness::new() else { return };
        let layout = h.run(
            r#"{"type":"paragraph","content":["normal ",{"type":"text","text":"negrito","style":{"fontWeight":"bold"}}," normal"]}"#,
            400.0,
        );
        let runs = runs(&layout);
        assert_eq!(runs.len(), 3);
        assert_eq!(baselines(&layout).len(), 1);
        // Runs are laid end to end, in order.
        assert!(runs[0].x < runs[1].x && runs[1].x < runs[2].x);
        assert!((runs[0].x + runs[0].width - runs[1].x).abs() < 0.01);
    }

    #[test]
    fn glyph_offsets_are_cumulative_within_a_run() {
        let Some(h) = Harness::new() else { return };
        let layout = h.run(r#""abcdef""#, 400.0);
        let run = runs(&layout)[0];
        assert_eq!(run.glyphs[0].x, 0.0);
        for pair in run.glyphs.windows(2) {
            assert!((pair[0].x + pair[0].advance - pair[1].x).abs() < 1e-9);
        }
        let last = run.glyphs.last().unwrap();
        assert!((last.x + last.advance - run.width).abs() < 1e-9);
    }

    #[test]
    fn runs_carry_provenance_back_to_the_source_inline() {
        let Some(h) = Harness::new() else { return };
        let layout = h.run(
            r#"{"type":"paragraph","content":["um ",{"type":"text","text":"dois","style":{"fontWeight":"bold"}}]}"#,
            400.0,
        );
        let runs = runs(&layout);
        let first = runs[0].source.as_ref().unwrap();
        assert_eq!(first.frame, "f1");
        assert_eq!(first.block, Some(0));
        assert_eq!(first.inline, Some(0));

        let second = runs[1].source.as_ref().unwrap();
        assert_eq!(second.inline, Some(1));
        assert_eq!(runs[1].text, "dois");
    }

    #[test]
    fn wrapped_runs_report_their_offset_into_the_inline() {
        let Some(h) = Harness::new() else { return };
        let layout = h.run(r#""alpha bravo charlie delta echo foxtrot golf""#, 90.0);
        assert!(layout.line_count > 1);

        let runs = runs(&layout);
        // The second line starts partway through the single source inline.
        let second = runs[1].source.as_ref().unwrap();
        assert_eq!(second.inline, Some(0));
        assert!(second.offset.unwrap() > 0);
        // Its text must match the slice at that offset.
        let full = "alpha bravo charlie delta echo foxtrot golf";
        let offset = second.offset.unwrap() as usize;
        assert!(full[offset..].starts_with(&runs[1].text));
    }

    #[test]
    fn underline_emits_a_line_under_the_baseline() {
        let Some(h) = Harness::new() else { return };
        let layout = h.run(
            r#"{"type":"paragraph","content":[{"type":"text","text":"sublinhado","style":{"underline":true}}]}"#,
            400.0,
        );
        let run = runs(&layout)[0];
        let line = layout
            .items
            .iter()
            .find_map(|i| match i {
                DisplayItem::Line(l) => Some(l),
                _ => None,
            })
            .expect("underline emitted");
        assert!(line.y1 > run.y, "underline must sit below the baseline");
        assert!((line.x2 - line.x1 - run.width).abs() < 0.01);
    }

    #[test]
    fn highlight_is_painted_behind_the_glyphs() {
        let Some(h) = Harness::new() else { return };
        let layout = h.run(
            r##"{"type":"paragraph","content":[{"type":"text","text":"marcado","style":{"background":"#ff0"}}]}"##,
            400.0,
        );
        let rect_index = layout
            .items
            .iter()
            .position(|i| matches!(i, DisplayItem::Rect(_)))
            .expect("highlight emitted");
        let run_index = layout
            .items
            .iter()
            .position(|i| matches!(i, DisplayItem::Glyphs(_)))
            .unwrap();
        assert!(rect_index < run_index, "highlight must be painted first");
    }

    #[test]
    fn an_inline_rule_with_no_width_fills_the_line() {
        let Some(h) = Harness::new() else { return };
        let layout = h.run(
            r#"{"type":"paragraph","content":["Nome: ",{"type":"rule"}]}"#,
            300.0,
        );
        let line = layout
            .items
            .iter()
            .find_map(|i| match i {
                DisplayItem::Line(l) => Some(l),
                _ => None,
            })
            .expect("rule emitted");
        assert!((line.x2 - 300.0).abs() < 0.5, "rule should reach the margin");
        assert!(line.x1 > 0.0, "rule should start after the label");
    }

    #[test]
    fn a_marker_indents_the_text_and_hangs() {
        let Some(h) = Harness::new() else { return };
        let plain = h.run(r#""texto que precisa quebrar em varias linhas aqui""#, 120.0);
        let marked = h.run(
            r#"{"type":"paragraph","marker":{"text":"a)"},"content":["texto que precisa quebrar em varias linhas aqui"]}"#,
            120.0,
        );

        let marker_run = &runs(&marked)[0];
        assert_eq!(marker_run.text, "a)");
        assert!((marker_run.x - 0.0).abs() < 0.01, "marker sits at the margin");

        // Every text line starts past the marker column.
        let text_runs: Vec<_> = runs(&marked).into_iter().skip(1).collect();
        assert!(text_runs.iter().all(|r| r.x > marker_run.width));
        // Hanging indent costs width, so it wraps at least as much as plain text.
        assert!(marked.line_count >= plain.line_count);
    }

    #[test]
    fn max_height_splits_the_paragraph_and_returns_the_rest() {
        let Some(h) = Harness::new() else { return };
        let json = r#""alpha bravo charlie delta echo foxtrot golf hotel india juliet kilo lima""#;
        let full = h.run(json, 120.0);
        assert!(full.line_count >= 4);

        let line_height = full.height / full.line_count as f64;
        let capped = h.run_capped(json, 120.0, Some(line_height * 2.5));

        assert_eq!(capped.line_count, 2);
        assert!(capped.height <= line_height * 2.5 + 0.01);

        // The tail must continue exactly where the placed part stopped.
        let placed: String = runs(&capped).iter().map(|r| r.text.as_str()).collect();
        let remainder = capped.remainder.clone().expect("remainder returned");
        let full_text = "alpha bravo charlie delta echo foxtrot golf hotel india juliet kilo lima";
        assert!(full_text.starts_with(placed.trim_end()));
        assert!(full_text.ends_with(&remainder.plain_text()));
    }

    #[test]
    fn a_paragraph_that_cannot_fit_one_line_is_moved_whole() {
        let Some(h) = Harness::new() else { return };
        let capped = h.run_capped(r#""qualquer texto""#, 200.0, Some(1.0));
        assert_eq!(capped.line_count, 0);
        assert!(capped.items.is_empty());
        assert!(capped.remainder.is_some());
    }

    #[test]
    fn space_before_and_after_extend_the_paragraph_box() {
        let Some(h) = Harness::new() else { return };
        let plain = h.run(r#""texto""#, 300.0);
        let spaced = h.run(
            r#"{"type":"paragraph","style":{"spaceBefore":10,"spaceAfter":20},"content":["texto"]}"#,
            300.0,
        );
        assert!((spaced.height - plain.height - 30.0).abs() < 0.01);
        assert!((runs(&spaced)[0].y - runs(&plain)[0].y - 10.0).abs() < 0.01);
    }

    #[test]
    fn line_height_controls_the_baseline_pitch() {
        let Some(h) = Harness::new() else { return };
        let tight = h.run(
            r#"{"type":"paragraph","style":{"lineHeight":1.0},"content":["um",{"type":"break"},"dois"]}"#,
            300.0,
        );
        let loose = h.run(
            r#"{"type":"paragraph","style":{"lineHeight":2.0},"content":["um",{"type":"break"},"dois"]}"#,
            300.0,
        );

        let pitch = |l: &ParagraphLayout| {
            let ys = baselines(l);
            ys[1] - ys[0]
        };
        assert!((pitch(&loose) - pitch(&tight) * 2.0).abs() < 0.01);
        assert!((loose.height - tight.height * 2.0).abs() < 0.01);
    }

    #[test]
    fn an_inline_image_reserves_its_box_on_the_line() {
        let Some(h) = Harness::new() else { return };
        let layout = h.run(
            r#"{"type":"paragraph","content":["antes ",{"type":"image","src":"x.png","width":40,"height":20}," depois"]}"#,
            400.0,
        );
        let image = layout
            .items
            .iter()
            .find_map(|i| match i {
                DisplayItem::Image(img) => Some(img),
                _ => None,
            })
            .expect("image emitted");
        assert_eq!(image.rect.w, 40.0);
        assert_eq!(image.rect.h, 20.0);

        let after = runs(&layout).into_iter().find(|r| r.text.contains("depois")).unwrap();
        assert!(after.x >= image.rect.right() - 0.01, "text must follow the image");
    }

    #[test]
    fn tab_advances_to_an_explicit_stop() {
        let Some(h) = Harness::new() else { return };
        let layout = h.run(
            r#"{"type":"paragraph","content":["a",{"type":"tab","to":100},"b"]}"#,
            400.0,
        );
        let after = runs(&layout).into_iter().find(|r| r.text == "b").unwrap();
        assert!((after.x - 100.0).abs() < 0.01);
    }
}
