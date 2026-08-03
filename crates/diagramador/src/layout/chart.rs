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
//! Bars and lines are drawn here, into the plot the first half worked out.
//! Dispersão and área are still only vocabulary — T4.4 and T4.6.

use crate::color::Color;
use crate::display::{
    DisplayGroup, DisplayItem, FillRule, LineItem, PathCommand, PathItem, RectItem, Stroke,
};
use crate::spec::ResolvedStyle;
use crate::spec::chart::{
    Axis as AxisSpec, Channel, ChartFrame, FieldKind, Mark, Row, ScaleKind, Value,
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

/// Share of a group given up to separate one series' bar from the next.
///
/// Narrower than the gap between categories, because that is what says the
/// bars of a group belong together and the groups do not.
const GROUP_PADDING: f64 = 0.05;

/// Widest a single bar may be, as a share of the plot across the band's axis.
///
/// A bar is a length read against an axis, and past a certain thickness it
/// stops being a bar and becomes a slab — a chart of one category would
/// otherwise be a rectangle the width of the page. The band's own padding
/// already keeps a bar off its neighbour; this is what keeps a bar off being
/// the whole picture, and it binds only where there are few categories.
const MAX_BAR_SHARE: f64 = 1.0 / 6.0;

/// Thickness of a line mark.
const LINE_WIDTH: f64 = 1.5;

/// The categorical palette, in the order the slots are handed out.
///
/// The order is the mechanism and not the decoration: it is an ordering whose
/// *neighbouring* pairs stay apart under colour-blind simulation, which is
/// exactly what a chart puts side by side. Checked against white paper —
/// worst adjacent pair ΔE 9,1 under protanopia and 19,6 under normal vision,
/// against floors of 8 and 15.
///
/// Three of the eight fall under 3:1 against white, so identity may never
/// rest on colour alone: the legend of T4.4 is what carries it, and it is not
/// optional for a chart of two series or more.
const PALETTE: [Color; 8] = [
    Color::rgb(0.164706, 0.470588, 0.839216), // azul
    Color::rgb(0.921569, 0.407843, 0.203922), // laranja
    Color::rgb(0.105882, 0.686275, 0.478431), // água
    Color::rgb(0.929412, 0.631373, 0.0),      // amarelo
    Color::rgb(0.909804, 0.482353, 0.643137), // magenta
    Color::rgb(0.0, 0.513725, 0.0),           // verde
    Color::rgb(0.290196, 0.227451, 0.654902), // violeta
    Color::rgb(0.890196, 0.286275, 0.282353), // vermelho
];

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
    // The marks are drawn inside this module, so outside the tests nothing
    // reads these three yet. They are public because a geometry nobody can
    // read is a geometry nobody can check — and because the legend of T4.4
    // has to be placed against the same rectangle.
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
    /// Bars asked for, but neither axis holds categories to stand them on.
    BarsWithoutCategories,
    /// More series than the palette has colours, so two of them share one.
    ///
    /// Repeating a colour and saying so beats the alternatives: inventing a
    /// ninth hue puts two indistinguishable colours on the page without
    /// warning, and dropping the ninth series drops data the author wrote.
    /// The fix is the author's — group the tail, or declare a palette.
    SeriesOutnumberPalette { series: usize, colours: usize },
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
    let meaning = channel.kind.unwrap_or_else(|| infer(rows, &channel.field));

    // The field's kind picks the scale; the author only ever contradicts it.
    // A category on a bar chart wants an interval to stand a bar in, and on a
    // line chart a single position to pass through — which is the whole
    // difference between `Band` and `Point`.
    let kind = spec.kind.unwrap_or(match (meaning, mark) {
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

/// What a field means, when the author did not say.
///
/// Only ever asked where the answer is not a guess. A field holding a number
/// anywhere is a quantity — a year written `2024` belongs on a numeric axis
/// unless the author says otherwise. A field holding text and no numbers
/// cannot go on a continuous scale at all, so it is categorical; reading it
/// as a quantity gave an axis of `0` to `1` over a column of month names.
/// A field holding nothing either way is left quantitative, which draws an
/// empty numeric axis rather than an empty categorical one.
fn infer(rows: &[Row], field: &str) -> FieldKind {
    let mut saw_text = false;
    for row in rows {
        match row.get(field) {
            Some(Value::Number(_)) => return FieldKind::Quantitative,
            Some(Value::Text(_)) => saw_text = true,
            _ => {}
        }
    }
    if saw_text { FieldKind::Categorical } else { FieldKind::Quantitative }
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
    // A vertical axis runs one way for quantities and the other for names.
    // A quantity grows upward while the page grows down, so its range is
    // reversed once here and no mark has to remember it again. A list of
    // categories is read top to bottom, the way any list is — reversing it
    // too would put the first row of the data at the foot of the chart.
    let y_scale = y.domain.scale(match y.domain {
        Domain::Continuous { .. } => (plot.bottom(), plot.y),
        Domain::Bands(_) | Domain::Points(_) => (plot.y, plot.bottom()),
    });

    let mut items = Vec::new();
    if plot.w > 0.0 && plot.h > 0.0 {
        // Marks first, axes over them: an axis line under a bar is a line the
        // bar rubs out, and the rule a reader measures against has to be the
        // one they can see.
        items.extend(marks(chart, rows, &x_scale, &y_scale, plot, &mut issues));

        emit_x(&mut items, &x, &x_scale, &x_labels, x_title, plot, style, labels, tick_length, gap);
        emit_y(&mut items, &y, &y_scale, &y_labels, y_title, plot, style, labels, tick_length, gap);
    }

    Plotted { plot, x: x_scale, y: y_scale, items, issues }
}

// ─────────────────────────────────────────────────────────────────────────────
// Marks
// ─────────────────────────────────────────────────────────────────────────────

/// One series: the rows that share a colour.
struct Series<'a> {
    /// What the `color` channel called it. `None` when the chart has one
    /// series and nothing to name it after.
    #[allow(dead_code)] // The legend of T4.4 is what reads it.
    name: Option<String>,
    rows: Vec<&'a Row>,
    colour: Color,
}

/// Split the rows into series, one colour each.
///
/// In the order the series are first met, which is the order the author typed
/// them — the same rule the categories of an axis follow, and for the same
/// reason. A colour belongs to a series and not to its rank: were they sorted
/// by size, adding a row could repaint the whole chart.
fn series<'a>(
    chart: &ChartFrame,
    rows: &'a [Row],
    issues: &mut Vec<Issue>,
) -> Vec<Series<'a>> {
    let palette: &[Color] = if chart.palette.is_empty() { &PALETTE } else { &chart.palette };

    let Some(channel) = &chart.encoding.color else {
        return vec![Series {
            name: None,
            rows: rows.iter().collect(),
            colour: palette.first().copied().unwrap_or(Color::BLACK),
        }];
    };

    let mut names: Vec<String> = Vec::new();
    let mut grouped: Vec<Vec<&Row>> = Vec::new();
    for row in rows {
        let name = row
            .get(&channel.field)
            .and_then(Value::as_category)
            .unwrap_or_default();
        match names.iter().position(|seen| *seen == name) {
            Some(index) => grouped[index].push(row),
            None => {
                names.push(name);
                grouped.push(vec![row]);
            }
        }
    }

    if names.len() > palette.len() {
        issues.push(Issue::SeriesOutnumberPalette {
            series: names.len(),
            colours: palette.len(),
        });
    }

    names
        .into_iter()
        .zip(grouped)
        .enumerate()
        .map(|(index, (name, rows))| Series {
            name: Some(name),
            rows,
            // Past the end of the palette a colour repeats. The author has
            // already been told; drawing nothing would be worse.
            colour: palette
                .get(index % palette.len().max(1))
                .copied()
                .unwrap_or(Color::BLACK),
        })
        .collect()
}

