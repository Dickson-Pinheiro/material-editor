//! Which cell sits where.
//!
//! A table is written as a flat list of cells. Most of them say nothing about
//! position and simply follow the one before; some pin themselves to a column
//! and a row; and any of them may cover more than one slot. Turning that list
//! into a grid is this module, and nothing else here knows about text,
//! widths or drawing.
//!
//! Sizing and drawing follow, in that order, and the engine reaches all three
//! through `Cells` — the one seam where a cell's blocks become text.

use super::grid::{self, Track};
use super::text::Intrinsic;
use crate::display::{DisplayItem, LineItem, RectItem, SourceRef, Stroke};
use crate::color::Color;
use crate::spec::content::{
    Block, Cell, CellAlign, GridAxis, GridLine, RepeatRows, Stripe, TableBlock, TrackSize,
};
use crate::spec::ResolvedStyle;
use crate::units::{Insets, Len, Rect};

/// Where a cell ended up.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Placed {
    /// Index into the table's `cells`.
    pub cell: usize,
    pub x: u32,
    pub y: u32,
    pub colspan: u32,
    pub rowspan: u32,
}

/// The grid, once every cell has a home.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct Grid {
    pub cells: Vec<Placed>,
    pub columns: usize,
    pub rows: usize,
}

/// Something the author should be told about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Issue {
    /// Two cells claim the same slot. The later one is dropped rather than
    /// painted over the earlier: a cell you cannot see is a cell you cannot
    /// fix.
    Overlap { cell: usize, x: u32, y: u32 },
    /// A cell wider than the table has columns.
    TooWide { cell: usize, colspan: u32 },
    /// A row taller than the whole space it was offered, emitted anyway.
    ///
    /// Overflowing is the lesser evil: a row that is never emitted because it
    /// never fits is a document that never finishes.
    RowTooTall { row: usize },
}

/// Cap on how far down the scan will look for a free slot.
///
/// Reached only when spans have left a table pathologically sparse. Without
/// it a malformed document could search forever.
const MAX_ROWS: u32 = 4096;

/// Place every cell.
///
/// Explicit positions are honoured first, then the rest fill the gaps in
/// order, row by row. Doing it the other way round — filling as you go and
/// letting a later explicit cell displace what is already there — makes the
/// result depend on the order cells happen to be written in, which is the
/// kind of thing an author discovers by accident.
pub(crate) fn place(table: &TableBlock, issues: &mut Vec<Issue>) -> Grid {
    let columns = column_count(table);
    if columns == 0 {
        return Grid::default();
    }

    let mut taken: Vec<bool> = Vec::new();
    let mut placed: Vec<Option<Placed>> = vec![None; table.cells.len()];

    let occupy = |taken: &mut Vec<bool>, x: u32, y: u32, w: u32, h: u32| {
        let needed = ((y + h) as usize) * columns;
        if taken.len() < needed {
            taken.resize(needed, false);
        }
        for row in y..y + h {
            for column in x..x + w {
                taken[row as usize * columns + column as usize] = true;
            }
        }
    };

    let free = |taken: &[bool], x: u32, y: u32, w: u32, h: u32| -> bool {
        if (x + w) as usize > columns {
            return false;
        }
        (y..y + h).all(|row| {
            (x..x + w).all(|column| {
                let index = row as usize * columns + column as usize;
                taken.get(index).is_none_or(|used| !used)
            })
        })
    };

    // ── The pinned ones, in the order they were written ─────────────────────
    for (index, cell) in table.cells.iter().enumerate() {
        let (Some(x), Some(y)) = (cell.x, cell.y) else { continue };
        let w = cell.colspan.max(1);
        let h = cell.rowspan.max(1);

        if (x + w) as usize > columns {
            issues.push(Issue::TooWide { cell: index, colspan: w });
            continue;
        }
        if !free(&taken, x, y, w, h) {
            issues.push(Issue::Overlap { cell: index, x, y });
            continue;
        }
        occupy(&mut taken, x, y, w, h);
        placed[index] = Some(Placed { cell: index, x, y, colspan: w, rowspan: h });
    }

    // ── The rest, into whatever is left ─────────────────────────────────────
    let mut cursor = 0u32;
    for (index, cell) in table.cells.iter().enumerate() {
        if placed[index].is_some() || (cell.x.is_some() && cell.y.is_some()) {
            continue;
        }
        let w = cell.colspan.max(1);
        let h = cell.rowspan.max(1);

        if w as usize > columns {
            issues.push(Issue::TooWide { cell: index, colspan: w });
            continue;
        }

        // A cell that pinned only its column waits for that column to come
        // round; one that pinned only its row starts scanning there.
        let wanted_x = cell.x;
        let mut at = match cell.y {
            Some(row) => row * columns as u32,
            None => cursor,
        };

        loop {
            let y = at / columns as u32;
            let x = at % columns as u32;
            if y > MAX_ROWS {
                issues.push(Issue::TooWide { cell: index, colspan: w });
                break;
            }
            let matches_column = wanted_x.is_none_or(|want| want == x);
            if matches_column && free(&taken, x, y, w, h) {
                occupy(&mut taken, x, y, w, h);
                placed[index] = Some(Placed { cell: index, x, y, colspan: w, rowspan: h });
                if cell.y.is_none() && wanted_x.is_none() {
                    cursor = at + w;
                }
                break;
            }
            at += 1;
        }
    }

    let cells: Vec<Placed> = placed.into_iter().flatten().collect();
    let rows = cells
        .iter()
        .map(|p| (p.y + p.rowspan) as usize)
        .max()
        .unwrap_or(0)
        .max(table.rows.len());

    Grid { cells, columns, rows }
}

/// How many columns the table has.
///
/// Declared when `columns` says so. Otherwise inferred from the cells that
/// pinned themselves — and, when none did, one: a bare list of cells with
/// nothing else said is a single column of rows, which is at least a shape
/// the author can see and correct.
fn column_count(table: &TableBlock) -> usize {
    if !table.columns.is_empty() {
        return table.columns.len();
    }
    table
        .cells
        .iter()
        .filter_map(|cell| cell.x.map(|x| (x + cell.colspan.max(1)) as usize))
        .max()
        .unwrap_or(1)
        .max(1)
}

// ─────────────────────────────────────────────────────────────────────────────
// Sizing
// ─────────────────────────────────────────────────────────────────────────────

/// What the table needs from the engine to size and fill its cells.
///
/// Behind a trait because a cell holds blocks, and laying blocks out lives in
/// the engine — while everything here is arithmetic and geometry. It also
/// lets the tests drive that arithmetic with a ruler of their own, so a
/// column calculation can be checked without a font anywhere near it.
pub(crate) trait Cells {
    /// How narrow and how wide this content can be.
    fn intrinsic(&self, blocks: &[Block], style: &ResolvedStyle) -> Intrinsic;
    /// How tall it turns out at a given width.
    fn height(&self, blocks: &[Block], style: &ResolvedStyle, width: f64) -> f64;
    /// Distance from the top of the content to its first baseline.
    ///
    /// `None` when there is no text in it — a cell holding only a rule has no
    /// baseline to align anything with, and pretending otherwise would drag
    /// every other cell in the row to meet a line that is not there.
    fn first_baseline(&self, blocks: &[Block], style: &ResolvedStyle, width: f64) -> Option<f64>;
    /// Lay it out inside `rect`, in page coordinates.
    fn render(
        &self,
        blocks: &[Block],
        style: &ResolvedStyle,
        rect: Rect,
        source: &SourceRef,
    ) -> Vec<DisplayItem>;
}

/// The table's geometry, once resolved.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct Sizes {
    pub columns: Vec<f64>,
    pub rows: Vec<f64>,
    /// Where each row's shared baseline sits, measured from the row's top.
    ///
    /// `0.0` in a row nobody asked to align. Computed here rather than at
    /// emission because it is not only where the content goes — a cell with a
    /// taller first line pushes the shared baseline down, and the row has to
    /// grow to hold what that displaces.
    pub baselines: Vec<f64>,
    /// By how much the columns exceed the room, `0.0` when they fit.
    pub overflow: f64,
}

/// Resolve column widths and row heights.
pub(crate) fn size(
    table: &TableBlock,
    grid_layout: &Grid,
    cells: &dyn Cells,
    style: &ResolvedStyle,
    available: f64,
) -> Sizes {
    if grid_layout.columns == 0 {
        return Sizes::default();
    }

    let column_gap = table.column_gap.get();
    let row_gap = table.row_gap.get();
    let inset = table.inset;

    let wants = wants(table, grid_layout, cells, style);

    // ── Columns ─────────────────────────────────────────────────────────────
    let tracks: Vec<Track> = (0..grid_layout.columns)
        .map(|index| match table.columns.get(index) {
            Some(TrackSize::Fixed(length)) => Track::Fixed(length.get()),
            Some(TrackSize::Relative(share)) => Track::Relative(*share),
            Some(TrackSize::Fraction(share)) => Track::Fraction(*share),
            Some(TrackSize::Auto) | None => Track::Auto,
        })
        .collect();

    let resolved = grid::resolve(&tracks, &wants, available, column_gap);

    // ── Rows ────────────────────────────────────────────────────────────────
    let mut rows = vec![0.0f64; grid_layout.rows];
    let mut baselines = vec![0.0f64; grid_layout.rows];

    // What each cell asks for, measured once: three passes read it, and
    // measuring a cell means laying its blocks out.
    let asks: Vec<Ask> = grid_layout
        .cells
        .iter()
        .map(|placed| {
            let cell = &table.cells[placed.cell];
            let padding = cell.inset.unwrap_or(inset);
            let width = spanned(&resolved.lengths, placed.x, placed.colspan, column_gap);
            let inner = (width - padding.horizontal()).max(1.0);
            let baseline = match cell.vertical_align {
                CellAlign::Baseline => cells
                    .first_baseline(&cell.blocks, style, inner)
                    .map(|b| b + padding.top),
                _ => None,
            };
            Ask {
                tall: cells.height(&cell.blocks, style, inner) + padding.vertical(),
                baseline,
            }
        })
        .collect();

    // Where the shared baseline lands: the lowest any participating cell needs
    // it to be, since a baseline can be pushed down but never pulled up
    // through the text above it.
    for (placed, ask) in grid_layout.cells.iter().zip(&asks) {
        if let Some(baseline) = ask.baseline {
            let row = &mut baselines[placed.y as usize];
            *row = row.max(baseline);
        }
    }

    // What a cell needs of its row, once the shift onto that baseline is paid
    // for. A cell whose own first line sits higher is pushed down, and the row
    // has to hold both the shift and the rest of the cell.
    let needs = |placed: &Placed, ask: &Ask| -> f64 {
        match ask.baseline {
            Some(baseline) => baselines[placed.y as usize] - baseline + ask.tall,
            None => ask.tall,
        }
    };

    for (placed, ask) in grid_layout.cells.iter().zip(&asks) {
        if placed.rowspan == 1 {
            let row = &mut rows[placed.y as usize];
            *row = row.max(needs(placed, ask));
        }
    }

    // A cell crossing rows adds what is missing to the **last** row it
    // crosses. Spreading it would stretch rows above that are already the
    // right height for their own content, and a table where an unrelated row
    // grew is a table nobody can reason about.
    for (placed, ask) in grid_layout.cells.iter().zip(&asks) {
        if placed.rowspan <= 1 {
            continue;
        }
        let tall = needs(placed, ask);

        let first = placed.y as usize;
        let last = (placed.y + placed.rowspan - 1) as usize;
        let bridged = row_gap * (placed.rowspan - 1) as f64;
        let have: f64 = rows[first..=last.min(rows.len().saturating_sub(1))].iter().sum::<f64>()
            + bridged;

        if tall > have && last < rows.len() {
            rows[last] += tall - have;
        }
    }

    // Declared heights win over measured ones.
    for (index, row) in rows.iter_mut().enumerate() {
        match table.rows.get(index) {
            Some(TrackSize::Fixed(length)) => *row = length.get().max(0.0),
            Some(TrackSize::Relative(_)) | Some(TrackSize::Fraction(_)) => {}
            Some(TrackSize::Auto) | None => {}
        }
    }

    Sizes { columns: resolved.lengths, rows, baselines, overflow: resolved.overflow }
}

