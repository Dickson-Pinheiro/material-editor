//! Text wrap geometry.
//!
//! One question, asked once per line: *given this horizontal band, which
//! stretches of it can hold text?* Everything here is pure geometry — no
//! fonts, no text, no document. The line breaker calls in, gets back a list
//! of gaps, and fills them.
//!
//! The engine never looks at an image's pixels. A silhouette arrives as a
//! ring in the document, traced by the editor, so the same document lays out
//! the same way on every platform and in both wasm targets.

use crate::display::Diagnostic;
use crate::spec::frame::{Frame, FrameContent, Wrap, WrapMode};
use crate::units::{Insets, Rect};

/// A horizontal stretch, in page points.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Interval {
    pub left: f64,
    pub right: f64,
}

impl Interval {
    pub const fn new(left: f64, right: f64) -> Self {
        Interval { left, right }
    }

    #[inline]
    pub fn width(&self) -> f64 {
        (self.right - self.left).max(0.0)
    }
}

/// Something that pushes text aside, already in page coordinates.
#[derive(Debug, Clone, PartialEq)]
pub struct Obstacle {
    /// The frame it came from, for diagnostics.
    pub id: String,
    pub shape: ObstacleShape,
    /// Axis-aligned bounds, padding included. Cheap rejection before any
    /// scanline work.
    pub bounds: Rect,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ObstacleShape {
    /// Padding is already baked into the rect.
    Box(Rect),
    /// A closed ring in page points. Padding is applied to the interval the
    /// ring produces, not to the ring itself: growing a concave polygon
    /// outward is a hard problem, and growing the interval gets the same
    /// clearance for a line of text.
    Polygon {
        points: Vec<[f64; 2]>,
        padding: Insets,
    },
}

/// Which vertical extent of a line asks the question.
///
/// The choice changes how close text may sit to a shape: the line box carries
/// the leading, the ink box only rises and falls as far as the font does.
/// Kept as a parameter because it is a measured decision, not a taste one.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum BandMode {
    /// Top of the line box to the bottom of the line box.
    #[default]
    LineBox,
    /// Ascender to descender, ignoring the leading.
    InkBox,
}

impl Obstacle {
    /// Build an obstacle from a frame's rect, rotation and wrap settings.
    ///
    /// `rect` must already be in page coordinates — the caller walks groups
    /// and applies their translation. Returns `None` when the wrap cannot
    /// block anything.
    pub fn build(id: &str, rect: Rect, rotation: f64, wrap: &Wrap) -> Option<Obstacle> {
        if rect.w <= 0.0 || rect.h <= 0.0 {
            return None;
        }

        // A ring that encloses nothing falls back to the box rather than
        // silently switching the wrap off: the author asked for a wrap.
        let ring = match &wrap.mode {
            WrapMode::Contour { points } if wrap.mode.usable() => Some(points),
            _ => None,
        };

        let shape = match ring {
            None => ObstacleShape::Box(inflate(rotated_bounds(rect, rotation), wrap.padding)),
            Some(points) => ObstacleShape::Polygon {
                points: place_ring(points, rect, rotation),
                padding: wrap.padding,
            },
        };

        let bounds = match &shape {
            ObstacleShape::Box(r) => *r,
            ObstacleShape::Polygon { points, padding } => inflate(ring_bounds(points)?, *padding),
        };

        Some(Obstacle {
            id: id.to_string(),
            shape,
            bounds,
        })
    }

    /// The stretches this obstacle denies within `[top, bottom]`.
    ///
    /// More than one, because a shape can be in two places at the same
    /// height: the arms of a `⊐` block their own columns and leave the notch
    /// between them usable. Collapsing them to one span would deny text a gap
    /// the author drew on purpose.
    fn blocked(&self, top: f64, bottom: f64) -> Vec<Interval> {
        if bottom <= self.bounds.y || top >= self.bounds.bottom() {
            return Vec::new();
        }

        match &self.shape {
            ObstacleShape::Box(r) => vec![Interval::new(r.x, r.right())],
            ObstacleShape::Polygon { points, padding } => ring_runs(points, top, bottom)
                .into_iter()
                .map(|run| Interval::new(run.left - padding.left, run.right + padding.right))
                .collect(),
        }
    }
}

