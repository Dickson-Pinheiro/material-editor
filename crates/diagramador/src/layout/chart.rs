//! Where a chart's marks go, and what the axes cost to draw.
//!
//! Two things happen here, in this order and for a reason. First the data
//! settle their domains and the marks on each axis — arithmetic that knows
//! nothing about the page. Only then are the labels measured, the gutters
//! taken out of the frame, and the scales given the rectangle that is left.
//!
//! The order is what breaks a circle. How wide the left gutter must be
//! depends on the y labels; the y labels depend on the marks; the marks
//! depend on the domain alone, and never on the geometry. Anything that
//! reached back the other way — a tick count chosen from the plot's width,
//! say — would have to be iterated to a fixed point, and a chart that lays
//! out twice is a chart that can lay out differently on two machines.
//!
//! **The margin is measured from the real text of the real labels**, through
//! the document's own layouter. An estimate by character count is the
//! difference between an axis that fits and one that clips its last digit —
//! and, since the labels go through the same shaping as every other word in
//! the document, an axis has no text of its own to keep in parity.
//!
//! Nothing here draws a mark: bars, lines, points and areas arrive from T4.3
//! onward, into the [`Plotted::plot`] this module hands back.

use crate::display::{DisplayGroup, DisplayItem, LineItem, Stroke};
use crate::spec::ResolvedStyle;
use crate::spec::chart::{
    Axis as AxisSpec, Channel, ChartFrame, FieldKind, Mark, Row, ScaleKind,
};
use crate::units::Rect;

use super::scale::Scale;
use super::ticks;

/// Length of a mark on an axis, in ems of that axis's own type.
///
/// Relative to the type and not absolute, so an axis set in 7pt does not
/// carry the ticks of one set in 12.
const TICK_EM: f64 = 0.4;

/// Space between a mark and its label, and between the labels and the title.
const GAP_EM: f64 = 0.35;

/// Roughly how many points of axis each mark is worth.
///
/// Read against the *frame*, never the plot: the plot's size is what the
/// labels decide, and deciding the labels from it would close the circle this
/// module exists to keep open. The count is a wish in any case — `ticks`
/// returns whatever lands on round numbers near it.
const PT_PER_TICK: f64 = 60.0;

/// Thickness of an axis line, matching a rule block's default.
const AXIS_WIDTH: f64 = 0.75;

/// Share of a band given up to separate one bar from the next.
const BAND_PADDING_INNER: f64 = 0.1;
/// Share of a band held back at each end of a band scale.
const BAND_PADDING_OUTER: f64 = 0.05;
/// Share of a step held back at each end of a point scale.
const POINT_PADDING: f64 = 0.5;

// ─────────────────────────────────────────────────────────────────────────────
// What the engine lends the chart
// ─────────────────────────────────────────────────────────────────────────────

/// One line of text, measured.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub(crate) struct Label {
    pub width: f64,
    /// How far the ink rises above the baseline.
    pub ascent: f64,
    /// How far it drops below.
    pub descent: f64,
}

impl Label {
    /// Top of the ink to the bottom of it.
    pub fn height(&self) -> f64 {
        self.ascent + self.descent
    }
}

/// What a chart needs from the engine to put text beside an axis.
///
/// Behind a trait for the same reason the table's `Cells` is: everything in
/// this module is arithmetic, and arithmetic should be checkable with a ruler
/// rather than a font. The implementation the document uses is the document's
/// own layouter, which is what keeps an axis from having text of its own.
pub(crate) trait Labels {
    /// How much room one line of `text` takes in `style`.
    fn measure(&self, text: &str, style: &ResolvedStyle) -> Label;
    /// Draw one line with its left edge at `x` and its baseline at `y`.
    fn draw(&self, text: &str, style: &ResolvedStyle, x: f64, y: f64) -> Vec<DisplayItem>;
}

// ─────────────────────────────────────────────────────────────────────────────
// Result
// ─────────────────────────────────────────────────────────────────────────────