/// Where a row's value sits along an axis, whichever kind of axis it is.
///
/// `None` for a hole, for a category the scale never heard of, and for a
/// number a logarithmic scale cannot place — all three are gaps in the data,
/// and a gap is drawn as a gap.
fn position(scale: &Scale, value: Option<&Value>) -> Option<f64> {
    let value = value?;
    match scale {
        Scale::Band { .. } | Scale::Point { .. } => scale
            .map_category(&value.as_category()?)
            .ok()
            .map(|start| start + scale.bandwidth() / 2.0),
        Scale::Linear { .. } | Scale::Log { .. } => scale.map(value.as_number()?).ok(),
    }
}

fn marks(
    chart: &ChartFrame,
    rows: &[Row],
    x: &Scale,
    y: &Scale,
    plot: Rect,
    issues: &mut Vec<Issue>,
) -> Vec<DisplayItem> {
    let series = series(chart, rows, issues);

    match chart.mark {
        Mark::Bar => bars(chart, &series, x, y, plot, issues),
        Mark::Line => lines(chart, &series, x, y),
        // Dispersão is T4.4 and área is T4.6. Drawing nothing is the honest
        // state of a mark that has a vocabulary and no geometry yet.
        Mark::Point | Mark::Area => Vec::new(),
    }
}

