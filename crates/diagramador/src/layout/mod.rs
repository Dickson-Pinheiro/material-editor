//! The layout engine: [`Document`] → [`DisplayList`].
//!
//! Walks pages, stamps masters, resolves stories, flows text through columns
//! and threaded frames, and emits final coordinates. Nothing downstream makes
//! a layout decision — this module is the sole authority.

pub mod cascade;
pub(crate) mod grid;
pub mod shape;
pub(crate) mod scale;
pub(crate) mod table;
pub(crate) mod ticks;
mod text;
pub mod wrap;

use std::collections::{BTreeMap, HashMap};

use crate::display::{
    ClipShape, Diagnostic, DisplayFont, DisplayFrame, DisplayGroup, DisplayItem, DisplayList,
    DisplayPage, EllipseItem, ImageItem, LineItem, RectItem, SourceRef, Stroke,
};
use crate::fonts::FontRegistry;
use crate::images::ImageStore;
use crate::spec::{
    Block, Border, Document, Frame, FrameContent, ImageFit, ImageFrame, Origin, Overflow, Page,
    ResolvedStyle, ShapeKind, Style, TextFrame, VerticalAlign,
};
use crate::units::{PT_PER_PX, Rect};

use text::{TextLayouter, Variables, uses_total_pages};

/// Guard against a threading cycle (`a → b → a`) looping forever.
const MAX_THREAD_HOPS: usize = 512;

/// Ceiling on pages produced by `autoFlow`. Reaching it means something is
/// wrong with the document, not that the book is long.
const MAX_AUTO_PAGES: u32 = 4096;

/// Extra layout passes allowed while `{pages}` settles on a value.
const MAX_TOTAL_PASSES: usize = 2;

/// Narrowest gap, in ems, that a wrap will offer to text.
///
/// One em was the first guess and it is too generous: a gap that fits a single
/// character produces a column of orphaned letters down the side of a picture,
/// which is worse than no text there at all. Three ems holds a short word.
const MIN_SLOT_EM: f64 = 3.0;

// ─────────────────────────────────────────────────────────────────────────────
// Engine
// ─────────────────────────────────────────────────────────────────────────────

pub struct LayoutEngine<'a> {
    registry: &'a FontRegistry,
    images: &'a ImageStore,
    /// Total pages, substituted for `{pages}`. Only correct once auto-flow has
    /// settled, which is why [`LayoutEngine::layout`] may run more than once.
    page_count: u32,
}

impl<'a> LayoutEngine<'a> {
    pub fn new(registry: &'a FontRegistry, images: &'a ImageStore) -> Self {
        LayoutEngine {
            registry,
            images,
            page_count: 0,
        }
    }

    /// Lay out a whole document.
    ///
    /// Never fails: problems become [`Diagnostic`]s on the display list, so the
    /// editor can show a broken document rather than nothing at all.
    ///
    /// A document that prints `{pages}` needs the total before it can be laid
    /// out, and the total is a result of laying it out. The knot is untied by
    /// repeating until the count stops changing — twice over, at most, because
    /// a running header does not repaginate when "9" becomes "10".
    pub fn layout(&self, document: &Document) -> DisplayList {
        let mut total = (document.pages.len() as u32).max(1);
        let mut list = self.pass(document, total);

        if wants_total_pages(document) {
            for _ in 0..MAX_TOTAL_PASSES {
                let produced = list.pages.len() as u32;
                if produced == total {
                    break;
                }
                total = produced;
                list = self.pass(document, total);
            }
        }

        list
    }

    fn pass(&self, document: &Document, page_count: u32) -> DisplayList {
        let engine = LayoutEngine {
            registry: self.registry,
            images: self.images,
            page_count,
        };
        engine.layout_once(document)
    }

    fn layout_once(&self, document: &Document) -> DisplayList {
        let mut doc = document.clone();
        assign_frame_ids(&mut doc);

        let mut list = DisplayList::new();
        list.fonts = self.font_table();

        if self.registry.is_empty() {
            list.diagnostics.push(Diagnostic::error(
                "noFont",
                "nenhuma fonte registrada: chame add_font antes de renderizar",
            ));
        }

        let styles = doc.resources.styles.clone();
        let root_style = ResolvedStyle::default().apply(&doc.style);

        // Content waiting to flow into a frame, keyed by that frame's id.
        // The story name travels with it so provenance survives the whole chain.
        let mut pending: HashMap<String, PendingFlow> = HashMap::new();
        let mut auto = AutoFlow::default();

        // Indexed rather than iterated: `autoFlow` appends pages as it goes, and
        // the page it appends is the next one laid out.
        let mut index = 0usize;
        while index < doc.pages.len() {
            let rendered = self.layout_page(
                &doc,
                index,
                &styles,
                &root_style,
                &mut pending,
                &mut auto,
                &mut list.diagnostics,
            );
            list.pages.push(rendered);

            // The immutable borrow of `doc` ends here, so the requested pages
            // can be spliced in right after the one that overflowed.
            for (offset, mut frame) in auto.requests.drain(..).enumerate() {
                let position = index + 1 + offset;
                let template = &doc.pages[index];

                let page = Page {
                    id: frame.id.clone(),
                    name: None,
                    size: template.size,
                    margins: template.margins,
                    master: template.master.clone(),
                    background: template.background,
                    style: template.style.clone(),
                    frames: Vec::new(),
                };

                // On facing pages the text block follows the gutter. The frame
                // being cloned may already sit mirrored, so the test is whether
                // the side *changes* — not what the new page's parity is.
                if doc.page.facing && index % 2 != position % 2 {
                    let width = doc.geometry_of(&page, position).size.width;
                    frame.rect.x = width - frame.rect.x - frame.rect.w;
                }

                doc.pages.insert(
                    position,
                    Page {
                        frames: vec![frame],
                        ..page
                    },
                );
            }

            index += 1;
        }

        if auto.generated >= MAX_AUTO_PAGES {
            list.diagnostics.push(Diagnostic::warning(
                "autoFlowLimit",
                format!("limite de {MAX_AUTO_PAGES} páginas geradas atingido"),
            ));
        }

        // Anything still queued had nowhere to go.
        for (frame, flow) in pending {
            if !flow.blocks.is_empty() {
                list.diagnostics.push(
                    Diagnostic::warning(
                        "overset",
                        format!("conteúdo destinado ao frame `{frame}` não foi colocado"),
                    ),
                );
            }
        }

        list
    }

    fn font_table(&self) -> Vec<DisplayFont> {
        self.registry
            .faces()
            .iter()
            .enumerate()
            .map(|(index, face)| DisplayFont {
                id: index as u32,
                family: face.family.clone(),
                weight: face.weight.0,
                italic: face.italic,
                post_script_name: face.post_script_name.clone(),
                units_per_em: face.metrics.units_per_em,
                ascender: face.metrics.ascender,
                descender: face.metrics.descender,
            })
            .collect()
    }

    #[allow(clippy::too_many_arguments)]
    fn layout_page(
        &self,
        doc: &Document,
        page_index: usize,
        styles: &BTreeMap<String, Style>,
        root_style: &ResolvedStyle,
        pending: &mut HashMap<String, PendingFlow>,
        auto: &mut AutoFlow,
        diagnostics: &mut Vec<Diagnostic>,
    ) -> DisplayPage {
        let page = &doc.pages[page_index];
        let index = page_index as u32;
        let geometry = doc.geometry_of(page, page_index);
        let master = page
            .master
            .as_ref()
            .and_then(|name| doc.resources.masters.get(name));

        if let (Some(name), None) = (page.master.as_ref(), master) {
            diagnostics.push(
                Diagnostic::warning("unknownMaster", format!("página mestre `{name}` não existe"))
                    .on(index, ""),
            );
        }

        let page_style = cascade::resolve(root_style, styles, None, page.style.as_ref());

        let mut out = DisplayPage {
            index,
            id: page.id.clone(),
            width: geometry.size.width,
            height: geometry.size.height,
            background: page.background.or_else(|| master.and_then(|m| m.background)),
            margin_box: geometry.margin_box(),
            frames: Vec::new(),
            items: Vec::new(),
        };

        // Master frames are painted beneath the page's own.
        let master_frames = master.map(|m| m.frames.as_slice()).unwrap_or_default();

        // Every wrap on the page, in page coordinates, before a single frame
        // is laid out. Nothing here depends on text, so one pass is enough.
        let obstacles = wrap::collect(&[master_frames, &page.frames], index, diagnostics);

        for frame in master_frames.iter().chain(page.frames.iter()) {
            self.layout_frame(
                doc,
                frame,
                index,
                0.0,
                0.0,
                styles,
                &page_style,
                &[],
                &obstacles,
                pending,
                auto,
                &mut out,
                diagnostics,
            );
        }

        out
    }