/// A chart, once its geometry is settled.
pub(crate) struct Plotted {
    // Nothing outside the tests reads these three yet: T4.3 is what draws
    // into the rectangle and maps values onto the scales. They are here now
    // because getting the geometry right is what T4.2 is, and a geometry
    // nobody can read is a geometry nobody can check.
    /// Where the marks are drawn. Everything outside it is axis furniture.
    #[allow(dead_code)]
    pub plot: Rect,
    /// The horizontal scale, already carrying the plot's extent as its range.
    #[allow(dead_code)]
    pub x: Scale,
    /// The vertical scale. Its range runs bottom to top, because value grows
    /// upward while the page grows down.
    #[allow(dead_code)]
    pub y: Scale,
    /// Axis lines, marks, labels and titles, in paint order.
    pub items: Vec<DisplayItem>,
    pub issues: Vec<Issue>,
}

/// Something the author should hear about.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Issue {
    /// A logarithmic axis whose data reach zero or below. The logarithm has no
    /// value there, so the axis falls back to linear and says so — drawing
    /// nothing would leave the author guessing which of the two went wrong.
    LogDomainCrossesZero { axis: &'static str },
}

// ─────────────────────────────────────────────────────────────────────────────
// Stage one: domains and marks, with no page in sight
// ─────────────────────────────────────────────────────────────────────────────

/// An axis before it knows where on the page it lives.
struct Draft {
    domain: Domain,
    /// Each mark and the text that goes beside it.
    ticks: Vec<(Tick, String)>,
    title: Option<String>,
    visible: bool,
}

enum Domain {
    Continuous { low: f64, high: f64, log: Option<f64> },
    /// Categories, each holding an interval. The domain of bars.
    Bands(Vec<String>),
    /// Categories, each holding a position. The domain of lines, which pass
    /// through a category rather than across it.
    Points(Vec<String>),
}

/// Where one mark sits, in the terms its own axis understands.
enum Tick {
    At(f64),
    In(String),
}

impl Domain {
    fn scale(&self, range: (f64, f64)) -> Scale {
        match self {
            Domain::Continuous { low, high, log: Some(base) } => Scale::Log {
                domain: (*low, *high),
                range,
                base: *base,
            },
            Domain::Continuous { low, high, log: None } => Scale::Linear {
                domain: (*low, *high),
                range,
                clamp: false,
            },
            Domain::Bands(categories) => Scale::Band {
                categories: categories.clone(),
                range,
                padding_inner: BAND_PADDING_INNER,
                padding_outer: BAND_PADDING_OUTER,
                align: 0.5,
            },
            Domain::Points(categories) => Scale::Point {
                categories: categories.clone(),
                range,
                padding: POINT_PADDING,
            },
        }
    }

    /// True when the last mark sits exactly on the far end of the range, so a
    /// label centred on it hangs half its width past the plot.
    fn reaches_the_edge(&self) -> bool {
        matches!(self, Domain::Continuous { .. } | Domain::Points(_))
    }
}

/// Roughly how many marks an axis that long should carry.
fn tick_target(length: f64) -> usize {
    if !length.is_finite() || length <= 0.0 {
        return 2;
    }
    (length / PT_PER_TICK).round().clamp(2.0, 10.0) as usize
}