/// Which way the bars grow.
enum Standing {
    /// Categories across the bottom, values upward. The common case.
    Upright,
    /// Categories down the side, values rightward — which is what saves a
    /// chart of eleven long names from an axis of turned type.
    Lying,
}

fn bars(
    chart: &ChartFrame,
    series: &[Series<'_>],
    x: &Scale,
    y: &Scale,
    plot: Rect,
    issues: &mut Vec<Issue>,
) -> Vec<DisplayItem> {
    let banded = |scale: &Scale| matches!(scale, Scale::Band { .. } | Scale::Point { .. });

    let standing = match (banded(x), banded(y)) {
        (true, false) => Standing::Upright,
        (false, true) => Standing::Lying,
        _ => {
            // Two continuous axes, or two categorical ones: there is nothing
            // to stand a bar on, and a guess would be a chart nobody asked for.
            issues.push(Issue::BarsWithoutCategories);
            return Vec::new();
        }
    };

    let (band, value, band_field, value_field) = match standing {
        Standing::Upright => (x, y, &chart.encoding.x.field, &chart.encoding.y.field),
        Standing::Lying => (y, x, &chart.encoding.y.field, &chart.encoding.x.field),
    };

    // Where a bar stands. Zero when the axis reaches it — which for a bar it
    // does unless the author said otherwise — and the near end of the axis
    // when it does not, because a bar has to grow from somewhere.
    let range = match standing {
        Standing::Upright => (plot.y, plot.bottom()),
        Standing::Lying => (plot.x, plot.right()),
    };
    let foot = value
        .map(0.0)
        .map(|at| at.clamp(range.0, range.1))
        .unwrap_or(match standing {
            Standing::Upright => plot.bottom(),
            Standing::Lying => plot.x,
        });

    let across = match standing {
        Standing::Upright => plot.w,
        Standing::Lying => plot.h,
    };
    // The group is capped, then divided: capping each bar instead would open
    // gaps inside a group, and the bars of one group belong together.
    let group_width = band
        .bandwidth()
        .min(across * MAX_BAR_SHARE * series.len().max(1) as f64);

    let mut items = Vec::new();
    for (index, one) in series.iter().enumerate() {
        for row in &one.rows {
            let Some(centre) = position(band, row.get(band_field)) else {
                continue;
            };
            let Some(head) = position(value, row.get(value_field)) else {
                continue;
            };

            let slot = share(group_width, series.len(), index);
            let start = centre - group_width / 2.0 + slot.0;
            let (near, far) = (foot.min(head), foot.max(head));

            items.push(DisplayItem::Rect(RectItem {
                rect: match standing {
                    Standing::Upright => Rect::new(start, near, slot.1, far - near),
                    Standing::Lying => Rect::new(near, start, far - near, slot.1),
                },
                radius: 0.0,
                fill: Some(one.colour),
                stroke: None,
                source: None,
            }));
        }
    }
    items
}

/// Offset and width of one series' share of a group.
fn share(width: f64, count: usize, index: usize) -> (f64, f64) {
    // One series has nothing to be separated from: the band's own padding
    // already holds it off its neighbours, and taking a gap here as well
    // would narrow every bar of every ordinary chart for nothing.
    if count <= 1 {
        return (0.0, width.max(0.0));
    }
    // The gap comes out of each bar and not out of its place, so the bars
    // stay evenly spaced however many there are.
    let step = width / count as f64;
    let bar = (step * (1.0 - GROUP_PADDING)).max(0.0);
    (step * index as f64 + (step - bar) / 2.0, bar)
}

/// A line per series, broken wherever the data is.
///
/// One path rather than a segment per pair: a path joins, and two independent
/// segments meeting at an angle leave a notch that grows with the line's
/// weight. It is also the primitive T1.0 exists for.
fn lines(chart: &ChartFrame, series: &[Series<'_>], x: &Scale, y: &Scale) -> Vec<DisplayItem> {
    let mut items = Vec::new();

    for one in series {
        // A reading with no place on the horizontal axis cannot be drawn at
        // all, so it is not a hole in the line — it is not on the line. One
        // with a place but no value is the hole, and it breaks the line there.
        let mut points: Vec<(f64, Option<f64>)> = one
            .rows
            .iter()
            .filter_map(|row| {
                let at = position(x, row.get(&chart.encoding.x.field))?;
                Some((at, position(y, row.get(&chart.encoding.y.field))))
            })
            .collect();

        // In the order the axis reads, not the order the file is written: a
        // line through unsorted points is a scribble, and the axis is what
        // says which way along it "next" means.
        points.sort_by(|a, b| a.0.total_cmp(&b.0));

        let mut commands = Vec::new();
        let mut pen_down = false;
        for (x, y) in points {
            match y {
                Some(y) if !pen_down => {
                    commands.push(PathCommand::MoveTo { x, y });
                    pen_down = true;
                }
                Some(y) => commands.push(PathCommand::LineTo { x, y }),
                None => pen_down = false,
            }
        }

        // A series of lone readings, each cut off from the next by a hole, is
        // a path of moves with nowhere to go. It paints nothing in either
        // emitter, so it is not emitted at all.
        if !commands.iter().any(|c| matches!(c, PathCommand::LineTo { .. })) {
            continue;
        }

        items.push(DisplayItem::Path(PathItem {
            commands,
            fill: None,
            stroke: Some(Stroke { color: one.colour, width: LINE_WIDTH, dash: None }),
            fill_rule: FillRule::NonZero,
            source: None,
        }));
    }

    items
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
                x: Channel { field: "mes".into(), kind: Some(FieldKind::Categorical), ..Channel::default() },
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

    fn bars_of(items: &[DisplayItem]) -> Vec<RectItem> {
        items
            .iter()
            .filter_map(|item| match item {
                DisplayItem::Rect(rect) => Some(rect.clone()),
                _ => None,
            })
            .collect()
    }

    fn paths_of(items: &[DisplayItem]) -> Vec<PathItem> {
        items
            .iter()
            .filter_map(|item| match item {
                DisplayItem::Path(path) => Some(path.clone()),
                _ => None,
            })
            .collect()
    }

    /// Rows carrying a series name as well, for the grouped cases.
    fn split(triples: &[(&str, &str, f64)]) -> Vec<Row> {
        triples
            .iter()
            .map(|(month, series, value)| {
                Row::from([
                    ("mes".to_string(), Value::Text(month.to_string())),
                    ("serie".to_string(), Value::Text(series.to_string())),
                    ("v".to_string(), Value::Number(*value)),
                ])
            })
            .collect()
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
        // The bar is still drawn — hiding an axis hides the axis, not the
        // data. What must be gone is every line and every word of furniture.
        assert!(
            !out.items.iter().any(|item| matches!(item, DisplayItem::Line(_))),
            "nenhum traço de eixo: {:?}",
            out.items,
        );
        assert!(runs(&out.items).is_empty(), "nem rótulo nenhum");
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

    // ── Marks ───────────────────────────────────────────────────────────────

    #[test]
    fn a_bar_stands_on_the_zero_and_reaches_its_value() {
        let out = plot(
            &chart(bare()),
            &rows(&[("jan", 40.0), ("fev", 80.0)]),
            &style(),
            &Ruler,
            FRAME,
        );
        let bars = bars_of(&out.items);
        assert_eq!(bars.len(), 2, "uma barra por observação");

        let foot = out.y.map(0.0).unwrap();
        for (bar, value) in bars.iter().zip([40.0, 80.0]) {
            assert!((bar.rect.bottom() - foot).abs() < 1e-9, "assenta no zero: {bar:?}");
            assert!(
                (bar.rect.y - out.y.map(value).unwrap()).abs() < 1e-9,
                "e o topo está no valor {value}: {bar:?}",
            );
        }
        // Twice the value, twice the bar — which is the whole claim a bar
        // chart makes, and the reason the axis has to reach zero.
        assert!(
            (bars[1].rect.h / bars[0].rect.h - 2.0).abs() < 1e-9,
            "{} vs {}",
            bars[1].rect.h,
            bars[0].rect.h,
        );
    }

    #[test]
    fn a_negative_value_hangs_below_the_zero() {
        let out = plot(
            &chart(bare()),
            &rows(&[("jan", 30.0), ("fev", -30.0)]),
            &style(),
            &Ruler,
            FRAME,
        );
        let bars = bars_of(&out.items);
        let foot = out.y.map(0.0).unwrap();

        assert!(bars[0].rect.bottom() <= foot + 1e-9, "a positiva sobe do zero");
        assert!(bars[1].rect.y >= foot - 1e-9, "a negativa desce dele");
        assert!(
            (bars[0].rect.h - bars[1].rect.h).abs() < 1e-9,
            "e trinta para baixo mede o mesmo que trinta para cima",
        );
        assert!(foot > out.plot.y && foot < out.plot.bottom(), "o zero fica no meio");
    }

    #[test]
    fn a_lone_category_gets_a_bar_and_not_a_slab() {
        let out = plot(&chart(bare()), &rows(&[("jan", 5.0)]), &style(), &Ruler, FRAME);
        let bars = bars_of(&out.items);
        assert_eq!(bars.len(), 1);
        assert!(
            bars[0].rect.w <= out.plot.w * MAX_BAR_SHARE + 1e-9,
            "uma barra só não ocupa a largura toda: {} de {}",
            bars[0].rect.w,
            out.plot.w,
        );
        let centre = bars[0].rect.x + bars[0].rect.w / 2.0;
        let band = out.x.map_category("jan").unwrap() + out.x.bandwidth() / 2.0;
        assert!((centre - band).abs() < 1e-9, "e fica centrada na sua banda");
    }

    #[test]
    fn eleven_categories_each_get_their_own_bar_inside_the_plot() {
        let months = [
            "janeiro", "fevereiro", "março", "abril", "maio", "junho", "julho", "agosto",
            "setembro", "outubro", "novembro",
        ];
        let data: Vec<(&str, f64)> =
            months.iter().enumerate().map(|(i, m)| (*m, (i + 1) as f64 * 10.0)).collect();
        let out = plot(&chart(bare()), &rows(&data), &style(), &Ruler, FRAME);

        let bars = bars_of(&out.items);
        assert_eq!(bars.len(), 11);
        for bar in &bars {
            assert!(bar.rect.w > 0.0, "nenhuma barra desaparece: {bar:?}");
            assert!(
                bar.rect.x >= out.plot.x - 1e-9 && bar.rect.right() <= out.plot.right() + 1e-9,
                "e nenhuma sai da área de desenho: {:?} em {:?}",
                bar.rect,
                out.plot,
            );
        }
        // Ordered along the axis, so the eleventh is not drawn over the first.
        for pair in bars.windows(2) {
            assert!(pair[0].rect.x < pair[1].rect.x, "as barras seguem as categorias");
        }
    }

    #[test]
    fn three_series_share_the_band_without_overlapping() {
        let mut spec = chart(bare());
        spec.encoding.color = Some(Channel {
            field: "serie".into(),
            kind: Some(FieldKind::Categorical),
            ..Channel::default()
        });

        let out = plot(
            &spec,
            &split(&[
                ("jan", "norte", 10.0),
                ("jan", "sul", 20.0),
                ("jan", "leste", 30.0),
                ("fev", "norte", 15.0),
                ("fev", "sul", 25.0),
                ("fev", "leste", 35.0),
            ]),
            &style(),
            &Ruler,
            FRAME,
        );

        let bars = bars_of(&out.items);
        assert_eq!(bars.len(), 6);

        // One colour per series, three distinct ones, and the same colour for
        // the same series in both months.
        let colours: Vec<_> = bars.iter().map(|bar| bar.fill.unwrap()).collect();
        assert_eq!(colours[0], colours[1], "norte em jan e fev tem uma cor só");
        assert_ne!(colours[0], colours[2], "e séries diferentes, cores diferentes");
        assert_ne!(colours[2], colours[4]);

        // Inside January, the three bars sit side by side and never overlap.
        let mut janeiro: Vec<&RectItem> = bars
            .iter()
            .filter(|bar| bar.rect.x < out.plot.x + out.plot.w / 2.0)
            .collect();
        janeiro.sort_by(|a, b| a.rect.x.total_cmp(&b.rect.x));
        assert_eq!(janeiro.len(), 3, "três em janeiro");
        for pair in janeiro.windows(2) {
            assert!(
                pair[0].rect.right() <= pair[1].rect.x + 1e-9,
                "sem sobreposição: {:?} e {:?}",
                pair[0].rect,
                pair[1].rect,
            );
        }
    }

    #[test]
    fn one_series_keeps_the_whole_band_it_was_given() {
        // The gap between grouped bars comes out of the group. With a single
        // series there is no neighbour to be held off, and taking the gap
        // anyway would narrow every ordinary bar chart for nothing.
        let out = plot(&chart(bare()), &rows(&[("jan", 5.0), ("fev", 6.0)]), &style(), &Ruler, FRAME);
        let bars = bars_of(&out.items);
        let expected = out.x.bandwidth().min(out.plot.w * MAX_BAR_SHARE);
        assert!((bars[0].rect.w - expected).abs() < 1e-9, "{} vs {expected}", bars[0].rect.w);
    }

    #[test]
    fn categories_down_a_vertical_axis_read_top_to_bottom() {
        // Lying bars: the categories go on `y`. A vertical axis is reversed
        // for quantities, because value grows upward while the page grows
        // down — and reversing it for names as well put the first row of the
        // data at the foot of the chart, which is not how a list is read.
        let mut spec = chart(bare());
        spec.encoding.x = Channel { field: "v".into(), ..Channel::default() };
        spec.encoding.y = Channel {
            field: "mes".into(),
            kind: Some(FieldKind::Categorical),
            ..Channel::default()
        };

        let out = plot(
            &spec,
            &rows(&[("jan", 5.0), ("fev", 9.0), ("mar", 3.0)]),
            &style(),
            &Ruler,
            FRAME,
        );

        let janeiro = out.y.map_category("jan").unwrap();
        let marco = out.y.map_category("mar").unwrap();
        assert!(
            janeiro < marco,
            "a primeira categoria fica em cima: jan em {janeiro}, mar em {marco}",
        );

        // And the bars follow it, rather than the scale being right on its own.
        let bars = bars_of(&out.items);
        assert_eq!(bars.len(), 3);
        assert!(bars[0].rect.y < bars[2].rect.y, "{:?}", bars);
        // Lying, so they grow rightward from the left edge of the plot.
        for bar in &bars {
            assert!((bar.rect.x - out.plot.x).abs() < 1e-9, "assenta no zero: {bar:?}");
            assert!(bar.rect.w > 0.0);
        }
    }

    #[test]
    fn a_bar_chart_with_no_categorical_axis_says_so_and_draws_none() {
        let mut spec = chart(bare());
        spec.encoding.x = Channel {
            field: "n".into(),
            kind: Some(FieldKind::Quantitative),
            ..Channel::default()
        };
        let data = vec![Row::from([
            ("n".to_string(), Value::Number(1.0)),
            ("v".to_string(), Value::Number(5.0)),
        ])];

        let out = plot(&spec, &data, &style(), &Ruler, FRAME);
        assert_eq!(out.issues, vec![Issue::BarsWithoutCategories]);
        assert!(bars_of(&out.items).is_empty(), "e não se adivinha uma barra");
    }

    #[test]
    fn a_line_joins_its_points_in_the_order_the_axis_reads() {
        let mut spec = chart(bare());
        spec.mark = Mark::Line;
        spec.encoding.x = Channel { field: "t".into(), ..Channel::default() };

        // Written out of order on purpose: the file's order is not the axis's.
        let data: Vec<Row> = [(30.0, 3.0), (10.0, 1.0), (20.0, 2.0)]
            .iter()
            .map(|(t, v)| {
                Row::from([
                    ("t".to_string(), Value::Number(*t)),
                    ("v".to_string(), Value::Number(*v)),
                ])
            })
            .collect();

        let out = plot(&spec, &data, &style(), &Ruler, FRAME);
        let paths = paths_of(&out.items);
        assert_eq!(paths.len(), 1, "uma linha por série");

        let xs: Vec<f64> = paths[0]
            .commands
            .iter()
            .map(|command| match command {
                PathCommand::MoveTo { x, .. } | PathCommand::LineTo { x, .. } => *x,
                _ => f64::NAN,
            })
            .collect();
        assert_eq!(xs.len(), 3);
        for pair in xs.windows(2) {
            assert!(pair[0] < pair[1], "a linha sobe o eixo, não o ficheiro: {xs:?}");
        }
        assert!(paths[0].fill.is_none(), "uma linha não se preenche");
        assert!(paths[0].stroke.is_some());
    }

    #[test]
    fn a_hole_breaks_the_line_instead_of_bridging_it() {
        let mut spec = chart(bare());
        spec.mark = Mark::Line;
        spec.encoding.x = Channel { field: "t".into(), ..Channel::default() };

        let data: Vec<Row> = [
            (10.0, Value::Number(1.0)),
            (20.0, Value::Null),
            (30.0, Value::Number(3.0)),
            (40.0, Value::Number(4.0)),
        ]
        .into_iter()
        .map(|(t, v)| Row::from([("t".to_string(), Value::Number(t)), ("v".to_string(), v)]))
        .collect();

        let out = plot(&spec, &data, &style(), &Ruler, FRAME);
        let commands = &paths_of(&out.items)[0].commands;

        let moves = commands
            .iter()
            .filter(|c| matches!(c, PathCommand::MoveTo { .. }))
            .count();
        assert_eq!(moves, 2, "dois traços, e não um por cima do vazio: {commands:?}");
        assert!(matches!(commands[1], PathCommand::MoveTo { .. }), "{commands:?}");
    }

    #[test]
    fn a_series_of_lone_readings_paints_nothing_rather_than_an_empty_path() {
        let mut spec = chart(bare());
        spec.mark = Mark::Line;
        spec.encoding.x = Channel { field: "t".into(), ..Channel::default() };

        let data: Vec<Row> = [
            (10.0, Value::Number(1.0)),
            (20.0, Value::Null),
            (30.0, Value::Number(3.0)),
        ]
        .into_iter()
        .map(|(t, v)| Row::from([("t".to_string(), Value::Number(t)), ("v".to_string(), v)]))
        .collect();

        let out = plot(&spec, &data, &style(), &Ruler, FRAME);
        assert!(
            paths_of(&out.items).is_empty(),
            "dois pontos soltos não são uma linha: {:?}",
            paths_of(&out.items),
        );
    }

    #[test]
    fn a_colour_belongs_to_a_series_and_not_to_its_rank() {
        let mut spec = chart(bare());
        spec.mark = Mark::Line;
        spec.encoding.x = Channel { field: "t".into(), ..Channel::default() };
        spec.encoding.color = Some(Channel {
            field: "serie".into(),
            kind: Some(FieldKind::Categorical),
            ..Channel::default()
        });

        let row = |t: f64, series: &str, v: f64| {
            Row::from([
                ("t".to_string(), Value::Number(t)),
                ("serie".to_string(), Value::Text(series.to_string())),
                ("v".to_string(), Value::Number(v)),
            ])
        };

        // The same two series, the second one a hundred times larger in the
        // second reading. Were the colours handed out by size, `b` would take
        // the first slot there and both series would repaint.
        let miudo = [row(1.0, "a", 1.0), row(2.0, "a", 2.0), row(1.0, "b", 3.0), row(2.0, "b", 4.0)];
        let graudo =
            [row(1.0, "a", 1.0), row(2.0, "a", 2.0), row(1.0, "b", 300.0), row(2.0, "b", 400.0)];

        assert_eq!(paths_of(&plot(&spec, &miudo, &style(), &Ruler, FRAME).items).len(), 2);

        // Read by name, not by position. A list of colours taken off the
        // paths comes out the same however the series were sorted, so it
        // would have proved nothing at all — that was the first version of
        // this test, and a mutation that sorted the series by size passed it.
        let named = |data: &[Row]| -> Vec<(Option<String>, Color)> {
            series(&spec, data, &mut Vec::new())
                .into_iter()
                .map(|one| (one.name, one.colour))
                .collect()
        };

        assert_eq!(
            named(&miudo).iter().map(|(name, _)| name.clone()).collect::<Vec<_>>(),
            vec![Some("a".to_string()), Some("b".to_string())],
            "as séries saem na ordem em que foram escritas",
        );
        assert_eq!(named(&miudo), named(&graudo), "e cada nome fica com a cor que tinha");
    }

    #[test]
    fn more_series_than_colours_is_said_out_loud() {
        let mut spec = chart(bare());
        spec.encoding.color = Some(Channel {
            field: "serie".into(),
            kind: Some(FieldKind::Categorical),
            ..Channel::default()
        });

        let data: Vec<Row> = (0..PALETTE.len() + 1)
            .map(|index| {
                Row::from([
                    ("mes".to_string(), Value::Text("jan".to_string())),
                    ("serie".to_string(), Value::Text(format!("s{index}"))),
                    ("v".to_string(), Value::Number(1.0)),
                ])
            })
            .collect();

        let out = plot(&spec, &data, &style(), &Ruler, FRAME);
        assert_eq!(
            out.issues,
            vec![Issue::SeriesOutnumberPalette { series: 9, colours: 8 }],
        );
        assert_eq!(bars_of(&out.items).len(), 9, "e desenha as nove à mesma");
    }

    #[test]
    fn the_axis_is_drawn_over_the_bars_and_not_under_them() {
        let out = plot(&chart(bare()), &rows(&[("jan", 5.0)]), &style(), &Ruler, FRAME);
        let first_bar = out
            .items
            .iter()
            .position(|item| matches!(item, DisplayItem::Rect(_)))
            .expect("uma barra");
        let first_line = out
            .items
            .iter()
            .position(|item| matches!(item, DisplayItem::Line(_)))
            .expect("um eixo");
        assert!(
            first_bar < first_line,
            "a régua que o leitor mede tem de ser a que ele vê",
        );
    }

    #[test]
    fn a_field_of_names_is_read_as_categories_without_being_told() {
        // What anyone writes first, with no `kind` anywhere: months against
        // numbers. Read as a quantity it gave an axis of 0 a 1 over text and
        // a bar chart with nothing to stand on.
        let spec = ChartFrame {
            mark: Mark::Bar,
            encoding: Encoding {
                x: Channel { field: "mes".into(), ..Channel::default() },
                y: Channel::of("v"),
                color: None,
            },
            axes: bare(),
            ..ChartFrame::default()
        };
        let out = plot(&spec, &rows(&[("jan", 5.0), ("fev", 9.0)]), &style(), &Ruler, FRAME);

        assert!(out.issues.is_empty(), "{:?}", out.issues);
        assert!(matches!(out.x, Scale::Band { .. }), "o eixo é de bandas");
        assert_eq!(bars_of(&out.items).len(), 2);
    }

    #[test]
    fn a_number_stays_a_number_even_beside_names() {
        // The inference only ever answers where there is no doubt. A year is
        // written `2024` and belongs on a numeric axis; one text value in the
        // column does not turn the column into labels.
        let data = vec![
            Row::from([("ano".to_string(), Value::Number(2023.0))]),
            Row::from([("ano".to_string(), Value::Text("s/d".to_string()))]),
        ];
        assert_eq!(infer(&data, "ano"), FieldKind::Quantitative);
        assert_eq!(infer(&data, "inexistente"), FieldKind::Quantitative, "sem dados, sem palpite");
    }
}