/// The horizontal room a line of text may use.
///
/// This is the seam. Before wrap existed, a paragraph asked one question —
/// *how wide is the column, and is this the first line?* — and the answer was
/// a number. Now it asks the same question of a band of the page, and the
/// answer is a list of gaps.
///
/// Coordinates are local to the column, matching the space a paragraph lays
/// itself out in: `x = 0` is the column's left edge, `y = 0` is the
/// paragraph's own top. `flow_blocks` translates the finished items to the
/// page afterwards, exactly as it always has.
pub trait LineSpace {
    /// Usable stretches for a line occupying `[top, bottom]`, left to right,
    /// appended to `out`.
    ///
    /// Nothing appended means this band is fully blocked and the line has to
    /// move down.
    ///
    /// Writes into a caller-owned buffer rather than returning one: this is
    /// asked once per line of every paragraph in the document, and the answer
    /// is usually a single interval. Housekeeping, not a measured win — see
    /// `docs/contorno/medicao.md` for what the benchmark actually said.
    fn slots(&self, top: f64, bottom: f64, out: &mut Vec<Interval>);
}

/// No obstacle: the whole column, always.
///
/// The behaviour every document had before wrap, kept as its own type so the
/// common path costs one virtual call and no geometry.
pub struct WholeColumn {
    pub width: f64,
}

impl LineSpace for WholeColumn {
    fn slots(&self, _top: f64, _bottom: f64, out: &mut Vec<Interval>) {
        out.push(Interval::new(0.0, self.width));
    }
}

/// A column with obstacles cut out of it.
pub struct ColumnSpace<'a> {
    pub obstacles: &'a [Obstacle],
    /// The column in page coordinates.
    pub column: Rect,
    /// Page `y` where the paragraph's local `y = 0` sits.
    pub origin_y: f64,
    /// Gaps narrower than this are not offered to text.
    pub min_slot: f64,
}

impl LineSpace for ColumnSpace<'_> {
    fn slots(&self, top: f64, bottom: f64, out: &mut Vec<Interval>) {
        let cuts = blocked(self.obstacles, self.origin_y + top, self.origin_y + bottom);
        if cuts.is_empty() {
            out.push(Interval::new(0.0, self.column.w));
            return;
        }

        for slot in carve(
            Interval::new(self.column.x, self.column.right()),
            &cuts,
            self.min_slot,
        ) {
            // Back to the paragraph's own coordinates.
            out.push(Interval::new(
                slot.left - self.column.x,
                slot.right - self.column.x,
            ));
        }
    }
}

/// Everything on a page that pushes text aside.
///
/// Walks the frame tree once, before any frame is laid out, and puts every
/// wrap into page coordinates. Master frames come first, exactly as they do
/// when painting: a wrap stamped on the master has to move the text on every
/// page that uses it.
///
/// **This cannot become circular.** A wrap comes from a frame's authored
/// `rect`, never from a measurement — an image does not change size because
/// of the text beside it. The one shape that would be circular is a text
/// frame with `overflow: grow`, whose height depends on its own content, and
/// text frames carry no wrap at all.
pub fn collect(
    frames: &[&[Frame]],
    page: u32,
    diagnostics: &mut Vec<Diagnostic>,
) -> Vec<Obstacle> {
    let mut out = Vec::new();
    for group in frames {
        walk(group, 0.0, 0.0, false, page, &mut out, diagnostics);
    }
    out
}

fn walk(
    frames: &[Frame],
    origin_x: f64,
    origin_y: f64,
    turned: bool,
    page: u32,
    out: &mut Vec<Obstacle>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    for frame in frames {
        if !frame.visible {
            continue;
        }
        let rect = frame.rect.translate(origin_x, origin_y);
        let id = frame.id.clone().unwrap_or_default();

        if let Some(wrap) = frame.wrap() {
            // A group's rotation is painted as a transform over its children,
            // so a child's rect is still axis-aligned in page space — the same
            // simplification `DisplayFrame` already makes. Rather than let the
            // wrap sit somewhere the picture is not, say so.
            if turned {
                diagnostics.push(
                    Diagnostic::warning(
                        "wrapInRotatedGroup",
                        "o contorno usa a posição sem a rotação do grupo",
                    )
                    .on(page, id.clone()),
                );
            }
            if let Some(obstacle) = Obstacle::build(&id, rect, frame.rotation, wrap) {
                out.push(obstacle);
            }
        }

        if let FrameContent::Group(group) = &frame.content {
            // Children are positioned from the group's own corner, matching
            // `layout_frame`.
            walk(
                &group.children,
                rect.x,
                rect.y,
                turned || frame.rotation != 0.0,
                page,
                out,
                diagnostics,
            );
        }
    }
}