/// Settle one axis: what its domain is, and what it will be marked with.
fn draft(
    channel: &Channel,
    axis: &AxisSpec,
    rows: &[Row],
    mark: Mark,
    want: usize,
    name: &'static str,
    issues: &mut Vec<Issue>,
) -> Draft {
    let spec = channel.scale.clone().unwrap_or_default();

    // The field's kind picks the scale; the author only ever contradicts it.
    // A category on a bar chart wants an interval to stand a bar in, and on a
    // line chart a single position to pass through — which is the whole
    // difference between `Band` and `Point`.
    let kind = spec.kind.unwrap_or(match (channel.kind, mark) {
        (FieldKind::Quantitative, _) => ScaleKind::Linear,
        (FieldKind::Categorical, Mark::Bar | Mark::Area) => ScaleKind::Band,
        (FieldKind::Categorical, Mark::Line | Mark::Point) => ScaleKind::Point,
    });

    let title = axis
        .title
        .clone()
        .or_else(|| channel.title.clone())
        .or_else(|| Some(channel.field.clone()))
        .filter(|text| !text.is_empty());

    let domain = match kind {
        ScaleKind::Band | ScaleKind::Point => {
            let categories = spec
                .categories
                .clone()
                .unwrap_or_else(|| categories_of(rows, &channel.field));
            if kind == ScaleKind::Band {
                Domain::Bands(categories)
            } else {
                Domain::Points(categories)
            }
        }
        ScaleKind::Linear | ScaleKind::Log => {
            let (mut low, mut high) = spec.domain.unwrap_or_else(|| span_of(rows, &channel.field));

            // A bar or an area measures a quantity from a baseline, and a
            // baseline that is not zero misstates every proportion drawn
            // against it. The author can say otherwise; the default will not
            // say it for them.
            let wants_zero = spec
                .zero
                .unwrap_or(matches!(mark, Mark::Bar | Mark::Area));

            let mut log = (kind == ScaleKind::Log).then(|| spec.base.unwrap_or(10.0));

            if log.is_some() && (low <= 0.0 || high <= 0.0) {
                issues.push(Issue::LogDomainCrossesZero { axis: name });
                log = None;
            }

            // Zero has no logarithm, so a log axis never reaches for it.
            if wants_zero && log.is_none() {
                low = low.min(0.0);
                high = high.max(0.0);
            }

            // A domain of no width would divide by zero downstream and would
            // give an axis with one mark on it. Open it by a unit either way.
            if low == high {
                low -= 1.0;
                high += 1.0;
            }

            if spec.nice.unwrap_or(true) && log.is_none() {
                let (nice_low, nice_high) = ticks::nice(low, high, want);
                low = nice_low;
                high = nice_high;
            }

            Domain::Continuous { low, high, log }
        }
    };

    // An axis nobody will see is measured all the same — the scale still has
    // to map values — but it is marked and titled with nothing, which is what
    // makes it cost no room.
    let visible = axis.visible;
    Draft {
        ticks: if visible { marks_of(&domain, axis, want) } else { Vec::new() },
        title: if visible { title } else { None },
        domain,
        visible,
    }
}

/// The marks on an axis, and the text beside each.
fn marks_of(domain: &Domain, axis: &AxisSpec, want: usize) -> Vec<(Tick, String)> {
    let want = axis.ticks.map_or(want, |asked| asked.max(1) as usize);

    match domain {
        Domain::Bands(categories) | Domain::Points(categories) => categories
            .iter()
            .map(|name| (Tick::In(name.clone()), name.clone()))
            .collect(),
        Domain::Continuous { .. } => {
            // Built through the scale so there is one place that decides what
            // a mark is — powers of the base on a log axis, round numbers on
            // a linear one — rather than a second copy of the rule here.
            let values = domain.scale((0.0, 1.0)).ticks(want);
            let labels = label_numbers(&values);
            values.into_iter().zip(labels).map(|(v, l)| (Tick::At(v), l)).collect()
        }
    }
}

/// The categories of a field, in the order they are first met.
///
/// The order the author typed, which is the order they meant: months run
/// January to December because that is how they were written, not because
/// anyone taught the engine about months.
fn categories_of(rows: &[Row], field: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for row in rows {
        if let Some(name) = row.get(field).and_then(|value| value.as_category())
            && !out.contains(&name)
        {
            out.push(name);
        }
    }
    out
}

/// The lowest and highest numbers a field holds.
///
/// Holes are skipped rather than read as zero: a month with no reading should
/// not pull the axis down to a value nobody recorded.
fn span_of(rows: &[Row], field: &str) -> (f64, f64) {
    let mut low = f64::INFINITY;
    let mut high = f64::NEG_INFINITY;
    for row in rows {
        if let Some(number) = row.get(field).and_then(|value| value.as_number())
            && number.is_finite()
        {
            low = low.min(number);
            high = high.max(number);
        }
    }
    if low > high { (0.0, 1.0) } else { (low, high) }
}

/// Label a whole set of marks so they read as one axis.
///
/// The decimals come from the set, not from each number on its own. An axis
/// that reads 0 · 0,5 · 1 · 1,5 · 2 has changed its mind halfway down; one
/// that reads 0,0 · 0,5 · 1,0 · 1,5 · 2,0 is scanned without stumbling.
///
/// The separator is a comma, which is what the documents this engine sets are
/// written in. A point is a locale away, and a locale is worth having the day
/// a document asks for one — not before.
fn label_numbers(values: &[f64]) -> Vec<String> {
    let decimals = values.iter().map(|value| decimals_for(*value)).max().unwrap_or(0);
    values
        .iter()
        .map(|value| {
            // Round first, so a value that lands a hair below zero is written
            // `0` and never `-0`.
            let rounded = if *value == 0.0 { 0.0 } else { *value };
            format!("{rounded:.decimals$}").replace('.', ",")
        })
        .collect()
}

