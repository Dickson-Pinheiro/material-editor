//! Display list → PDF bytes.
//!
//! # The y flip
//!
//! Layout measures down from the top-left of the page; PDF measures up from the
//! bottom-left. Primitives convert with `pdf_y = page_height − y`.
//!
//! A group transform needs more care: the matrix was authored in y-down space,
//! so it is conjugated by the flip, `M' = F · M · F` (with `F = F⁻¹`). Children
//! still flip their own coordinates, and the two compose to exactly the y-down
//! result.

use std::collections::{BTreeMap, BTreeSet};

use pdf_writer::{Content, Name, Pdf, Rect as PdfRect, Ref, Str, TextStr};

use super::fonts::{FontMap, collect_glyphs, embed_fonts};
use super::images::{ImageMap, collect_images, embed_images};
use super::{PdfError, RefAlloc};
use crate::color::Color;
use crate::display::{
    DisplayItem, DisplayList, DisplayPage, EllipseItem, FillRule, GlyphRun, ImageItem, LineItem,
    PathCommand, PathItem, RectItem, Stroke,
};
use crate::fonts::FontRegistry;
use crate::images::ImageStore;
use crate::spec::Meta;
use crate::units::{Corners, Rect};

/// Control-point distance for approximating a quarter circle with a cubic.
const KAPPA: f64 = 0.552_284_749_8;

/// Alphas are bucketed to 1/1000 so near-identical values share one state.
fn alpha_key(alpha: f32) -> u32 {
    (alpha.clamp(0.0, 1.0) * 1000.0).round() as u32
}

// ─────────────────────────────────────────────────────────────────────────────
// Entry point
// ─────────────────────────────────────────────────────────────────────────────

/// Render a display list to PDF bytes.
pub fn render(
    list: &DisplayList,
    registry: &FontRegistry,
    images: &ImageStore,
    meta: &Meta,
) -> Result<Vec<u8>, PdfError> {
    let mut alloc = RefAlloc::new();
    let catalog_ref = alloc.alloc();
    let tree_ref = alloc.alloc();
    let info_ref = alloc.alloc();

    // A PDF must have at least one page.
    let blank;
    let pages: &[DisplayPage] = if list.pages.is_empty() {
        blank = [DisplayPage {
            width: 595.276,
            height: 841.89,
            ..Default::default()
        }];
        &blank
    } else {
        &list.pages
    };

    let page_refs: Vec<(Ref, Ref)> = pages.iter().map(|_| (alloc.alloc(), alloc.alloc())).collect();

    let mut pdf = Pdf::new();

    let glyph_usage = collect_glyphs(list);
    let font_map = embed_fonts(&mut pdf, registry, &glyph_usage, &mut alloc)?;

    let image_keys = collect_images(list);
    let image_map = embed_images(&mut pdf, images, &image_keys, &mut alloc)?;

    let alpha_map = embed_alphas(&mut pdf, &collect_alphas(list), &mut alloc);

    // ── Catalog and page tree ─────────────────────────────────────────────────
    {
        let mut catalog = pdf.catalog(catalog_ref);
        catalog.pages(tree_ref);
        if let Some(language) = &meta.language {
            catalog.lang(TextStr(language));
        }
    }

    pdf.pages(tree_ref)
        .kids(page_refs.iter().map(|(page, _)| *page))
        .count(pages.len() as i32);

    // ── Pages ─────────────────────────────────────────────────────────────────
    for (page, (page_ref, content_ref)) in pages.iter().zip(&page_refs) {
        let context = EmitContext {
            fonts: &font_map,
            images: &image_map,
            alphas: &alpha_map,
            height: page.height,
        };

        pdf.stream(*content_ref, &build_content(page, &context));

        let mut object = pdf.page(*page_ref);
        object
            .parent(tree_ref)
            .media_box(PdfRect::new(
                0.0,
                0.0,
                page.width as f32,
                page.height as f32,
            ))
            .contents(*content_ref);

        let mut resources = object.resources();
        if !font_map.is_empty() {
            let mut dict = resources.fonts();
            for font in font_map.fonts.values() {
                dict.pair(Name(font.resource_name.as_bytes()), font.type0_ref);
            }
        }
        if !image_map.is_empty() {
            let mut dict = resources.x_objects();
            for image in image_map.images.values() {
                dict.pair(Name(image.resource_name.as_bytes()), image.xobject_ref);
            }
        }
        if !alpha_map.is_empty() {
            let mut dict = resources.ext_g_states();
            for (state_ref, name) in alpha_map.values() {
                dict.pair(Name(name.as_bytes()), *state_ref);
            }
        }
    }

    // ── Document info ─────────────────────────────────────────────────────────
    {
        let mut info = pdf.document_info(info_ref);
        if let Some(title) = &meta.title {
            info.title(TextStr(title));
        }
        if let Some(author) = &meta.author {
            info.author(TextStr(author));
        }
        if let Some(subject) = &meta.subject {
            info.subject(TextStr(subject));
        }
        info.producer(TextStr("diagramador"));
    }

    Ok(pdf.finish())
}