    #[allow(clippy::too_many_arguments)]
    fn layout_frame(
        &self,
        doc: &Document,
        frame: &Frame,
        page: u32,
        origin_x: f64,
        origin_y: f64,
        styles: &BTreeMap<String, Style>,
        parent_style: &ResolvedStyle,
        ancestors: &[String],
        obstacles: &[wrap::Obstacle],
        pending: &mut HashMap<String, PendingFlow>,
        auto: &mut AutoFlow,
        out: &mut DisplayPage,
        diagnostics: &mut Vec<Diagnostic>,
    ) {
        if !frame.visible {
            return;
        }

        let id = frame.id.clone().unwrap_or_default();
        let rect = frame.rect.translate(origin_x, origin_y);
        let source = SourceRef::frame(page, id.clone());

        let mut items: Vec<DisplayItem> = Vec::new();
        let mut overset = false;
        let mut grown = rect;

        // ── Content ───────────────────────────────────────────────────────────
        let content_box = rect.deflate(frame.padding);

        match &frame.content {
            FrameContent::Text(tf) => {
                let (content, used_height, is_overset) = self.layout_text_frame(
                    doc, frame, tf, &id, page, content_box, styles, parent_style, obstacles,
                    pending, auto, diagnostics,
                );
                items.extend(content);
                overset = is_overset;

                if tf.overflow == Overflow::Grow && used_height > content_box.h {
                    grown.h = used_height + frame.padding.vertical();
                }
            }
            FrameContent::Image(image) => {
                items.extend(self.layout_image_frame(image, content_box, &source, diagnostics, page, &id));
            }
            FrameContent::Shape(shape) => {
                items.push(match shape.shape {
                    ShapeKind::Rect => DisplayItem::Rect(RectItem {
                        rect: content_box,
                        radius: frame.radius,
                        fill: frame.fill,
                        stroke: frame.border.as_ref().map(stroke_of),
                        source: Some(source.clone()),
                    }),
                    ShapeKind::Ellipse => DisplayItem::Ellipse(EllipseItem {
                        rect: content_box,
                        fill: frame.fill,
                        stroke: frame.border.as_ref().map(stroke_of),
                        source: Some(source.clone()),
                    }),
                    ShapeKind::Line => DisplayItem::Line(LineItem {
                        x1: content_box.x,
                        y1: content_box.y,
                        x2: content_box.right(),
                        y2: content_box.bottom(),
                        stroke: frame.border.as_ref().map(stroke_of).unwrap_or_default(),
                        source: Some(source.clone()),
                    }),
                });
            }
            FrameContent::Group(group) => {
                let mut nested = ancestors.to_vec();
                nested.push(id.clone());
                let group_style =
                    cascade::resolve(parent_style, styles, None, None);

                // Children are positioned relative to the group's own corner.
                let mut inner = DisplayPage {
                    index: out.index,
                    ..Default::default()
                };
                for child in &group.children {
                    self.layout_frame(
                        doc, child, page, rect.x, rect.y, styles, &group_style, &nested, obstacles,
                        pending, auto, &mut inner, diagnostics,
                    );
                }
                items.extend(inner.items);
                out.frames.extend(inner.frames);
            }
        }

        // ── Assemble ──────────────────────────────────────────────────────────
        // Background and border stay outside the clip: a stroke centred on the
        // frame edge would otherwise lose its outer half.
        let mut painted: Vec<DisplayItem> = Vec::new();

        // A shape's fill belongs to the shape. Painting a background box as well
        // would square off an ellipse and put a slab behind a line.
        if let Some(fill) = frame.fill
            && !matches!(frame.content, FrameContent::Shape(_))
        {
            painted.push(DisplayItem::Rect(RectItem {
                rect: grown,
                radius: frame.radius,
                fill: Some(fill),
                stroke: None,
                source: Some(source.clone()),
            }));
        }

        let needs_clip = frame.clip
            || matches!(&frame.content, FrameContent::Text(t) if t.overflow == Overflow::Clip)
            || matches!(&frame.content, FrameContent::Image(i) if i.fit == ImageFit::Cover);

        if needs_clip && !items.is_empty() {
            painted.push(DisplayItem::Group(DisplayGroup {
                clip: Some(ClipShape {
                    rect: grown,
                    radius: frame.radius,
                }),
                items,
                ..DisplayGroup::new()
            }));
        } else {
            painted.extend(items);
        }

        // Shapes stroke themselves; every other frame gets its border on top.
        if !matches!(frame.content, FrameContent::Shape(_))
            && let Some(border) = &frame.border
        {
            painted.extend(border_items(border, grown, frame.radius, &source));
        }

        let group = DisplayGroup {
            source: Some(source),
            transform: rotation_matrix(frame.rotation, grown),
            clip: None,
            opacity: frame.opacity,
            items: painted,
        };

        if group.is_pass_through() {
            out.items.extend(group.items);
        } else {
            out.items.push(DisplayItem::Group(group));
        }

        out.frames.push(DisplayFrame {
            id,
            name: frame.name.clone(),
            rect: grown,
            rotation: frame.rotation,
            kind: match &frame.content {
                FrameContent::Text(_) => "text",
                FrameContent::Image(_) => "image",
                FrameContent::Shape(_) => "shape",
                FrameContent::Group(_) => "group",
            }
            .to_string(),
            locked: frame.locked,
            overset,
            ancestors: ancestors.to_vec(),
        });
    }

    // ── Text frames ──────────────────────────────────────────────────────────

    #[allow(clippy::too_many_arguments)]
    fn layout_text_frame(
        &self,
        doc: &Document,
        frame: &Frame,
        tf: &TextFrame,
        id: &str,
        page: u32,
        content_box: Rect,
        styles: &BTreeMap<String, Style>,
        parent_style: &ResolvedStyle,
        obstacles: &[wrap::Obstacle],
        pending: &mut HashMap<String, PendingFlow>,
        auto: &mut AutoFlow,
        diagnostics: &mut Vec<Diagnostic>,
    ) -> (Vec<DisplayItem>, f64, bool) {
        // Threaded content wins over the frame's own; that is what makes a
        // chain of frames behave as one continuous story.
        let PendingFlow {
            story,
            mut blocks,
            min_page,
        } = match pending.remove(id) {
            Some(carried) => carried,
            None => {
                let mut fresh = match &tf.story {
                    Some(name) => match doc.resources.stories.get(name) {
                        Some(story) => story.clone(),
                        None => {
                            diagnostics.push(
                                Diagnostic::warning(
                                    "unknownStory",
                                    format!("story `{name}` não existe"),
                                )
                                .on(page, id),
                            );
                            Vec::new()
                        }
                    },
                    None => tf.blocks.clone(),
                };
                // Stamp each paragraph with its index before anything can move.
                tag_origins(&mut fresh);
                PendingFlow {
                    story: tf.story.clone(),
                    blocks: fresh,
                    min_page: None,
                }
            }
        };

        // An explicit page break parked this content until a later page. This
        // frame is too early, so it paints nothing and hands it straight on.
        let deferred = min_page.is_some_and(|earliest| page < earliest);

        let style = cascade::resolve(parent_style, styles, tf.use_style.as_deref(), tf.style.as_ref());
        let layouter = TextLayouter {
            registry: self.registry,
            images: self.images,
            styles,
            variables: Variables {
                page: page + 1,
                pages: self.page_count,
            },
        };

        let columns = tf.columns.max(1);
        let gap = tf.column_gap.get();
        let column_width =
            ((content_box.w - gap * (columns - 1) as f64) / columns as f64).max(1.0);
        let unbounded = tf.overflow != Overflow::Clip;

        let mut items = Vec::new();
        let mut max_used = 0.0f64;
        let source = SourceRef {
            story: story.clone(),
            ..SourceRef::frame(page, id.to_string())
        };

        // Set when a break sends the rest past this frame entirely.
        let mut forced: Option<BreakKind> = None;
        let mut reported_wrap = false;

        for column in 0..columns {
            if blocks.is_empty() || deferred {
                break;
            }
            let column_box = Rect::new(
                content_box.x + (column_width + gap) * column as f64,
                content_box.y,
                column_width,
                content_box.h,
            );

            let FlowResult {
                items: mut column_items,
                used,
                leftover,
                stopped,
                walled_in,
                diagnostics: said,
            } = self.flow_blocks(
                &layouter,
                &blocks,
                &style,
                column_box,
                if unbounded { None } else { Some(content_box.h) },
                &source,
                if tf.ignore_wrap { &[] } else { obstacles },
            );

            let offset = match tf.vertical_align {
                VerticalAlign::Top | VerticalAlign::Justify => 0.0,
                VerticalAlign::Middle => (content_box.h - used).max(0.0) / 2.0,
                VerticalAlign::Bottom => (content_box.h - used).max(0.0),
            };
            if offset != 0.0 {
                translate_items(&mut column_items, 0.0, offset);
            }

            items.extend(column_items);
            max_used = max_used.max(used);
            blocks = leftover;
            diagnostics.extend(said);

            // Once per frame: ten paragraphs behind the same photograph are
            // one problem, not ten.
            if walled_in && !reported_wrap {
                reported_wrap = true;
                diagnostics.push(
                    Diagnostic::warning(
                        "wrapLeavesNoRoom",
                        "o contorno de um objeto não deixa espaço utilizável para o texto",
                    )
                    .on(page, id),
                );
            }

            // A column break just moves along; the other two leave the frame.
            if matches!(stopped, Some(BreakKind::Frame) | Some(BreakKind::Page)) {
                forced = stopped;
                break;
            }
        }

        // ── Where the rest goes ───────────────────────────────────────────────
        let mut overset = false;

        if !blocks.is_empty() {
            // A page break parks the content until after this page; a deferral
            // already in flight keeps whatever page it was waiting for.
            let carry_min_page = match forced {
                Some(BreakKind::Page) => Some(page + 1),
                _ if deferred => min_page,
                _ => None,
            };

            // An explicit chain wins over autoFlow.
            match &tf.thread_next {
                Some(next) if next != id && pending.len() < MAX_THREAD_HOPS => {
                    let flow = pending.entry(next.clone()).or_insert_with(|| PendingFlow {
                        story: story.clone(),
                        blocks: Vec::new(),
                        min_page: carry_min_page,
                    });
                    flow.blocks.extend(blocks);
                }
                Some(next) if next != id => {
                    overset = true;
                    diagnostics.push(
                        Diagnostic::warning("threadCycle", "cadeia de frames muito longa")
                            .on(page, id),
                    );
                }
                // Nowhere declared to go: make a page like this one and continue.
                // Only when this frame actually took content, or was told to
                // stand aside — otherwise the same page would repeat forever.
                _ if tf.auto_flow
                    && auto.generated < MAX_AUTO_PAGES
                    && (max_used > 0.0 || deferred) =>
                {
                    auto.generated += 1;
                    let continuation = format!("{id}~{}", auto.generated);

                    let mut next_frame = frame.clone();
                    next_frame.id = Some(continuation.clone());
                    next_frame.name = None;
                    if let FrameContent::Text(text) = &mut next_frame.content {
                        // The content arrives through `pending`, not from here.
                        text.blocks = Vec::new();
                        text.story = None;
                    }

                    pending.insert(
                        continuation,
                        PendingFlow {
                            story: story.clone(),
                            blocks,
                            min_page: carry_min_page,
                        },
                    );
                    auto.requests.push(next_frame);
                }
                _ => {
                    overset = true;
                    diagnostics.push(
                        Diagnostic::warning(
                            "overset",
                            "o conteúdo não cabe no frame e não há threadNext nem autoFlow",
                        )
                        .on(page, id),
                    );
                }
            }
        }

        (items, max_used, overset)
    }