/// Fewest decimals that still write this number without losing it.
fn decimals_for(value: f64) -> usize {
    if !value.is_finite() {
        return 0;
    }
    let scale = value.abs().max(1.0);
    for decimals in 0..=6usize {
        let factor = 10f64.powi(decimals as i32);
        let rounded = (value * factor).round() / factor;
        if (rounded - value).abs() <= 1e-9 * scale {
            return decimals;
        }
    }
    6
}

// ─────────────────────────────────────────────────────────────────────────────
// Stage two: the page
// ─────────────────────────────────────────────────────────────────────────────

/// What the axes take out of the frame before the marks get what is left.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
struct Gutter {
    left: f64,
    bottom: f64,
    top: f64,
    right: f64,
}

/// Lay out a chart inside `frame`.
pub(crate) fn plot(
    chart: &ChartFrame,
    rows: &[Row],
    style: &ResolvedStyle,
    labels: &dyn Labels,
    frame: Rect,
) -> Plotted {
    let mut issues = Vec::new();

    let x = draft(
        &chart.encoding.x,
        &chart.axes.x,
        rows,
        chart.mark,
        tick_target(frame.w),
        "x",
        &mut issues,
    );
    let y = draft(
        &chart.encoding.y,
        &chart.axes.y,
        rows,
        chart.mark,
        tick_target(frame.h),
        "y",
        &mut issues,
    );

    let tick_length = style.font_size * TICK_EM;
    let gap = style.font_size * GAP_EM;

    // ── What the labels cost ────────────────────────────────────────────────
    let x_labels: Vec<Label> =
        x.ticks.iter().map(|(_, text)| labels.measure(text, style)).collect();
    let y_labels: Vec<Label> =
        y.ticks.iter().map(|(_, text)| labels.measure(text, style)).collect();

    let x_title = x.title.as_ref().map(|text| labels.measure(text, style));
    let y_title = y.title.as_ref().map(|text| labels.measure(text, style));

    let mut gutter = Gutter::default();

    // An axis with nothing written beside it reserves nothing. That is not a
    // special case — it falls out of measuring an empty list.
    if !y_labels.is_empty() {
        let widest = y_labels.iter().map(|label| label.width).fold(0.0, f64::max);
        gutter.left += tick_length + gap + widest;
        // The topmost label is centred on the top of the plot, so half of it
        // stands above — off the frame, unless the frame is told.
        gutter.top = gutter
            .top
            .max(y_labels.last().map_or(0.0, |label| label.height() / 2.0));
    }
    if !x_labels.is_empty() {
        let tallest = x_labels.iter().map(Label::height).fold(0.0, f64::max);
        gutter.bottom += tick_length + gap + tallest;
        if x.domain.reaches_the_edge() {
            gutter.right = gutter
                .right
                .max(x_labels.last().map_or(0.0, |label| label.width / 2.0));
        }
    }

    // The title goes outside the labels, never over them: whatever the labels
    // took is already in the gutter, and the title adds to it.
    if let Some(title) = &x_title {
        gutter.bottom += gap + title.height();
    }
    if let Some(title) = &y_title {
        // Rotated a quarter turn, so what it costs across the page is its
        // height and not its width.
        gutter.left += gap + title.height();
    }

    let plot = Rect::new(
        frame.x + gutter.left,
        frame.y + gutter.top,
        (frame.w - gutter.left - gutter.right).max(0.0),
        (frame.h - gutter.top - gutter.bottom).max(0.0),
    );

    let x_scale = x.domain.scale((plot.x, plot.right()));
    // Bottom to top: value grows upward while the page grows down, and one
    // reversed range here is what spares every mark from remembering it.
    let y_scale = y.domain.scale((plot.bottom(), plot.y));

    let mut items = Vec::new();
    if plot.w > 0.0 && plot.h > 0.0 {
        emit_x(&mut items, &x, &x_scale, &x_labels, x_title, plot, style, labels, tick_length, gap);
        emit_y(&mut items, &y, &y_scale, &y_labels, y_title, plot, style, labels, tick_length, gap);
    }

    Plotted { plot, x: x_scale, y: y_scale, items, issues }
}