struct EmitContext<'a> {
    fonts: &'a FontMap,
    images: &'a ImageMap,
    alphas: &'a BTreeMap<u32, (Ref, String)>,
    height: f64,
}

// ─────────────────────────────────────────────────────────────────────────────
// Transparency
// ─────────────────────────────────────────────────────────────────────────────

/// Every distinct alpha the document needs an `ExtGState` for.
fn collect_alphas(list: &DisplayList) -> BTreeSet<u32> {
    fn note(out: &mut BTreeSet<u32>, alpha: f32) {
        if alpha < 1.0 {
            out.insert(alpha_key(alpha));
        }
    }
    fn note_color(out: &mut BTreeSet<u32>, color: Option<Color>) {
        if let Some(color) = color {
            note(out, color.a);
        }
    }
    fn note_stroke(out: &mut BTreeSet<u32>, stroke: Option<&Stroke>) {
        if let Some(stroke) = stroke {
            note(out, stroke.color.a);
        }
    }

    fn walk(items: &[DisplayItem], out: &mut BTreeSet<u32>) {
        for item in items {
            match item {
                DisplayItem::Group(group) => {
                    note(out, group.opacity as f32);
                    walk(&group.items, out);
                }
                DisplayItem::Glyphs(run) => note(out, run.fill.a),
                DisplayItem::Rect(rect) => {
                    note_color(out, rect.fill);
                    note_stroke(out, rect.stroke.as_ref());
                }
                DisplayItem::Ellipse(ellipse) => {
                    note_color(out, ellipse.fill);
                    note_stroke(out, ellipse.stroke.as_ref());
                }
                DisplayItem::Line(line) => note(out, line.stroke.color.a),
                DisplayItem::Path(path) => {
                    if let Some(fill) = path.fill {
                        note(out, fill.a);
                    }
                    if let Some(stroke) = &path.stroke {
                        note(out, stroke.color.a);
                    }
                }
                DisplayItem::Image(_) => {}
            }
        }
    }

    let mut out = BTreeSet::new();
    for page in &list.pages {
        note_color(&mut out, page.background);
        walk(&page.items, &mut out);
    }
    out
}

fn embed_alphas(
    pdf: &mut Pdf,
    alphas: &BTreeSet<u32>,
    alloc: &mut RefAlloc,
) -> BTreeMap<u32, (Ref, String)> {
    let mut map = BTreeMap::new();
    for (index, key) in alphas.iter().enumerate() {
        let state_ref = alloc.alloc();
        let value = *key as f32 / 1000.0;
        pdf.ext_graphics(state_ref)
            .non_stroking_alpha(value)
            .stroking_alpha(value);
        map.insert(*key, (state_ref, format!("GS{index}")));
    }
    map
}

// ─────────────────────────────────────────────────────────────────────────────
// Content streams
// ─────────────────────────────────────────────────────────────────────────────

fn build_content(page: &DisplayPage, context: &EmitContext<'_>) -> Vec<u8> {
    let mut content = Content::new();

    if let Some(background) = page.background.filter(|c| !c.is_transparent()) {
        content.save_state();
        apply_alpha(&mut content, background.a, context);
        content.set_fill_rgb(background.r, background.g, background.b);
        content.rect(0.0, 0.0, page.width as f32, page.height as f32);
        content.fill_nonzero();
        content.restore_state();
    }

    write_items(&mut content, &page.items, context);
    content.finish().to_vec()
}