    /// Stack blocks down a column, splitting the first one that does not fit.
    ///
    /// Returns what it drew, how tall that was, what is left, and — when an
    /// explicit break stopped it — which kind, so the caller knows whether to
    /// move to the next column, the next frame, or the next page.
    #[allow(clippy::too_many_arguments)]
    fn flow_blocks(
        &self,
        layouter: &TextLayouter<'_>,
        blocks: &[Block],
        style: &ResolvedStyle,
        column: Rect,
        max_height: Option<f64>,
        source: &SourceRef,
        obstacles: &[wrap::Obstacle],
    ) -> FlowResult {
        let mut items = Vec::new();
        let mut walled_in = false;
        let mut diagnostics = Vec::new();
        let mut y = 0.0f64;
        let budget = max_height.unwrap_or(f64::INFINITY);

        for (index, block) in blocks.iter().enumerate() {
            let remaining = budget - y;

            match block {
                Block::Paragraph(para) => {
                    // A paragraph lays itself out from its own top-left, so the
                    // space it asks about has to know where that corner landed
                    // on the page.
                    let whole = wrap::WholeColumn { width: column.w };
                    let carved = wrap::ColumnSpace {
                        obstacles,
                        column,
                        origin_y: column.y + y,
                        min_slot: (style.font_size * MIN_SLOT_EM).max(1.0),
                    };
                    let space: &dyn wrap::LineSpace =
                        if obstacles.is_empty() { &whole } else { &carved };

                    let layout = layouter.layout_paragraph(
                        para,
                        style,
                        space,
                        max_height.map(|_| remaining),
                        index as u32,
                        source,
                    );

                    walled_in |= layout.walled_in;

                    let mut placed = layout.items;
                    translate_items(&mut placed, column.x, column.y + y);
                    items.extend(placed);
                    y += layout.height;

                    if let Some(remainder) = layout.remainder {
                        let mut leftover = vec![Block::Paragraph(remainder)];
                        leftover.extend_from_slice(&blocks[index + 1..]);
                        return FlowResult { items, used: y, leftover, stopped: None, walled_in, diagnostics };
                    }
                }

                Block::Rule(rule) => {
                    let thickness = rule.thickness.map_or(0.75, |t| t.get());
                    if y + thickness > budget {
                        return FlowResult { items, used: y, leftover: blocks[index..].to_vec(), stopped: None, walled_in, diagnostics };
                    }
                    let width = column.w * rule.width.unwrap_or(1.0).clamp(0.0, 1.0);
                    items.push(DisplayItem::Line(LineItem {
                        x1: column.x,
                        y1: column.y + y + thickness / 2.0,
                        x2: column.x + width,
                        y2: column.y + y + thickness / 2.0,
                        stroke: Stroke {
                            color: rule.color.unwrap_or(style.color),
                            width: thickness,
                            dash: None,
                        },
                        source: Some(source.clone()),
                    }));
                    y += thickness;
                }

                Block::Spacer(spacer) => {
                    let height = spacer.height.get();
                    if y + height > budget && y > 0.0 {
                        return FlowResult { items, used: y, leftover: blocks[index..].to_vec(), stopped: None, walled_in, diagnostics };
                    }
                    y += height;
                }

                Block::Table(table_block) => {
                    let cells = CellFlow { engine: self, text: layouter };
                    let mut here = source.clone();
                    here.block = Some(index as u32);

                    // At the top of an empty column there is nowhere better to
                    // send a row that does not fit, so it goes out anyway.
                    let room = match max_height {
                        None => table::Room::Unlimited,
                        Some(_) if y > 0.0 => table::Room::Upto(remaining),
                        Some(_) => table::Room::AtLeast(remaining),
                    };

                    let laid = table::emit(
                        table_block,
                        style,
                        &cells,
                        Rect::new(column.x, column.y + y, column.w, 0.0),
                        room,
                        &here,
                    );

                    diagnostics.extend(diagnose(&laid, &here));
                    items.extend(laid.items);
                    y += laid.height;

                    if let Some(rest) = laid.leftover {
                        let mut over = vec![Block::Table(rest)];
                        over.extend_from_slice(&blocks[index + 1..]);
                        return FlowResult { items, used: y, leftover: over, stopped: None, walled_in, diagnostics };
                    }
                }

                Block::ColumnBreak => {
                    return FlowResult { items, used: y, leftover: blocks[index + 1..].to_vec(), stopped: Some(BreakKind::Column), walled_in, diagnostics };
                }
                Block::FrameBreak => {
                    return FlowResult { items, used: y, leftover: blocks[index + 1..].to_vec(), stopped: Some(BreakKind::Frame), walled_in, diagnostics };
                }
                Block::PageBreak => {
                    return FlowResult { items, used: y, leftover: blocks[index + 1..].to_vec(), stopped: Some(BreakKind::Page), walled_in, diagnostics };
                }
            }
        }

        FlowResult { items, used: y, leftover: Vec::new(), stopped: None, walled_in, diagnostics }
    }

    // ── Image frames ─────────────────────────────────────────────────────────

    fn layout_image_frame(
        &self,
        image: &ImageFrame,
        box_rect: Rect,
        source: &SourceRef,
        diagnostics: &mut Vec<Diagnostic>,
        page: u32,
        frame_id: &str,
    ) -> Vec<DisplayItem> {
        let Some(entry) = self.images.get(&image.src) else {
            diagnostics.push(
                Diagnostic::warning(
                    "missingImage",
                    format!("imagem `{}` não foi registrada", image.src),
                )
                .on(page, frame_id),
            );
            return Vec::new();
        };

        let natural_w = (entry.width as f64 * PT_PER_PX).max(1.0);
        let natural_h = (entry.height as f64 * PT_PER_PX).max(1.0);

        let (w, h) = match image.fit {
            ImageFit::Stretch => (box_rect.w, box_rect.h),
            ImageFit::None => (natural_w, natural_h),
            ImageFit::Contain => {
                let scale = (box_rect.w / natural_w).min(box_rect.h / natural_h);
                (natural_w * scale, natural_h * scale)
            }
            ImageFit::Cover => {
                let scale = (box_rect.w / natural_w).max(box_rect.h / natural_h);
                (natural_w * scale, natural_h * scale)
            }
        };

        let (fx, fy) = image.align.factors();
        let rect = Rect::new(
            box_rect.x + (box_rect.w - w) * fx,
            box_rect.y + (box_rect.h - h) * fy,
            w,
            h,
        );

        vec![DisplayItem::Image(ImageItem {
            src: image.src.clone(),
            rect,
            crop: None,
            source: Some(source.clone()),
        })]
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Helpers
// ─────────────────────────────────────────────────────────────────────────────

/// What a table's findings sound like to the person who wrote it.
///
/// One line per cause, never one per row: a table where thirty cells overlap
/// has one mistake in it, not thirty. The count goes in the message, where it
/// is information, instead of in the list, where it is noise.
fn diagnose(laid: &table::Layout, source: &SourceRef) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    let here = |diagnostic: Diagnostic| diagnostic.on(source.page, source.frame.clone());

    let overlaps = laid
        .issues
        .iter()
        .filter(|issue| matches!(issue, table::Issue::Overlap { .. }))
        .count();
    if overlaps > 0 {
        out.push(here(Diagnostic::warning(
            "tableCellOverlap",
            format!("{overlaps} célula(s) da tabela caem sobre lugares já ocupados e não foram desenhadas"),
        )));
    }

    let wide = laid
        .issues
        .iter()
        .filter(|issue| matches!(issue, table::Issue::TooWide { .. }))
        .count();
    if wide > 0 {
        out.push(here(Diagnostic::warning(
            "tableCellTooWide",
            format!("{wide} célula(s) atravessam mais colunas do que a tabela tem"),
        )));
    }

    if laid.issues.iter().any(|issue| matches!(issue, table::Issue::RowTooTall { .. })) {
        out.push(here(Diagnostic::warning(
            "tableRowTooTall",
            "uma linha da tabela é mais alta que o espaço inteiro e transbordou",
        )));
    }

    if laid.sizes.overflow > 0.0 {
        out.push(here(Diagnostic::warning(
            "tableOverflows",
            format!(
                "as colunas da tabela excedem a largura disponível em {:.1} pt",
                laid.sizes.overflow,
            ),
        )));
    }

    out
}

/// The engine, wearing the face a table asks for.
///
/// A cell holds blocks and blocks are what `flow_blocks` already stacks, so
/// this is a shim and not a second text path — which is the point. Obstacles
/// stop at the table: text inside a cell wrapping around an image elsewhere on
/// the page would be a layout nobody asked for.
struct CellFlow<'a, 'b> {
    engine: &'a LayoutEngine<'a>,
    text: &'a TextLayouter<'b>,
}

impl table::Cells for CellFlow<'_, '_> {
    fn intrinsic(&self, blocks: &[Block], style: &ResolvedStyle) -> text::Intrinsic {
        let mut out = text::Intrinsic::default();
        for block in blocks {
            let want = match block {
                Block::Paragraph(para) => self.text.measure_paragraph(para, style),
                Block::Table(nested) => table::intrinsic(nested, self, style),
                // A rule is a share of whatever width it is given and a spacer
                // has none, so neither has an opinion about how wide the
                // column should be.
                _ => continue,
            };
            out.min = out.min.max(want.min);
            out.max = out.max.max(want.max);
        }
        out
    }

    fn height(&self, blocks: &[Block], style: &ResolvedStyle, width: f64) -> f64 {
        self.engine
            .flow_blocks(
                self.text,
                blocks,
                style,
                Rect::new(0.0, 0.0, width.max(1.0), 0.0),
                None,
                &SourceRef::default(),
                &[],
            )
            .used
    }

    /// Where the first line of type lands, measured from the content's top.
    ///
    /// Laid out and looked at rather than derived from the font: what a first
    /// baseline is depends on the leading, on a first-line indent, on whether
    /// a rule or a spacer comes before the text. Asking the layout is the only
    /// answer that stays true when any of those change.
    fn first_baseline(
        &self,
        blocks: &[Block],
        style: &ResolvedStyle,
        width: f64,
    ) -> Option<f64> {
        let laid = self.engine.flow_blocks(
            self.text,
            blocks,
            style,
            Rect::new(0.0, 0.0, width.max(1.0), 0.0),
            None,
            &SourceRef::default(),
            &[],
        );
        fn first(items: &[DisplayItem]) -> Option<f64> {
            items
                .iter()
                .filter_map(|item| match item {
                    DisplayItem::Glyphs(run) => Some(run.y),
                    DisplayItem::Group(group) => first(&group.items),
                    _ => None,
                })
                .min_by(f64::total_cmp)
        }
        first(&laid.items)
    }

    fn render(
        &self,
        blocks: &[Block],
        style: &ResolvedStyle,
        rect: Rect,
        source: &SourceRef,
    ) -> Vec<DisplayItem> {
        // No height budget: the row was sized from `height` at this same
        // width, so anything that spills is a disagreement worth seeing rather
        // than content quietly dropped.
        self.engine.flow_blocks(self.text, blocks, style, rect, None, source, &[]).items
    }
}

/// Content queued for a frame further down a thread.
#[derive(Debug, Default)]
/// What one pass over a column produced.
///
/// Named rather than a tuple because the wrap added a fifth thing to say and
/// `(items, used, leftover, stopped, walled_in)` at the call site tells the
/// reader nothing.
struct FlowResult {
    items: Vec<DisplayItem>,
    /// Vertical space consumed.
    used: f64,
    /// Blocks that did not fit, for the next column, frame or page.
    leftover: Vec<Block>,
    stopped: Option<BreakKind>,
    /// A wrap, not the height, is what stopped the text.
    walled_in: bool,
    /// What the author should be told, already placed on a page and a frame.
    ///
    /// Built here rather than handed up as raw findings because this is the
    /// last place that knows which block they came from — and the first that
    /// knows the page and the frame, both of which are in `source`.
    diagnostics: Vec<Diagnostic>,
}

struct PendingFlow {
    /// The story it came from, if any. Carried so provenance keeps pointing at
    /// the story rather than at whichever frame ended up painting the text.
    story: Option<String>,
    blocks: Vec<Block>,
    /// Earliest page this content may appear on, set by an explicit page break.
    /// Frames on earlier pages pass it along untouched.
    min_page: Option<u32>,
}

/// Why a column stopped taking content.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BreakKind {
    Column,
    Frame,
    Page,
}

/// Pages requested by `autoFlow` while laying out the current page.
#[derive(Debug, Default)]
struct AutoFlow {
    /// Frames to place on pages appended after the current one.
    requests: Vec<Frame>,
    /// Total generated so far — feeds the ids and the cap.
    generated: u32,
}

/// Does anything in the document print the total page count?
///
/// Checks masters and stories too — a running footer reading `{page}/{pages}`
/// is the whole reason this exists.
fn wants_total_pages(doc: &Document) -> bool {
    fn in_frames(frames: &[Frame]) -> bool {
        frames.iter().any(|frame| match &frame.content {
            FrameContent::Text(text) => uses_total_pages(&text.blocks),
            FrameContent::Group(group) => in_frames(&group.children),
            _ => false,
        })
    }

    doc.pages.iter().any(|page| in_frames(&page.frames))
        || doc.resources.masters.values().any(|m| in_frames(&m.frames))
        || doc.resources.stories.values().any(|s| uses_total_pages(s))
}