// ─────────────────────────────────────────────────────────────────────────────
// Emission
// ─────────────────────────────────────────────────────────────────────────────

/// A table, drawn.
#[derive(Debug, Clone, Default)]
pub(crate) struct Layout {
    pub items: Vec<DisplayItem>,
    /// Total height, gaps included.
    pub height: f64,
    /// The resolved geometry. Kept so that a table continuing on the next page
    /// can reuse the same columns instead of resolving them again against a
    /// different set of rows and changing shape halfway down.
    ///
    /// Read by the tests and by T3.1; nothing in the engine asks for it while
    /// a table still has to fit on one page.
    #[allow(dead_code)]
    pub sizes: Sizes,
    #[allow(dead_code)]
    pub grid: Grid,
    /// Anything the author should be told about, turned into diagnostics by
    /// the caller — which is the only place that knows the page and the frame.
    pub issues: Vec<Issue>,
    /// The rows that did not fit, as a table of their own, ready to be flowed
    /// into the next column or page. `None` when the whole table was drawn.
    pub leftover: Option<TableBlock>,
}

/// How much vertical space the table has been given.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Room {
    /// No ceiling — the frame grows, or nobody is counting.
    Unlimited,
    /// Break at the last row boundary that fits. May emit nothing at all,
    /// which tells the caller to try again further down.
    Upto(f64),
    /// The same, except that emitting nothing is not an answer: this is the
    /// top of an empty column, so a row taller than all of it goes out anyway
    /// rather than being carried forward forever.
    AtLeast(f64),
}

impl Room {
    /// The same room, with something already spoken for.
    fn less(self, amount: f64) -> Room {
        match self {
            Room::Unlimited => Room::Unlimited,
            Room::Upto(room) => Room::Upto(room - amount),
            Room::AtLeast(room) => Room::AtLeast(room - amount),
        }
    }
}

/// Slack allowed when asking whether a row still fits, in points.
///
/// Row heights are sums of measured line heights; a stack that should land
/// exactly on the budget can miss it by a float's width, and losing a row to
/// that would be invisible and infuriating.
const FITS: f64 = 0.01;

/// Thickness of a rule that declares none.
///
/// `Len` has no optional form here, so zero is what an author who wrote only
/// `{ axis, at }` gets — and a rule nobody can see is not what they asked for.
/// Same hairline `Block::Rule` falls back to.
const DEFAULT_RULE: f64 = 0.75;

/// Lay the table out at `origin`, whose width is the room it has.
///
/// Paint order is fixed and is the whole of the visual contract: table fill,
/// stripes, cell fills, rules, content. Anything else and a rule disappears
/// under the fill of the row below it.
pub(crate) fn emit(
    table: &TableBlock,
    style: &ResolvedStyle,
    cells: &dyn Cells,
    origin: Rect,
    room: Room,
    source: &SourceRef,
) -> Layout {
    let mut issues = Vec::new();
    let grid_layout = place(table, &mut issues);
    if grid_layout.columns == 0 || grid_layout.rows == 0 {
        return Layout { issues, ..Layout::default() };
    }

    let whole = size(table, &grid_layout, cells, style, origin.w);
    let column_gap = table.column_gap.get();
    let row_gap = table.row_gap.get();

    // ── The rows that repeat ────────────────────────────────────────────────
    //
    // Only a page that continues sees either of these. The page the table
    // begins on already has its header where the author wrote it, and the page
    // it ends on already has its footer — which is why the first page needs no
    // special case at all.
    let head = strip(table, &whole.columns, continuation_head(table, &grid_layout), true);
    let foot = strip(table, &whole.columns, continuation_foot(table, &grid_layout), false);
    let head_rows = head.as_ref().map_or(0, |strip| measured(strip, cells, style, origin.w).1);
    let (foot_height, _) = foot
        .as_ref()
        .map_or((0.0, 0), |strip| measured(strip, cells, style, origin.w));
    let foot_room = if foot_height > 0.0 { foot_height + row_gap } else { 0.0 };

    // ── Where to stop ───────────────────────────────────────────────────────
    //
    // Asked twice, because whether the footer needs room depends on whether
    // the table breaks at all, and whether it breaks depends on the room.
    let mut probe = Vec::new();
    let breaks =
        break_at(&grid_layout, &whole, row_gap, room, head_rows, &mut probe) < whole.rows.len();
    let budget = if breaks { room.less(foot_room) } else { room };

    let cut = break_at(&grid_layout, &whole, row_gap, budget, head_rows, &mut issues);
    if cut == 0 {
        // Nothing useful fits here. The caller is told so by getting everything
        // back, and tries again lower down.
        return Layout { issues, leftover: Some(table.clone()), ..Layout::default() };
    }

    let leftover = (cut < whole.rows.len())
        .then(|| remainder(table, &grid_layout, &whole, cut, head.as_ref(), head_rows));
    let sizes = if leftover.is_some() {
        Sizes {
            columns: whole.columns.clone(),
            rows: whole.rows[..cut].to_vec(),
            baselines: whole.baselines[..cut].to_vec(),
            overflow: whole.overflow,
        }
    } else {
        whole
    };

    // Track edges, precomputed: every rectangle below is two of these.
    let lefts = edges(origin.x, &sizes.columns, column_gap);
    let tops = edges(origin.y, &sizes.rows, row_gap);
    let width = span_of(&lefts, &sizes.columns, 0, sizes.columns.len());
    let height = span_of(&tops, &sizes.rows, 0, sizes.rows.len());

    let mut items = Vec::new();

    // ── Table fill ──────────────────────────────────────────────────────────
    if let Some(fill) = table.fill {
        items.push(fill_rect(Rect::new(origin.x, origin.y, width, height), fill, source));
    }

    // ── Stripes ─────────────────────────────────────────────────────────────
    if let Some(stripe) = &table.stripe
        && let Some(fill) = stripe.fill
        && stripe.every > 0
    {
        for (row, (top, tall)) in tops.iter().zip(&sizes.rows).enumerate() {
            let row = row as u32;
            if row >= stripe.offset && (row - stripe.offset).is_multiple_of(stripe.every) {
                items.push(fill_rect(Rect::new(origin.x, *top, width, *tall), fill, source));
            }
        }
    }

    // ── Cell fills ──────────────────────────────────────────────────────────
    for placed in &grid_layout.cells {
        if placed.y as usize >= cut {
            continue;
        }
        let cell = &table.cells[placed.cell];
        let Some(fill) = cell.fill else { continue };
        items.push(fill_rect(box_of(&lefts, &tops, &sizes, placed), fill, source));
    }

    // ── Rules ───────────────────────────────────────────────────────────────
    for line in &table.lines {
        let thickness = if line.width.get() > 0.0 { line.width.get() } else { DEFAULT_RULE };
        let color = line.color.unwrap_or(style.color);

        // A rule sits on the boundary itself, which with a gap means the
        // middle of the gap — equidistant from both tracks, and identical to
        // the exact boundary once the gap is zero.
        let (along, across, along_gap) = match line.axis {
            GridAxis::Horizontal => (&tops, &sizes.columns, row_gap),
            GridAxis::Vertical => (&lefts, &sizes.rows, column_gap),
        };
        let tracks = match line.axis {
            GridAxis::Horizontal => &sizes.rows,
            GridAxis::Vertical => &sizes.columns,
        };
        let at = line.at as usize;
        if at > tracks.len() {
            continue;
        }
        let position = if at == 0 {
            along[0]
        } else if at == tracks.len() {
            along[at - 1] + tracks[at - 1]
        } else {
            along[at] - along_gap / 2.0
        };

        let from = line.from.unwrap_or(0) as usize;
        let to = line.to.map_or(across.len(), |t| t as usize).min(across.len());
        if from >= to {
            continue;
        }
        let edge = match line.axis {
            GridAxis::Horizontal => &lefts,
            GridAxis::Vertical => &tops,
        };
        let start = edge[from];
        let end = span_of(edge, across, from, to) + start;

        let stroke = Stroke { color, width: thickness, dash: None };
        items.push(DisplayItem::Line(match line.axis {
            GridAxis::Horizontal => LineItem {
                x1: start,
                y1: position,
                x2: end,
                y2: position,
                stroke,
                source: Some(source.clone()),
            },
            GridAxis::Vertical => LineItem {
                x1: position,
                y1: start,
                x2: position,
                y2: end,
                stroke,
                source: Some(source.clone()),
            },
        }));
    }

    // ── Content ─────────────────────────────────────────────────────────────
    for placed in &grid_layout.cells {
        if placed.y as usize >= cut {
            continue;
        }
        let cell = &table.cells[placed.cell];
        if cell.blocks.is_empty() {
            continue;
        }
        let padding = cell.inset.unwrap_or(table.inset);
        let outer = box_of(&lefts, &tops, &sizes, placed);
        let width = (outer.w - padding.horizontal()).max(0.0);
        let height = (outer.h - padding.vertical()).max(0.0);

        // Only the alignments that need it pay for a second measurement.
        let shift = match cell.vertical_align {
            CellAlign::Top => 0.0,
            CellAlign::Middle => {
                (height - cells.height(&cell.blocks, style, width.max(1.0))) / 2.0
            }
            CellAlign::Bottom => height - cells.height(&cell.blocks, style, width.max(1.0)),
            CellAlign::Baseline => cells
                .first_baseline(&cell.blocks, style, width.max(1.0))
                // A cell with nothing to align by stays at the top rather than
                // being pushed to a baseline it does not reach.
                .map_or(0.0, |b| sizes.baselines[placed.y as usize] - padding.top - b),
        };

        let inner = Rect::new(
            outer.x + padding.left,
            outer.y + padding.top + shift.max(0.0),
            width,
            height,
        );
        items.extend(cells.render(&cell.blocks, style, inner, source));
    }

    // ── The continuation footer, under what was drawn ───────────────────────
    let mut height = height;
    if leftover.is_some()
        && let Some(strip) = &foot
    {
        let below = Rect::new(origin.x, origin.y + height + row_gap, origin.w, 0.0);
        let laid = emit(strip, style, cells, below, Room::Unlimited, source);
        if laid.height > 0.0 {
            items.extend(laid.items);
            height += row_gap + laid.height;
        }
    }

    Layout { items, height, sizes, grid: grid_layout, issues, leftover }
}

/// The rows a continuation opens with, when there is a header to repeat.
fn continuation_head(table: &TableBlock, grid_layout: &Grid) -> Vec<Cell> {
    repeated(table.header.as_ref(), table, grid_layout, 0)
}