fn write_items(content: &mut Content, items: &[DisplayItem], context: &EmitContext<'_>) {
    for item in items {
        match item {
            DisplayItem::Group(group) => {
                content.save_state();

                if let Some(matrix) = group.transform {
                    content.transform(to_pdf_matrix(matrix, context.height));
                }
                if let Some(clip) = &group.clip {
                    path_rect(content, clip.rect, clip.radius, context.height);
                    content.clip_nonzero();
                    content.end_path();
                }
                if group.opacity < 1.0 {
                    apply_alpha(content, group.opacity as f32, context);
                }

                write_items(content, &group.items, context);
                content.restore_state();
            }
            DisplayItem::Glyphs(run) => write_glyphs(content, run, context),
            DisplayItem::Rect(rect) => write_rect(content, rect, context),
            DisplayItem::Ellipse(ellipse) => write_ellipse(content, ellipse, context),
            DisplayItem::Line(line) => write_line(content, line, context),
            DisplayItem::Path(path) => write_path(content, path, context),
            DisplayItem::Image(image) => write_image(content, image, context),
        }
    }
}

/// Conjugate a y-down matrix by the page flip so it acts the same in PDF space.
fn to_pdf_matrix(m: [f64; 6], height: f64) -> [f32; 6] {
    let [a, b, c, d, e, f] = m;
    [
        a as f32,
        -b as f32,
        -c as f32,
        d as f32,
        (c * height + e) as f32,
        (height - f - d * height) as f32,
    ]
}

fn apply_alpha(content: &mut Content, alpha: f32, context: &EmitContext<'_>) {
    if alpha >= 1.0 {
        return;
    }
    if let Some((_, name)) = context.alphas.get(&alpha_key(alpha)) {
        content.set_parameters(Name(name.as_bytes()));
    }
}

// ── Text ─────────────────────────────────────────────────────────────────────

fn write_glyphs(content: &mut Content, run: &GlyphRun, context: &EmitContext<'_>) {
    if run.glyphs.is_empty() || run.size <= 0.0 {
        return;
    }
    let Some(font) = context.fonts.get(run.font) else {
        return;
    };

    let baseline = (context.height - run.y) as f32;

    content.save_state();
    apply_alpha(content, run.fill.a, context);
    content.begin_text();
    content.set_fill_rgb(run.fill.r, run.fill.g, run.fill.b);
    content.set_font(Name(font.resource_name.as_bytes()), run.size as f32);

    // The common case: every glyph sits on the baseline, so one text matrix and
    // a single TJ array carry the whole run.
    if run.glyphs.iter().all(|glyph| glyph.y.abs() < 1e-9) {
        content.set_text_matrix([1.0, 0.0, 0.0, 1.0, run.x as f32, baseline]);

        let mut positioned = content.show_positioned();
        let mut items = positioned.items();
        let mut pen = 0.0f64;

        for glyph in &run.glyphs {
            // /DW is 0, so nothing moves unless we say so.
            let delta = glyph.x - pen;
            if delta.abs() > 1e-9 {
                items.adjust((-delta * 1000.0 / run.size) as f32);
            }
            let gid = font.remapper.get(glyph.id).unwrap_or(0);
            items.show(Str(&gid.to_be_bytes()));
            pen = glyph.x;
        }
    } else {
        // Marks with a vertical offset get their own matrix.
        for glyph in &run.glyphs {
            content.set_text_matrix([
                1.0,
                0.0,
                0.0,
                1.0,
                (run.x + glyph.x) as f32,
                baseline - glyph.y as f32,
            ]);
            let gid = font.remapper.get(glyph.id).unwrap_or(0);
            content.show(Str(&gid.to_be_bytes()));
        }
    }

    content.end_text();
    content.restore_state();
}

// ── Shapes ───────────────────────────────────────────────────────────────────