/// Every stretch denied within the band, in no particular order.
pub fn blocked(obstacles: &[Obstacle], top: f64, bottom: f64) -> Vec<Interval> {
    obstacles
        .iter()
        .flat_map(|o| o.blocked(top, bottom))
        .collect()
}

/// Subtract the blocked stretches from `base`, left to right.
///
/// Gaps narrower than `min` are dropped: a three-point sliver between two
/// photographs is not somewhere a line of text can go, and offering it would
/// only produce a column of broken syllables.
pub fn carve(base: Interval, blocked: &[Interval], min: f64) -> Vec<Interval> {
    let mut slots = vec![base];

    for cut in blocked {
        if cut.width() <= 0.0 {
            continue;
        }
        let mut next = Vec::with_capacity(slots.len() + 1);
        for slot in &slots {
            if cut.right <= slot.left || cut.left >= slot.right {
                next.push(*slot);
                continue;
            }
            if cut.left > slot.left {
                next.push(Interval::new(slot.left, cut.left));
            }
            if cut.right < slot.right {
                next.push(Interval::new(cut.right, slot.right));
            }
        }
        slots = next;
    }

    slots.retain(|s| s.width() >= min);
    slots.sort_by(|a, b| a.left.total_cmp(&b.left));
    slots
}

// ── Geometry ────────────────────────────────────────────────────────────────

/// Map a `0..1` ring onto a rect, rotating about the rect's centre.
///
/// Clockwise degrees, matching `Frame::rotation` and `rotation_matrix`.
fn place_ring(points: &[[f64; 2]], rect: Rect, rotation: f64) -> Vec<[f64; 2]> {
    if rotation == 0.0 {
        return points
            .iter()
            .map(|p| [rect.x + p[0] * rect.w, rect.y + p[1] * rect.h])
            .collect();
    }

    let (sin, cos) = rotation.to_radians().sin_cos();
    let cx = rect.x + rect.w / 2.0;
    let cy = rect.y + rect.h / 2.0;

    points
        .iter()
        .map(|p| {
            let x = (p[0] - 0.5) * rect.w;
            let y = (p[1] - 0.5) * rect.h;
            [cx + x * cos - y * sin, cy + x * sin + y * cos]
        })
        .collect()
}

/// Bounds of a rect once rotated about its own centre.
fn rotated_bounds(rect: Rect, rotation: f64) -> Rect {
    if rotation == 0.0 {
        return rect;
    }
    let corners = place_ring(
        &[[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]],
        rect,
        rotation,
    );
    ring_bounds(&corners).unwrap_or(rect)
}

fn ring_bounds(points: &[[f64; 2]]) -> Option<Rect> {
    let first = points.first()?;
    let (mut min_x, mut max_x) = (first[0], first[0]);
    let (mut min_y, mut max_y) = (first[1], first[1]);
    for p in points {
        min_x = min_x.min(p[0]);
        max_x = max_x.max(p[0]);
        min_y = min_y.min(p[1]);
        max_y = max_y.max(p[1]);
    }
    Some(Rect::new(min_x, min_y, max_x - min_x, max_y - min_y))
}

fn inflate(rect: Rect, padding: Insets) -> Rect {
    Rect::new(
        rect.x - padding.left,
        rect.y - padding.top,
        rect.w + padding.horizontal(),
        rect.h + padding.vertical(),
    )
}