/// The rows a page that has not finished closes with.
fn continuation_foot(table: &TableBlock, grid_layout: &Grid) -> Vec<Cell> {
    let rows = table.footer.as_ref().map_or(0, |footer| footer.rows as usize);
    let from = grid_layout.rows.saturating_sub(rows);
    repeated(table.footer.as_ref(), table, grid_layout, from)
}

/// The `rows` rows starting at `from`, rebased to their own top — or whatever
/// the author said should stand in for them on a continuation.
fn repeated(
    spec: Option<&RepeatRows>,
    table: &TableBlock,
    grid_layout: &Grid,
    from: usize,
) -> Vec<Cell> {
    let Some(spec) = spec.filter(|spec| spec.repeat && spec.rows > 0) else {
        return Vec::new();
    };
    if let Some(cells) = &spec.continued {
        return cells.clone();
    }
    let until = from + spec.rows as usize;
    grid_layout
        .cells
        .iter()
        // A cell reaching out of the band is not part of it: repeating half a
        // span would draw a cell whose other half is on another page.
        .filter(|placed| {
            (placed.y as usize) >= from && (placed.y + placed.rowspan) as usize <= until
        })
        .map(|placed| Cell {
            x: Some(placed.x),
            y: Some(placed.y - from as u32),
            ..table.cells[placed.cell].clone()
        })
        .collect()
}

/// A band of repeated rows, as a table that can be drawn on its own.
///
/// It carries the resolved columns, so a repeated header lines up with the
/// body under it, and the horizontal rules that belong to the band — a rule
/// declared under the heading has to come back with the heading.
fn strip(
    table: &TableBlock,
    columns: &[f64],
    cells: Vec<Cell>,
    from_top: bool,
) -> Option<TableBlock> {
    if cells.is_empty() {
        return None;
    }
    let rows = cells
        .iter()
        .map(|cell| cell.y.unwrap_or(0) + cell.rowspan.max(1))
        .max()
        .unwrap_or(1);
    let lines = table
        .lines
        .iter()
        .filter(|line| matches!(line.axis, GridAxis::Horizontal) && from_top && line.at <= rows)
        .cloned()
        .collect();

    Some(TableBlock {
        columns: columns.iter().map(|width| TrackSize::Fixed(Len(*width))).collect(),
        rows: Vec::new(),
        cells,
        header: None,
        footer: None,
        // A repeated band stands outside the alternation: shading it as if it
        // were row three of the table would make the body look like it skipped.
        stripe: None,
        lines,
        ..table.clone()
    })
}

/// A strip's height and how many rows it takes.
fn measured(
    strip: &TableBlock,
    cells: &dyn Cells,
    style: &ResolvedStyle,
    width: f64,
) -> (f64, usize) {
    let mut ignored = Vec::new();
    let grid_layout = place(strip, &mut ignored);
    let sizes = size(strip, &grid_layout, cells, style, width);
    (
        stack(&sizes.rows, strip.row_gap.get(), sizes.rows.len()),
        grid_layout.rows,
    )
}

/// How many rows to emit here.
///
/// Only between rows, never inside one — and never across a cell that spans
/// the boundary, because half a cell is not a thing this can draw. Splitting
/// such a cell is T3.3; until then the boundary simply is not offered, which
/// costs a page break in an awkward place and loses nothing.
fn break_at(
    grid_layout: &Grid,
    sizes: &Sizes,
    row_gap: f64,
    room: Room,
    floor: usize,
    issues: &mut Vec<Issue>,
) -> usize {
    let total = sizes.rows.len();
    let ceiling = match room {
        Room::Unlimited => return total,
        Room::Upto(room) | Room::AtLeast(room) => room,
    };

    let crossed = |k: usize| {
        grid_layout
            .cells
            .iter()
            .any(|p| (p.y as usize) < k && (p.y + p.rowspan) as usize > k)
    };

    // `floor` is the rows a continuation would put back at the top. Cutting at
    // or below it produces a page whose continuation is that page again, so
    // those boundaries are not candidates however well they fit.
    let mut best = 0;
    let mut first = 0;
    for k in 1..=total {
        if crossed(k) || k <= floor {
            continue;
        }
        if first == 0 {
            first = k;
        }
        // Heights only grow, so the first k that misses is the last word.
        if stack(&sizes.rows, row_gap, k) <= ceiling + FITS {
            best = k;
        } else {
            break;
        }
    }

    if best > 0 {
        return best;
    }
    match room {
        // Unreachable: the unlimited case returned at the top.
        Room::Unlimited | Room::Upto(_) => 0,
        // A whole column and not one row fits. Overflowing is the lesser evil:
        // content that is never emitted because it never fits is a document
        // that never ends. It goes out at the first *legal* boundary — forcing
        // an illegal one would draw half of a cell and lose the other half.
        Room::AtLeast(_) => {
            let forced = if first == 0 { total } else { first };
            issues.push(Issue::RowTooTall { row: forced.saturating_sub(1) });
            forced
        }
    }
}

/// Height of the first `k` rows, gaps between them included.
fn stack(rows: &[f64], gap: f64, k: usize) -> f64 {
    if k == 0 {
        return 0.0;
    }
    rows[..k].iter().sum::<f64>() + gap * (k - 1) as f64
}

/// The rows past the cut, as a table that can be flowed on its own.
///
/// The columns come back as fixed lengths rather than as whatever they were
/// declared to be. Re-resolving them against a different set of rows would
/// give a table that changes shape halfway down the document, which is worse
/// than any width it might otherwise have found.
fn remainder(
    table: &TableBlock,
    grid_layout: &Grid,
    sizes: &Sizes,
    cut: usize,
    head: Option<&TableBlock>,
    head_rows: usize,
) -> TableBlock {
    let cut32 = cut as u32;
    let shift = head_rows as u32;

    // The repeated header goes in as ordinary rows. Nothing downstream has to
    // know it was repeated, and a continuation that breaks again repeats it
    // once more from the same declaration — the same answer every time.
    let mut cells: Vec<Cell> = head.map(|strip| strip.cells.clone()).unwrap_or_default();
    cells.extend(grid_layout.cells.iter().filter(|placed| placed.y as usize >= cut).map(
        |placed| Cell {
            // Pinned where they already are: the continuation must not be free
            // to arrange them differently from the page they came off.
            x: Some(placed.x),
            y: Some(placed.y - cut32 + shift),
            ..table.cells[placed.cell].clone()
        },
    ));

    let lines = table
        .lines
        .iter()
        .filter_map(|line| match line.axis {
            // The boundary at the cut belongs to both: it closes the part
            // above and opens the part below.
            // A rule belonging to the header comes back with it, at its own
            // boundary; a rule from the body moves down by the rows the
            // header put above it.
            GridAxis::Horizontal if line.at <= shift && head.is_some() => Some(line.clone()),
            GridAxis::Horizontal => (line.at >= cut32).then(|| GridLine {
                at: line.at - cut32 + shift,
                ..line.clone()
            }),
            // `from`/`to` count rows for a vertical rule, so they move too.
            GridAxis::Vertical => {
                let to = line.to.map(|to| to.saturating_sub(cut32) + shift);
                if to == Some(shift) {
                    return None;
                }
                Some(GridLine {
                    from: line.from.map(|from| from.saturating_sub(cut32) + shift),
                    to,
                    ..line.clone()
                })
            }
        })
        .collect();

    // The alternation carries on where it left off rather than restarting: a
    // continuation whose first row is shaded like the first row of the table
    // reads as a new table.
    let stripe = table.stripe.as_ref().map(|stripe| Stripe {
        offset: if stripe.every == 0 {
            stripe.offset
        } else {
            (i64::from(stripe.offset) - cut as i64 + i64::from(shift))
                .rem_euclid(i64::from(stripe.every)) as u32
        },
        ..stripe.clone()
    });

    // Declared row heights move down with the rows they were declared for;
    // the repeated header takes its own height, whatever it needs.
    let mut rows = vec![TrackSize::Auto; head_rows];
    rows.extend(table.rows.get(cut..).map_or_else(Vec::new, <[TrackSize]>::to_vec));

    TableBlock {
        columns: sizes.columns.iter().map(|width| TrackSize::Fixed(Len(*width))).collect(),
        rows,
        cells,
        lines,
        stripe,
        ..table.clone()
    }
}

/// The near edge of every track, in page coordinates.
fn edges(start: f64, lengths: &[f64], gap: f64) -> Vec<f64> {
    let mut out = Vec::with_capacity(lengths.len());
    let mut at = start;
    for length in lengths {
        out.push(at);
        at += length + gap;
    }
    out
}

/// Distance from the near edge of track `from` to the far edge of track `to-1`.
fn span_of(edges: &[f64], lengths: &[f64], from: usize, to: usize) -> f64 {
    if from >= to || to > lengths.len() || from >= edges.len() {
        return 0.0;
    }
    edges[to - 1] + lengths[to - 1] - edges[from]
}

/// The rectangle a placed cell covers, spanned tracks and their gaps included.
fn box_of(lefts: &[f64], tops: &[f64], sizes: &Sizes, placed: &Placed) -> Rect {
    let x = placed.x as usize;
    let y = placed.y as usize;
    Rect::new(
        lefts[x],
        tops[y],
        span_of(lefts, &sizes.columns, x, x + placed.colspan as usize),
        span_of(tops, &sizes.rows, y, y + placed.rowspan as usize),
    )
}

fn fill_rect(rect: Rect, fill: Color, source: &SourceRef) -> DisplayItem {
    DisplayItem::Rect(RectItem {
        rect,
        radius: 0.0,
        fill: Some(fill),
        stroke: None,
        source: Some(source.clone()),
    })
}

/// What one cell asks of its row.
struct Ask {
    /// Full height, padding included.
    tall: f64,
    /// Distance from the cell box's top to its first baseline, when the cell
    /// asked to be aligned by it and has one.
    baseline: Option<f64>,
}

/// What each column needs, before any track declaration is consulted.
fn wants(
    table: &TableBlock,
    grid_layout: &Grid,
    cells: &dyn Cells,
    style: &ResolvedStyle,
) -> Vec<Intrinsic> {
    let column_gap = table.column_gap.get();
    let inset = table.inset;
    let mut wants = vec![Intrinsic::default(); grid_layout.columns];

    for placed in &grid_layout.cells {
        if placed.colspan != 1 {
            continue;
        }
        let cell = &table.cells[placed.cell];
        let want = grown(cells.intrinsic(&cell.blocks, style), cell.inset.unwrap_or(inset));
        let slot = &mut wants[placed.x as usize];
        slot.min = slot.min.max(want.min);
        slot.max = slot.max.max(want.max);
    }

    // A cell that crosses columns widens the ones it crosses, "by
    // approximately the same amount" — the CSS rule. Done after the
    // single-column cells so that a column already wide enough is not
    // widened again for nothing.
    for placed in &grid_layout.cells {
        if placed.colspan <= 1 {
            continue;
        }
        let cell = &table.cells[placed.cell];
        let want = grown(cells.intrinsic(&cell.blocks, style), cell.inset.unwrap_or(inset));

        let span = placed.x as usize..(placed.x + placed.colspan) as usize;
        // The gaps between the columns it crosses are part of the room it has.
        let bridged = column_gap * (placed.colspan - 1) as f64;

        for (field, needed) in [(0, want.min - bridged), (1, want.max - bridged)] {
            let have: f64 = span
                .clone()
                .map(|index| if field == 0 { wants[index].min } else { wants[index].max })
                .sum();
            if needed <= have {
                continue;
            }
            let share = (needed - have) / placed.colspan as f64;
            for index in span.clone() {
                if field == 0 {
                    wants[index].min += share;
                } else {
                    wants[index].max += share;
                }
            }
        }
    }

    for want in &mut wants {
        want.max = want.max.max(want.min);
    }
    wants
}