/// Where a mark sits along its axis, whichever kind of axis it is.
///
/// A band's mark belongs in the middle of the band and not at its edge: the
/// label of a bar names the bar, so it stands under the middle of it.
fn offset(tick: &Tick, scale: &Scale) -> Option<f64> {
    match tick {
        Tick::At(value) => scale.map(*value).ok(),
        Tick::In(name) => scale
            .map_category(name)
            .ok()
            .map(|start| start + scale.bandwidth() / 2.0),
    }
}

#[allow(clippy::too_many_arguments)]
fn emit_x(
    items: &mut Vec<DisplayItem>,
    axis: &Draft,
    scale: &Scale,
    measured: &[Label],
    title: Option<Label>,
    plot: Rect,
    style: &ResolvedStyle,
    labels: &dyn Labels,
    tick_length: f64,
    gap: f64,
) {
    if !axis.visible {
        return;
    }

    let baseline_y = plot.bottom();
    items.push(rule(plot.x, baseline_y, plot.right(), baseline_y, style));

    let mut tallest: f64 = 0.0;
    for ((tick, text), label) in axis.ticks.iter().zip(measured) {
        let Some(at) = offset(tick, scale) else { continue };
        tallest = tallest.max(label.height());

        items.push(rule(at, baseline_y, at, baseline_y + tick_length, style));
        items.extend(labels.draw(
            text,
            style,
            at - label.width / 2.0,
            baseline_y + tick_length + gap + label.ascent,
        ));
    }

    if let (Some(title), Some(text)) = (title, axis.title.as_ref()) {
        let below = if measured.is_empty() { 0.0 } else { tick_length + gap + tallest };
        items.extend(labels.draw(
            text,
            style,
            plot.x + (plot.w - title.width) / 2.0,
            baseline_y + below + gap + title.ascent,
        ));
    }
}

#[allow(clippy::too_many_arguments)]
fn emit_y(
    items: &mut Vec<DisplayItem>,
    axis: &Draft,
    scale: &Scale,
    measured: &[Label],
    title: Option<Label>,
    plot: Rect,
    style: &ResolvedStyle,
    labels: &dyn Labels,
    tick_length: f64,
    gap: f64,
) {
    if !axis.visible {
        return;
    }

    items.push(rule(plot.x, plot.y, plot.x, plot.bottom(), style));

    let mut widest: f64 = 0.0;
    for ((tick, text), label) in axis.ticks.iter().zip(measured) {
        let Some(at) = offset(tick, scale) else { continue };
        widest = widest.max(label.width);

        items.push(rule(plot.x - tick_length, at, plot.x, at, style));
        items.extend(labels.draw(
            text,
            style,
            plot.x - tick_length - gap - label.width,
            // Centre the ink on the mark: the ink runs from `baseline -
            // ascent` to `baseline + descent`, so its middle is at `baseline
            // + (descent - ascent) / 2`.
            at + (label.ascent - label.descent) / 2.0,
        ));
    }

    let Some(title) = title else { return };
    let Some(text) = axis.title.as_ref() else { return };

    let left = if measured.is_empty() { 0.0 } else { tick_length + gap + widest };
    // A quarter turn anticlockwise, reading upward, which is where a reader
    // expects the name of a vertical axis. Drawn at the origin of a group
    // whose matrix carries it into place: a point `(u, v)` in the group lands
    // at `(v + e, -u + f)`, so the run's own left-to-right becomes the page's
    // bottom-to-top, and the ink — which lies between `-ascent` and `descent`
    // in `v` — falls between `e - ascent` and `e + descent` across the page.
    let e = plot.x - left - gap - title.height() + title.ascent;
    let f = plot.y + plot.h / 2.0 + title.width / 2.0;

    items.push(DisplayItem::Group(DisplayGroup {
        transform: Some([0.0, -1.0, 1.0, 0.0, e, f]),
        items: labels.draw(text, style, 0.0, 0.0),
        ..DisplayGroup::new()
    }));
}