/// Record each paragraph's index before any of them can be moved or split.
fn tag_origins(blocks: &mut [Block]) {
    for (index, block) in blocks.iter_mut().enumerate() {
        if let Block::Paragraph(paragraph) = block
            && paragraph.origin.is_none()
        {
            paragraph.origin = Some(Origin {
                block: index as u32,
                inline: 0,
                offset: 0,
            });
        }
    }
}

/// Give every frame a stable id, so provenance and threading can name them.
fn assign_frame_ids(doc: &mut Document) {
    fn walk(frames: &mut [Frame], prefix: &str) {
        for (index, frame) in frames.iter_mut().enumerate() {
            if frame.id.as_ref().is_none_or(String::is_empty) {
                frame.id = Some(format!("{prefix}f{index}"));
            }
            let child_prefix = format!("{}.", frame.id.as_deref().unwrap_or(prefix));
            if let FrameContent::Group(group) = &mut frame.content {
                walk(&mut group.children, &child_prefix);
            }
        }
    }

    for (name, master) in doc.resources.masters.iter_mut() {
        walk(&mut master.frames, &format!("m:{name}."));
    }
    for (index, page) in doc.pages.iter_mut().enumerate() {
        walk(&mut page.frames, &format!("p{index}."));
    }
}

fn stroke_of(border: &Border) -> Stroke {
    Stroke {
        color: border.color,
        width: border.width.get(),
        dash: border.dash_pattern(),
    }
}

/// A border is one rectangle when all sides are drawn, otherwise one line each.
fn border_items(border: &Border, rect: Rect, radius: f64, source: &SourceRef) -> Vec<DisplayItem> {
    if border.width.get() <= 0.0 || border.sides.none() {
        return Vec::new();
    }

    let stroke = stroke_of(border);

    if border.is_uniform() {
        return vec![DisplayItem::Rect(RectItem {
            rect,
            radius,
            fill: None,
            stroke: Some(stroke),
            source: Some(source.clone()),
        })];
    }

    let edge = |x1: f64, y1: f64, x2: f64, y2: f64| {
        DisplayItem::Line(LineItem {
            x1,
            y1,
            x2,
            y2,
            stroke: stroke.clone(),
            source: Some(source.clone()),
        })
    };

    let mut out = Vec::new();
    if border.sides.top {
        out.push(edge(rect.x, rect.y, rect.right(), rect.y));
    }
    if border.sides.right {
        out.push(edge(rect.right(), rect.y, rect.right(), rect.bottom()));
    }
    if border.sides.bottom {
        out.push(edge(rect.x, rect.bottom(), rect.right(), rect.bottom()));
    }
    if border.sides.left {
        out.push(edge(rect.x, rect.y, rect.x, rect.bottom()));
    }
    out
}

/// Clockwise rotation about the centre of `rect`, as an affine matrix.
fn rotation_matrix(degrees: f64, rect: Rect) -> Option<[f64; 6]> {
    if degrees.abs() < 1e-9 {
        return None;
    }
    let (sin, cos) = degrees.to_radians().sin_cos();
    let cx = rect.x + rect.w / 2.0;
    let cy = rect.y + rect.h / 2.0;
    Some([
        cos,
        sin,
        -sin,
        cos,
        cx - cos * cx + sin * cy,
        cy - sin * cx - cos * cy,
    ])
}