/// Where the ring sits, horizontally, anywhere inside the band.
///
/// The union of every row's runs, merged — not one row's answer, and not one
/// span from the leftmost to the rightmost point. Two properties matter, and
/// they pull in opposite directions:
///
/// - a shape that tapers downward must still keep its widest point clear
///   across the whole line, or a descender lands inside the picture between
///   two sampled rows. That is why it is a union over the band;
/// - a shape with a genuine horizontal gap must keep the gap usable. That is
///   why the runs stay separate instead of collapsing to min-left/max-right.
///
/// Sampling at 1pt steps plus both edges is enough: a feature thinner than a
/// point cannot hold a glyph away from anything.
fn ring_runs(points: &[[f64; 2]], top: f64, bottom: f64) -> Vec<Interval> {
    // Both edges of the band, plus every whole point between them.
    let mut rows = vec![top, bottom];
    let mut y = top.ceil();
    while y < bottom {
        rows.push(y);
        y += 1.0;
    }

    let runs = rows
        .into_iter()
        .flat_map(|y| crossings(points, y))
        .map(|(a, b)| Interval::new(a, b))
        .collect();

    merge(runs)
}

/// Collapse overlapping and touching intervals into the fewest that cover the
/// same ground, left to right.
fn merge(mut runs: Vec<Interval>) -> Vec<Interval> {
    if runs.len() < 2 {
        return runs;
    }
    runs.sort_by(|a, b| a.left.total_cmp(&b.left));

    let mut merged: Vec<Interval> = Vec::with_capacity(runs.len());
    for run in runs {
        match merged.last_mut() {
            Some(last) if run.left <= last.right => last.right = last.right.max(run.right),
            _ => merged.push(run),
        }
    }
    merged
}