fn rule(x1: f64, y1: f64, x2: f64, y2: f64, style: &ResolvedStyle) -> DisplayItem {
    DisplayItem::Line(LineItem {
        x1,
        y1,
        x2,
        y2,
        stroke: Stroke { color: style.color, width: AXIS_WIDTH, dash: None },
        source: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::display::GlyphRun;
    use crate::spec::chart::{Axes, Encoding, Value};

    /// A labeller with a ruler instead of a font.
    ///
    /// Every glyph is half an em wide and the ink runs from 0,8em above the
    /// baseline to 0,2em below. Made-up numbers, but fixed ones — which is
    /// what lets a test say "this label is wider, so the gutter grew" and
    /// mean it.
    struct Ruler;

    const WIDTH_EM: f64 = 0.5;
    const ASCENT_EM: f64 = 0.8;
    const DESCENT_EM: f64 = 0.2;

    impl Labels for Ruler {
        fn measure(&self, text: &str, style: &ResolvedStyle) -> Label {
            Label {
                width: text.chars().count() as f64 * style.font_size * WIDTH_EM,
                ascent: style.font_size * ASCENT_EM,
                descent: style.font_size * DESCENT_EM,
            }
        }

        fn draw(&self, text: &str, style: &ResolvedStyle, x: f64, y: f64) -> Vec<DisplayItem> {
            vec![DisplayItem::Glyphs(GlyphRun {
                x,
                y,
                width: self.measure(text, style).width,
                text: text.to_string(),
                ..GlyphRun::default()
            })]
        }
    }

    fn style() -> ResolvedStyle {
        ResolvedStyle { font_size: 10.0, ..ResolvedStyle::default() }
    }

    fn rows(pairs: &[(&str, f64)]) -> Vec<Row> {
        pairs
            .iter()
            .map(|(name, value)| {
                Row::from([
                    ("mes".to_string(), Value::Text(name.to_string())),
                    ("v".to_string(), Value::Number(*value)),
                ])
            })
            .collect()
    }

    /// A bar chart of months against values, with the axes it is given.
    fn chart(axes: Axes) -> ChartFrame {
        ChartFrame {
            mark: Mark::Bar,
            encoding: Encoding {
                x: Channel { field: "mes".into(), kind: FieldKind::Categorical, ..Channel::default() },
                y: Channel::of("v"),
                color: None,
            },
            axes,
            ..ChartFrame::default()
        }
    }

    /// Axes with no titles, so a test that is about labels is about labels.
    fn bare() -> Axes {
        Axes {
            x: AxisSpec { title: Some(String::new()), ..AxisSpec::default() },
            y: AxisSpec { title: Some(String::new()), ..AxisSpec::default() },
        }
    }

    fn runs(items: &[DisplayItem]) -> Vec<GlyphRun> {
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
        walk(items, &mut out);
        out
    }

    const FRAME: Rect = Rect::new(0.0, 0.0, 400.0, 300.0);

    #[test]
    fn long_labels_widen_the_gutter_they_are_written_in() {
        let short = plot(&chart(bare()), &rows(&[("jan", 5.0)]), &style(), &Ruler, FRAME);
        let long = plot(
            &chart(bare()),
            &rows(&[("jan", 5_000_000.0)]),
            &style(),
            &Ruler,
            FRAME,
        );

        assert!(
            long.plot.x > short.plot.x,
            "rótulos maiores empurram a área de desenho para dentro: {} vs {}",
            long.plot.x,
            short.plot.x,
        );
        assert!(long.plot.w < short.plot.w, "e o que sobra para as marcas encolhe");
    }

    #[test]
    fn the_gutter_is_the_widest_label_and_not_the_first_one() {
        // Were the margin taken from one label rather than the widest, this
        // pair would come out the same: both axes start at 0 and only the top
        // differs.
        let baixo = plot(&chart(bare()), &rows(&[("jan", 8.0)]), &style(), &Ruler, FRAME);
        let alto = plot(&chart(bare()), &rows(&[("jan", 80000.0)]), &style(), &Ruler, FRAME);
        assert!(alto.plot.x > baixo.plot.x, "{} vs {}", alto.plot.x, baixo.plot.x);
    }

    #[test]
    fn an_axis_with_nothing_beside_it_reserves_nothing() {
        let hidden = Axes {
            x: AxisSpec { visible: false, ..AxisSpec::default() },
            y: AxisSpec { visible: false, ..AxisSpec::default() },
        };
        let out = plot(&chart(hidden), &rows(&[("jan", 5.0)]), &style(), &Ruler, FRAME);

        assert_eq!(out.plot, FRAME, "a moldura inteira é área de desenho");
        assert!(out.items.is_empty(), "e não se desenha eixo nenhum: {:?}", out.items.len());
    }

    #[test]
    fn hiding_one_axis_frees_only_that_side() {
        let axes = Axes {
            x: AxisSpec { title: Some(String::new()), ..AxisSpec::default() },
            y: AxisSpec { visible: false, ..AxisSpec::default() },
        };
        let out = plot(&chart(axes), &rows(&[("jan", 5.0)]), &style(), &Ruler, FRAME);
        assert_eq!(out.plot.x, FRAME.x, "sem eixo y, nada é tirado da esquerda");
        assert!(out.plot.h < FRAME.h, "mas o eixo x continua a ocupar em baixo");
    }

    #[test]
    fn the_title_falls_outside_the_labels_and_not_over_them() {
        let axes = Axes {
            x: AxisSpec { title: Some("Mês".into()), ..AxisSpec::default() },
            y: AxisSpec { title: Some("Vendas".into()), ..AxisSpec::default() },
        };
        let out = plot(&chart(axes), &rows(&[("jan", 5.0), ("fev", 8.0)]), &style(), &Ruler, FRAME);
        let painted = runs(&out.items);

        let x_title = painted.iter().find(|run| run.text == "Mês").expect("título de x");
        let x_labels: Vec<&GlyphRun> =
            painted.iter().filter(|run| run.text == "jan" || run.text == "fev").collect();
        assert_eq!(x_labels.len(), 2, "os rótulos de x estão lá");
        for label in &x_labels {
            assert!(
                x_title.y > label.y,
                "o título de x fica abaixo do rótulo `{}`: {} vs {}",
                label.text,
                x_title.y,
                label.y,
            );
        }

        // The y title is turned a quarter of a turn, so where it lands is the
        // group's matrix and not the run's own coordinates.
        let turned = out
            .items
            .iter()
            .find_map(|item| match item {
                DisplayItem::Group(group) if !runs(&group.items).is_empty() => Some(group),
                _ => None,
            })
            .expect("título de y, rodado");
        let matrix = turned.transform.expect("com matriz");
        assert_eq!([matrix[0], matrix[1], matrix[2], matrix[3]], [0.0, -1.0, 1.0, 0.0]);

        let widest_y = painted
            .iter()
            .filter(|run| run.text.chars().all(|c| c.is_ascii_digit() || c == ',' || c == '-'))
            .map(|run| run.x)
            .fold(f64::INFINITY, f64::min);
        assert!(
            matrix[4] < widest_y,
            "o título de y fica à esquerda do rótulo mais à esquerda: {} vs {widest_y}",
            matrix[4],
        );
        assert!(matrix[4] >= FRAME.x, "e dentro da moldura: {}", matrix[4]);
    }

    #[test]
    fn a_title_costs_room_even_where_the_labels_cost_none() {
        let only_title = Axes {
            x: AxisSpec { title: Some("Mês".into()), ..AxisSpec::default() },
            y: AxisSpec { visible: false, ..AxisSpec::default() },
        };
        let no_title = Axes {
            x: AxisSpec { title: Some(String::new()), ..AxisSpec::default() },
            y: AxisSpec { visible: false, ..AxisSpec::default() },
        };
        let with = plot(&chart(only_title), &rows(&[("jan", 5.0)]), &style(), &Ruler, FRAME);
        let without = plot(&chart(no_title), &rows(&[("jan", 5.0)]), &style(), &Ruler, FRAME);
        assert!(with.plot.h < without.plot.h, "{} vs {}", with.plot.h, without.plot.h);
    }

    #[test]
    fn a_category_label_stands_under_the_middle_of_its_band() {
        let out = plot(
            &chart(bare()),
            &rows(&[("jan", 5.0), ("fev", 8.0), ("mar", 3.0)]),
            &style(),
            &Ruler,
            FRAME,
        );
        let painted = runs(&out.items);

        for name in ["jan", "fev", "mar"] {
            let run = painted.iter().find(|run| run.text == name).expect(name);
            let centre = run.x + run.width / 2.0;
            let band = out.x.map_category(name).expect(name);
            assert!(
                (centre - (band + out.x.bandwidth() / 2.0)).abs() < 1e-9,
                "`{name}` centrado em {centre}, banda em {band}",
            );
        }
    }

    #[test]
    fn a_number_sits_centred_on_its_own_mark_by_its_ink() {
        let out = plot(&chart(bare()), &rows(&[("jan", 0.0), ("fev", 10.0)]), &style(), &Ruler, FRAME);

        let mut checked = 0;
        for run in runs(&out.items) {
            // The numbers of the vertical axis, told apart from the months.
            let Ok(value) = run.text.replace(',', ".").parse::<f64>() else {
                continue;
            };
            let at = out.y.map(value).expect("na escala");
            // The ink runs from `baseline - ascent` to `baseline + descent`.
            let ascent = style().font_size * ASCENT_EM;
            let descent = style().font_size * DESCENT_EM;
            let middle = run.y - ascent + (ascent + descent) / 2.0;
            assert!(
                (middle - at).abs() < 1e-9,
                "`{}` centrado em {middle}, marca em {at}",
                run.text,
            );
            checked += 1;
        }
        assert!(checked >= 2, "houve números para conferir: {checked}");
    }

    #[test]
    fn a_bar_chart_reaches_zero_without_being_asked() {
        let out = plot(&chart(bare()), &rows(&[("jan", 40.0), ("fev", 50.0)]), &style(), &Ruler, FRAME);
        // Zero maps to the bottom of the plot, which is where a bar stands.
        assert!(
            (out.y.map(0.0).unwrap() - out.plot.bottom()).abs() < 1e-9,
            "o zero está na base: {} vs {}",
            out.y.map(0.0).unwrap(),
            out.plot.bottom(),
        );
    }

    #[test]
    fn the_marks_of_an_axis_share_one_number_of_decimals() {
        assert_eq!(label_numbers(&[0.0, 0.5, 1.0]), ["0,0", "0,5", "1,0"]);
        assert_eq!(label_numbers(&[0.0, 1.0, 2.0]), ["0", "1", "2"]);
        assert_eq!(label_numbers(&[0.0, 200_000.0]), ["0", "200000"]);
    }

    #[test]
    fn a_negative_axis_does_not_write_minus_zero() {
        let written = label_numbers(&[-1.0, 0.0, 1.0]);
        assert_eq!(written, ["-1", "0", "1"], "veio {written:?}");
    }

    #[test]
    fn a_log_axis_that_reaches_zero_says_so_and_falls_back() {
        let mut axes = bare();
        axes.y = AxisSpec { title: Some(String::new()), ..AxisSpec::default() };
        let mut spec = chart(axes);
        spec.encoding.y.scale = Some(crate::spec::chart::ScaleSpec {
            kind: Some(ScaleKind::Log),
            ..Default::default()
        });

        let out = plot(&spec, &rows(&[("jan", 0.0), ("fev", 100.0)]), &style(), &Ruler, FRAME);
        assert_eq!(out.issues, vec![Issue::LogDomainCrossesZero { axis: "y" }]);
        assert!(
            matches!(out.y, Scale::Linear { .. }),
            "cai para linear em vez de não desenhar nada",
        );
    }

    #[test]
    fn a_field_that_never_varies_still_gets_an_axis() {
        let out = plot(&chart(bare()), &rows(&[("jan", 7.0), ("fev", 7.0)]), &style(), &Ruler, FRAME);
        assert!(out.plot.h > 0.0);
        assert!(!runs(&out.items).is_empty(), "e marcas com números legíveis");
    }

    #[test]
    fn a_frame_too_small_for_its_own_axes_does_not_go_negative() {
        let tiny = plot(&chart(bare()), &rows(&[("jan", 5.0)]), &style(), &Ruler, Rect::new(0.0, 0.0, 4.0, 4.0));
        assert!(tiny.plot.w >= 0.0 && tiny.plot.h >= 0.0, "veio {:?}", tiny.plot);
        assert!(tiny.items.is_empty(), "e não se desenha num plano de área nula");
    }
}