fn write_rect(content: &mut Content, rect: &RectItem, context: &EmitContext<'_>) {
    let fill = rect.fill.filter(|c| !c.is_transparent());
    let stroke = rect.stroke.as_ref().filter(|s| s.width > 0.0);
    if fill.is_none() && stroke.is_none() {
        return;
    }

    content.save_state();
    prepare_paint(content, fill, stroke, context);
    path_rect(content, rect.rect, rect.radius, context.height);
    paint(content, fill.is_some(), stroke.is_some());
    content.restore_state();
}

/// Write an outline.
///
/// The y flip happens here, once per point, the same way `path_rect` and
/// `path_ellipse` do it — the display list is y-down and PDF is y-up, and the
/// boundary between them is this module.
fn write_path(content: &mut Content, item: &PathItem, context: &EmitContext<'_>) {
    let fill = item.fill.filter(|c| !c.is_transparent());
    let stroke = item.stroke.as_ref().filter(|s| s.width > 0.0);
    if (fill.is_none() && stroke.is_none()) || item.commands.is_empty() {
        return;
    }

    content.save_state();
    prepare_paint(content, fill, stroke, context);

    let f = |v: f64| v as f32;
    let up = |y: f64| f(context.height - y);
    for command in &item.commands {
        match *command {
            PathCommand::MoveTo { x, y } => {
                content.move_to(f(x), up(y));
            }
            PathCommand::LineTo { x, y } => {
                content.line_to(f(x), up(y));
            }
            PathCommand::CurveTo { x1, y1, x2, y2, x, y } => {
                content.cubic_to(f(x1), up(y1), f(x2), up(y2), f(x), up(y));
            }
            PathCommand::Close => {
                content.close_path();
            }
        };
    }

    match (fill.is_some(), stroke.is_some(), item.fill_rule) {
        (true, true, FillRule::EvenOdd) => content.fill_even_odd_and_stroke(),
        (true, true, FillRule::NonZero) => content.fill_nonzero_and_stroke(),
        (true, false, FillRule::EvenOdd) => content.fill_even_odd(),
        (true, false, FillRule::NonZero) => content.fill_nonzero(),
        (false, true, _) => content.stroke(),
        (false, false, _) => content.end_path(),
    };
    content.restore_state();
}

fn write_ellipse(content: &mut Content, ellipse: &EllipseItem, context: &EmitContext<'_>) {
    let fill = ellipse.fill.filter(|c| !c.is_transparent());
    let stroke = ellipse.stroke.as_ref().filter(|s| s.width > 0.0);
    if fill.is_none() && stroke.is_none() {
        return;
    }

    content.save_state();
    prepare_paint(content, fill, stroke, context);
    path_ellipse(content, ellipse.rect, context.height);
    paint(content, fill.is_some(), stroke.is_some());
    content.restore_state();
}

fn write_line(content: &mut Content, line: &LineItem, context: &EmitContext<'_>) {
    if line.stroke.width <= 0.0 || line.stroke.color.is_transparent() {
        return;
    }

    content.save_state();
    apply_alpha(content, line.stroke.color.a, context);
    set_stroke(content, &line.stroke);
    content.move_to(line.x1 as f32, (context.height - line.y1) as f32);
    content.line_to(line.x2 as f32, (context.height - line.y2) as f32);
    content.stroke();
    content.restore_state();
}

fn write_image(content: &mut Content, image: &ImageItem, context: &EmitContext<'_>) {
    let Some(embedded) = context.images.get(&image.src) else {
        return;
    };
    let rect = image.rect;
    if rect.w <= 0.0 || rect.h <= 0.0 {
        return;
    }

    content.save_state();
    // An image XObject fills the unit square; this matrix maps it to the frame.
    content.transform([
        rect.w as f32,
        0.0,
        0.0,
        rect.h as f32,
        rect.x as f32,
        (context.height - rect.y - rect.h) as f32,
    ]);
    content.x_object(Name(embedded.resource_name.as_bytes()));
    content.restore_state();
}

fn prepare_paint(
    content: &mut Content,
    fill: Option<Color>,
    stroke: Option<&Stroke>,
    context: &EmitContext<'_>,
) {
    let alpha = fill
        .map(|c| c.a)
        .into_iter()
        .chain(stroke.map(|s| s.color.a))
        .fold(1.0f32, f32::min);
    apply_alpha(content, alpha, context);

    if let Some(color) = fill {
        content.set_fill_rgb(color.r, color.g, color.b);
    }
    if let Some(stroke) = stroke {
        set_stroke(content, stroke);
    }
}