/// How narrow and how wide a whole table can be, for when one sits inside a
/// cell of another — or inside any column that has to size itself around it.
pub(crate) fn intrinsic(
    table: &TableBlock,
    cells: &dyn Cells,
    style: &ResolvedStyle,
) -> Intrinsic {
    let mut issues = Vec::new();
    let grid_layout = place(table, &mut issues);
    if grid_layout.columns == 0 {
        return Intrinsic::default();
    }
    let wants = wants(table, &grid_layout, cells, style);
    let gaps = table.column_gap.get() * (grid_layout.columns - 1) as f64;
    Intrinsic {
        min: wants.iter().map(|w| w.min).sum::<f64>() + gaps,
        max: wants.iter().map(|w| w.max).sum::<f64>() + gaps,
    }
}

/// An intrinsic with the cell's padding added — the padding is width the
/// content cannot use, so the column has to carry it.
fn grown(want: Intrinsic, padding: Insets) -> Intrinsic {
    let sides = padding.horizontal();
    Intrinsic { min: want.min + sides, max: want.max + sides }
}

/// Total width of the columns a cell crosses, gaps included.
fn spanned(columns: &[f64], x: u32, colspan: u32, gap: f64) -> f64 {
    let from = x as usize;
    let to = (x + colspan) as usize;
    let width: f64 = columns.get(from..to.min(columns.len())).unwrap_or(&[]).iter().sum();
    width + gap * (colspan.saturating_sub(1)) as f64
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::spec::content::{
        Block, Cell, CellAlign, GridLine, RepeatRows, Stripe, TrackSize,
    };
    #[allow(unused_imports)]
    use crate::units::Len;

    fn table(columns: usize, cells: Vec<Cell>) -> TableBlock {
        TableBlock {
            columns: vec![TrackSize::Auto; columns],
            cells,
            ..TableBlock::default()
        }
    }

    fn cell(label: &str) -> Cell {
        Cell { blocks: vec![Block::text(label)], ..Cell::default() }
    }

    fn spanning(label: &str, colspan: u32, rowspan: u32) -> Cell {
        Cell { colspan, rowspan, ..cell(label) }
    }

    fn pinned(label: &str, x: u32, y: u32) -> Cell {
        Cell { x: Some(x), y: Some(y), ..cell(label) }
    }

    /// Where each cell landed, in declaration order.
    fn spots(grid: &Grid) -> Vec<(u32, u32)> {
        let mut out: Vec<(usize, (u32, u32))> =
            grid.cells.iter().map(|p| (p.cell, (p.x, p.y))).collect();
        out.sort_by_key(|(index, _)| *index);
        out.into_iter().map(|(_, spot)| spot).collect()
    }

    #[test]
    fn cells_without_positions_fill_row_by_row() {
        let mut issues = Vec::new();
        let grid = place(
            &table(3, (0..6).map(|i| cell(&format!("c{i}"))).collect()),
            &mut issues,
        );
        assert_eq!(
            spots(&grid),
            vec![(0, 0), (1, 0), (2, 0), (0, 1), (1, 1), (2, 1)],
        );
        assert_eq!(grid.rows, 2);
        assert!(issues.is_empty());
    }

    #[test]
    fn a_wide_cell_pushes_the_ones_after_it() {
        let mut issues = Vec::new();
        let grid = place(
            &table(3, vec![spanning("largo", 2, 1), cell("b"), cell("c")]),
            &mut issues,
        );
        assert_eq!(
            spots(&grid),
            vec![(0, 0), (2, 0), (0, 1)],
            "o largo toma duas, o seguinte fica com a terceira, o outro desce",
        );
        assert!(issues.is_empty());
    }

    #[test]
    fn a_tall_cell_keeps_its_column_busy_on_the_next_row() {
        let mut issues = Vec::new();
        let grid = place(
            &table(2, vec![spanning("alto", 1, 2), cell("b"), cell("c")]),
            &mut issues,
        );
        assert_eq!(
            spots(&grid),
            vec![(0, 0), (1, 0), (1, 1)],
            "a segunda linha começa na coluna 1, porque a 0 está ocupada",
        );
        assert_eq!(grid.rows, 2);
    }

    #[test]
    fn a_pinned_cell_does_not_displace_the_automatic_ones_before_it() {
        // The pinned cell claims (0,0) even though it is written last; the
        // automatic ones flow around what is already taken.
        let mut issues = Vec::new();
        let grid = place(
            &table(2, vec![cell("a"), cell("b"), pinned("fixo", 0, 0)]),
            &mut issues,
        );
        assert_eq!(spots(&grid), vec![(1, 0), (0, 1), (0, 0)]);
        assert!(issues.is_empty(), "não há sobreposição: {issues:?}");
    }

    #[test]
    fn two_cells_claiming_the_same_slot_is_reported_not_painted_over() {
        let mut issues = Vec::new();
        let grid = place(
            &table(2, vec![pinned("primeiro", 0, 0), pinned("segundo", 0, 0)]),
            &mut issues,
        );
        assert_eq!(grid.cells.len(), 1, "só o primeiro fica");
        assert_eq!(issues, vec![Issue::Overlap { cell: 1, x: 0, y: 0 }]);
    }

    #[test]
    fn a_cell_wider_than_the_table_is_reported_not_clipped() {
        let mut issues = Vec::new();
        let grid = place(&table(2, vec![spanning("largo", 5, 1)]), &mut issues);
        assert!(grid.cells.is_empty());
        assert_eq!(issues, vec![Issue::TooWide { cell: 0, colspan: 5 }]);
    }

    #[test]
    fn a_cell_that_pins_only_its_row_starts_scanning_there() {
        let mut issues = Vec::new();
        let cells = vec![
            cell("a"),
            Cell { y: Some(2), ..cell("na terceira linha") },
            cell("c"),
        ];
        let grid = place(&table(2, cells), &mut issues);
        let at = spots(&grid);
        assert_eq!(at[0], (0, 0));
        assert_eq!(at[1], (0, 2), "salta para a linha pedida");
        assert_eq!(at[2], (1, 0), "e o seguinte continua de onde o cursor estava");
    }

    #[test]
    fn a_column_left_empty_by_spans_still_counts() {
        let mut issues = Vec::new();
        let grid = place(&table(4, vec![spanning("tudo", 4, 1)]), &mut issues);
        assert_eq!(grid.columns, 4, "as colunas são as declaradas, não as ocupadas");
        assert_eq!(grid.rows, 1);
    }

    #[test]
    fn without_declared_columns_a_list_of_cells_is_one_column() {
        let mut issues = Vec::new();
        let bare = TableBlock {
            cells: vec![cell("a"), cell("b"), cell("c")],
            ..TableBlock::default()
        };
        let grid = place(&bare, &mut issues);
        assert_eq!(grid.columns, 1);
        assert_eq!(spots(&grid), vec![(0, 0), (0, 1), (0, 2)]);
    }

    #[test]
    fn an_empty_table_is_an_empty_grid_not_a_crash() {
        let mut issues = Vec::new();
        let grid = place(&TableBlock::default(), &mut issues);
        assert_eq!(grid.rows, 0);
        assert!(grid.cells.is_empty());
        assert!(issues.is_empty());
    }

    // ── Sizing ─────────────────────────────────────────────────────────────

    /// A ruler with no font in it: every character is 10 wide, every line 12
    /// tall, and a line holds `width / 10` characters. Deterministic, and it
    /// makes the arithmetic below readable — 3 characters is 30, not 17.43.
    struct Ruler;

    impl Ruler {
        fn text_of(blocks: &[Block]) -> String {
            blocks
                .iter()
                .filter_map(|block| block.as_paragraph())
                .flat_map(|para| para.content.iter())
                .filter_map(|inline| match inline {
                    crate::spec::Inline::Text(run) => Some(run.text.clone()),
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join(" ")
        }
    }

    impl Cells for Ruler {
        fn intrinsic(&self, blocks: &[Block], _style: &ResolvedStyle) -> Intrinsic {
            let text = Self::text_of(blocks);
            let longest = text.split_whitespace().map(str::len).max().unwrap_or(0);
            Intrinsic { min: longest as f64 * 10.0, max: text.len() as f64 * 10.0 }
        }

        fn height(&self, blocks: &[Block], _style: &ResolvedStyle, width: f64) -> f64 {
            let text = Self::text_of(blocks);
            if text.is_empty() {
                return 0.0;
            }
            let per_line = (width / 10.0).floor().max(1.0);
            let lines = (text.len() as f64 / per_line).ceil();
            lines * 12.0
        }

        /// Eight points into the first line — a stand-in for an ascent, and
        /// constant so a baseline test measures the alignment and not the font.
        fn first_baseline(
            &self,
            blocks: &[Block],
            _style: &ResolvedStyle,
            _width: f64,
        ) -> Option<f64> {
            (!Self::text_of(blocks).is_empty()).then_some(8.0)
        }

        /// One rectangle marking where the content was told to go. Enough to
        /// check the geometry of a cell without dragging a font in; the real
        /// glyphs are what `tests/tabela.golden` is for.
        fn render(
            &self,
            blocks: &[Block],
            _style: &ResolvedStyle,
            rect: Rect,
            _source: &SourceRef,
        ) -> Vec<DisplayItem> {
            if blocks.is_empty() {
                return Vec::new();
            }
            vec![DisplayItem::Rect(RectItem {
                rect,
                radius: 0.0,
                fill: None,
                stroke: None,
                source: None,
            })]
        }
    }

    fn sized(table: &TableBlock, available: f64) -> Sizes {
        let mut issues = Vec::new();
        let grid = place(table, &mut issues);
        assert!(issues.is_empty(), "grelha com problemas: {issues:?}");
        size(table, &grid, &Ruler, &ResolvedStyle::default(), available)
    }

    // ── Breaking ───────────────────────────────────────────────────────────

    /// Every row of the table, in order, read back out of a `TableBlock`.
    fn labels(table: &TableBlock) -> Vec<String> {
        let mut issues = Vec::new();
        let grid = place(table, &mut issues);
        let mut out: Vec<(u32, u32, String)> = grid
            .cells
            .iter()
            .map(|placed| (placed.y, placed.x, Ruler::text_of(&table.cells[placed.cell].blocks)))
            .collect();
        out.sort_by_key(|(y, x, _)| (*y, *x));
        out.into_iter().map(|(_, _, text)| text).collect()
    }

    #[test]
    fn a_table_that_fits_leaves_nothing_behind() {
        let out = laid(&rows_of(3), Rect::new(0.0, 0.0, 60.0, 0.0), Room::Upto(100.0));
        assert!(out.leftover.is_none());
        assert_eq!(out.height, 36.0);
    }

    #[test]
    fn it_stops_at_the_last_row_boundary_that_fits() {
        // Rows of twelve, and room for three and a half.
        let out = laid(&rows_of(10), Rect::new(0.0, 0.0, 60.0, 0.0), Room::Upto(42.0));
        assert_eq!(out.height, 36.0, "três linhas, não três e meia");
        let rest = out.leftover.expect("sobra");
        assert_eq!(labels(&rest).len(), 7);
    }

    #[test]
    fn what_it_drew_and_what_it_left_are_exactly_what_it_was_given() {
        let table = rows_of(10);
        let before = labels(&table);
        let out = laid(&table, Rect::new(0.0, 0.0, 60.0, 0.0), Room::Upto(42.0));
        let rest = out.leftover.expect("sobra");

        let mut after: Vec<String> = before[..3].to_vec();
        after.extend(labels(&rest));
        assert_eq!(after, before, "nada se perdeu nem se repetiu");

        // The content being right is not the whole promise: a continuation
        // that opens with a blank row still reads every row in the right
        // order, and is still wrong.
        let mut issues = Vec::new();
        assert_eq!(place(&rest, &mut issues).rows, 7, "e abre na primeira linha que sobrou");
    }

    #[test]
    fn a_boundary_a_cell_straddles_is_not_a_boundary() {
        // A cell covering rows 2 and 3 makes the break after row 2 illegal,
        // so the cut falls back to after row 1 even though row 2 would fit.
        let table = TableBlock {
            columns: vec![TrackSize::Fixed(Len(60.0)), TrackSize::Fixed(Len(60.0))],
            cells: vec![
                cell("a0"),
                cell("b0"),
                cell("a1"),
                cell("b1"),
                Cell { x: Some(0), y: Some(2), rowspan: 2, ..cell("atravessa") },
                Cell { x: Some(1), y: Some(2), ..cell("b2") },
                Cell { x: Some(1), y: Some(3), ..cell("b3") },
            ],
            ..TableBlock::default()
        };
        let out = laid(&table, Rect::new(0.0, 0.0, 240.0, 0.0), Room::Upto(40.0));
        assert_eq!(out.height, 24.0, "corta antes da célula que atravessa: {:?}", out.sizes.rows);
        // Three cells, not five slots: the crossing cell is one cell.
        assert_eq!(labels(&out.leftover.expect("sobra")).len(), 3);
    }

    #[test]
    fn nothing_fitting_hands_the_whole_table_back() {
        let out = laid(&rows_of(5), Rect::new(0.0, 0.0, 60.0, 0.0), Room::Upto(5.0));
        assert!(out.items.is_empty(), "não desenha meia linha");
        assert_eq!(out.height, 0.0);
        assert_eq!(labels(&out.leftover.expect("sobra")).len(), 5, "devolve tudo");
    }

    #[test]
    fn at_the_top_of_a_column_a_row_too_tall_goes_out_anyway() {
        let out = laid(&rows_of(5), Rect::new(0.0, 0.0, 60.0, 0.0), Room::AtLeast(5.0));
        assert_eq!(out.height, 12.0, "uma linha, transbordando");
        assert_eq!(labels(&out.leftover.expect("sobra")).len(), 4);
        assert!(
            out.issues.contains(&Issue::RowTooTall { row: 0 }),
            "e diz-se que transbordou: {:?}",
            out.issues,
        );
    }

    #[test]
    fn the_continuation_keeps_the_widths_the_first_page_resolved() {
        let table = TableBlock {
            columns: vec![TrackSize::Auto, TrackSize::Fraction(1.0)],
            cells: vec![
                cell("curto"),
                cell("outro"),
                cell("consideravelmente mais longo"),
                cell("x"),
            ],
            ..TableBlock::default()
        };
        let out = laid(&table, Rect::new(0.0, 0.0, 400.0, 0.0), Room::Upto(12.0));
        let rest = out.leftover.expect("sobra");
        assert_eq!(
            rest.columns,
            out.sizes.columns.iter().map(|w| TrackSize::Fixed(Len(*w))).collect::<Vec<_>>(),
            "a continuação não volta a negociar a largura",
        );

        // And drawing it gives back the same columns, not new ones.
        let second = laid(&rest, Rect::new(0.0, 0.0, 400.0, 0.0), Room::Unlimited);
        assert_eq!(second.sizes.columns, out.sizes.columns);
    }

    #[test]
    fn the_stripe_carries_on_instead_of_starting_over() {
        let table = TableBlock {
            stripe: Some(Stripe { every: 2, offset: 1, fill: Some(Color::rgb(0.5, 0.5, 0.5)) }),
            ..rows_of(8)
        };
        // Three rows out: rows 0, 1, 2, of which 1 was striped.
        let out = laid(&table, Rect::new(0.0, 0.0, 60.0, 0.0), Room::Upto(36.0));
        let rest = out.leftover.expect("sobra");
        // Rows 3, 5, 7 are next, which is the continuation's rows 0, 2, 4.
        assert_eq!(rest.stripe.expect("zebra").offset, 0);
    }

    #[test]
    fn a_rule_at_the_cut_closes_one_part_and_opens_the_other() {
        let table = TableBlock {
            lines: vec![
                GridLine { axis: GridAxis::Horizontal, at: 0, width: Len(1.0), ..GridLine::default() },
                GridLine { axis: GridAxis::Horizontal, at: 3, width: Len(1.0), ..GridLine::default() },
                GridLine { axis: GridAxis::Horizontal, at: 6, width: Len(1.0), ..GridLine::default() },
            ],
            ..rows_of(6)
        };
        let out = laid(&table, Rect::new(0.0, 0.0, 60.0, 0.0), Room::Upto(36.0));
        let drawn_at: Vec<f64> = lines(&out.items).iter().map(|l| l.y1).collect();
        assert_eq!(drawn_at, vec![0.0, 36.0], "a de baixo fecha a parte emitida");

        let rest = out.leftover.expect("sobra");
        let at: Vec<u32> = rest.lines.iter().map(|line| line.at).collect();
        assert_eq!(at, vec![0, 3], "e reabre a continuação");
    }

    #[test]
    fn a_vertical_rule_is_trimmed_to_the_rows_each_part_actually_has() {
        let table = TableBlock {
            columns: vec![TrackSize::Fixed(Len(30.0)), TrackSize::Fixed(Len(30.0))],
            cells: (0..8).map(|i| cell(&format!("c{i}"))).collect(),
            lines: vec![GridLine {
                axis: GridAxis::Vertical,
                at: 1,
                from: Some(1),
                to: Some(4),
                width: Len(1.0),
                ..GridLine::default()
            }],
            ..TableBlock::default()
        };
        // Two rows out of four.
        let out = laid(&table, Rect::new(0.0, 0.0, 60.0, 0.0), Room::Upto(24.0));
        let rule = lines(&out.items)[0];
        assert_eq!((rule.y1, rule.y2), (12.0, 24.0), "só até onde há linhas");

        let rest = out.leftover.expect("sobra");
        assert_eq!((rest.lines[0].from, rest.lines[0].to), (Some(0), Some(2)));
    }

    #[test]
    fn a_rule_entirely_above_the_cut_does_not_follow_the_continuation() {
        let table = TableBlock {
            columns: vec![TrackSize::Fixed(Len(30.0)), TrackSize::Fixed(Len(30.0))],
            cells: (0..8).map(|i| cell(&format!("c{i}"))).collect(),
            lines: vec![GridLine {
                axis: GridAxis::Vertical,
                at: 1,
                to: Some(2),
                width: Len(1.0),
                ..GridLine::default()
            }],
            ..TableBlock::default()
        };
        let out = laid(&table, Rect::new(0.0, 0.0, 60.0, 0.0), Room::Upto(24.0));
        assert!(out.leftover.expect("sobra").lines.is_empty());
    }

    // ── Repeated bands ─────────────────────────────────────────────────────

    /// Rows of two cells each, so a header is visibly a row and not one cell.
    fn pairs(labels: &[&str]) -> Vec<Cell> {
        labels.iter().map(|label| cell(label)).collect()
    }

    fn book(rows: usize) -> TableBlock {
        let mut labels = vec!["Espécie".to_string(), "Peso".to_string()];
        for row in 0..rows {
            labels.push(format!("e{row}"));
            labels.push(format!("p{row}"));
        }
        TableBlock {
            // Wide enough that every label is one line of twelve, so a row
            // count and a height are the same statement.
            columns: vec![TrackSize::Fixed(Len(120.0)), TrackSize::Fixed(Len(120.0))],
            cells: labels.iter().map(|label| cell(label)).collect(),
            header: Some(RepeatRows::default()),
            ..TableBlock::default()
        }
    }

    #[test]
    fn the_header_comes_back_at_the_top_of_the_continuation() {
        // Six rows of twelve; room for three.
        let out = laid(&book(5), Rect::new(0.0, 0.0, 240.0, 0.0), Room::Upto(36.0));
        let rest = out.leftover.expect("sobra");
        assert_eq!(
            labels(&rest)[..2],
            ["Espécie".to_string(), "Peso".to_string()],
            "a continuação abre pelo cabeçalho: {:?}",
            labels(&rest),
        );
        // Rows 0, 1, 2 went out; rows 3, 4, 5 remain, under a repeated header.
        assert_eq!(labels(&rest).len(), 8);
    }

    #[test]
    fn the_header_keeps_coming_back_page_after_page() {
        let pages = flowed(&book(11), Room::Upto(36.0));
        assert!(pages.len() >= 4, "atravessou várias páginas: {}", pages.len());
        for (number, page) in pages.iter().enumerate() {
            assert_eq!(page[0], "Espécie", "a página {} abre pelo cabeçalho", number + 1);
        }
    }

    #[test]
    fn no_row_is_lost_or_repeated_when_a_header_rides_along() {
        // The header is not data: skipped wherever it appears.
        let body: Vec<String> = flowed(&book(11), Room::Upto(36.0))
            .into_iter()
            .flatten()
            .filter(|text| text.starts_with('e') || text.starts_with('p'))
            .collect();
        let expected: Vec<String> = (0..11)
            .flat_map(|row| [format!("e{row}"), format!("p{row}")])
            .collect();
        assert_eq!(body, expected, "nem uma linha a menos nem a mais");
    }

    #[test]
    fn a_distinct_continuation_header_stands_in_for_the_real_one() {
        let mut table = book(5);
        table.header = Some(RepeatRows {
            rows: 1,
            repeat: true,
            continued: Some(pairs(&["Espécie (cont.)", "Peso"])),
        });
        let out = laid(&table, Rect::new(0.0, 0.0, 240.0, 0.0), Room::Upto(36.0));
        let rest = out.leftover.expect("sobra");
        assert_eq!(labels(&rest)[0], "Espécie (cont.)");

        // And it is the continuation header again on the page after that,
        // never the original.
        let again = laid(&rest, Rect::new(0.0, 0.0, 240.0, 0.0), Room::Upto(36.0));
        assert_eq!(labels(&again.leftover.expect("sobra"))[0], "Espécie (cont.)");
    }

    #[test]
    fn the_first_page_shows_the_header_the_author_wrote_not_the_continuation_one() {
        let mut table = book(5);
        table.header = Some(RepeatRows {
            rows: 1,
            repeat: true,
            continued: Some(pairs(&["Espécie (cont.)", "Peso"])),
        });
        let out = laid(&table, Rect::new(0.0, 0.0, 240.0, 0.0), Room::Upto(36.0));
        // Three rows drawn, the first of them the original header.
        assert_eq!(out.sizes.rows.len(), 3);
        assert_eq!(labels(&table)[0], "Espécie");
        // The original header is drawn, at the top, and the stand-in is not.
        assert_eq!(contents(&out.items)[0].1, 0.0);
    }

    #[test]
    fn repeat_off_means_the_header_is_seen_once_and_not_again() {
        let mut table = book(5);
        table.header = Some(RepeatRows { rows: 1, repeat: false, continued: None });
        let out = laid(&table, Rect::new(0.0, 0.0, 240.0, 0.0), Room::Upto(36.0));
        let rest = out.leftover.expect("sobra");
        assert_eq!(labels(&rest)[0], "e2", "a continuação começa nos dados");
    }

    #[test]
    fn the_continuation_footer_closes_a_page_that_has_not_finished() {
        let mut table = book(5);
        table.footer = Some(RepeatRows {
            rows: 1,
            repeat: true,
            continued: Some(pairs(&["(continua)", ""])),
        });
        let out = laid(&table, Rect::new(0.0, 0.0, 240.0, 0.0), Room::Upto(36.0));
        assert!(out.leftover.is_some());
        // Two rows of body, then the footer under them: still within the room.
        assert!(out.height <= 36.0 + FITS, "coube: {}", out.height);
        assert!(out.height > 24.0, "e o rodapé foi desenhado: {}", out.height);
    }

    #[test]
    fn the_continuation_footer_stays_off_the_last_page() {
        let mut table = book(1);
        table.footer = Some(RepeatRows {
            rows: 1,
            repeat: true,
            continued: Some(pairs(&["(continua)", ""])),
        });
        // Two rows, and room for far more: nothing continues.
        let out = laid(&table, Rect::new(0.0, 0.0, 240.0, 0.0), Room::Upto(500.0));
        assert!(out.leftover.is_none());
        assert_eq!(out.height, 24.0, "sem rodapé de continuação: {}", out.height);
    }

    #[test]
    fn a_rule_under_the_heading_comes_back_with_the_heading() {
        let mut table = book(5);
        table.lines = vec![
            GridLine { axis: GridAxis::Horizontal, at: 1, width: Len(0.5), ..GridLine::default() },
        ];
        let out = laid(&table, Rect::new(0.0, 0.0, 240.0, 0.0), Room::Upto(36.0));
        let rest = out.leftover.expect("sobra");
        assert_eq!(
            rest.lines.iter().map(|line| line.at).collect::<Vec<_>>(),
            vec![1],
            "a régua do cabeçalho reaparece sob ele",
        );
    }

    #[test]
    fn a_header_that_leaves_no_room_for_a_row_overflows_rather_than_looping() {
        let out = laid(&book(5), Rect::new(0.0, 0.0, 240.0, 0.0), Room::AtLeast(14.0));
        // One row would fit, but a continuation opening with the header and
        // nothing else is the same page again.
        assert_eq!(out.sizes.rows.len(), 2, "cabeçalho e ao menos uma linha");
        assert!(out.leftover.is_some());
        assert!(out.issues.iter().any(|i| matches!(i, Issue::RowTooTall { .. })));
    }

    // ── Cells over the break ───────────────────────────────────────────────

    /// Flow a table page by page, returning what each page drew.
    ///
    /// Bounded, and it insists the table gets shorter every round. A test that
    /// trusts the code to make progress and loops until it does cannot fail —
    /// it hangs, which tells nobody anything.
    fn flowed(table: &TableBlock, room: Room) -> Vec<Vec<String>> {
        let mut table = table.clone();
        let mut pages = Vec::new();
        for round in 0..64 {
            let out = laid(&table, Rect::new(0.0, 0.0, 240.0, 0.0), room);
            let all = labels(&table);
            let Some(rest) = out.leftover else {
                pages.push(all);
                return pages;
            };
            // What survived is the tail of what there was — a continuation may
            // also have grown a header on top, so counting cells would be
            // wrong. The longest common suffix says where the page ended
            // without having to know how many rows rode along.
            let kept = labels(&rest);
            let carried = all
                .iter()
                .rev()
                .zip(kept.iter().rev())
                .take_while(|(before, after)| before == after)
                .count();
            let drawn = &all[..all.len() - carried];
            assert!(
                !drawn.is_empty(),
                "ronda {round}: a página não desenhou nada e a tabela ficou igual",
            );
            pages.push(drawn.to_vec());
            table = rest;
        }
        panic!("não terminou em 64 páginas");
    }

    /// A header row, then a cell covering rows 1 to 3, then two loose rows.
    fn straddling() -> TableBlock {
        TableBlock {
            columns: vec![TrackSize::Fixed(Len(120.0)), TrackSize::Fixed(Len(120.0))],
            cells: vec![
                Cell { x: Some(0), y: Some(0), ..cell("Espécie") },
                Cell { x: Some(1), y: Some(0), ..cell("Peso") },
                Cell { x: Some(0), y: Some(1), rowspan: 3, ..cell("atravessa") },
                Cell { x: Some(1), y: Some(1), ..cell("p1") },
                Cell { x: Some(1), y: Some(2), ..cell("p2") },
                Cell { x: Some(1), y: Some(3), ..cell("p3") },
                Cell { x: Some(0), y: Some(4), ..cell("e4") },
                Cell { x: Some(1), y: Some(4), ..cell("p4") },
                Cell { x: Some(0), y: Some(5), ..cell("e5") },
                Cell { x: Some(1), y: Some(5), ..cell("p5") },
            ],
            header: Some(RepeatRows::default()),
            ..TableBlock::default()
        }
    }

    #[test]
    fn a_span_that_falls_over_the_break_goes_down_whole() {
        // Room for four rows; the span covers 1 to 3, so the only boundaries
        // are after row 1 and after row 4. Four rows fit, so it is four.
        let table = straddling();
        let out = laid(&table, Rect::new(0.0, 0.0, 240.0, 0.0), Room::Upto(52.0));
        assert_eq!(out.sizes.rows.len(), 4, "corta depois da célula, não dentro dela");
        let rest = out.leftover.expect("sobra");
        assert!(
            !labels(&rest).contains(&"atravessa".to_string()),
            "e a célula não vai também para a continuação: {:?}",
            labels(&rest),
        );
    }

    #[test]
    fn a_page_with_room_only_for_the_header_draws_nothing_at_all() {
        // The only boundary below the header is after the span, and that does
        // not fit. A page holding a heading and no rows is not worth a page.
        let out = laid(&straddling(), Rect::new(0.0, 0.0, 240.0, 0.0), Room::Upto(30.0));
        assert!(out.items.is_empty());
        assert_eq!(out.height, 0.0);
        assert_eq!(labels(&out.leftover.expect("sobra")).len(), 10, "devolve tudo");
    }

    #[test]
    fn a_span_the_page_cannot_reach_descends_entire() {
        // At the top of a column the span goes out whole, overflowing, with
        // the header above it and the loose rows after.
        let table = straddling();
        let out = laid(&table, Rect::new(0.0, 0.0, 240.0, 0.0), Room::AtLeast(30.0));
        assert_eq!(out.sizes.rows.len(), 4, "cabeçalho e as três da célula");

        let rest = out.leftover.expect("sobra");
        let names = labels(&rest);
        assert!(!names.contains(&"atravessa".to_string()), "não ficou para trás");
        assert_eq!(names, vec!["Espécie", "Peso", "e4", "p4", "e5", "p5"]);
    }

    #[test]
    fn the_span_arrives_whole_when_it_is_the_continuation_that_carries_it() {
        // The acceptance case: a `rowspan: 3` falling over the break appears
        // once, on the continuation, with its three rows together. No header
        // here, so the boundary after row 0 is available and the span is what
        // the next page opens with.
        let table = TableBlock { header: None, ..straddling() };
        let out = laid(&table, Rect::new(0.0, 0.0, 240.0, 0.0), Room::Upto(12.0));
        assert_eq!(out.sizes.rows.len(), 1);
        let rest = out.leftover.expect("sobra");

        let mut issues = Vec::new();
        let grid = place(&rest, &mut issues);
        let span = grid
            .cells
            .iter()
            .find(|placed| Ruler::text_of(&rest.cells[placed.cell].blocks) == "atravessa")
            .expect("a célula que atravessa");
        assert_eq!(span.rowspan, 3, "com as três linhas juntas");
        assert_eq!(span.y, 0, "à cabeça da continuação");
        assert_eq!(
            labels(&rest).iter().filter(|t| *t == "atravessa").count(),
            1,
            "uma vez só",
        );
    }

    #[test]
    fn nothing_is_lost_when_a_span_sits_right_under_the_header() {
        let seen: Vec<String> = flowed(&straddling(), Room::AtLeast(30.0))
            .into_iter()
            .flatten()
            .filter(|text| text != "Espécie" && text != "Peso")
            .collect();
        assert_eq!(
            seen,
            vec!["atravessa", "p1", "p2", "p3", "e4", "p4", "e5", "p5"],
            "nem uma célula perdida nem repetida",
        );
    }

    #[test]
    fn a_span_taller_than_the_whole_page_overflows_rather_than_looping() {
        let table = TableBlock {
            columns: vec![TrackSize::Fixed(Len(120.0))],
            cells: vec![
                // Four lines at twelve characters a line: forty-eight tall.
                Cell {
                    x: Some(0),
                    y: Some(0),
                    rowspan: 4,
                    ..cell("aaaaaaaaaaaa bbbbbbbbbbb ccccccccccc ddddddddddd")
                },
                Cell { x: Some(0), y: Some(4), ..cell("depois") },
            ],
            ..TableBlock::default()
        };
        // Four rows of twelve held together, and room for one.
        let out = laid(&table, Rect::new(0.0, 0.0, 120.0, 0.0), Room::AtLeast(12.0));
        assert_eq!(out.sizes.rows.len(), 4, "sai inteira, transbordando");
        assert!(
            out.issues.iter().any(|issue| matches!(issue, Issue::RowTooTall { .. })),
            "e diz-se: {:?}",
            out.issues,
        );
        assert_eq!(labels(&out.leftover.expect("sobra")), vec!["depois".to_string()]);
    }

    // ── Emission ───────────────────────────────────────────────────────────

    fn drawn(table: &TableBlock, origin: Rect) -> Layout {
        laid(table, origin, Room::Unlimited)
    }

    fn laid(table: &TableBlock, origin: Rect, room: Room) -> Layout {
        emit(table, &ResolvedStyle::default(), &Ruler, origin, room, &SourceRef::default())
    }

    /// `count` rows of one column, each a single line twelve tall.
    fn rows_of(count: usize) -> TableBlock {
        TableBlock {
            columns: vec![TrackSize::Fixed(Len(60.0))],
            cells: (0..count).map(|i| cell(&format!("r{i}"))).collect(),
            ..TableBlock::default()
        }
    }

    /// A one-word label for each item, in paint order.
    fn order(items: &[DisplayItem]) -> Vec<&'static str> {
        items
            .iter()
            .map(|item| match item {
                DisplayItem::Rect(r) if r.fill.is_some() => "fill",
                DisplayItem::Rect(_) => "content",
                DisplayItem::Line(_) => "rule",
                _ => "outro",
            })
            .collect()
    }

    fn rects(items: &[DisplayItem]) -> Vec<Rect> {
        items
            .iter()
            .filter_map(|item| match item {
                DisplayItem::Rect(r) => Some(r.rect),
                _ => None,
            })
            .collect()
    }

    fn lines(items: &[DisplayItem]) -> Vec<&LineItem> {
        items
            .iter()
            .filter_map(|item| match item {
                DisplayItem::Line(l) => Some(l),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn the_paint_order_is_fill_then_stripe_then_cell_then_rule_then_content() {
        let t = TableBlock {
            columns: vec![TrackSize::Fixed(Len(50.0)), TrackSize::Fixed(Len(50.0))],
            cells: vec![
                Cell { fill: Some(Color::rgb(1.0, 0.0, 0.0)), ..cell("a") },
                cell("b"),
                cell("c"),
                cell("d"),
            ],
            fill: Some(Color::rgb(0.9, 0.9, 0.9)),
            stripe: Some(Stripe { every: 2, offset: 1, fill: Some(Color::rgb(0.5, 0.5, 0.5)) }),
            lines: vec![GridLine { axis: GridAxis::Horizontal, at: 1, width: Len(1.0), ..GridLine::default() }],
            ..TableBlock::default()
        };
        let out = drawn(&t, Rect::new(0.0, 0.0, 100.0, 0.0));
        // fill (tabela), fill (zebra), fill (célula), régua, e quatro conteúdos.
        assert_eq!(
            order(&out.items),
            vec!["fill", "fill", "fill", "rule", "content", "content", "content", "content"],
            "uma régua por baixo do fundo da linha seguinte seria uma régua invisível",
        );
    }

    #[test]
    fn the_table_fill_covers_every_track_and_the_gaps_between_them() {
        let t = TableBlock {
            columns: vec![TrackSize::Fixed(Len(40.0)), TrackSize::Fixed(Len(40.0))],
            rows: vec![TrackSize::Fixed(Len(20.0)), TrackSize::Fixed(Len(20.0))],
            column_gap: Len(10.0),
            row_gap: Len(6.0),
            cells: vec![cell("a"), cell("b"), cell("c"), cell("d")],
            fill: Some(Color::rgb(0.9, 0.9, 0.9)),
            ..TableBlock::default()
        };
        let out = drawn(&t, Rect::new(5.0, 7.0, 200.0, 0.0));
        let background = rects(&out.items)[0];
        assert_eq!(background, Rect::new(5.0, 7.0, 90.0, 46.0));
        assert!((out.height - 46.0).abs() < 0.01, "veio {}", out.height);
    }

    #[test]
    fn the_stripe_paints_every_other_row_starting_at_its_offset() {
        let t = TableBlock {
            columns: vec![TrackSize::Fixed(Len(50.0))],
            rows: vec![TrackSize::Fixed(Len(10.0)); 5],
            cells: (0..5).map(|i| cell(&format!("c{i}"))).collect(),
            stripe: Some(Stripe { every: 2, offset: 1, fill: Some(Color::rgb(0.5, 0.5, 0.5)) }),
            ..TableBlock::default()
        };
        let out = drawn(&t, Rect::new(0.0, 0.0, 50.0, 0.0));
        let filled: Vec<f64> = out
            .items
            .iter()
            .filter_map(|item| match item {
                DisplayItem::Rect(r) if r.fill.is_some() => Some(r.rect.y),
                _ => None,
            })
            .collect();
        assert_eq!(filled, vec![10.0, 30.0], "linhas 1 e 3, não 0, 2 e 4");
    }

    #[test]
    fn a_horizontal_rule_lands_on_the_boundary_between_two_rows() {
        let t = TableBlock {
            columns: vec![TrackSize::Fixed(Len(50.0))],
            rows: vec![TrackSize::Fixed(Len(20.0)), TrackSize::Fixed(Len(20.0))],
            cells: vec![cell("a"), cell("b")],
            lines: vec![GridLine { axis: GridAxis::Horizontal, at: 1, width: Len(1.0), ..GridLine::default() }],
            ..TableBlock::default()
        };
        let out = drawn(&t, Rect::new(0.0, 0.0, 50.0, 0.0));
        let rule = lines(&out.items)[0];
        assert_eq!((rule.y1, rule.y2), (20.0, 20.0));
        assert_eq!((rule.x1, rule.x2), (0.0, 50.0), "atravessa a tabela inteira");
    }

    #[test]
    fn a_rule_between_rows_that_are_apart_sits_in_the_middle_of_the_gap() {
        let t = TableBlock {
            columns: vec![TrackSize::Fixed(Len(50.0))],
            rows: vec![TrackSize::Fixed(Len(20.0)), TrackSize::Fixed(Len(20.0))],
            row_gap: Len(8.0),
            cells: vec![cell("a"), cell("b")],
            lines: vec![GridLine { axis: GridAxis::Horizontal, at: 1, width: Len(1.0), ..GridLine::default() }],
            ..TableBlock::default()
        };
        let out = drawn(&t, Rect::new(0.0, 0.0, 50.0, 0.0));
        assert_eq!(lines(&out.items)[0].y1, 24.0, "equidistante das duas linhas");
    }

    #[test]
    fn a_rule_at_the_last_boundary_closes_the_table_instead_of_falling_off_it() {
        let t = TableBlock {
            columns: vec![TrackSize::Fixed(Len(50.0))],
            rows: vec![TrackSize::Fixed(Len(20.0)), TrackSize::Fixed(Len(20.0))],
            cells: vec![cell("a"), cell("b")],
            lines: vec![
                GridLine { axis: GridAxis::Horizontal, at: 0, width: Len(1.0), ..GridLine::default() },
                GridLine { axis: GridAxis::Horizontal, at: 2, width: Len(1.0), ..GridLine::default() },
                GridLine { axis: GridAxis::Horizontal, at: 9, width: Len(1.0), ..GridLine::default() },
            ],
            ..TableBlock::default()
        };
        let out = drawn(&t, Rect::new(0.0, 3.0, 50.0, 0.0));
        let drawn_at: Vec<f64> = lines(&out.items).iter().map(|l| l.y1).collect();
        assert_eq!(drawn_at, vec![3.0, 43.0], "a de fora não se desenha nem estoira");
    }

    #[test]
    fn a_rule_can_be_told_which_tracks_to_run_across() {
        let t = TableBlock {
            columns: vec![TrackSize::Fixed(Len(30.0)); 4],
            rows: vec![TrackSize::Fixed(Len(10.0))],
            cells: (0..4).map(|i| cell(&format!("c{i}"))).collect(),
            lines: vec![GridLine {
                axis: GridAxis::Horizontal,
                at: 1,
                from: Some(1),
                to: Some(3),
                width: Len(1.0),
                ..GridLine::default()
            }],
            ..TableBlock::default()
        };
        let out = drawn(&t, Rect::new(0.0, 0.0, 120.0, 0.0));
        let rule = lines(&out.items)[0];
        assert_eq!((rule.x1, rule.x2), (30.0, 90.0), "só as colunas 1 e 2");
    }

    #[test]
    fn a_vertical_rule_runs_down_a_column_boundary() {
        let t = TableBlock {
            columns: vec![TrackSize::Fixed(Len(30.0)), TrackSize::Fixed(Len(30.0))],
            rows: vec![TrackSize::Fixed(Len(10.0)), TrackSize::Fixed(Len(10.0))],
            cells: vec![cell("a"), cell("b"), cell("c"), cell("d")],
            lines: vec![GridLine { axis: GridAxis::Vertical, at: 1, width: Len(1.0), ..GridLine::default() }],
            ..TableBlock::default()
        };
        let out = drawn(&t, Rect::new(0.0, 0.0, 60.0, 0.0));
        let rule = lines(&out.items)[0];
        assert_eq!((rule.x1, rule.x2), (30.0, 30.0));
        assert_eq!((rule.y1, rule.y2), (0.0, 20.0), "de cima a baixo");
    }

    #[test]
    fn a_rule_that_declares_no_width_is_still_visible() {
        let t = TableBlock {
            columns: vec![TrackSize::Fixed(Len(50.0))],
            rows: vec![TrackSize::Fixed(Len(10.0))],
            cells: vec![cell("a")],
            lines: vec![GridLine { axis: GridAxis::Horizontal, at: 1, ..GridLine::default() }],
            ..TableBlock::default()
        };
        let out = drawn(&t, Rect::new(0.0, 0.0, 50.0, 0.0));
        assert!(lines(&out.items)[0].stroke.width > 0.0, "uma régua de zero não é uma régua");
    }

    #[test]
    fn the_content_of_a_cell_is_placed_inside_its_padding() {
        let t = TableBlock {
            columns: vec![TrackSize::Fixed(Len(50.0))],
            rows: vec![TrackSize::Fixed(Len(30.0))],
            inset: Insets::all(4.0),
            cells: vec![cell("a")],
            ..TableBlock::default()
        };
        let out = drawn(&t, Rect::new(10.0, 20.0, 50.0, 0.0));
        assert_eq!(rects(&out.items)[0], Rect::new(14.0, 24.0, 42.0, 22.0));
    }

    #[test]
    fn a_spanning_cell_is_drawn_across_the_tracks_and_the_gap_between_them() {
        let t = TableBlock {
            columns: vec![TrackSize::Fixed(Len(30.0)), TrackSize::Fixed(Len(30.0))],
            rows: vec![TrackSize::Fixed(Len(10.0)), TrackSize::Fixed(Len(10.0))],
            column_gap: Len(6.0),
            row_gap: Len(4.0),
            cells: vec![spanning("largo", 2, 2)],
            ..TableBlock::default()
        };
        let out = drawn(&t, Rect::new(0.0, 0.0, 66.0, 0.0));
        assert_eq!(rects(&out.items)[0], Rect::new(0.0, 0.0, 66.0, 24.0));
    }

    /// A cell that is `tall` points of content, aligned as asked.
    fn aligned(label: &str, align: CellAlign) -> Cell {
        Cell { vertical_align: align, ..cell(label) }
    }

    /// Top-left corner of each cell's content, in declaration order.
    fn contents(items: &[DisplayItem]) -> Vec<(f64, f64)> {
        items
            .iter()
            .filter_map(|item| match item {
                DisplayItem::Rect(r) if r.fill.is_none() => Some((r.rect.x, r.rect.y)),
                _ => None,
            })
            .collect()
    }

    /// One short cell and one tall one, so the row is taller than the short
    /// cell and the alignment has somewhere to move it.
    fn uneven(align: CellAlign) -> TableBlock {
        TableBlock {
            columns: vec![TrackSize::Fixed(Len(60.0)), TrackSize::Fixed(Len(60.0))],
            cells: vec![
                aligned("curto", align),
                // Six words at ten characters a line: several lines tall.
                cell("aaaaa bbbbb ccccc ddddd eeeee fffff"),
            ],
            ..TableBlock::default()
        }
    }

    #[test]
    fn top_is_where_a_cell_sits_when_it_says_nothing() {
        let out = drawn(&uneven(CellAlign::Top), Rect::new(0.0, 0.0, 120.0, 0.0));
        let placed = contents(&out.items);
        assert_eq!(placed[0].1, 0.0);
        assert_eq!(placed[1].1, 0.0, "as duas encostam ao topo");
    }

    #[test]
    fn middle_puts_the_short_cell_halfway_down_the_row() {
        let table = uneven(CellAlign::Middle);
        let out = drawn(&table, Rect::new(0.0, 0.0, 120.0, 0.0));
        let placed = contents(&out.items);
        let row = out.sizes.rows[0];
        // The short cell is one line of twelve; the row is as tall as the
        // other one. Half the difference, and no more.
        assert!((placed[0].1 - (row - 12.0) / 2.0).abs() < 0.01, "veio {:?}", placed[0]);
        assert!(placed[0].1 > 0.0 && placed[0].1 < row - 12.0, "entre os dois extremos");
    }

    #[test]
    fn bottom_rests_the_short_cell_on_the_floor_of_the_row() {
        let table = uneven(CellAlign::Bottom);
        let out = drawn(&table, Rect::new(0.0, 0.0, 120.0, 0.0));
        let placed = contents(&out.items);
        assert!((placed[0].1 - (out.sizes.rows[0] - 12.0)).abs() < 0.01, "veio {:?}", placed[0]);
    }

    #[test]
    fn alignment_counts_the_padding_as_part_of_the_room_it_cannot_use() {
        let mut table = uneven(CellAlign::Bottom);
        table.inset = Insets::all(5.0);
        let out = drawn(&table, Rect::new(0.0, 0.0, 120.0, 0.0));
        let placed = contents(&out.items);
        let floor = out.sizes.rows[0] - 5.0 - 12.0;
        assert!((placed[0].1 - floor).abs() < 0.01, "assenta acima do padding: {:?}", placed[0]);
    }

    #[test]
    fn baseline_lines_up_the_first_lines_of_the_cells_that_ask_for_it() {
        // Different paddings, so top alignment would put the two first lines
        // at different heights and only the baseline rule can save them.
        let table = TableBlock {
            columns: vec![TrackSize::Fixed(Len(60.0)), TrackSize::Fixed(Len(60.0))],
            cells: vec![
                Cell { inset: Some(Insets::all(2.0)), ..aligned("a", CellAlign::Baseline) },
                Cell { inset: Some(Insets::all(14.0)), ..aligned("b", CellAlign::Baseline) },
            ],
            ..TableBlock::default()
        };
        let out = drawn(&table, Rect::new(0.0, 0.0, 120.0, 0.0));
        let placed = contents(&out.items);
        // The ruler puts a baseline 8 into the content, so the two contents
        // land at the same height however differently they are padded.
        assert!(
            (placed[0].1 - placed[1].1).abs() < 0.01,
            "as duas primeiras linhas assentam juntas: {placed:?}",
        );
        assert!((out.sizes.baselines[0] - 22.0).abs() < 0.01, "14 + 8: {:?}", out.sizes.baselines);
    }

    #[test]
    fn a_baseline_pushed_down_makes_the_row_grow_to_hold_what_it_displaced() {
        // The tall cell is the one that gets pushed: its own content already
        // decides the row, so the shift has to be added on top of it. Written
        // the other way round — a short cell pushed under a tall one — the row
        // is tall enough by accident and the rule cannot be seen to work.
        let table = TableBlock {
            columns: vec![TrackSize::Fixed(Len(60.0)), TrackSize::Fixed(Len(60.0))],
            cells: vec![
                // Three lines of twelve, and a baseline 8 into the first.
                aligned("aaaaa bbbbb ccccc", CellAlign::Baseline),
                // Deep padding puts the shared baseline 20 lower.
                Cell { inset: Some(Insets::all(20.0)), ..aligned("b", CellAlign::Baseline) },
            ],
            ..TableBlock::default()
        };
        let out = drawn(&table, Rect::new(0.0, 0.0, 120.0, 0.0));
        // 28 - 8 of shift, then the 36 the tall cell needs for itself.
        assert!(
            (out.sizes.rows[0] - 56.0).abs() < 0.01,
            "a linha cresce com o que empurrou para baixo: {:?}",
            out.sizes.rows,
        );
        assert_eq!(contents(&out.items)[0].1, 20.0, "e a célula alta desce mesmo");
    }

    #[test]
    fn a_cell_with_no_text_does_not_drag_the_row_to_a_baseline_it_has_not_got() {
        let table = TableBlock {
            columns: vec![TrackSize::Fixed(Len(60.0)), TrackSize::Fixed(Len(60.0))],
            cells: vec![
                aligned("a", CellAlign::Baseline),
                // Padded deeply enough that a baseline invented for it would
                // win the row and drag the cell that has one down with it.
                Cell {
                    vertical_align: CellAlign::Baseline,
                    inset: Some(Insets::all(20.0)),
                    ..Cell::default()
                },
            ],
            ..TableBlock::default()
        };
        let out = drawn(&table, Rect::new(0.0, 0.0, 120.0, 0.0));
        assert!((out.sizes.baselines[0] - 8.0).abs() < 0.01, "veio {:?}", out.sizes.baselines);
        assert_eq!(contents(&out.items)[0].1, 0.0, "e a que tem texto não se mexe");
    }

    #[test]
    fn a_cell_taller_than_its_row_is_not_dragged_up_by_an_alignment() {
        // The row was declared shorter than the content. Middle would compute
        // a negative shift; the content stays where it can still be read.
        let table = TableBlock {
            columns: vec![TrackSize::Fixed(Len(60.0))],
            rows: vec![TrackSize::Fixed(Len(6.0))],
            cells: vec![aligned("aaaaa bbbbb ccccc", CellAlign::Middle)],
            ..TableBlock::default()
        };
        let out = drawn(&table, Rect::new(0.0, 0.0, 60.0, 0.0));
        assert_eq!(contents(&out.items)[0].1, 0.0, "nunca acima do topo da célula");
    }

    #[test]
    fn a_table_with_no_cells_draws_nothing_and_takes_no_room() {
        let out = drawn(&TableBlock::default(), Rect::new(0.0, 0.0, 100.0, 0.0));
        assert!(out.items.is_empty());
        assert_eq!(out.height, 0.0);
    }

    #[test]
    fn an_auto_column_never_narrows_past_its_longest_word() {
        // "incompreensivel" is 15 characters: 150 wide, and the table only
        // has 100 to give.
        let t = TableBlock {
            columns: vec![TrackSize::Auto],
            cells: vec![cell("incompreensivel")],
            ..TableBlock::default()
        };
        let out = sized(&t, 100.0);
        assert_eq!(out.columns, vec![150.0]);
        assert!((out.overflow - 50.0).abs() < 0.01, "e o que falta é dito: {}", out.overflow);
    }

    #[test]
    fn a_spanning_cell_widens_the_columns_it_crosses_equally() {
        // Two narrow cells above, one wide cell across both below.
        let t = TableBlock {
            columns: vec![TrackSize::Auto, TrackSize::Auto],
            cells: vec![
                cell("ab"),
                cell("cd"),
                spanning("abcdefghij", 2, 1),
            ],
            ..TableBlock::default()
        };
        let out = sized(&t, 1000.0);
        assert_eq!(out.columns.len(), 2);
        assert!(
            (out.columns[0] - out.columns[1]).abs() < 0.01,
            "as duas crescem por igual: {:?}",
            out.columns,
        );
        assert!(
            (out.columns.iter().sum::<f64>() - 100.0).abs() < 0.01,
            "e juntas cabem a célula larga: {:?}",
            out.columns,
        );
    }

    #[test]
    fn a_column_already_wide_enough_is_not_widened_again() {
        let t = TableBlock {
            columns: vec![TrackSize::Auto, TrackSize::Auto],
            cells: vec![
                cell("abcdefghijklmnop"),
                cell("cd"),
                spanning("abcde", 2, 1),
            ],
            ..TableBlock::default()
        };
        let out = sized(&t, 1000.0);
        assert!((out.columns[0] - 160.0).abs() < 0.01, "a primeira mantém-se: {:?}", out.columns);
        assert!((out.columns[1] - 20.0).abs() < 0.01, "e a segunda também: {:?}", out.columns);
    }

    #[test]
    fn a_row_is_as_tall_as_its_tallest_cell() {
        let t = TableBlock {
            columns: vec![TrackSize::Fixed(Len(100.0)), TrackSize::Fixed(Len(100.0))],
            cells: vec![
                cell("curto"),
                // Thirty characters at ten per line: three lines.
                cell("aaaaaaaaaa bbbbbbbbb ccccccc"),
            ],
            ..TableBlock::default()
        };
        let out = sized(&t, 200.0);
        assert_eq!(out.rows.len(), 1);
        assert!(out.rows[0] >= 36.0, "três linhas de doze: {:?}", out.rows);
    }

    #[test]
    fn a_tall_cell_grows_the_last_row_it_crosses_not_the_first() {
        let t = TableBlock {
            columns: vec![TrackSize::Fixed(Len(100.0)), TrackSize::Fixed(Len(100.0))],
            cells: vec![
                Cell { rowspan: 2, ..cell("aaaaaaaaaa bbbbbbbbb ccccccc dddddd") },
                cell("a"),
                cell("b"),
            ],
            ..TableBlock::default()
        };
        let out = sized(&t, 200.0);
        assert_eq!(out.rows.len(), 2);
        assert!(
            out.rows[1] > out.rows[0],
            "o que falta vai para a última linha, não distorce a de cima: {:?}",
            out.rows,
        );
    }

    #[test]
    fn the_inset_is_width_the_column_has_to_carry() {
        let sem = TableBlock {
            columns: vec![TrackSize::Auto],
            cells: vec![cell("abc")],
            ..TableBlock::default()
        };
        let com = TableBlock {
            inset: Insets::all(9.0),
            ..sem.clone()
        };
        let a = sized(&sem, 1000.0);
        let b = sized(&com, 1000.0);
        assert!(
            (b.columns[0] - a.columns[0] - 18.0).abs() < 0.01,
            "o padding entra na coluna: {:?} vs {:?}",
            b.columns,
            a.columns,
        );
    }

    #[test]
    fn the_gap_between_spanned_columns_counts_as_room_the_cell_can_use() {
        let sem_gap = TableBlock {
            columns: vec![TrackSize::Auto, TrackSize::Auto],
            cells: vec![spanning("abcdefghij", 2, 1)],
            ..TableBlock::default()
        };
        let com_gap = TableBlock { column_gap: Len(20.0), ..sem_gap.clone() };

        let a = sized(&sem_gap, 1000.0);
        let b = sized(&com_gap, 1000.0);
        assert!(
            b.columns.iter().sum::<f64>() < a.columns.iter().sum::<f64>(),
            "com intervalo as colunas precisam de menos: {:?} vs {:?}",
            b.columns,
            a.columns,
        );
    }

    #[test]
    fn a_declared_row_height_wins_over_the_measured_one() {
        let t = TableBlock {
            columns: vec![TrackSize::Fixed(Len(100.0))],
            rows: vec![TrackSize::Fixed(Len(80.0))],
            cells: vec![cell("curto")],
            ..TableBlock::default()
        };
        let out = sized(&t, 100.0);
        assert_eq!(out.rows, vec![80.0]);
    }

    #[test]
    fn declared_rows_survive_even_with_nothing_in_them() {
        let mut issues = Vec::new();
        let sparse = TableBlock {
            columns: vec![TrackSize::Auto],
            rows: vec![TrackSize::Auto; 5],
            cells: vec![cell("só uma")],
            ..TableBlock::default()
        };
        let grid = place(&sparse, &mut issues);
        assert_eq!(grid.rows, 5, "as linhas declaradas existem mesmo vazias");
    }
}