/// Pairs of x where the ring crosses the horizontal line `y`, inside-out.
fn crossings(points: &[[f64; 2]], y: f64) -> Vec<(f64, f64)> {
    let mut xs: Vec<f64> = Vec::new();
    let mut a = *points.last().unwrap();

    for b in points {
        // Half-open on purpose: a vertex exactly on `y` counts once, so a
        // ring is never reported as crossing an odd number of times.
        if (a[1] <= y && y < b[1]) || (b[1] <= y && y < a[1]) {
            xs.push(a[0] + (y - a[1]) * (b[0] - a[0]) / (b[1] - a[1]));
        }
        a = *b;
    }

    xs.sort_by(f64::total_cmp);
    xs.chunks_exact(2).map(|p| (p[0], p[1])).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn boxed(rect: Rect, padding: f64) -> Obstacle {
        Obstacle::build(
            "o",
            rect,
            0.0,
            &Wrap {
                mode: WrapMode::Box,
                padding: Insets::all(padding),
            },
        )
        .unwrap()
    }

    /// `LineSpace::slots` into a fresh vector, for readability in tests.
    fn slots_of(space: &dyn LineSpace, top: f64, bottom: f64) -> Vec<Interval> {
        let mut out = Vec::new();
        space.slots(top, bottom, &mut out);
        out
    }

    /// The single stretch an obstacle denies in this band, asserting there is
    /// exactly one — most shapes block one run, and a test that silently read
    /// the first of several would prove nothing.
    fn one(cuts: Vec<Interval>) -> Interval {
        assert_eq!(cuts.len(), 1, "expected a single blocked run, got {cuts:?}");
        cuts[0]
    }

    fn contour(rect: Rect, points: Vec<[f64; 2]>, rotation: f64, padding: f64) -> Obstacle {
        Obstacle::build(
            "o",
            rect,
            rotation,
            &Wrap {
                mode: WrapMode::Contour { points },
                padding: Insets::all(padding),
            },
        )
        .unwrap()
    }

    // ── carve ──────────────────────────────────────────────────────────────

    #[test]
    fn nothing_blocked_leaves_the_whole_base() {
        let slots = carve(Interval::new(0.0, 100.0), &[], 1.0);
        assert_eq!(slots, vec![Interval::new(0.0, 100.0)]);
    }

    #[test]
    fn a_block_in_the_middle_leaves_two_slots() {
        let slots = carve(
            Interval::new(80.0, 420.0),
            &[Interval::new(200.0, 310.0)],
            1.0,
        );
        assert_eq!(
            slots,
            vec![Interval::new(80.0, 200.0), Interval::new(310.0, 420.0)]
        );
    }

    #[test]
    fn a_block_covering_everything_leaves_nothing() {
        let slots = carve(Interval::new(80.0, 420.0), &[Interval::new(0.0, 500.0)], 1.0);
        assert!(slots.is_empty());
    }

    #[test]
    fn overlapping_blocks_merge_into_one_hole() {
        let slots = carve(
            Interval::new(0.0, 300.0),
            &[Interval::new(100.0, 200.0), Interval::new(150.0, 250.0)],
            1.0,
        );
        assert_eq!(
            slots,
            vec![Interval::new(0.0, 100.0), Interval::new(250.0, 300.0)]
        );
    }

    #[test]
    fn blocks_arriving_out_of_order_still_come_back_left_to_right() {
        let slots = carve(
            Interval::new(0.0, 400.0),
            &[
                Interval::new(300.0, 320.0),
                Interval::new(100.0, 120.0),
                Interval::new(200.0, 220.0),
            ],
            1.0,
        );
        let lefts: Vec<f64> = slots.iter().map(|s| s.left).collect();
        assert_eq!(lefts, vec![0.0, 120.0, 220.0, 320.0]);
    }

    #[test]
    fn a_sliver_narrower_than_the_minimum_is_dropped() {
        let slots = carve(
            Interval::new(0.0, 100.0),
            &[Interval::new(5.0, 95.0)],
            24.0,
        );
        assert!(slots.is_empty(), "5pt and 5pt slivers are not text columns");
    }

    #[test]
    fn a_block_touching_the_edge_only_trims() {
        let slots = carve(Interval::new(0.0, 100.0), &[Interval::new(-10.0, 30.0)], 1.0);
        assert_eq!(slots, vec![Interval::new(30.0, 100.0)]);
    }

    // ── box obstacles ──────────────────────────────────────────────────────

    #[test]
    fn a_box_blocks_its_own_width_plus_padding() {
        let o = boxed(Rect::new(100.0, 50.0, 80.0, 40.0), 6.0);
        let cut = one(o.blocked(60.0, 72.0));
        assert_eq!(cut, Interval::new(94.0, 186.0));
    }

    #[test]
    fn a_box_outside_the_band_blocks_nothing() {
        let o = boxed(Rect::new(100.0, 50.0, 80.0, 40.0), 0.0);
        assert!(o.blocked(0.0, 40.0).is_empty(), "band above");
        assert!(o.blocked(100.0, 120.0).is_empty(), "band below");
    }

    #[test]
    fn vertical_padding_extends_the_bands_a_box_reaches() {
        let o = boxed(Rect::new(100.0, 50.0, 80.0, 40.0), 6.0);
        assert!(
            !o.blocked(38.0, 46.0).is_empty(),
            "a band just above the box is still within its clearance"
        );
    }

    // ── polygon obstacles ──────────────────────────────────────────────────

    #[test]
    fn a_ring_shaped_like_its_rect_blocks_like_a_box() {
        let rect = Rect::new(100.0, 50.0, 80.0, 40.0);
        let o = contour(
            rect,
            vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]],
            0.0,
            0.0,
        );
        let cut = one(o.blocked(60.0, 72.0));
        assert!((cut.left - 100.0).abs() < 1e-9, "left was {}", cut.left);
        assert!((cut.right - 180.0).abs() < 1e-9, "right was {}", cut.right);
    }

    #[test]
    fn a_triangle_blocks_more_as_the_band_descends() {
        // Apex at the top, base at the bottom.
        let o = contour(
            Rect::new(0.0, 0.0, 100.0, 100.0),
            vec![[0.5, 0.0], [1.0, 1.0], [0.0, 1.0]],
            0.0,
            0.0,
        );
        let high = one(o.blocked(10.0, 22.0));
        let low = one(o.blocked(80.0, 92.0));
        assert!(
            low.width() > high.width(),
            "high {:?} should be narrower than low {:?}",
            high,
            low
        );
    }

    #[test]
    fn the_band_gets_the_widest_reach_inside_it_not_the_middle() {
        let o = contour(
            Rect::new(0.0, 0.0, 100.0, 100.0),
            vec![[0.5, 0.0], [1.0, 1.0], [0.0, 1.0]],
            0.0,
            0.0,
        );
        // Across this band the triangle is widest at the bottom edge.
        let cut = one(o.blocked(50.0, 62.0));
        let at_bottom = one(o.blocked(61.5, 62.0));
        assert!(
            (cut.width() - at_bottom.width()).abs() < 1.0,
            "band {:?} should match its widest row {:?}",
            cut,
            at_bottom
        );
    }

    #[test]
    fn a_concave_ring_blocks_only_where_it_actually_is() {
        // A "C" opening rightward. Across the mouth the shape is only the
        // spine, so that is all it may deny — the mouth is free page.
        let o = contour(
            Rect::new(0.0, 0.0, 100.0, 100.0),
            vec![
                [0.0, 0.0],
                [1.0, 0.0],
                [1.0, 0.2],
                [0.3, 0.2],
                [0.3, 0.8],
                [1.0, 0.8],
                [1.0, 1.0],
                [0.0, 1.0],
            ],
            0.0,
            0.0,
        );
        let cuts = o.blocked(40.0, 52.0);
        assert_eq!(cuts.len(), 1);
        assert!((cuts[0].left - 0.0).abs() < 1e-9);
        assert!((cuts[0].right - 30.0).abs() < 1e-9, "right was {}", cuts[0].right);
    }

    #[test]
    fn two_arms_at_the_same_height_leave_the_notch_between_them_usable() {
        // A "⊔" standing on its base: at mid height only the two uprights are
        // there. Collapsing them into one span would deny a gap the author
        // drew on purpose.
        let o = contour(
            Rect::new(0.0, 0.0, 100.0, 100.0),
            vec![
                [0.0, 0.0],
                [0.3, 0.0],
                [0.3, 0.7],
                [0.7, 0.7],
                [0.7, 0.0],
                [1.0, 0.0],
                [1.0, 1.0],
                [0.0, 1.0],
            ],
            0.0,
            0.0,
        );
        let cuts = o.blocked(20.0, 32.0);
        assert_eq!(cuts.len(), 2, "got {cuts:?}");

        let slots = carve(Interval::new(0.0, 100.0), &cuts, 10.0);
        assert_eq!(
            slots,
            vec![Interval::new(30.0, 70.0)],
            "the notch is the only place text fits, and it has to survive"
        );
    }

    #[test]
    fn a_band_past_the_ring_blocks_nothing() {
        let o = contour(
            Rect::new(0.0, 0.0, 100.0, 100.0),
            vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]],
            0.0,
            0.0,
        );
        assert!(o.blocked(200.0, 212.0).is_empty());
    }

    #[test]
    fn rotating_a_square_ring_by_ninety_degrees_swaps_its_extents() {
        let rect = Rect::new(0.0, 0.0, 100.0, 40.0);
        let ring = vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]];
        let upright = contour(rect, ring.clone(), 0.0, 0.0);
        let turned = contour(rect, ring, 90.0, 0.0);

        assert_eq!(upright.bounds.w, 100.0);
        assert!(
            (turned.bounds.w - 40.0).abs() < 1e-9,
            "width after turning was {}",
            turned.bounds.w
        );
        assert!(
            (turned.bounds.h - 100.0).abs() < 1e-9,
            "height after turning was {}",
            turned.bounds.h
        );
    }

    #[test]
    fn padding_widens_the_interval_not_the_ring() {
        let rect = Rect::new(100.0, 0.0, 100.0, 100.0);
        let ring = vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]];
        let bare = contour(rect, ring.clone(), 0.0, 0.0);
        let padded = contour(rect, ring, 0.0, 7.0);

        let a = one(bare.blocked(40.0, 52.0));
        let b = one(padded.blocked(40.0, 52.0));
        assert!((b.left - (a.left - 7.0)).abs() < 1e-9);
        assert!((b.right - (a.right + 7.0)).abs() < 1e-9);
    }

    // ── build ──────────────────────────────────────────────────────────────

    #[test]
    fn a_degenerate_ring_falls_back_to_the_box() {
        let o = contour(
            Rect::new(0.0, 0.0, 100.0, 100.0),
            vec![[0.0, 0.0], [1.0, 1.0]],
            0.0,
            0.0,
        );
        assert!(
            matches!(o.shape, ObstacleShape::Box(_)),
            "two points cannot enclose anything; the wrap still has to happen"
        );
    }

    #[test]
    fn an_empty_rect_is_not_an_obstacle() {
        let none = Obstacle::build(
            "o",
            Rect::new(10.0, 10.0, 0.0, 50.0),
            0.0,
            &Wrap::default(),
        );
        assert_eq!(none, None);
    }

    // ── line space ─────────────────────────────────────────────────────────

    #[test]
    fn a_column_without_obstacles_offers_itself_whole() {
        let space = WholeColumn { width: 300.0 };
        assert_eq!(slots_of(&space, 0.0, 12.0), vec![Interval::new(0.0, 300.0)]);
        assert_eq!(
            slots_of(&space, 900.0, 912.0),
            vec![Interval::new(0.0, 300.0)],
            "a banda não importa quando nada bloqueia"
        );
    }

    #[test]
    fn obstacles_are_reported_in_the_columns_own_coordinates() {
        // Column starts at x=100 on the page; the obstacle covers 150..200.
        let obstacles = vec![boxed(Rect::new(150.0, 0.0, 50.0, 100.0), 0.0)];
        let space = ColumnSpace {
            obstacles: &obstacles,
            column: Rect::new(100.0, 0.0, 300.0, 500.0),
            origin_y: 0.0,
            min_slot: 10.0,
        };

        assert_eq!(
            slots_of(&space, 20.0, 32.0),
            vec![Interval::new(0.0, 50.0), Interval::new(100.0, 300.0)],
            "x=150 na página é x=50 na coluna"
        );
    }

    #[test]
    fn the_paragraphs_origin_shifts_which_band_is_asked_about() {
        let obstacles = vec![boxed(Rect::new(0.0, 400.0, 50.0, 100.0), 0.0)];
        let space = ColumnSpace {
            obstacles: &obstacles,
            column: Rect::new(0.0, 0.0, 300.0, 800.0),
            origin_y: 380.0,
            min_slot: 10.0,
        };

        assert_eq!(
            slots_of(&space, 0.0, 12.0),
            vec![Interval::new(0.0, 300.0)],
            "local 0 é página 380, acima do obstáculo"
        );
        assert_eq!(
            slots_of(&space, 40.0, 52.0),
            vec![Interval::new(50.0, 300.0)],
            "local 40 é página 420, dentro dele"
        );
    }

    #[test]
    fn a_band_the_obstacle_swallows_whole_offers_nothing() {
        let obstacles = vec![boxed(Rect::new(0.0, 0.0, 300.0, 100.0), 0.0)];
        let space = ColumnSpace {
            obstacles: &obstacles,
            column: Rect::new(0.0, 0.0, 300.0, 500.0),
            origin_y: 0.0,
            min_slot: 10.0,
        };
        assert!(slots_of(&space, 20.0, 32.0).is_empty());
    }

    // ── collect ────────────────────────────────────────────────────────────

    fn frames(json: &str) -> Vec<Frame> {
        serde_json::from_str(json).expect("frames válidos")
    }

    /// Collect from one list of page frames, discarding diagnostics.
    fn gather(json: &str) -> Vec<Obstacle> {
        let f = frames(json);
        let mut ignored = Vec::new();
        collect(&[&f], 0, &mut ignored)
    }

    #[test]
    fn an_image_without_wrap_is_not_an_obstacle() {
        let got = gather(r#"[{"type":"image","rect":[0,0,50,50],"src":"a.png"}]"#);
        assert!(got.is_empty());
    }

    #[test]
    fn text_and_shape_frames_never_block_text() {
        let got = gather(
            r#"[
                {"type":"text","rect":[0,0,50,50],"blocks":["oi"]},
                {"type":"shape","rect":[0,0,50,50],"shape":"rect"}
            ]"#,
        );
        assert!(
            got.is_empty(),
            "só imagem carrega wrap hoje; um frame de texto que crescesse seria circular"
        );
    }

    #[test]
    fn an_invisible_image_blocks_nothing() {
        let got = gather(
            r#"[{
                "type":"image","rect":[0,0,50,50],"src":"a.png","visible":false,
                "wrap":{"mode":{"kind":"box"}}
            }]"#,
        );
        assert!(got.is_empty());
    }

    #[test]
    fn an_image_inside_a_group_lands_at_its_absolute_position() {
        let got = gather(
            r#"[{
                "type":"group","rect":[100,200,300,300],
                "children":[{
                    "type":"image","rect":[10,20,50,60],"src":"a.png",
                    "wrap":{"mode":{"kind":"box"}}
                }]
            }]"#,
        );
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].bounds, Rect::new(110.0, 220.0, 50.0, 60.0));
    }

    #[test]
    fn nesting_stacks_the_translations() {
        let got = gather(
            r#"[{
                "type":"group","rect":[100,100,400,400],
                "children":[{
                    "type":"group","rect":[10,10,200,200],
                    "children":[{
                        "type":"image","rect":[5,5,20,20],"src":"a.png",
                        "wrap":{"mode":{"kind":"box"}}
                    }]
                }]
            }]"#,
        );
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].bounds, Rect::new(115.0, 115.0, 20.0, 20.0));
    }

    #[test]
    fn a_wrap_inside_a_rotated_group_says_so_instead_of_lying() {
        let f = frames(
            r#"[{
                "type":"group","rect":[0,0,300,300],"rotation":30,
                "children":[{
                    "type":"image","rect":[10,10,50,50],"src":"a.png","id":"foto",
                    "wrap":{"mode":{"kind":"box"}}
                }]
            }]"#,
        );
        let mut diagnostics = Vec::new();
        let got = collect(&[&f], 3, &mut diagnostics);

        assert_eq!(got.len(), 1, "o obstáculo continua valendo, só desalinhado");
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].code, "wrapInRotatedGroup");
        assert_eq!(diagnostics[0].page, Some(3));
        assert_eq!(diagnostics[0].frame.as_deref(), Some("foto"));
    }

    #[test]
    fn a_frames_own_rotation_needs_no_warning() {
        let f = frames(
            r#"[{
                "type":"image","rect":[0,0,100,40],"src":"a.png","rotation":90,
                "wrap":{"mode":{"kind":"box"}}
            }]"#,
        );
        let mut diagnostics = Vec::new();
        let got = collect(&[&f], 0, &mut diagnostics);

        assert!(diagnostics.is_empty(), "rotação própria é tratada de verdade");
        assert!(
            (got[0].bounds.w - 40.0).abs() < 1e-9,
            "a caixa girada mede {}",
            got[0].bounds.w
        );
    }

    #[test]
    fn master_frames_come_before_the_pages_own() {
        let master = frames(
            r#"[{"type":"image","rect":[0,0,10,10],"src":"m.png","id":"do-mestre",
                "wrap":{"mode":{"kind":"box"}}}]"#,
        );
        let page = frames(
            r#"[{"type":"image","rect":[0,0,10,10],"src":"p.png","id":"da-pagina",
                "wrap":{"mode":{"kind":"box"}}}]"#,
        );
        let mut ignored = Vec::new();
        let got = collect(&[&master, &page], 0, &mut ignored);

        let ids: Vec<&str> = got.iter().map(|o| o.id.as_str()).collect();
        assert_eq!(ids, vec!["do-mestre", "da-pagina"]);
    }

    #[test]
    fn several_obstacles_answer_one_band_together() {
        let a = boxed(Rect::new(0.0, 0.0, 50.0, 100.0), 0.0);
        let b = boxed(Rect::new(200.0, 0.0, 50.0, 100.0), 0.0);
        let c = boxed(Rect::new(0.0, 500.0, 50.0, 50.0), 0.0);

        let cuts = blocked(&[a, b, c], 20.0, 32.0);
        assert_eq!(cuts.len(), 2, "the third one is nowhere near this band");

        let slots = carve(Interval::new(0.0, 300.0), &cuts, 10.0);
        assert_eq!(
            slots,
            vec![Interval::new(50.0, 200.0), Interval::new(250.0, 300.0)]
        );
    }
}