fn set_stroke(content: &mut Content, stroke: &Stroke) {
    content.set_line_width(stroke.width as f32);
    content.set_stroke_rgb(stroke.color.r, stroke.color.g, stroke.color.b);
    match stroke.dash {
        Some([on, off]) => {
            content.set_dash_pattern([on as f32, off as f32], 0.0);
        }
        None => {
            content.set_dash_pattern([], 0.0);
        }
    }
}

fn paint(content: &mut Content, fill: bool, stroke: bool) {
    match (fill, stroke) {
        (true, true) => content.fill_nonzero_and_stroke(),
        (true, false) => content.fill_nonzero(),
        (false, true) => content.stroke(),
        (false, false) => content.end_path(),
    };
}

/// Append a rectangle path, rounding each corner by its own radius.
///
/// The walk is counter-clockwise because this is already PDF space, where `y`
/// grows upward: it starts on the bottom edge and comes back round to it. The
/// corner named for where it sits on the **page** therefore appears flipped
/// here — the document's top-left is this path's `(x0, y1)`.
fn path_rect(content: &mut Content, rect: Rect, radius: Corners, height: f64) {
    let x = rect.x;
    let y = height - rect.y - rect.h;
    let w = rect.w;
    let h = rect.h;

    let r = radius.fitted(w, h);
    if r.is_zero() {
        content.rect(x as f32, y as f32, w as f32, h as f32);
        return;
    }

    let (x0, y0) = (x, y);
    let (x1, y1) = (x + w, y + h);
    let f = |v: f64| v as f32;

    // Named for the page, so `bl` is the bottom-left of the document even
    // though it is the corner at `(x0, y0)` after the flip.
    let (tl, tr, br, bl) = (r.top_left, r.top_right, r.bottom_right, r.bottom_left);
    let k = |v: f64| v * KAPPA;

    // Bottom edge, left to right.
    content.move_to(f(x0 + bl), f(y0));
    content.line_to(f(x1 - br), f(y0));
    if br > 0.0 {
        content.cubic_to(f(x1 - br + k(br)), f(y0), f(x1), f(y0 + br - k(br)), f(x1), f(y0 + br));
    }
    // Right edge, bottom to top.
    content.line_to(f(x1), f(y1 - tr));
    if tr > 0.0 {
        content.cubic_to(f(x1), f(y1 - tr + k(tr)), f(x1 - tr + k(tr)), f(y1), f(x1 - tr), f(y1));
    }
    // Top edge, right to left.
    content.line_to(f(x0 + tl), f(y1));
    if tl > 0.0 {
        content.cubic_to(f(x0 + tl - k(tl)), f(y1), f(x0), f(y1 - tl + k(tl)), f(x0), f(y1 - tl));
    }
    // Left edge, top to bottom.
    content.line_to(f(x0), f(y0 + bl));
    if bl > 0.0 {
        content.cubic_to(f(x0), f(y0 + bl - k(bl)), f(x0 + bl - k(bl)), f(y0), f(x0 + bl), f(y0));
    }
    content.close_path();
}