/// Shift already-positioned items. Used for column offsets and vertical
/// alignment, where the total height is only known after laying out.
fn translate_items(items: &mut [DisplayItem], dx: f64, dy: f64) {
    for item in items {
        match item {
            DisplayItem::Group(group) => match &mut group.transform {
                Some(matrix) => {
                    matrix[4] += dx;
                    matrix[5] += dy;
                }
                None => {
                    if let Some(clip) = &mut group.clip {
                        clip.rect = clip.rect.translate(dx, dy);
                    }
                    translate_items(&mut group.items, dx, dy);
                }
            },
            DisplayItem::Glyphs(run) => {
                run.x += dx;
                run.y += dy;
            }
            DisplayItem::Rect(r) => r.rect = r.rect.translate(dx, dy),
            DisplayItem::Ellipse(e) => e.rect = e.rect.translate(dx, dy),
            DisplayItem::Path(path) => {
                for command in &mut path.commands {
                    command.translate(dx, dy);
                }
            }
            DisplayItem::Image(i) => i.rect = i.rect.translate(dx, dy),
            DisplayItem::Line(l) => {
                l.x1 += dx;
                l.x2 += dx;
                l.y1 += dy;
                l.y2 += dy;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::display::GlyphRun;
    use crate::fonts::test_fonts;
    use crate::spec::FontWeight;

    fn engine_parts() -> Option<(FontRegistry, ImageStore)> {
        let mut registry = FontRegistry::new();
        registry.add("body", test_fonts::dejavu()?.to_vec(), None, None).ok()?;
        if let Some(bold) = test_fonts::dejavu_bold() {
            let _ = registry.add("body", bold.to_vec(), Some(FontWeight::BOLD), Some(false));
        }
        Some((registry, ImageStore::new()))
    }

    fn layout_json(json: &str) -> Option<DisplayList> {
        let (registry, images) = engine_parts()?;
        let doc: Document = serde_json::from_str(json).expect("valid document");
        Some(LayoutEngine::new(&registry, &images).layout(&doc))
    }

    fn all_runs(list: &DisplayList) -> Vec<GlyphRun> {
        fn walk(items: &[DisplayItem], out: &mut Vec<GlyphRun>) {
            for item in items {
                match item {
                    DisplayItem::Glyphs(run) => out.push(run.clone()),
                    DisplayItem::Group(group) => walk(&group.items, out),
                    _ => {}
                }
            }
        }
        let mut out = Vec::new();
        for page in &list.pages {
            walk(&page.items, &mut out);
        }
        out
    }

    /// Runs on one page, for the tests that care which page text landed on.
    fn page_runs(list: &DisplayList, index: usize) -> Vec<GlyphRun> {
        let single = DisplayList {
            pages: vec![list.pages[index].clone()],
            ..DisplayList::new()
        };
        all_runs(&single)
    }

    #[test]
    fn empty_document_produces_no_pages() {
        let Some(list) = layout_json("{}") else { return };
        assert!(list.pages.is_empty());
        assert!(!list.has_errors());
    }

    #[test]
    fn a_page_reports_its_geometry() {
        let Some(list) = layout_json(r#"{"page":{"size":"A4","margins":50},"pages":[{}]}"#) else {
            return;
        };
        let page = &list.pages[0];
        assert!((page.width - 595.28).abs() < 0.1);
        assert!((page.height - 841.89).abs() < 0.1);
        assert_eq!(page.margin_box, Rect::new(50.0, 50.0, page.width - 100.0, page.height - 100.0));
    }

    #[test]
    fn text_lands_inside_its_frame() {
        let Some(list) = layout_json(
            r#"{"pages":[{"frames":[{"type":"text","rect":[56,80,400,200],"blocks":["Material didático"]}]}]}"#,
        ) else {
            return;
        };
        let runs = all_runs(&list);
        assert_eq!(runs.len(), 1);
        assert!((runs[0].x - 56.0).abs() < 0.01);
        assert!(runs[0].y > 80.0 && runs[0].y < 120.0);
        assert_eq!(runs[0].text, "Material didático");
    }

    /// A page with a picture on the left and a paragraph across the whole
    /// width. `wrap` decides whether the two collide.
    fn wrapped_page(wrap: &str, ignore: bool) -> Option<DisplayList> {
        layout_json(&format!(
            r#"{{"pages":[{{"frames":[
                {{"type":"image","rect":[56,80,150,300],"src":"foto.png"{wrap}}},
                {{"type":"text","rect":[56,80,400,300],"ignoreWrap":{ignore},
                  "blocks":["Material didático para a unidade de estudo"]}}
            ]}}]}}"#
        ))
    }

    #[test]
    fn an_image_with_a_wrap_pushes_the_text_off_it() {
        let Some(plain) = wrapped_page("", false) else {
            return;
        };
        let Some(wrapped) = wrapped_page(r#", "wrap": {"mode": {"kind": "box"}, "padding": 8}"#, false)
        else {
            return;
        };

        let before = all_runs(&plain)[0].x;
        let after = all_runs(&wrapped)[0].x;

        assert!((before - 56.0).abs() < 0.01, "sem wrap o texto começa na borda");
        assert!(
            (after - 214.0).abs() < 0.01,
            "com wrap o texto começa depois da foto mais a folga, veio {after}"
        );
    }

    /// A picture in the middle of a column, with room on both sides of it.
    fn picture_in_the_middle() -> Option<DisplayList> {
        layout_json(
            r#"{"pages":[{"frames":[
                {"type":"image","rect":[200,80,120,60],"src":"foto.png",
                 "wrap":{"mode":{"kind":"box"},"padding":6}},
                {"type":"text","rect":[56,80,440,400],"style":{"fontSize":10},
                 "blocks":["Um parágrafo bem comprido que precisa correr dos dois lados da fotografia posta no meio da coluna e depois seguir usando a largura inteira até o fim do texto disponível aqui."]}
            ]}]}"#,
        )
    }

    /// Right edge of a run ignoring trailing whitespace. A space at a line
    /// break stays in the run — the caret has to be able to sit after it — but
    /// hangs past the margin, exactly as it does in print.
    fn visible_right(run: &GlyphRun) -> f64 {
        let trimmed = run.text.trim_end().len();
        run.glyphs
            .iter()
            .filter(|g| (g.cluster as usize) < trimmed)
            .map(|g| run.x + g.x + g.advance)
            .fold(run.x, f64::max)
    }

    /// The same page, laid out with a given alignment. The photograph leaves
    /// a gap of 56..194 to its left and 326..496 to its right.
    fn aligned_around_a_picture(align: &str) -> Option<DisplayList> {
        layout_json(&format!(
            r#"{{"pages":[{{"frames":[
                {{"type":"image","rect":[200,80,120,60],"src":"foto.png",
                 "wrap":{{"mode":{{"kind":"box"}},"padding":6}}}},
                {{"type":"text","rect":[56,80,440,400],
                 "style":{{"fontSize":10,"textAlign":"{align}"}},
                 "blocks":["Um parágrafo bem comprido que precisa correr dos dois lados da fotografia posta no meio da coluna e depois seguir usando a largura inteira até o fim."]}}
            ]}}]}}"#
        ))
    }

    /// The two segments of the first line, left one first.
    fn first_line_pair(list: &DisplayList) -> Vec<GlyphRun> {
        let runs = all_runs(list);
        let top = runs[0].y;
        let pair: Vec<GlyphRun> = runs.into_iter().filter(|r| (r.y - top).abs() < 0.01).collect();
        assert_eq!(pair.len(), 2, "esperava a linha partida em dois trechos");
        pair
    }

    #[test]
    fn justified_text_fills_each_gap_to_its_own_edge() {
        let Some(list) = aligned_around_a_picture("justify") else {
            return;
        };
        let pair = first_line_pair(&list);

        assert!((pair[0].x - 56.0).abs() < 0.01);
        assert!(
            (visible_right(&pair[0]) - 194.0).abs() < 0.01,
            "o trecho da esquerda tem de encostar na foto, veio {}",
            visible_right(&pair[0])
        );
        assert!((pair[1].x - 326.0).abs() < 0.01);
        assert!(
            (visible_right(&pair[1]) - 496.0).abs() < 0.01,
            "o da direita tem de encostar na margem, veio {}",
            visible_right(&pair[1])
        );
    }

    #[test]
    fn right_aligned_text_hangs_off_each_gaps_right_edge() {
        let Some(list) = aligned_around_a_picture("right") else {
            return;
        };
        let pair = first_line_pair(&list);

        assert!((visible_right(&pair[0]) - 194.0).abs() < 0.01);
        assert!((visible_right(&pair[1]) - 496.0).abs() < 0.01);
        assert!(pair[0].x > 56.0, "e sobra espaço à esquerda de cada trecho");
        assert!(pair[1].x > 326.0);
    }

    #[test]
    fn centred_text_is_centred_in_its_own_gap_not_in_the_column() {
        let Some(list) = aligned_around_a_picture("center") else {
            return;
        };
        let pair = first_line_pair(&list);

        for (run, left, right) in [(&pair[0], 56.0, 194.0), (&pair[1], 326.0, 496.0)] {
            let before = run.x - left;
            let after = right - visible_right(run);
            assert!(
                (before - after).abs() < 0.01,
                "sobras desiguais no vão {left}..{right}: {before} antes, {after} depois"
            );
            assert!(before > 0.0, "e o trecho não pode preencher o vão inteiro");
        }
    }

    #[test]
    fn only_the_segment_that_ends_the_paragraph_escapes_justification() {
        let Some(list) = aligned_around_a_picture("justify") else {
            return;
        };
        let runs = all_runs(&list);
        let last = runs.last().unwrap();
        let before = &runs[runs.len() - 2];

        // Both sit on the last band. The left one is a full line and gets
        // stretched to its gap; the right one ends the paragraph and stays
        // as short as its words make it.
        assert!(
            (last.y - before.y).abs() < 0.01,
            "os dois trechos deveriam dividir a mesma baseline"
        );
        assert!(
            (visible_right(before) - 194.0).abs() < 0.01,
            "o trecho da esquerda continua justificado, veio {}",
            visible_right(before)
        );
        assert!(
            visible_right(last) < 496.0 - 1.0,
            "o que termina o parágrafo não pode ser esticado até a borda do vão, veio {}",
            visible_right(last)
        );
    }

    #[test]
    fn the_text_runs_down_both_sides_of_a_picture() {
        let Some(list) = picture_in_the_middle() else {
            return;
        };
        let runs = all_runs(&list);

        // The first line is in two pieces sharing a baseline: one to the left
        // of the photograph, one to its right. This is the thing `pretext`
        // does not do — it keeps the widest gap and drops the rest.
        let top = runs[0].y;
        let on_first: Vec<&GlyphRun> = runs.iter().filter(|r| (r.y - top).abs() < 0.01).collect();

        assert_eq!(on_first.len(), 2, "esperava dois trechos na mesma linha");
        assert!((on_first[0].x - 56.0).abs() < 0.01, "trecho da esquerda");
        assert!(
            (on_first[1].x - 326.0).abs() < 0.01,
            "trecho da direita começa depois da foto mais a folga, veio {}",
            on_first[1].x
        );
        assert!(
            on_first[0].x + on_first[0].width <= 194.0 + 0.01,
            "o trecho da esquerda não pode invadir a foto"
        );
    }

    #[test]
    fn flowing_both_sides_keeps_the_reading_order() {
        let Some(list) = picture_in_the_middle() else {
            return;
        };
        // Runs come out in flow order, so pasting them back together has to
        // reproduce the paragraph — no word repeated, none lost.
        let joined: String = all_runs(&list)
            .iter()
            .map(|r| r.text.clone())
            .collect::<Vec<_>>()
            .join("");
        let normalised = joined.split_whitespace().collect::<Vec<_>>().join(" ");

        assert_eq!(
            normalised,
            "Um parágrafo bem comprido que precisa correr dos dois lados da fotografia \
posta no meio da coluna e depois seguir usando a largura inteira até o fim do \
texto disponível aqui."
                .replace('\n', "")
        );
    }

    #[test]
    fn the_text_returns_to_the_full_width_once_it_is_past_the_picture() {
        // The picture is only 40pt tall, so it can push the paragraph's first
        // line and nothing else. Before the break and placement loops were
        // fused this was impossible: the whole paragraph took one width.
        let Some(list) = layout_json(
            r#"{"pages":[{"frames":[
                {"type":"image","rect":[56,80,150,40],"src":"foto.png",
                 "wrap":{"mode":{"kind":"box"},"padding":4}},
                {"type":"text","rect":[56,80,400,400],"style":{"fontSize":11},
                 "blocks":["Um parágrafo suficientemente longo para ocupar várias linhas seguidas dentro da coluna, o bastante para passar da altura da fotografia e voltar a usar a largura inteira da coluna sem nenhum obstáculo pela frente."]}
            ]}]}"#,
        ) else {
            return;
        };

        let runs = all_runs(&list);
        assert!(runs.len() >= 3, "esperava várias linhas, veio {}", runs.len());

        let first = runs[0].x;
        let last = runs[runs.len() - 1].x;

        assert!(
            (first - 210.0).abs() < 0.01,
            "a primeira linha corre ao lado da foto, veio {first}"
        );
        assert!(
            (last - 56.0).abs() < 0.01,
            "a última já passou da foto e volta à margem, veio {last}"
        );
    }

    #[test]
    fn a_picture_covering_the_column_pushes_the_text_below_it() {
        let Some(list) = layout_json(
            r#"{"pages":[{"frames":[
                {"type":"image","rect":[56,80,400,100],"src":"foto.png",
                 "wrap":{"mode":{"kind":"box"}}},
                {"type":"text","rect":[56,80,400,400],
                 "blocks":["Depois da fotografia"]}
            ]}]}"#,
        ) else {
            return;
        };

        let runs = all_runs(&list);
        assert_eq!(runs.len(), 1);
        assert!(
            runs[0].y > 180.0,
            "a linha tem de descer para depois da foto, veio y={}",
            runs[0].y
        );
        assert!((runs[0].x - 56.0).abs() < 0.01, "e voltar à margem");
    }

    #[test]
    fn ignore_wrap_lets_a_caption_sit_on_its_own_photograph() {
        let Some(list) = wrapped_page(
            r#", "wrap": {"mode": {"kind": "box"}, "padding": 8}"#,
            true,
        ) else {
            return;
        };
        let x = all_runs(&list)[0].x;
        assert!(
            (x - 56.0).abs() < 0.01,
            "o frame que abre mão do contorno volta à borda, veio {x}"
        );
    }

    #[test]
    fn a_picture_over_one_column_leaves_the_other_alone() {
        // Two columns of 193 with a 14 gap: 56..249 and 263..456. The picture
        // covers the left one only.
        let Some(list) = layout_json(
            r#"{"pages":[{"frames":[
                {"type":"image","rect":[56,80,100,300],"src":"foto.png",
                 "wrap":{"mode":{"kind":"box"}}},
                {"type":"text","rect":[56,80,400,300],"columns":2,"style":{"fontSize":10},
                 "blocks":["Texto suficientemente longo para encher a primeira coluna inteira e transbordar para a segunda coluna do mesmo frame. Texto suficientemente longo para encher a primeira coluna inteira e transbordar para a segunda coluna do mesmo frame. Texto suficientemente longo para encher a primeira coluna inteira e transbordar para a segunda coluna do mesmo frame. Texto suficientemente longo para encher a primeira coluna inteira e transbordar para a segunda coluna do mesmo frame."]}
            ]}]}"#,
        ) else {
            return;
        };

        let runs = all_runs(&list);
        let left: Vec<&GlyphRun> = runs.iter().filter(|r| r.x < 260.0).collect();
        let right: Vec<&GlyphRun> = runs.iter().filter(|r| r.x >= 260.0).collect();

        assert!(!left.is_empty() && !right.is_empty(), "esperava as duas colunas");
        assert!(
            left.iter().all(|r| r.x >= 156.0 - 0.01),
            "a coluna da esquerda tem de contornar a foto"
        );
        assert!(
            right.iter().any(|r| (r.x - 263.0).abs() < 0.01),
            "a da direita não é tocada por uma foto que não a alcança"
        );
    }

    #[test]
    fn a_threaded_frame_obeys_the_obstacles_of_its_own_page() {
        let Some(list) = layout_json(
            r#"{"pages":[
                {"frames":[
                    {"type":"text","id":"a","rect":[56,80,400,40],"threadNext":"b",
                     "style":{"fontSize":10},
                     "blocks":["Um texto que começa na primeira página sem nenhuma fotografia atrapalhando e continua na segunda página, onde existe uma, e por isso precisa desviar dela ao chegar lá."]}
                ]},
                {"frames":[
                    {"type":"image","rect":[56,80,150,300],"src":"foto.png",
                     "wrap":{"mode":{"kind":"box"}}},
                    {"type":"text","id":"b","rect":[56,80,400,300]}
                ]}
            ]}"#,
        ) else {
            return;
        };

        let first = page_runs(&list, 0);
        let second = page_runs(&list, 1);

        assert!(!first.is_empty() && !second.is_empty(), "o texto tem de atravessar");
        assert!(
            (first[0].x - 56.0).abs() < 0.01,
            "a primeira página não tem obstáculo"
        );
        assert!(
            (second[0].x - 206.0).abs() < 0.01,
            "a segunda tem, e o texto que chega lá desvia: veio {}",
            second[0].x
        );
    }

    #[test]
    fn a_picture_covering_the_whole_frame_oversets_instead_of_hanging() {
        let Some(list) = layout_json(
            r#"{"pages":[{"frames":[
                {"type":"image","rect":[56,80,400,300],"src":"foto.png",
                 "wrap":{"mode":{"kind":"box"}}},
                {"type":"text","id":"preso","rect":[56,80,400,300],
                 "blocks":["Não há lugar nenhum para esta linha."]}
            ]}]}"#,
        ) else {
            return;
        };

        assert!(
            all_runs(&list).is_empty(),
            "não sobra vão nenhum, então nada pode ser pintado"
        );
        assert!(
            list.diagnostics.iter().any(|d| d.code == "overset"),
            "e o conteúdo tem de ser reportado como overset: {:?}",
            list.diagnostics.iter().map(|d| &d.code).collect::<Vec<_>>()
        );
    }

    #[test]
    fn an_unbounded_frame_still_stops_instead_of_stepping_down_forever() {
        // `overflow: visible` removes the height budget, so the only thing
        // between a fully blocked column and an endless loop is the band
        // guard. The picture is taller than any page.
        let Some(list) = layout_json(
            r#"{"pages":[{"frames":[
                {"type":"image","rect":[56,80,400,100000],"src":"foto.png",
                 "wrap":{"mode":{"kind":"box"}}},
                {"type":"text","rect":[56,80,400,300],"overflow":"visible",
                 "blocks":["Uma linha sem lugar nenhum."]}
            ]}]}"#,
        ) else {
            return;
        };
        assert!(all_runs(&list).is_empty(), "não há vão onde pintar");
    }

    #[test]
    fn text_resumes_below_a_picture_in_a_frame_with_no_height_budget() {
        let Some(list) = layout_json(
            r#"{"pages":[{"frames":[
                {"type":"image","rect":[56,80,400,120],"src":"foto.png",
                 "wrap":{"mode":{"kind":"box"}}},
                {"type":"text","rect":[56,80,400,60],"overflow":"visible",
                 "blocks":["Passa por baixo da fotografia."]}
            ]}]}"#,
        ) else {
            return;
        };
        let runs = all_runs(&list);
        assert_eq!(runs.len(), 1);
        assert!(
            runs[0].y > 200.0,
            "sem orçamento de altura o texto desce até achar espaço, veio y={}",
            runs[0].y
        );
    }

    #[test]
    fn a_wrap_that_leaves_no_room_says_so_rather_than_only_reporting_overset() {
        let Some(list) = layout_json(
            r#"{"pages":[{"frames":[
                {"type":"image","rect":[56,80,400,300],"src":"foto.png",
                 "wrap":{"mode":{"kind":"box"}}},
                {"type":"text","id":"preso","rect":[56,80,400,300],
                 "blocks":["Não há lugar nenhum para esta linha."]}
            ]}]}"#,
        ) else {
            return;
        };

        let wrap: Vec<&Diagnostic> = list
            .diagnostics
            .iter()
            .filter(|d| d.code == "wrapLeavesNoRoom")
            .collect();

        assert_eq!(wrap.len(), 1, "um aviso, e um só: {:?}", list.diagnostics);
        assert_eq!(wrap[0].page, Some(0));
        assert_eq!(wrap[0].frame.as_deref(), Some("preso"));
        assert!(
            list.diagnostics.iter().any(|d| d.code == "overset"),
            "o overset continua, porque o conteúdo de facto não foi colocado"
        );
    }

    #[test]
    fn text_that_merely_steps_around_a_picture_raises_no_warning() {
        let Some(list) = layout_json(
            r#"{"pages":[{"frames":[
                {"type":"image","rect":[56,80,150,60],"src":"foto.png",
                 "wrap":{"mode":{"kind":"box"}}},
                {"type":"text","rect":[56,80,400,400],"style":{"fontSize":10},
                 "blocks":["Um texto que apenas contorna a fotografia e segue em frente sem nunca ficar sem lugar para as suas linhas seguintes."]}
            ]}]}"#,
        ) else {
            return;
        };
        assert!(
            !list.diagnostics.iter().any(|d| d.code == "wrapLeavesNoRoom"),
            "contornar é o funcionamento normal, não um problema: {:?}",
            list.diagnostics
        );
    }

    #[test]
    fn padding_pushes_content_inward() {
        let Some(list) = layout_json(
            r#"{"pages":[{"frames":[{"type":"text","rect":[0,0,400,200],"padding":20,"blocks":["x"]}]}]}"#,
        ) else {
            return;
        };
        assert!((all_runs(&list)[0].x - 20.0).abs() < 0.01);
    }

    #[test]
    fn frames_are_indexed_for_the_editor() {
        let Some(list) = layout_json(
            r#"{"pages":[{"frames":[
                {"type":"text","rect":[0,0,100,50],"blocks":["a"]},
                {"id":"foto","type":"shape","rect":[10,60,80,80],"shape":"ellipse","name":"Círculo"}
            ]}]}"#,
        ) else {
            return;
        };
        let frames = &list.pages[0].frames;
        assert_eq!(frames.len(), 2);
        // Ids are auto-assigned when omitted and preserved when given.
        assert_eq!(frames[0].id, "p0.f0");
        assert_eq!(frames[0].kind, "text");
        assert_eq!(frames[1].id, "foto");
        assert_eq!(frames[1].kind, "shape");
        assert_eq!(frames[1].name.as_deref(), Some("Círculo"));
        assert_eq!(frames[1].rect, Rect::new(10.0, 60.0, 80.0, 80.0));
    }

    #[test]
    fn two_columns_split_the_frame_width() {
        let Some(list) = layout_json(
            r#"{"pages":[{"frames":[{"type":"text","rect":[0,0,400,35],"columns":2,"columnGap":20,
                "blocks":["alpha bravo charlie delta echo foxtrot golf hotel india juliet kilo lima mike"]}]}]}"#,
        ) else {
            return;
        };
        let runs = all_runs(&list);
        // (400 - 20) / 2 = 190 per column; the second starts at 210.
        assert!(runs.iter().any(|r| r.x < 190.0));
        assert!(
            runs.iter().any(|r| r.x >= 210.0 - 0.01),
            "no content reached the second column"
        );
        for run in &runs {
            let in_first = run.x + run.width <= 190.0 + 1.0;
            let in_second = run.x >= 210.0 - 0.01 && run.x + run.width <= 400.0 + 1.0;
            assert!(in_first || in_second, "run straddles the gutter: {run:?}");
        }
    }

    #[test]
    fn overflowing_text_reports_overset() {
        let Some(list) = layout_json(
            r#"{"pages":[{"frames":[{"type":"text","rect":[0,0,100,20],
                "blocks":["texto muito maior do que a caixa pode conter em uma única linha apertada"]}]}]}"#,
        ) else {
            return;
        };
        assert!(list.pages[0].frames[0].overset);
        assert!(list.diagnostics.iter().any(|d| d.code == "overset"));
    }

    #[test]
    fn threading_carries_the_rest_into_the_next_frame() {
        let Some(list) = layout_json(
            r#"{"pages":[{"frames":[
                {"id":"a","type":"text","rect":[0,0,120,40],"threadNext":"b",
                 "blocks":["alpha bravo charlie delta echo foxtrot golf hotel india juliet"]},
                {"id":"b","type":"text","rect":[200,0,120,200]}
            ]}]}"#,
        ) else {
            return;
        };

        let runs = all_runs(&list);
        assert!(runs.iter().any(|r| r.x < 120.0), "nothing in the first frame");
        assert!(runs.iter().any(|r| r.x >= 200.0), "nothing flowed to the second");

        assert!(!list.pages[0].frames[0].overset, "threaded frame must not be overset");
        assert!(!list.diagnostics.iter().any(|d| d.code == "overset"));
    }

    #[test]
    fn threading_reaches_across_pages() {
        let Some(list) = layout_json(
            r#"{"pages":[
                {"frames":[{"id":"a","type":"text","rect":[0,0,120,40],"threadNext":"b",
                  "blocks":["alpha bravo charlie delta echo foxtrot golf hotel india juliet kilo"]}]},
                {"frames":[{"id":"b","type":"text","rect":[0,0,120,400]}]}
            ]}"#,
        ) else {
            return;
        };
        let page_two_runs: Vec<_> = {
            let mut out = Vec::new();
            fn walk(items: &[DisplayItem], out: &mut Vec<GlyphRun>) {
                for item in items {
                    match item {
                        DisplayItem::Glyphs(r) => out.push(r.clone()),
                        DisplayItem::Group(g) => walk(&g.items, out),
                        _ => {}
                    }
                }
            }
            walk(&list.pages[1].items, &mut out);
            out
        };
        assert!(!page_two_runs.is_empty(), "story did not reach page two");
    }

    /// The book case: one page plus a story, and the engine makes the rest.
    #[test]
    fn auto_flow_generates_as_many_pages_as_the_story_needs() {
        let paragraph = "\"alpha bravo charlie delta echo foxtrot golf hotel india juliet kilo lima\"";
        let mut story: Vec<String> = (0..30).map(|_| paragraph.to_string()).collect();
        // A distinctive tail, so "nothing was lost" is a real assertion.
        story.push("\"ultimo paragrafo do livro\"".to_string());

        let json = format!(
            r#"{{
                "page": {{ "size": "A6", "margins": 20 }},
                "resources": {{ "stories": {{ "corpo": [{}] }} }},
                "pages": [{{ "frames": [
                    {{"id":"corpo","type":"text","rect":[20,20,258,258],
                      "story":"corpo","autoFlow":true}}
                ]}}]
            }}"#,
            story.join(",")
        );

        let Some(list) = layout_json(&json) else { return };

        assert!(list.pages.len() >= 4, "esperava várias páginas, veio {}", list.pages.len());
        // Nothing was left behind.
        assert!(!list.diagnostics.iter().any(|d| d.code == "overset"));
        assert!(list.pages.iter().all(|page| !page.frames[0].overset));

        // Every generated page carries content and keeps the geometry.
        for page in &list.pages {
            assert_eq!(page.frames.len(), 1, "página {} tem frames demais", page.index);
            assert_eq!(page.frames[0].rect, Rect::new(20.0, 20.0, 258.0, 258.0));
        }
        let words: String = all_runs(&list).iter().map(|r| r.text.clone()).collect();
        assert!(words.contains("ultimo paragrafo"), "o fim da story não foi colocado");
    }

    /// With facing pages on, the auto-flowed text block follows the gutter.
    #[test]
    fn auto_flow_mirrors_the_frame_on_facing_versos() {
        let paragraph = "\"alpha bravo charlie delta echo foxtrot golf hotel india juliet kilo lima\"";
        let story: Vec<String> = (0..20).map(|_| paragraph.to_string()).collect();

        let json = format!(
            r#"{{
                "page": {{ "size": [400, 400], "margins": [30, 25, 30, 60], "facing": true }},
                "resources": {{ "stories": {{ "corpo": [{}] }} }},
                "pages": [{{ "frames": [
                    {{"id":"corpo","type":"text","rect":[60,30,315,100],
                      "story":"corpo","autoFlow":true}}
                ]}}]
            }}"#,
            story.join(",")
        );

        let Some(list) = layout_json(&json) else { return };
        assert!(list.pages.len() >= 3, "esperava várias páginas");

        // Recto keeps the declared position; verso mirrors it about the centre.
        assert_eq!(list.pages[0].frames[0].rect.x, 60.0);
        assert_eq!(list.pages[1].frames[0].rect.x, 400.0 - 60.0 - 315.0);
        assert_eq!(list.pages[2].frames[0].rect.x, 60.0);

        // The gutter is 60 on both, just on opposite sides.
        let verso = list.pages[1].frames[0].rect;
        assert_eq!(400.0 - (verso.x + verso.w), 60.0);
    }

    /// A running footer numbers every page, including generated ones.
    #[test]
    fn page_tokens_resolve_per_page() {
        let paragraph = "\"alpha bravo charlie delta echo foxtrot golf hotel india juliet kilo lima\"";
        let story: Vec<String> = (0..20).map(|_| paragraph.to_string()).collect();

        let json = format!(
            r#"{{
                "page": {{ "size": [400, 400], "margins": 30 }},
                "resources": {{
                    "masters": {{ "miolo": {{ "frames": [
                        {{"type":"text","rect":[30,360,340,20],
                          "blocks":[{{"type":"paragraph",
                            "style":{{"textAlign":"center"}},
                            "content":["{{page}} de {{pages}}"]}}]}}
                    ]}} }},
                    "stories": {{ "corpo": [{}] }}
                }},
                "pages": [{{ "master": "miolo", "frames": [
                    {{"id":"corpo","type":"text","rect":[30,30,340,100],
                      "story":"corpo","autoFlow":true}}
                ]}}]
            }}"#,
            story.join(",")
        );

        let Some(list) = layout_json(&json) else { return };
        let total = list.pages.len();
        assert!(total >= 4, "esperava várias páginas, veio {total}");

        // Every page's footer must read its own number and the settled total.
        for (index, page) in list.pages.iter().enumerate() {
            let mut runs = Vec::new();
            fn walk(items: &[DisplayItem], out: &mut Vec<GlyphRun>) {
                for item in items {
                    match item {
                        DisplayItem::Glyphs(run) => out.push(run.clone()),
                        DisplayItem::Group(group) => walk(&group.items, out),
                        _ => {}
                    }
                }
            }
            walk(&page.items, &mut runs);

            let text: String = runs.iter().map(|run| run.text.clone()).collect();
            let expected = format!("{} de {}", index + 1, total);
            assert!(
                text.contains(&expected),
                "página {} deveria conter {expected:?}, veio {text:?}",
                index + 1
            );
        }
    }

    /// Without `{pages}` there is nothing to settle, so one pass is enough.
    #[test]
    fn only_the_page_token_needs_no_second_pass() {
        let Some(list) = layout_json(
            r#"{"pages":[
                {"frames":[{"type":"text","rect":[0,0,200,40],"blocks":["página {page}"]}]},
                {"frames":[{"type":"text","rect":[0,0,200,40],"blocks":["página {page}"]}]}
            ]}"#,
        ) else {
            return;
        };
        let texts: Vec<String> = all_runs(&list).iter().map(|r| r.text.clone()).collect();
        assert!(texts.iter().any(|t| t.contains("página 1")));
        assert!(texts.iter().any(|t| t.contains("página 2")));
    }

    /// A frame too small for a single line must stop, not paginate forever.
    #[test]
    fn auto_flow_stops_when_nothing_fits() {
        let Some(list) = layout_json(
            r#"{"pages":[{"frames":[
                {"id":"minusculo","type":"text","rect":[0,0,200,2],"autoFlow":true,
                 "blocks":["texto que não cabe de jeito nenhum nessa altura"]}
            ]}]}"#,
        ) else {
            return;
        };
        assert_eq!(list.pages.len(), 1, "não deveria gerar páginas");
        assert!(list.diagnostics.iter().any(|d| d.code == "overset"));
    }

    /// An explicit chain still wins over autoFlow.
    #[test]
    fn thread_next_takes_precedence_over_auto_flow() {
        let Some(list) = layout_json(
            r#"{"pages":[{"frames":[
                {"id":"a","type":"text","rect":[0,0,120,40],"threadNext":"b","autoFlow":true,
                 "blocks":["alpha bravo charlie delta echo foxtrot golf hotel india juliet"]},
                {"id":"b","type":"text","rect":[200,0,120,400]}
            ]}]}"#,
        ) else {
            return;
        };
        assert_eq!(list.pages.len(), 1, "a cadeia explícita deveria bastar");
        assert!(all_runs(&list).iter().any(|r| r.x >= 200.0));
    }

    #[test]
    fn a_page_break_moves_the_rest_to_the_next_page() {
        let Some(list) = layout_json(
            r#"{"pages":[{"frames":[
                {"id":"corpo","type":"text","rect":[0,0,300,600],"autoFlow":true,
                 "blocks":["antes da quebra",{"type":"pageBreak"},"depois da quebra"]}
            ]}]}"#,
        ) else {
            return;
        };

        assert_eq!(list.pages.len(), 2, "a quebra deveria criar uma página");

        let text_of = |page: &crate::display::DisplayPage| {
            let mut out = Vec::new();
            fn walk(items: &[DisplayItem], out: &mut Vec<String>) {
                for item in items {
                    match item {
                        DisplayItem::Glyphs(run) => out.push(run.text.clone()),
                        DisplayItem::Group(group) => walk(&group.items, out),
                        _ => {}
                    }
                }
            }
            walk(&page.items, &mut out);
            out.concat()
        };

        assert!(text_of(&list.pages[0]).contains("antes"));
        assert!(!text_of(&list.pages[0]).contains("depois"));
        assert!(text_of(&list.pages[1]).contains("depois"));
    }

    /// A page break must skip a frame that sits on the same page.
    #[test]
    fn a_page_break_skips_frames_on_the_same_page() {
        let Some(list) = layout_json(
            r#"{"pages":[
                {"frames":[
                    {"id":"a","type":"text","rect":[0,0,140,600],"threadNext":"b",
                     "blocks":["antes",{"type":"pageBreak"},"depois"]},
                    {"id":"b","type":"text","rect":[160,0,140,600],"threadNext":"c"}
                ]},
                {"frames":[{"id":"c","type":"text","rect":[0,0,140,600]}]}
            ]}"#,
        ) else {
            return;
        };

        let on_page = |index: usize, needle: &str| {
            all_runs(&list)
                .iter()
                .filter(|run| run.source.as_ref().is_some_and(|s| s.page as usize == index))
                .any(|run| run.text.contains(needle))
        };

        assert!(on_page(0, "antes"));
        // Frame `b` is on the same page as the break, so it must stay empty.
        assert!(!on_page(0, "depois"), "a quebra não pulou o frame vizinho");
        assert!(on_page(1, "depois"));
    }

    /// A frame break skips the remaining columns, unlike a column break.
    #[test]
    fn a_frame_break_abandons_the_remaining_columns() {
        let Some(list) = layout_json(
            r#"{"pages":[{"frames":[
                {"id":"a","type":"text","rect":[0,0,300,200],"columns":2,"threadNext":"b",
                 "blocks":["antes",{"type":"frameBreak"},"depois"]},
                {"id":"b","type":"text","rect":[0,300,300,200]}
            ]}]}"#,
        ) else {
            return;
        };

        let after = all_runs(&list)
            .into_iter()
            .find(|run| run.text.contains("depois"))
            .expect("o texto seguinte foi colocado");
        assert!(after.y > 280.0, "deveria estar no frame b, veio em y={}", after.y);
    }

    #[test]
    fn stories_feed_frames_by_name() {
        let Some(list) = layout_json(
            r#"{
                "resources":{"stories":{"corpo":["primeiro parágrafo","segundo parágrafo"]}},
                "pages":[{"frames":[{"type":"text","rect":[0,0,400,200],"story":"corpo"}]}]
            }"#,
        ) else {
            return;
        };
        let text: String = all_runs(&list).iter().map(|r| r.text.clone()).collect();
        assert!(text.contains("primeiro"));
        assert!(text.contains("segundo"));
    }

    /// The editor writes back through provenance, so a run painted in the
    /// *second* frame of a thread must still address the original story.
    #[test]
    fn threaded_runs_point_back_at_the_story_they_came_from() {
        const TEXT: &str =
            "alpha bravo charlie delta echo foxtrot golf hotel india juliet kilo lima mike november";

        let json = format!(
            r#"{{
                "resources": {{ "stories": {{ "corpo": [
                    "cabeçalho curto",
                    {{"type":"paragraph","content":["{TEXT}"]}}
                ] }} }},
                "pages": [{{ "frames": [
                    {{"id":"a","type":"text","rect":[0,0,120,60],"story":"corpo","threadNext":"b"}},
                    {{"id":"b","type":"text","rect":[200,0,120,400]}}
                ]}}]
            }}"#
        );

        let Some(list) = layout_json(&json) else { return };

        // Runs painted by the second frame of the chain.
        let carried: Vec<GlyphRun> = all_runs(&list)
            .into_iter()
            .filter(|run| run.source.as_ref().is_some_and(|s| s.frame == "b"))
            .collect();
        assert!(!carried.is_empty(), "nothing flowed into the second frame");

        let story = ["cabeçalho curto", TEXT];

        for run in &carried {
            let source = run.source.as_ref().unwrap();
            assert_eq!(source.story.as_deref(), Some("corpo"), "story name lost");

            let block = source.block.expect("block index") as usize;
            let offset = source.offset.expect("byte offset") as usize;

            // Indices must address the original story, not the leftover slice.
            assert!(block < story.len(), "block index {block} out of range");
            let original = story[block];
            assert!(
                original[offset..].starts_with(run.text.as_str()),
                "run {:?} does not sit at offset {offset} of block {block}",
                run.text
            );
        }

        // The carried text must be the tail of the story, in order.
        let placed: String = carried.iter().map(|r| r.text.as_str()).collect();
        assert!(TEXT.ends_with(placed.trim_end()));
    }

    #[test]
    fn a_missing_story_is_a_diagnostic_not_a_crash() {
        let Some(list) = layout_json(
            r#"{"pages":[{"frames":[{"type":"text","rect":[0,0,400,200],"story":"nao-existe"}]}]}"#,
        ) else {
            return;
        };
        assert!(list.diagnostics.iter().any(|d| d.code == "unknownStory"));
    }

    #[test]
    fn master_frames_are_stamped_beneath_page_frames() {
        let Some(list) = layout_json(
            r#"{
                "resources":{"masters":{"padrao":{"frames":[
                    {"type":"text","rect":[0,800,400,30],"blocks":["rodapé"]}
                ]}}},
                "pages":[{"master":"padrao","frames":[{"type":"text","rect":[0,0,400,30],"blocks":["corpo"]}]}]
            }"#,
        ) else {
            return;
        };
        let runs = all_runs(&list);
        assert_eq!(runs.len(), 2);
        // The master's frame is painted first.
        assert_eq!(runs[0].text, "rodapé");
        assert_eq!(runs[1].text, "corpo");
    }

    #[test]
    fn named_styles_reach_the_glyphs() {
        let Some(list) = layout_json(
            r##"{
                "resources":{"styles":{"titulo":{"fontSize":24,"color":"#ff0000"}}},
                "pages":[{"frames":[{"type":"text","rect":[0,0,400,100],
                    "blocks":[{"type":"paragraph","use":"titulo","content":["Título"]}]}]}]
            }"##,
        ) else {
            return;
        };
        let run = &all_runs(&list)[0];
        assert_eq!(run.size, 24.0);
        assert_eq!(run.fill.to_hex(), "#ff0000");
    }

    #[test]
    fn frame_fill_and_border_are_emitted() {
        let Some(list) = layout_json(
            r##"{"pages":[{"frames":[{"type":"text","rect":[10,10,100,50],
                "fill":"#eeeeee","border":{"width":2,"color":"#000"},"blocks":[]}]}]}"##,
        ) else {
            return;
        };
        let rects: Vec<&RectItem> = list.pages[0]
            .items
            .iter()
            .filter_map(|i| match i {
                DisplayItem::Rect(r) => Some(r),
                _ => None,
            })
            .collect();
        assert!(rects.iter().any(|r| r.fill.is_some()), "no background");
        assert!(rects.iter().any(|r| r.stroke.is_some()), "no border");
    }

    /// A filled ellipse must not also paint the box it is inscribed in — that
    /// squares off the circle.
    #[test]
    fn a_filled_shape_paints_only_the_shape() {
        let Some(list) = layout_json(
            r##"{"pages":[{"frames":[
                {"type":"shape","shape":"ellipse","rect":[10,10,80,80],"fill":"#0e7490"}
            ]}]}"##,
        ) else {
            return;
        };

        let items = &list.pages[0].items;
        assert_eq!(items.len(), 1, "esperava só a elipse, veio {items:?}");
        assert!(
            matches!(&items[0], DisplayItem::Ellipse(e) if e.fill.is_some()),
            "o item deveria ser a elipse preenchida"
        );
    }

    /// A line with a fill must not drag a slab along behind it.
    #[test]
    fn a_filled_line_paints_only_the_line() {
        let Some(list) = layout_json(
            r##"{"pages":[{"frames":[
                {"type":"shape","shape":"line","rect":[0,0,100,1],"fill":"#000000",
                 "border":{"width":1,"color":"#000000"}}
            ]}]}"##,
        ) else {
            return;
        };
        assert_eq!(list.pages[0].items.len(), 1);
        assert!(matches!(&list.pages[0].items[0], DisplayItem::Line(_)));
    }

    /// A rectangle shape still fills, since that is where its fill belongs.
    #[test]
    fn a_rect_shape_still_fills() {
        let Some(list) = layout_json(
            r##"{"pages":[{"frames":[
                {"type":"shape","shape":"rect","rect":[0,0,50,50],"fill":"#123456"}
            ]}]}"##,
        ) else {
            return;
        };
        assert_eq!(list.pages[0].items.len(), 1);
        assert!(
            matches!(&list.pages[0].items[0], DisplayItem::Rect(r)
                if r.fill.map(|c| c.to_hex()) == Some("#123456".to_string())),
            "o retângulo deveria manter o preenchimento"
        );
    }

    /// A text frame keeps its background box; only shapes are the exception.
    #[test]
    fn a_text_frame_still_paints_its_background() {
        let Some(list) = layout_json(
            r##"{"pages":[{"frames":[
                {"type":"text","rect":[0,0,100,50],"fill":"#eeeeee","blocks":[]}
            ]}]}"##,
        ) else {
            return;
        };
        assert!(
            list.pages[0]
                .items
                .iter()
                .any(|item| matches!(item, DisplayItem::Rect(r) if r.fill.is_some())),
            "o frame de texto perdeu o fundo"
        );
    }

    #[test]
    fn partial_borders_become_individual_edges() {
        let Some(list) = layout_json(
            r#"{"pages":[{"frames":[{"type":"text","rect":[0,0,100,50],
                "border":{"width":1,"sides":{"top":false,"right":false,"left":false}},"blocks":[]}]}]}"#,
        ) else {
            return;
        };
        let lines: Vec<_> = list.pages[0]
            .items
            .iter()
            .filter(|i| matches!(i, DisplayItem::Line(_)))
            .collect();
        assert_eq!(lines.len(), 1, "only the bottom edge should be drawn");
    }

    #[test]
    fn rotation_becomes_a_group_transform() {
        let Some(list) = layout_json(
            r#"{"pages":[{"frames":[{"type":"text","rect":[0,0,100,50],"rotation":90,"blocks":["a"]}]}]}"#,
        ) else {
            return;
        };
        let group = list.pages[0]
            .items
            .iter()
            .find_map(|i| match i {
                DisplayItem::Group(g) => Some(g),
                _ => None,
            })
            .expect("rotated frame is wrapped in a group");
        let matrix = group.transform.expect("transform present");
        assert!((matrix[0] - 0.0).abs() < 1e-9, "cos 90° should be 0");
        assert!((matrix[1] - 1.0).abs() < 1e-9, "sin 90° should be 1");
    }

    #[test]
    fn groups_position_children_relative_to_themselves() {
        let Some(list) = layout_json(
            r#"{"pages":[{"frames":[{"id":"g","type":"group","rect":[100,50,200,200],"children":[
                {"id":"filho","type":"text","rect":[10,10,100,50],"blocks":["dentro"]}
            ]}]}]}"#,
        ) else {
            return;
        };
        let run = &all_runs(&list)[0];
        assert!((run.x - 110.0).abs() < 0.01, "child not offset by the group");

        let child = list.pages[0].frames.iter().find(|f| f.id == "filho").unwrap();
        assert_eq!(child.rect, Rect::new(110.0, 60.0, 100.0, 50.0));
        assert_eq!(child.ancestors, vec!["g".to_string()]);
    }

    #[test]
    fn vertical_align_moves_content_down() {
        let top = layout_json(
            r#"{"pages":[{"frames":[{"type":"text","rect":[0,0,300,200],"blocks":["x"]}]}]}"#,
        );
        let bottom = layout_json(
            r#"{"pages":[{"frames":[{"type":"text","rect":[0,0,300,200],"verticalAlign":"bottom","blocks":["x"]}]}]}"#,
        );
        let (Some(top), Some(bottom)) = (top, bottom) else {
            return;
        };
        assert!(all_runs(&bottom)[0].y > all_runs(&top)[0].y + 100.0);
    }

    // ── Table diagnostics ───────────────────────────────────────────────────

    /// One table in one frame, with the given extra declarations.
    fn table_page(extra: &str, cells: &str) -> String {
        format!(
            r#"{{"pages":[{{"frames":[
                {{"id":"quadro","type":"text","rect":[0,0,200,400],"blocks":[
                    {{"type":"table",{extra}"cells":[{cells}]}}
                ]}}
            ]}}]}}"#
        )
    }

    fn codes(list: &DisplayList) -> Vec<&str> {
        list.diagnostics.iter().map(|d| d.code.as_str()).collect()
    }

    #[test]
    fn a_table_that_fits_says_nothing_at_all() {
        let Some(list) = layout_json(&table_page(
            r#""columns":["auto","auto"],"#,
            r#"{"blocks":["a"]},{"blocks":["b"]},{"blocks":["c"]},{"blocks":["d"]}"#,
        )) else {
            return;
        };
        assert!(codes(&list).is_empty(), "sem queixas: {:?}", list.diagnostics);
    }

    #[test]
    fn two_cells_on_the_same_slot_are_reported_once_with_a_count() {
        let Some(list) = layout_json(&table_page(
            r#""columns":["auto","auto"],"#,
            r#"{"x":0,"y":0,"blocks":["a"]},
               {"x":0,"y":0,"blocks":["b"]},
               {"x":0,"y":0,"blocks":["c"]}"#,
        )) else {
            return;
        };
        let said: Vec<_> = list
            .diagnostics
            .iter()
            .filter(|d| d.code == "tableCellOverlap")
            .collect();
        assert_eq!(said.len(), 1, "uma linha por causa: {:?}", codes(&list));
        assert!(said[0].message.contains('2'), "com a conta: {}", said[0].message);
        assert_eq!(said[0].page, Some(0), "página, contada de zero como as outras");
        assert_eq!(said[0].frame.as_deref(), Some("quadro"));
    }

    #[test]
    fn a_cell_wider_than_the_table_is_named_for_what_it_is() {
        let Some(list) = layout_json(&table_page(
            r#""columns":["auto","auto"],"#,
            r#"{"x":0,"y":0,"colspan":5,"blocks":["larga"]},{"blocks":["b"]}"#,
        )) else {
            return;
        };
        assert!(
            codes(&list).contains(&"tableCellTooWide"),
            "e não confundida com uma sobreposição: {:?}",
            codes(&list),
        );
    }

    #[test]
    fn columns_that_do_not_fit_report_by_how_much() {
        // A word of its own, in a column narrower than the word.
        let Some(list) = layout_json(&table_page(
            r#""columns":["auto"],"#,
            r#"{"blocks":["incompreensibilissimamenteinterminavel"]}"#,
        )) else {
            return;
        };
        let said = list
            .diagnostics
            .iter()
            .find(|d| d.code == "tableOverflows")
            .expect("transbordo reportado");
        assert!(said.message.contains("pt"), "com a medida: {}", said.message);
        assert_eq!(said.page, Some(0));
        assert_eq!(said.frame.as_deref(), Some("quadro"));
    }

    #[test]
    fn a_row_taller_than_the_frame_says_so() {
        let filler = "palavra ".repeat(200);
        let Some(list) = layout_json(&format!(
            r#"{{"pages":[{{"frames":[
                {{"id":"quadro","type":"text","rect":[0,0,200,60],"blocks":[
                    {{"type":"table","columns":["auto"],"cells":[{{"blocks":["{filler}"]}}]}}
                ]}}
            ]}}]}}"#
        )) else {
            return;
        };
        assert!(
            codes(&list).contains(&"tableRowTooTall"),
            "a linha que transbordou é dita: {:?}",
            codes(&list),
        );
    }

    #[test]
    fn every_table_diagnostic_carries_a_page_and_a_frame() {
        // A word too wide for the frame, and a second cell claiming the slot
        // it already took. A cell dropped for being too wide would never size
        // a column, so the two faults have to be independent to both show.
        let Some(list) = layout_json(&table_page(
            r#""columns":["auto"],"#,
            r#"{"x":0,"y":0,"blocks":["incompreensibilissimamenteinterminavel"]},
               {"x":0,"y":0,"blocks":["b"]}"#,
        )) else {
            return;
        };
        let table_said: Vec<_> = list
            .diagnostics
            .iter()
            .filter(|d| d.code.starts_with("table"))
            .collect();
        assert!(table_said.len() >= 2, "mais de um código: {:?}", codes(&list));
        for said in table_said {
            assert!(said.page.is_some(), "{} sem página", said.code);
            assert!(said.frame.is_some(), "{} sem frame", said.code);
        }
    }

    #[test]
    fn a_missing_image_is_reported_and_skipped() {
        let Some(list) = layout_json(
            r#"{"pages":[{"frames":[{"type":"image","rect":[0,0,100,100],"src":"ausente.png"}]}]}"#,
        ) else {
            return;
        };
        assert!(list.diagnostics.iter().any(|d| d.code == "missingImage"));
        assert!(!list.pages[0].items.iter().any(|i| matches!(i, DisplayItem::Image(_))));
    }

    #[test]
    fn image_fit_contain_preserves_the_aspect_ratio() {
        let (Some((registry, mut images)), ()) = (engine_parts(), ()) else {
            return;
        };
        // 2:1 image in a square frame.
        let png = png_of(200, 100);
        images.add("wide", png);

        let doc: Document = serde_json::from_str(
            r#"{"pages":[{"frames":[{"type":"image","rect":[0,0,100,100],"src":"wide","fit":"contain"}]}]}"#,
        )
        .unwrap();
        let list = LayoutEngine::new(&registry, &images).layout(&doc);

        let image = list.pages[0]
            .items
            .iter()
            .find_map(|i| match i {
                DisplayItem::Image(img) => Some(img),
                _ => None,
            })
            .expect("image emitted");
        assert!((image.rect.w - 100.0).abs() < 0.01);
        assert!((image.rect.h - 50.0).abs() < 0.01);
        // Centred vertically in the square.
        assert!((image.rect.y - 25.0).abs() < 0.01);
    }

    /// Build a valid PNG header with the given dimensions, enough for probing.
    fn png_of(width: u32, height: u32) -> Vec<u8> {
        let mut bytes = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
        bytes.extend_from_slice(&13u32.to_be_bytes());
        bytes.extend_from_slice(b"IHDR");
        bytes.extend_from_slice(&width.to_be_bytes());
        bytes.extend_from_slice(&height.to_be_bytes());
        bytes.extend_from_slice(&[8, 6, 0, 0, 0]);
        bytes
    }

    #[test]
    fn missing_fonts_are_an_error_diagnostic() {
        let registry = FontRegistry::new();
        let images = ImageStore::new();
        let doc: Document = serde_json::from_str(r#"{"pages":[{}]}"#).unwrap();
        let list = LayoutEngine::new(&registry, &images).layout(&doc);
        assert!(list.has_errors());
        assert!(list.diagnostics.iter().any(|d| d.code == "noFont"));
    }
}