/// Append an ellipse inscribed in `rect`, as four cubic segments.
fn path_ellipse(content: &mut Content, rect: Rect, height: f64) {
    let cx = rect.x + rect.w / 2.0;
    let cy = height - rect.y - rect.h / 2.0;
    let rx = rect.w / 2.0;
    let ry = rect.h / 2.0;
    let (kx, ky) = (rx * KAPPA, ry * KAPPA);

    let f = |v: f64| v as f32;

    content.move_to(f(cx), f(cy + ry));
    content.cubic_to(f(cx + kx), f(cy + ry), f(cx + rx), f(cy + ky), f(cx + rx), f(cy));
    content.cubic_to(f(cx + rx), f(cy - ky), f(cx + kx), f(cy - ry), f(cx), f(cy - ry));
    content.cubic_to(f(cx - kx), f(cy - ry), f(cx - rx), f(cy - ky), f(cx - rx), f(cy));
    content.cubic_to(f(cx - rx), f(cy + ky), f(cx - kx), f(cy + ry), f(cx), f(cy + ry));
    content.close_path();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::display::{ClipShape, DisplayGroup};

    #[test]
    fn identity_matrix_survives_the_flip() {
        let out = to_pdf_matrix([1.0, 0.0, 0.0, 1.0, 0.0, 0.0], 800.0);
        assert_eq!(out, [1.0, 0.0, 0.0, 1.0, 0.0, 0.0]);
    }

    #[test]
    fn downward_translation_becomes_upward_in_pdf_space() {
        // Move 10pt down the page in layout space.
        let out = to_pdf_matrix([1.0, 0.0, 0.0, 1.0, 5.0, 10.0], 800.0);
        assert_eq!(out[4], 5.0);
        assert_eq!(out[5], -10.0);
    }

    #[test]
    fn rotation_reverses_its_sense_under_the_flip() {
        // 90° clockwise in y-down space is 90° anticlockwise in y-up space.
        let m = [0.0, 1.0, -1.0, 0.0, 0.0, 0.0];
        let out = to_pdf_matrix(m, 800.0);
        assert_eq!(out[1], -1.0);
        assert_eq!(out[2], 1.0);
    }

    #[test]
    fn alphas_bucket_by_thousandths() {
        assert_eq!(alpha_key(0.5), 500);
        assert_eq!(alpha_key(1.0), 1000);
        assert_eq!(alpha_key(0.5004), alpha_key(0.4996));
    }

    #[test]
    fn only_translucent_things_need_a_graphics_state() {
        let mut list = DisplayList::new();
        list.pages.push(DisplayPage {
            items: vec![
                DisplayItem::Rect(RectItem {
                    fill: Some(Color::BLACK),
                    ..Default::default()
                }),
                DisplayItem::Group(DisplayGroup {
                    opacity: 0.4,
                    clip: Some(ClipShape::default()),
                    ..DisplayGroup::new()
                }),
            ],
            ..Default::default()
        });

        let alphas = collect_alphas(&list);
        assert_eq!(alphas.len(), 1);
        assert!(alphas.contains(&400));
    }

    /// The operators of a freshly built path, as text.
    fn path_ops(radius: Corners) -> String {
        let mut content = Content::new();
        // A 100×100 box on a 100-tall page, so PDF space and page space agree
        // on the origin and the numbers stay readable.
        path_rect(&mut content, Rect::new(0.0, 0.0, 100.0, 100.0), radius, 100.0);
        String::from_utf8_lossy(&content.finish()).into_owned()
    }

    #[test]
    fn a_square_box_is_still_one_rectangle_operator() {
        let ops = path_ops(Corners::ZERO);
        assert!(ops.contains(" re"), "expected a rect operator, got {ops}");
        assert!(!ops.contains(" c\n"), "nothing to curve: {ops}");
    }

    #[test]
    fn each_rounded_corner_costs_exactly_one_curve() {
        let count = |radius: Corners| path_ops(radius).matches(" c").count();

        assert_eq!(count(Corners::all(10.0)), 4);
        assert_eq!(count(Corners::new(10.0, 0.0, 0.0, 0.0)), 1);
        assert_eq!(count(Corners::new(10.0, 0.0, 10.0, 0.0)), 2);
        assert_eq!(count(Corners::new(1.0, 2.0, 3.0, 0.0)), 3);
    }

    #[test]
    fn the_curve_lands_on_the_corner_that_asked_for_it() {
        // Only the document's top-left is rounded. After the flip that corner
        // is at (0, 100), so the arc must sit up there and nowhere else.
        let ops = path_ops(Corners::new(20.0, 0.0, 0.0, 0.0));

        // The top edge stops 20pt short of the left…
        assert!(ops.contains("20 100 l"), "{ops}");
        // …and the arc from there lands 20pt down the left edge.
        assert!(ops.contains("0 80 c"), "{ops}");
        // The other three corners are reached square, on the box itself.
        for corner in ["0 0", "100 0", "100 100"] {
            assert!(
                ops.contains(&format!("{corner} l")) || ops.contains(&format!("{corner} m")),
                "corner {corner} should be square: {ops}"
            );
        }
    }
}
