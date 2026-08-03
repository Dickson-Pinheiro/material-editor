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
    DisplayGroup, DisplayItem, EllipseItem, FillRule, LineItem, PathCommand, PathItem, RectItem,
    Stroke,
};
use crate::spec::ResolvedStyle;
use crate::spec::chart::{
    Axis as AxisSpec, Channel, ChartFrame, FieldKind, LegendPosition, Mark, Row, ScaleKind, Value,
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
/// returns whatever lands on round numbers near it, and the fitting below
/// takes marks away again when the labels will not sit side by side.
///
/// Was 60 until a scatter 190pt tall came out with three marks, which made
/// `nice` widen a domain of 35–110 to 0–150 and spend a third of the height
/// on nothing. Forty is what Vega-Lite asks of a vertical axis, and it is the
/// number that stopped that happening.
const PT_PER_TICK: f64 = 40.0;

/// Thickness of an axis line, matching a rule block's default.
const AXIS_WIDTH: f64 = 0.75;

/// How much of the document's ink a gridline keeps.
///
/// A wash rather than a colour of its own: the grid then follows whatever the
/// text is set in and stays behind it on any paper, where a fixed grey would
/// be right on white and wrong everywhere else. It has to be read past, not
/// read.
const GRID_ALPHA: f32 = 0.14;

/// Clearance between two neighbouring labels on an axis, in ems.
///
/// Below this they are not overlapping yet, but they read as one word.
const CLEARANCE_EM: f64 = 0.6;

/// Most of the frame a vertical axis's labels may take across it.
///
/// A vertical label never collides with its neighbour — they are stacked, not
/// set side by side — so crowding is not what goes wrong on that axis. Width
/// is: `1000000000` down the side of a small chart spends a fifth of the
/// picture saying what `1 bi` says. Past this share the numbers are larger
/// than the drawing, and that is a label not fitting just as much as an
/// overlap is.
///
/// A fifth and not a quarter, which was the first guess: a quarter of a 235pt
/// frame is 59 points, and ten digits fit inside that with room to spare — so
/// the very case this exists for slipped through.
const SIDE_SHARE: f64 = 1.0 / 5.0;

/// An eighth of a turn: what a label that will not fit flat is given.
const TURN_COS: f64 = std::f64::consts::FRAC_1_SQRT_2;

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

/// Diameter of a scatter mark, in ems of the chart's own type.
///
/// Big enough to read as a mark of its own rather than a speck of dirt, and
/// tied to the type so a chart set small gets marks to match.
const POINT_EM: f64 = 0.6;

/// Side of a legend's swatch, in ems.
const SWATCH_EM: f64 = 0.7;

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

    let drawn = series(chart, rows, &mut issues);

    // ── What the legend costs ───────────────────────────────────────────────
    //
    // Taken out first, before the axes measure anything, because the axes
    // have to fit what is left rather than the other way round: an axis that
    // sized itself to the whole frame and then found a legend beside it would
    // run its last label off the edge.
    let legend = legend_of(chart, &drawn, style, labels, frame);
    let field = legend.as_ref().map_or(frame, |box_| box_.leaves(frame));

    // ── What the labels cost ────────────────────────────────────────────────
    //
    // The vertical axis first, because nothing about it depends on the
    // horizontal one — and because how much room the horizontal labels have
    // to fit in is exactly what is left after it.
    let mut y = y;
    fit(
        &mut y,
        &chart.axes.y,
        field.h,
        Side::Across(field.w * SIDE_SHARE),
        style,
        labels,
    );

    let y_labels: Vec<Label> =
        y.ticks.iter().map(|(_, text)| labels.measure(text, style)).collect();
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
    if let Some(title) = &y_title {
        // Rotated a quarter turn, so what it costs across the page is its
        // height and not its width.
        gutter.left += gap + title.height();
    }

    // Make the horizontal labels fit what the vertical axis left over. The
    // room is read before the horizontal labels are measured, which is what
    // keeps this from becoming a loop: the reserve on the right is at most
    // half a label, and it is covered by the clearance the fit insists on.
    let mut x = x;
    let turned = fit(
        &mut x,
        &chart.axes.x,
        (field.w - gutter.left).max(0.0),
        Side::Along,
        style,
        labels,
    );

    let x_labels: Vec<Label> =
        x.ticks.iter().map(|(_, text)| labels.measure(text, style)).collect();
    let x_title = x.title.as_ref().map(|text| labels.measure(text, style));

    if !x_labels.is_empty() {
        gutter.bottom += tick_length + gap + turned.depth(&x_labels);
        if turned == Turn::Flat && x.domain.reaches_the_edge() {
            gutter.right = gutter
                .right
                .max(x_labels.last().map_or(0.0, |label| label.width / 2.0));
        }
        if turned == Turn::Eighth {
            // A turned label hangs down and to the left of its own mark, so
            // the first one reaches back past the start of the plot. Widening
            // the left gutter here can only narrow the plot, never change the
            // decision that was already taken against a wider one.
            let first = x_labels.first().map_or(0.0, |label| label.width);
            gutter.left = gutter.left.max(first * TURN_COS);
        }
    }

    // The title goes outside the labels, never over them: whatever the labels
    // took is already in the gutter, and the title adds to it.
    if let Some(title) = &x_title {
        gutter.bottom += gap + title.height();
    }

    let plot = Rect::new(
        field.x + gutter.left,
        field.y + gutter.top,
        (field.w - gutter.left - gutter.right).max(0.0),
        (field.h - gutter.top - gutter.bottom).max(0.0),
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
        // The grid first, under everything: it is there to be read past, and
        // a gridline over a bar is a line drawn on the data.
        grid(&mut items, &x, &x_scale, &chart.axes.x, plot, style, true);
        grid(&mut items, &y, &y_scale, &chart.axes.y, plot, style, false);

        // Then the marks, and the axes over them: an axis line under a bar is
        // a line the bar rubs out, and the rule a reader measures against has
        // to be the one they can see.
        items.extend(marks(chart, &drawn, &x_scale, &y_scale, plot, style, &mut issues));

        emit_x(&mut items, &x, &x_scale, &x_labels, x_title, plot, style, labels, tick_length, gap, turned);
        emit_y(&mut items, &y, &y_scale, &y_labels, y_title, plot, style, labels, tick_length, gap);

        if let Some(box_) = &legend {
            // Placed against the plot and not the frame it was measured out
            // of: a legend beside a chart lines up with the drawing, not with
            // the axis furniture below and to the left of it.
            items.extend(box_.emit(chart.mark, plot, frame, style, labels, gap));
        }
    }

    Plotted { plot, x: x_scale, y: y_scale, items, issues }
}

// ─────────────────────────────────────────────────────────────────────────────
// Marks
// ─────────────────────────────────────────────────────────────────────────────

/// One series: the rows that share a colour.
struct Series<'a> {
    /// What the `color` channel called it. `None` when the chart has one
    /// series and nothing to name it after — and one series has no legend, so
    /// there is nothing that would have wanted the name.
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
    series: &[Series<'_>],
    x: &Scale,
    y: &Scale,
    plot: Rect,
    style: &ResolvedStyle,
    issues: &mut Vec<Issue>,
) -> Vec<DisplayItem> {
    match chart.mark {
        Mark::Bar => bars(chart, series, x, y, plot, issues),
        Mark::Line => lines(chart, series, x, y),
        Mark::Point => points(chart, series, x, y, style.font_size * POINT_EM),
        // Área is T4.6. Drawing nothing is the honest state of a mark that
        // has a vocabulary and no geometry yet.
        Mark::Area => Vec::new(),
    }
}

/// One mark per observation, at the crossing of its two values.
///
/// No line joins them and none should: the whole claim a scatter makes is
/// that the readings are independent, and a line through them would assert an
/// order the data does not have.
fn points(
    chart: &ChartFrame,
    series: &[Series<'_>],
    x: &Scale,
    y: &Scale,
    diameter: f64,
) -> Vec<DisplayItem> {
    let mut items = Vec::new();

    for one in series {
        for row in &one.rows {
            let Some(at_x) = position(x, row.get(&chart.encoding.x.field)) else {
                continue;
            };
            let Some(at_y) = position(y, row.get(&chart.encoding.y.field)) else {
                continue;
            };
            items.push(DisplayItem::Ellipse(EllipseItem {
                rect: Rect::new(
                    at_x - diameter / 2.0,
                    at_y - diameter / 2.0,
                    diameter,
                    diameter,
                ),
                fill: Some(one.colour),
                stroke: None,
                source: None,
            }));
        }
    }

    items
}

// ─────────────────────────────────────────────────────────────────────────────
// Legend
// ─────────────────────────────────────────────────────────────────────────────

/// The legend, once it knows what it has to say and how much room that takes.
///
/// Measured before the axes and placed after them, for the same reason the
/// axes are measured before the plot exists: what a thing costs can be known
/// from its text alone, and only where it sits needs the geometry.
struct LegendBox {
    position: LegendPosition,
    entries: Vec<(String, Color, Label)>,
    title: Option<(String, Label)>,
    /// Across the frame — width for a side legend, height for a row.
    thickness: f64,
    /// Baseline-to-baseline within the legend.
    line: f64,
    /// How many rows the entries were broken into. One, for a side legend.
    rows: usize,
    swatch: f64,
}

/// Work out whether there is a legend, and what it costs.
///
/// A legend appears when there are two series or more, unless the author says
/// otherwise. One series needs none — there is a single colour, and the
/// chart's own title already names what is drawn; a box with one swatch
/// restates it and takes room from the drawing.
///
/// For two or more it is not decoration. Three of the eight palette slots sit
/// below 3:1 against white, so a reader who cannot tell two hues apart has
/// nothing else to go on. Identity never rests on colour alone.
fn legend_of(
    chart: &ChartFrame,
    series: &[Series<'_>],
    style: &ResolvedStyle,
    labels: &dyn Labels,
    frame: Rect,
) -> Option<LegendBox> {
    let spec = chart.legend.clone().unwrap_or_default();
    if !spec.visible || series.len() < 2 {
        return None;
    }

    let entries: Vec<(String, Color, Label)> = series
        .iter()
        .map(|one| {
            let name = one.name.clone().unwrap_or_default();
            let measured = labels.measure(&name, style);
            (name, one.colour, measured)
        })
        .collect();
    if entries.is_empty() {
        return None;
    }

    let title = spec
        .title
        .as_ref()
        .filter(|text| !text.is_empty())
        .map(|text| (text.clone(), labels.measure(text, style)));

    let gap = style.font_size * GAP_EM;
    let swatch = style.font_size * SWATCH_EM;
    let line = style.leading();
    let width_of = |label: &Label| swatch + gap + label.width;

    let (thickness, rows) = match spec.position {
        LegendPosition::Right | LegendPosition::Left => {
            let widest = entries
                .iter()
                .map(|(_, _, label)| width_of(label))
                .chain(title.iter().map(|(_, label)| label.width))
                .fold(0.0, f64::max);
            (widest + gap, 1)
        }
        LegendPosition::Top | LegendPosition::Bottom => {
            // Broken into rows against the *frame*, never the plot: the plot
            // is what this measurement decides, and reading it here would
            // close the circle the module keeps open.
            let mut rows = 1usize;
            let mut used = 0.0f64;
            for (_, _, label) in &entries {
                let want = width_of(label);
                if used > 0.0 && used + gap * 2.0 + want > frame.w {
                    rows += 1;
                    used = want;
                } else {
                    used += if used > 0.0 { gap * 2.0 + want } else { want };
                }
            }
            let lines = rows + usize::from(title.is_some());
            (lines as f64 * line + gap, rows)
        }
    };

    Some(LegendBox { position: spec.position, entries, title, thickness, line, rows, swatch })
}

impl LegendBox {
    /// What is left of the frame once the legend has taken its strip.
    fn leaves(&self, frame: Rect) -> Rect {
        let take = self.thickness.min(match self.position {
            LegendPosition::Right | LegendPosition::Left => frame.w,
            LegendPosition::Top | LegendPosition::Bottom => frame.h,
        });
        match self.position {
            LegendPosition::Right => Rect::new(frame.x, frame.y, frame.w - take, frame.h),
            LegendPosition::Left => Rect::new(frame.x + take, frame.y, frame.w - take, frame.h),
            LegendPosition::Top => Rect::new(frame.x, frame.y + take, frame.w, frame.h - take),
            LegendPosition::Bottom => Rect::new(frame.x, frame.y, frame.w, frame.h - take),
        }
    }

    /// The swatch that stands for a mark of this kind.
    ///
    /// Shaped like the thing it names: a block for a bar, a stroke for a
    /// line, a dot for a scatter. A reader should not have to learn that a
    /// square means a line.
    fn key(&self, mark: Mark, colour: Color, x: f64, middle: f64) -> DisplayItem {
        match mark {
            Mark::Line => DisplayItem::Line(LineItem {
                x1: x,
                y1: middle,
                x2: x + self.swatch,
                y2: middle,
                stroke: Stroke { color: colour, width: LINE_WIDTH, dash: None },
                source: None,
            }),
            Mark::Point => {
                let size = self.swatch * 0.8;
                DisplayItem::Ellipse(EllipseItem {
                    rect: Rect::new(
                        x + (self.swatch - size) / 2.0,
                        middle - size / 2.0,
                        size,
                        size,
                    ),
                    fill: Some(colour),
                    stroke: None,
                    source: None,
                })
            }
            Mark::Bar | Mark::Area => DisplayItem::Rect(RectItem {
                rect: Rect::new(x, middle - self.swatch / 2.0, self.swatch, self.swatch),
                radius: 0.0,
                fill: Some(colour),
                stroke: None,
                source: None,
            }),
        }
    }

    fn emit(
        &self,
        mark: Mark,
        plot: Rect,
        frame: Rect,
        style: &ResolvedStyle,
        labels: &dyn Labels,
        gap: f64,
    ) -> Vec<DisplayItem> {
        let mut items = Vec::new();
        // The text of a legend wears the document's ink, never the colour of
        // the series it names: a pale hue that reads as a mark is illegible
        // as type. Identity comes from the swatch beside the words.
        let entry_width = |label: &Label| self.swatch + gap + label.width;

        match self.position {
            LegendPosition::Right | LegendPosition::Left => {
                let lines = self.entries.len() + usize::from(self.title.is_some());
                let height = lines as f64 * self.line;
                let mut top = plot.y + (plot.h - height) / 2.0;
                let left = match self.position {
                    LegendPosition::Right => frame.right() - self.thickness + gap,
                    _ => frame.x,
                };

                if let Some((text, label)) = &self.title {
                    items.extend(labels.draw(text, style, left, top + label.ascent));
                    top += self.line;
                }
                for (name, colour, label) in &self.entries {
                    let middle = top + self.line / 2.0;
                    items.push(self.key(mark, *colour, left, middle));
                    items.extend(labels.draw(
                        name,
                        style,
                        left + self.swatch + gap,
                        middle + (label.ascent - label.descent) / 2.0,
                    ));
                    top += self.line;
                }
            }
            LegendPosition::Top | LegendPosition::Bottom => {
                let lines = self.rows + usize::from(self.title.is_some());
                let mut top = match self.position {
                    LegendPosition::Top => frame.y,
                    _ => frame.bottom() - lines as f64 * self.line,
                };

                if let Some((text, label)) = &self.title {
                    items.extend(labels.draw(text, style, plot.x, top + label.ascent));
                    top += self.line;
                }

                // Broken the same way it was measured, then each row centred
                // on the plot: measuring one way and drawing another is how a
                // legend ends up half off the page.
                for row in self.break_rows(frame.w, gap) {
                    let width: f64 = row
                        .iter()
                        .map(|index| entry_width(&self.entries[*index].2))
                        .sum::<f64>()
                        + gap * 2.0 * (row.len().saturating_sub(1)) as f64;
                    let mut left = plot.x + (plot.w - width) / 2.0;
                    let middle = top + self.line / 2.0;

                    for index in row {
                        let (name, colour, label) = &self.entries[index];
                        items.push(self.key(mark, *colour, left, middle));
                        items.extend(labels.draw(
                            name,
                            style,
                            left + self.swatch + gap,
                            middle + (label.ascent - label.descent) / 2.0,
                        ));
                        left += entry_width(label) + gap * 2.0;
                    }
                    top += self.line;
                }
            }
        }

        items
    }

    /// Which entries fall on which row, by the same arithmetic that counted
    /// the rows in the first place.
    fn break_rows(&self, width: f64, gap: f64) -> Vec<Vec<usize>> {
        let mut out: Vec<Vec<usize>> = vec![Vec::new()];
        let mut used = 0.0f64;
        for (index, (_, _, label)) in self.entries.iter().enumerate() {
            let want = self.swatch + gap + label.width;
            if used > 0.0 && used + gap * 2.0 + want > width {
                out.push(vec![index]);
                used = want;
            } else {
                used += if used > 0.0 { gap * 2.0 + want } else { want };
                out.last_mut().expect("uma linha há sempre").push(index);
            }
        }
        out
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

// ─────────────────────────────────────────────────────────────────────────────
// Making the labels fit
// ─────────────────────────────────────────────────────────────────────────────

/// Whether the labels of an axis are written flat or turned.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Turn {
    Flat,
    /// An eighth of a turn, reading up to the right, with the label's far end
    /// at its own mark.
    Eighth,
}

impl Turn {
    /// How far the labels reach below the axis.
    fn depth(self, measured: &[Label]) -> f64 {
        match self {
            Turn::Flat => measured.iter().map(Label::height).fold(0.0, f64::max),
            // Turned, a label's own length is most of what it costs downward.
            Turn::Eighth => {
                let longest = measured.iter().map(|label| label.width).fold(0.0, f64::max);
                let tallest = measured.iter().map(Label::height).fold(0.0, f64::max);
                longest * TURN_COS + tallest * TURN_COS
            }
        }
    }
}

/// Make an axis's labels fit the room it has, by the three escapes in order.
///
/// **Fewer marks first**, because a mark on a continuum is a sample of it and
/// dropping some loses nothing. A list of names is not a continuum: sampling
/// it would leave bars nobody can identify, so a categorical axis skips this
/// escape entirely. Nor is it taken when the author asked for a count — that
/// was an instruction, not a default.
///
/// **Then a shorter way of writing the number**, because `1200` and `1,2 mil`
/// say the same thing and one of them fits.
///
/// **Turning them is last**, and only because the alternative is worse.
/// Turned type is read slowly, and an axis is meant to be read at a glance.
/// It is also offered to the horizontal axis alone: turning the numbers down
/// the side of a chart makes them no narrower and much harder to read.
fn fit(
    draft: &mut Draft,
    axis: &AxisSpec,
    room: f64,
    side: Side,
    style: &ResolvedStyle,
    labels: &dyn Labels,
) -> Turn {
    if draft.ticks.is_empty() || room <= 0.0 {
        return Turn::Flat;
    }

    let clearance = style.font_size * CLEARANCE_EM;
    let fits = |ticks: &[(Tick, String)]| {
        let measured: Vec<Label> =
            ticks.iter().map(|(_, text)| labels.measure(text, style)).collect();

        // A vertical axis's labels are stacked, so what crowds them is their
        // height — and what spoils the chart is their width, which is checked
        // against the room the drawing would otherwise have.
        if let Side::Across(budget) = side
            && measured.iter().any(|label| label.width > budget)
        {
            return false;
        }

        // Measured against a scale laid on the room itself: where the marks
        // fall relative to one another is all this needs, and that is settled
        // by the domain long before the plot is.
        let probe = draft.domain.scale((0.0, room));
        let placed: Vec<(f64, f64)> = ticks
            .iter()
            .zip(&measured)
            .filter_map(|((tick, _), label)| {
                let extent = match side {
                    Side::Along => label.width,
                    Side::Across(_) => label.height(),
                };
                Some((offset(tick, &probe)?, extent))
            })
            .collect();
        placed.windows(2).all(|pair| {
            (pair[1].0 - pair[0].0).abs() >= (pair[0].1 + pair[1].1) / 2.0 + clearance
        })
    };

    if fits(&draft.ticks) {
        return Turn::Flat;
    }

    // ── One: fewer marks ────────────────────────────────────────────────────
    //
    // A count the author asked for needs no guard here. `marks_of` honours it
    // whatever is wished of it, so every attempt below comes back the same
    // length and none is taken. One place decides what an explicit count
    // means; a second copy of that rule here would be one more to keep in step.
    if matches!(draft.domain, Domain::Continuous { .. }) {
        for want in (2..draft.ticks.len()).rev() {
            let fewer = marks_of(&draft.domain, axis, want);
            if fewer.len() < draft.ticks.len() && fits(&fewer) {
                draft.ticks = fewer;
                return Turn::Flat;
            }
        }
    }

    // ── Two: a shorter format ───────────────────────────────────────────────
    if let Some(shorter) = shorten(&draft.ticks)
        && fits(&shorter)
    {
        draft.ticks = shorter;
        return Turn::Flat;
    }

    // ── Three: turn them ────────────────────────────────────────────────────
    //
    // Only across the foot of a chart. Down its side a turned number is no
    // narrower than a flat one and far slower to read, so a vertical axis
    // that has run out of escapes writes its labels out and lets them be wide.
    match side {
        Side::Along => Turn::Eighth,
        Side::Across(_) => Turn::Flat,
    }
}

/// Which way the axis being fitted runs.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Side {
    /// Across the foot: labels sit next to one another and crowd by width.
    Along,
    /// Down the side: labels stack and crowd by height, and may take no more
    /// than this much of the frame across it.
    Across(f64),
}

/// The same marks, written with a scale word instead of the zeros.
///
/// `None` when the marks are names, or when no scale word shortens them. The
/// divisor is chosen so every mark stays a whole number — `200 mil` reads,
/// `0,2 mi` does not — and zero is written plain, because "0 mil" is not
/// something anybody writes.
fn shorten(ticks: &[(Tick, String)]) -> Option<Vec<(Tick, String)>> {
    let values: Vec<f64> = ticks
        .iter()
        .map(|(tick, _)| match tick {
            Tick::At(value) => Some(*value),
            Tick::In(_) => None,
        })
        .collect::<Option<Vec<f64>>>()?;

    let mut chosen: Option<(f64, &str)> = None;
    for (divisor, word) in [(1e9, "bi"), (1e6, "mi"), (1e3, "mil")] {
        if values
            .iter()
            .all(|value| *value == 0.0 || (value / divisor).abs() >= 1.0)
        {
            chosen = Some((divisor, word));
            break;
        }
    }
    let (divisor, word) = chosen?;

    let scaled: Vec<f64> = values.iter().map(|value| value / divisor).collect();
    let written = label_numbers(&scaled);
    Some(
        ticks
            .iter()
            .zip(values)
            .zip(written)
            .map(|(((tick, _), value), text)| {
                let tick = match tick {
                    Tick::At(at) => Tick::At(*at),
                    Tick::In(name) => Tick::In(name.clone()),
                };
                (tick, if value == 0.0 { "0".to_string() } else { format!("{text} {word}") })
            })
            .collect(),
    )
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
    turn: Turn,
) {
    if !axis.visible {
        return;
    }

    let baseline_y = plot.bottom();
    items.push(rule(plot.x, baseline_y, plot.right(), baseline_y, style));

    let top = baseline_y + tick_length + gap;
    for ((tick, text), label) in axis.ticks.iter().zip(measured) {
        let Some(at) = offset(tick, scale) else { continue };
        items.push(rule(at, baseline_y, at, baseline_y + tick_length, style));

        match turn {
            Turn::Flat => items.extend(labels.draw(
                text,
                style,
                at - label.width / 2.0,
                top + label.ascent,
            )),
            Turn::Eighth => {
                // An eighth of a turn, reading up to the right, with the far
                // end of the label at its own mark. Drawn from the origin of
                // a group whose matrix carries it there: a point `(u, v)`
                // lands at `(cu + cv + e, -cu + cv + f)` for `c` the cosine
                // of the turn, so the run's far end — `(width, 0)` — sits at
                // `(c·width + e, -c·width + f)`, and that is what is pinned
                // to the mark.
                let c = TURN_COS;
                let e = at - c * label.width;
                let f = top + c * label.width + label.ascent * c;
                items.push(DisplayItem::Group(DisplayGroup {
                    transform: Some([c, -c, c, c, e, f]),
                    items: labels.draw(text, style, 0.0, 0.0),
                    ..DisplayGroup::new()
                }));
            }
        }
    }

    if let (Some(title), Some(text)) = (title, axis.title.as_ref()) {
        let below =
            if measured.is_empty() { 0.0 } else { tick_length + gap + turn.depth(measured) };
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

/// Rules across the plot, one at every mark of an axis that asked for them.
///
/// Honoured on whichever axis declares it, categorical included: a grid
/// through the middle of a band is not what most charts want, but it is what
/// the author asked for, and second-guessing a declaration is how a document
/// stops meaning what it says.
#[allow(clippy::too_many_arguments)]
fn grid(
    items: &mut Vec<DisplayItem>,
    axis: &Draft,
    scale: &Scale,
    spec: &AxisSpec,
    plot: Rect,
    style: &ResolvedStyle,
    vertical: bool,
) {
    if !spec.grid || !axis.visible {
        return;
    }

    let mut colour = style.color;
    colour.a *= GRID_ALPHA;

    for (tick, _) in &axis.ticks {
        let Some(at) = offset(tick, scale) else { continue };
        let (x1, y1, x2, y2) = if vertical {
            (at, plot.y, at, plot.bottom())
        } else {
            (plot.x, at, plot.right(), at)
        };
        items.push(DisplayItem::Line(LineItem {
            x1,
            y1,
            x2,
            y2,
            stroke: Stroke { color: colour, width: AXIS_WIDTH, dash: None },
            source: None,
        }));
    }
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

    /// True when a rectangle's middle falls inside the drawing area.
    ///
    /// What tells a bar from a legend's swatch. Both are filled rectangles of
    /// a series colour, and counting them together was how the first version
    /// of these helpers reported nine bars for a chart of six.
    fn inside(plot: Rect, rect: Rect) -> bool {
        plot.contains(rect.x + rect.w / 2.0, rect.y + rect.h / 2.0)
    }

    fn bars_of(out: &Plotted) -> Vec<RectItem> {
        out.items
            .iter()
            .filter_map(|item| match item {
                DisplayItem::Rect(rect) if inside(out.plot, rect.rect) => Some(rect.clone()),
                _ => None,
            })
            .collect()
    }

    fn dots_of(out: &Plotted) -> Vec<EllipseItem> {
        out.items
            .iter()
            .filter_map(|item| match item {
                DisplayItem::Ellipse(dot) if inside(out.plot, dot.rect) => Some(dot.clone()),
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
        let bars = bars_of(&out);
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
        let bars = bars_of(&out);
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
        let bars = bars_of(&out);
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

        let bars = bars_of(&out);
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

        let bars = bars_of(&out);
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
        let bars = bars_of(&out);
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
        let bars = bars_of(&out);
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
        assert!(bars_of(&out).is_empty(), "e não se adivinha uma barra");
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
        assert_eq!(bars_of(&out).len(), 9, "e desenha as nove à mesma");
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
        assert_eq!(bars_of(&out).len(), 2);
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

    // ── Dispersão ───────────────────────────────────────────────────────────

    /// A scatter of two numeric fields, coloured by `serie` when asked.
    fn scatter(coloured: bool) -> ChartFrame {
        ChartFrame {
            mark: Mark::Point,
            encoding: Encoding {
                x: Channel { field: "t".into(), ..Channel::default() },
                y: Channel { field: "v".into(), ..Channel::default() },
                color: coloured.then(|| Channel {
                    field: "serie".into(),
                    kind: Some(FieldKind::Categorical),
                    ..Channel::default()
                }),
            },
            axes: bare(),
            ..ChartFrame::default()
        }
    }

    fn readings(triples: &[(f64, &str, f64)]) -> Vec<Row> {
        triples
            .iter()
            .map(|(t, series, v)| {
                Row::from([
                    ("t".to_string(), Value::Number(*t)),
                    ("serie".to_string(), Value::Text(series.to_string())),
                    ("v".to_string(), Value::Number(*v)),
                ])
            })
            .collect()
    }

    /// The `color` channel every multi-series test splits on.
    fn by_series() -> Channel {
        Channel {
            field: "serie".into(),
            kind: Some(FieldKind::Categorical),
            ..Channel::default()
        }
    }

    #[test]
    fn a_scatter_puts_one_mark_where_its_two_values_cross() {
        let data = readings(&[(10.0, "a", 1.0), (20.0, "a", 5.0), (30.0, "a", 3.0)]);
        let out = plot(&scatter(false), &data, &style(), &Ruler, FRAME);

        let dots = dots_of(&out);
        assert_eq!(dots.len(), 3, "uma marca por observação");
        for (dot, (t, v)) in dots.iter().zip([(10.0, 1.0), (20.0, 5.0), (30.0, 3.0)]) {
            let centre = (dot.rect.x + dot.rect.w / 2.0, dot.rect.y + dot.rect.h / 2.0);
            assert!((centre.0 - out.x.map(t).unwrap()).abs() < 1e-9, "{dot:?}");
            assert!((centre.1 - out.y.map(v).unwrap()).abs() < 1e-9, "{dot:?}");
            assert!((dot.rect.w - dot.rect.h).abs() < 1e-9, "redonda, não oval");
        }
        assert!(
            paths_of(&out.items).is_empty(),
            "e nada as liga: uma dispersão afirma que as leituras são independentes",
        );
    }

    #[test]
    fn a_scatter_axis_is_not_dragged_down_to_zero() {
        // Only a bar and an area measure from a baseline. A scatter of
        // readings between 80 and 90 that reached zero would spend nine
        // tenths of its height on emptiness.
        let data = readings(&[(1.0, "a", 80.0), (2.0, "a", 90.0)]);
        let out = plot(&scatter(false), &data, &style(), &Ruler, FRAME);
        match out.y {
            Scale::Linear { domain, .. } => assert!(domain.0 > 0.0, "veio {domain:?}"),
            _ => panic!("escala contínua"),
        }
    }

    #[test]
    fn a_hole_is_a_mark_not_drawn_and_never_a_mark_at_zero() {
        let data = vec![
            Row::from([("t".to_string(), Value::Number(1.0)), ("v".to_string(), Value::Number(5.0))]),
            Row::from([("t".to_string(), Value::Number(2.0)), ("v".to_string(), Value::Null)]),
        ];
        let out = plot(&scatter(false), &data, &style(), &Ruler, FRAME);
        assert_eq!(dots_of(&out).len(), 1);
    }

    // ── Legenda ─────────────────────────────────────────────────────────────

    /// Text the chart wrote outside the drawing area — which is the legend.
    fn outside_text(out: &Plotted) -> Vec<String> {
        runs(&out.items)
            .into_iter()
            .filter(|run| !inside(out.plot, Rect::new(run.x, run.y, run.width.max(1.0), 1.0)))
            .map(|run| run.text)
            .collect()
    }

    fn named(out: &Plotted, names: &[&str]) -> Vec<String> {
        outside_text(out).into_iter().filter(|t| names.contains(&t.as_str())).collect()
    }

    /// Two series of bars, with the legend the caller wants.
    fn two_series(legend: Option<crate::spec::chart::Legend>) -> Plotted {
        let mut spec = chart(bare());
        spec.encoding.color = Some(by_series());
        spec.legend = legend;
        plot(
            &spec,
            &split(&[("jan", "norte", 10.0), ("jan", "sul", 20.0)]),
            &style(),
            &Ruler,
            FRAME,
        )
    }

    #[test]
    fn one_series_gets_no_legend_at_all() {
        // There is one colour, and the chart's own title already names what
        // is drawn. A box with a single swatch restates it and takes room
        // from the drawing.
        let out = plot(&chart(bare()), &rows(&[("jan", 5.0), ("fev", 9.0)]), &style(), &Ruler, FRAME);
        assert_eq!(out.plot.right(), FRAME.right(), "nada reservado à direita");
        // The month names are outside the plot too — they are the axis. What
        // must not exist is a swatch, which is the one thing only a legend
        // draws.
        assert!(
            !out.items
                .iter()
                .any(|item| matches!(item, DisplayItem::Rect(r) if !inside(out.plot, r.rect))),
            "nenhuma tarja de legenda",
        );
    }

    #[test]
    fn two_series_get_a_legend_without_being_asked() {
        let out = two_series(None);
        assert_eq!(named(&out, &["norte", "sul"]), vec!["norte".to_string(), "sul".to_string()]);

        // And it costs room: the drawing gives way to it rather than being
        // drawn over.
        let sozinho = plot(&chart(bare()), &rows(&[("jan", 10.0)]), &style(), &Ruler, FRAME);
        assert!(
            out.plot.right() < sozinho.plot.right(),
            "a área de desenho encolhe para a legenda caber: {} vs {}",
            out.plot.right(),
            sozinho.plot.right(),
        );
    }

    #[test]
    fn the_legend_turns_off_only_when_told_to() {
        let out = two_series(Some(crate::spec::chart::Legend {
            visible: false,
            ..Default::default()
        }));
        assert!(named(&out, &["norte", "sul"]).is_empty(), "desligada, não aparece");
        assert_eq!(out.plot.right(), FRAME.right(), "e não reserva nada");
    }

    #[test]
    fn a_legend_beside_takes_width_and_a_legend_below_takes_height() {
        let at = |position: LegendPosition| {
            two_series(Some(crate::spec::chart::Legend { position, ..Default::default() }))
        };

        let direita = at(LegendPosition::Right);
        let esquerda = at(LegendPosition::Left);
        let baixo = at(LegendPosition::Bottom);
        let cima = at(LegendPosition::Top);
        let nenhuma = two_series(Some(crate::spec::chart::Legend {
            visible: false,
            ..Default::default()
        }));

        assert!(direita.plot.right() < nenhuma.plot.right(), "à direita tira largura");
        assert_eq!(direita.plot.h, nenhuma.plot.h, "e não tira altura");

        assert!(esquerda.plot.x > nenhuma.plot.x, "à esquerda tira do outro lado");
        assert!((esquerda.plot.w - direita.plot.w).abs() < 1e-9, "e custa o mesmo");

        assert!(baixo.plot.bottom() < nenhuma.plot.bottom(), "em baixo tira altura");
        assert!((baixo.plot.w - nenhuma.plot.w).abs() < 1e-9, "e devolve a largura");

        assert!(cima.plot.y > nenhuma.plot.y, "em cima tira do topo");
        assert!((cima.plot.h - baixo.plot.h).abs() < 1e-9, "pelo mesmo preço");
    }

    #[test]
    fn the_swatch_is_shaped_like_the_mark_it_names() {
        // A block for a bar, a stroke for a line, a dot for a scatter. Nobody
        // should have to learn that a square stands for a line.
        let build = |mark: Mark| {
            let mut spec = chart(bare());
            spec.mark = mark;
            spec.encoding.color = Some(by_series());
            let data = split(&[("jan", "norte", 10.0), ("fev", "sul", 20.0)]);
            plot(&spec, &data, &style(), &Ruler, FRAME)
        };

        let barras = build(Mark::Bar);
        let tarjas = barras
            .items
            .iter()
            .filter(|item| matches!(item, DisplayItem::Rect(r) if !inside(barras.plot, r.rect)))
            .count();
        assert_eq!(tarjas, 2, "duas tarjas quadradas para duas séries");

        let linhas = build(Mark::Line);
        let chaves = linhas
            .items
            .iter()
            .filter(|item| matches!(item, DisplayItem::Line(l) if l.x1 > linhas.plot.right()))
            .count();
        assert_eq!(chaves, 2, "duas chaves em traço");

        let pontos = build(Mark::Point);
        let bolas = pontos
            .items
            .iter()
            .filter(|item| matches!(item, DisplayItem::Ellipse(e) if !inside(pontos.plot, e.rect)))
            .count();
        assert_eq!(bolas, 2, "duas chaves redondas");
    }

    #[test]
    fn a_row_legend_is_broken_the_way_it_was_measured() {
        // Eight long names cannot sit on one row of a 400pt frame. What the
        // reserve counted and what the drawing lays out have to be the same
        // count, or the legend runs off the frame it was measured against.
        let mut spec = chart(bare());
        spec.encoding.color = Some(by_series());
        spec.legend = Some(crate::spec::chart::Legend {
            position: LegendPosition::Bottom,
            ..Default::default()
        });

        let names = [
            "regiao-norte", "regiao-sul", "regiao-leste", "regiao-oeste",
            "regiao-centro", "regiao-litoral", "regiao-serra", "regiao-vale",
        ];
        let data: Vec<Row> = names
            .iter()
            .map(|name| {
                Row::from([
                    ("mes".to_string(), Value::Text("jan".to_string())),
                    ("serie".to_string(), Value::Text((*name).to_string())),
                    ("v".to_string(), Value::Number(1.0)),
                ])
            })
            .collect();

        let out = plot(&spec, &data, &style(), &Ruler, FRAME);
        let written = named(&out, &names);
        assert_eq!(written.len(), 8, "todas as séries são nomeadas: {written:?}");

        for run in runs(&out.items).iter().filter(|r| names.contains(&r.text.as_str())) {
            assert!(
                run.x >= FRAME.x - 1e-9 && run.x + run.width <= FRAME.right() + 1e-9,
                "`{}` sai da moldura: {} a {}",
                run.text,
                run.x,
                run.x + run.width,
            );
            // The reserve and the drawing have to agree on how many rows
            // there are. Counting one way and laying out another puts the
            // last row past the foot of the frame or over the drawing, and
            // both read as the legend simply being in the wrong place.
            assert!(
                run.y <= FRAME.bottom() + 1e-9,
                "`{}` cai abaixo do pé da moldura: {} > {}",
                run.text,
                run.y,
                FRAME.bottom(),
            );
            assert!(
                run.y > out.plot.bottom(),
                "`{}` cai sobre a área de desenho: {} <= {}",
                run.text,
                run.y,
                out.plot.bottom(),
            );
        }

        // More than one row, and the rows are distinct baselines.
        let baselines: Vec<f64> = runs(&out.items)
            .iter()
            .filter(|r| names.contains(&r.text.as_str()))
            .map(|r| r.y)
            .collect();
        let mut distinct = baselines.clone();
        distinct.sort_by(f64::total_cmp);
        distinct.dedup_by(|a, b| (*a - *b).abs() < 1e-6);
        assert!(distinct.len() > 1, "oito nomes longos não cabem numa linha: {baselines:?}");
    }

    // ── Grelha ──────────────────────────────────────────────────────────────

    /// Lines that cross the whole plot, which is what a gridline is.
    fn gridlines(out: &Plotted) -> Vec<LineItem> {
        out.items
            .iter()
            .filter_map(|item| match item {
                DisplayItem::Line(line) => {
                    let across = (line.x1 - out.plot.x).abs() < 1e-9
                        && (line.x2 - out.plot.right()).abs() < 1e-9;
                    let down = (line.y1 - out.plot.y).abs() < 1e-9
                        && (line.y2 - out.plot.bottom()).abs() < 1e-9;
                    (across || down).then(|| line.clone())
                }
                _ => None,
            })
            .collect()
    }

    #[test]
    fn no_grid_unless_it_was_asked_for() {
        let out = plot(&chart(bare()), &rows(&[("jan", 5.0), ("fev", 9.0)]), &style(), &Ruler, FRAME);
        // The axis lines themselves span the plot, so what is counted is a
        // line that is neither of them.
        let extra = gridlines(&out).len();
        assert_eq!(extra, 2, "só os dois eixos: {extra}");
    }

    #[test]
    fn a_grid_is_one_line_per_mark_and_a_wash_of_the_ink() {
        let mut axes = bare();
        axes.y.grid = true;
        let out = plot(&chart(axes), &rows(&[("jan", 5.0), ("fev", 9.0)]), &style(), &Ruler, FRAME);

        let faint: Vec<LineItem> = gridlines(&out)
            .into_iter()
            .filter(|line| line.stroke.color.a < style().color.a)
            .collect();

        // One per mark of the axis that asked, and the marks are whatever the
        // fitting left — read off the chart rather than guessed at here.
        let marks = runs(&out.items)
            .into_iter()
            .filter(|run| run.x < out.plot.x && run.text.parse::<f64>().is_ok())
            .count();
        assert!(marks > 1, "há marcas no eixo vertical: {marks}");
        assert_eq!(faint.len(), marks, "uma linha de grelha por marca");

        for line in &faint {
            assert!(
                (line.x1 - out.plot.x).abs() < 1e-9 && (line.x2 - out.plot.right()).abs() < 1e-9,
                "a grelha do eixo vertical atravessa o desenho: {line:?}",
            );
            assert!(line.stroke.color.a < 0.3, "e é um lavado, não tinta: {}", line.stroke.color.a);
        }
    }

    #[test]
    fn the_grid_goes_under_the_marks_and_not_over_them() {
        let mut axes = bare();
        axes.y.grid = true;
        let out = plot(&chart(axes), &rows(&[("jan", 5.0)]), &style(), &Ruler, FRAME);

        let first_grid = out
            .items
            .iter()
            .position(|item| {
                matches!(item, DisplayItem::Line(l) if l.stroke.color.a < style().color.a)
            })
            .expect("uma linha de grelha");
        let first_bar = out
            .items
            .iter()
            .position(|item| matches!(item, DisplayItem::Rect(_)))
            .expect("uma barra");
        assert!(first_grid < first_bar, "a grelha lê-se por trás, não por cima");
    }

    // ── O rótulo que não cabe ───────────────────────────────────────────────

    /// A continuous horizontal axis over `span`, in a frame `wide` points wide.
    fn wide_axis(span: f64, wide: f64) -> Plotted {
        let mut spec = chart(bare());
        spec.mark = Mark::Line;
        spec.encoding.x = Channel { field: "t".into(), ..Channel::default() };
        let data: Vec<Row> = [0.0, span]
            .iter()
            .map(|t| {
                Row::from([
                    ("t".to_string(), Value::Number(*t)),
                    ("v".to_string(), Value::Number(1.0)),
                ])
            })
            .collect();
        plot(&spec, &data, &style(), &Ruler, Rect::new(0.0, 0.0, wide, 200.0))
    }

    #[test]
    fn a_crowded_numeric_axis_drops_marks_before_anything_else() {
        // A mark on a continuum is a sample of it, so dropping some loses
        // nothing. Asked of `fit` directly, and of one axis rather than two
        // frames: the first version of this test compared a narrow chart
        // against a wide one, and those get different tick counts from the
        // frame alone — so it passed with the escape taken out altogether.
        let ticks = |axis: &AxisSpec, room: f64| {
            let data: Vec<Row> = [0.0, 1000.0]
                .iter()
                .map(|t| {
                    Row::from([
                        ("t".to_string(), Value::Number(*t)),
                        ("v".to_string(), Value::Number(1.0)),
                    ])
                })
                .collect();
            let channel = Channel { field: "t".into(), ..Channel::default() };
            let mut drafted =
                draft(&channel, axis, &data, Mark::Line, 12, "x", &mut Vec::new());
            let before = drafted.ticks.len();
            let turn = fit(&mut drafted, axis, room, Side::Along, &style(), &Ruler);
            (before, drafted.ticks.len(), turn)
        };

        let axis = AxisSpec { title: Some(String::new()), ..AxisSpec::default() };
        let (before, after, turn) = ticks(&axis, 90.0);
        assert!(before > 6, "há marcas a mais para o espaço dado: {before}");
        assert_eq!(turn, Turn::Flat, "menos marcas resolve, sem virar nada");
        assert!(after < before, "{after} contra {before}");
        assert!(after >= 2, "mas nunca abaixo de duas");
    }

    #[test]
    fn a_count_the_author_asked_for_is_an_instruction_and_not_a_default() {
        // The first escape revises a default. An explicit count is not a
        // default, so it survives even when it crowds — and what gives way
        // instead is the last escape.
        let data: Vec<Row> = [0.0, 1000.0]
            .iter()
            .map(|t| {
                Row::from([
                    ("t".to_string(), Value::Number(*t)),
                    ("v".to_string(), Value::Number(1.0)),
                ])
            })
            .collect();
        let channel = Channel { field: "t".into(), ..Channel::default() };
        let axis =
            AxisSpec { title: Some(String::new()), ticks: Some(12), ..AxisSpec::default() };

        let mut drafted = draft(&channel, &axis, &data, Mark::Line, 12, "x", &mut Vec::new());
        let before = drafted.ticks.len();
        let turn = fit(&mut drafted, &axis, 60.0, Side::Along, &style(), &Ruler);
        assert_eq!(drafted.ticks.len(), before, "as marcas pedidas ficam todas");
        assert_eq!(turn, Turn::Eighth, "e o que cede é a orientação");
    }

    #[test]
    fn a_shorter_number_is_tried_before_the_labels_are_turned() {
        // Where fewer marks is not on the table — the author fixed the count
        // — a shorter way of writing the same number is what stands between
        // the axis and turned type. Written out, the three marks need 107
        // points to sit side by side; written short they need 77, and the
        // axis below has 90.
        let data: Vec<Row> = [0.0, 1e9]
            .iter()
            .map(|t| {
                Row::from([
                    ("t".to_string(), Value::Number(*t)),
                    ("v".to_string(), Value::Number(1.0)),
                ])
            })
            .collect();
        let channel = Channel { field: "t".into(), ..Channel::default() };
        let axis =
            AxisSpec { title: Some(String::new()), ticks: Some(2), ..AxisSpec::default() };

        let mut drafted = draft(&channel, &axis, &data, Mark::Line, 2, "x", &mut Vec::new());
        assert_eq!(
            drafted.ticks.iter().map(|(_, t)| t.clone()).collect::<Vec<_>>(),
            vec!["0", "500000000", "1000000000"],
            "escritos por extenso, não cabem",
        );

        let turn = fit(&mut drafted, &axis, 90.0, Side::Along, &style(), &Ruler);
        assert_eq!(turn, Turn::Flat, "a forma curta resolve, sem virar nada");
        assert_eq!(
            drafted.ticks.iter().map(|(_, t)| t.clone()).collect::<Vec<_>>(),
            vec!["0", "500 mi", "1000 mi"],
        );
    }

    #[test]
    fn the_labels_that_survive_never_touch_each_other() {
        for wide in [120.0, 200.0, 340.0, 500.0] {
            let out = wide_axis(1_000_000.0, wide);
            let mut placed: Vec<(f64, f64)> = runs(&out.items)
                .into_iter()
                .filter(|run| run.y > out.plot.bottom())
                .map(|run| (run.x, run.width))
                .collect();
            placed.sort_by(|a, b| a.0.total_cmp(&b.0));
            for pair in placed.windows(2) {
                assert!(
                    pair[0].0 + pair[0].1 <= pair[1].0 + 1e-9,
                    "rótulos sobrepostos numa moldura de {wide}: {placed:?}",
                );
            }
        }
    }

    #[test]
    fn a_number_too_long_is_written_shorter_before_it_is_turned() {
        // `1200000` will not fit five times across a narrow axis; `1,2 mi`
        // says exactly the same and does.
        assert_eq!(
            shorten(&[
                (Tick::At(0.0), "0".into()),
                (Tick::At(500_000.0), "500000".into()),
                (Tick::At(1_000_000.0), "1000000".into()),
            ])
            .expect("encurta")
            .into_iter()
            .map(|(_, text)| text)
            .collect::<Vec<_>>(),
            vec!["0".to_string(), "500 mil".to_string(), "1000 mil".to_string()],
        );

        // The divisor keeps every mark a whole number: `0,2 mi` reads worse
        // than `200 mil`, so `mil` is the one chosen.
        assert!(
            shorten(&[(Tick::At(0.0), "0".into()), (Tick::At(200.0), "200".into())]).is_none(),
            "nada a encurtar abaixo do milhar",
        );
        assert!(
            shorten(&[(Tick::In("jan".into()), "jan".into())]).is_none(),
            "um nome não tem forma curta",
        );
    }

    #[test]
    fn names_are_turned_rather_than_thinned_out() {
        // A list of names is not a continuum: dropping every other one leaves
        // bars nobody can identify. So the escape a categorical axis takes is
        // the last one, and it takes it rather than losing a name.
        let names = [
            "Fotossíntese", "Respiração", "Transpiração", "Germinação",
            "Polinização", "Frutificação", "Senescência", "Dormência",
        ];
        let data: Vec<Row> = names
            .iter()
            .enumerate()
            .map(|(index, name)| {
                Row::from([
                    ("mes".to_string(), Value::Text((*name).to_string())),
                    ("v".to_string(), Value::Number(index as f64 + 1.0)),
                ])
            })
            .collect();

        let out = plot(&chart(bare()), &data, &style(), &Ruler, Rect::new(0.0, 0.0, 260.0, 200.0));

        let written: Vec<String> = runs(&out.items)
            .into_iter()
            .map(|run| run.text)
            .filter(|text| names.contains(&text.as_str()))
            .collect();
        assert_eq!(written.len(), 8, "nenhum nome se perde: {written:?}");

        // Turned, which means each one lives inside a group with a matrix.
        let turned = out
            .items
            .iter()
            .filter(|item| matches!(item, DisplayItem::Group(g) if g.transform.is_some()))
            .count();
        assert_eq!(turned, 8, "e todos virados");
    }

    #[test]
    fn a_turned_label_ends_at_its_own_mark() {
        let names = ["Fotossíntese", "Respiração", "Transpiração", "Germinação"];
        let data: Vec<Row> = names
            .iter()
            .enumerate()
            .map(|(index, name)| {
                Row::from([
                    ("mes".to_string(), Value::Text((*name).to_string())),
                    ("v".to_string(), Value::Number(index as f64 + 1.0)),
                ])
            })
            .collect();
        let out = plot(&chart(bare()), &data, &style(), &Ruler, Rect::new(0.0, 0.0, 150.0, 200.0));

        for item in &out.items {
            let DisplayItem::Group(group) = item else { continue };
            let Some([a, b, c, d, e, f]) = group.transform else { continue };
            let run = runs(&group.items).into_iter().next().expect("texto");
            if !names.contains(&run.text.as_str()) {
                continue;
            }

            // The far end of the run, carried through the matrix, is what sits
            // at the mark. Where the mark is comes from the scale, so this
            // checks the two against each other rather than against a number.
            let far = (a * run.width + c * 0.0 + e, b * run.width + d * 0.0 + f);
            let at = out.x.map_category(&run.text).expect("categoria")
                + out.x.bandwidth() / 2.0;
            assert!(
                (far.0 - at).abs() < 1e-6,
                "`{}` acaba em {} e a marca está em {at}",
                run.text,
                far.0,
            );
            assert!(far.1 > out.plot.bottom(), "e abaixo do eixo");
        }
    }

    #[test]
    fn turned_labels_stay_inside_the_frame_they_were_measured_for() {
        let names = ["Fotossíntese", "Respiração", "Transpiração", "Germinação"];
        let data: Vec<Row> = names
            .iter()
            .enumerate()
            .map(|(index, name)| {
                Row::from([
                    ("mes".to_string(), Value::Text((*name).to_string())),
                    ("v".to_string(), Value::Number(index as f64 + 1.0)),
                ])
            })
            .collect();
        let frame = Rect::new(0.0, 0.0, 150.0, 200.0);
        let out = plot(&chart(bare()), &data, &style(), &Ruler, frame);

        for item in &out.items {
            let DisplayItem::Group(group) = item else { continue };
            let Some([a, b, c, d, e, f]) = group.transform else { continue };
            let run = runs(&group.items).into_iter().next().expect("texto");
            if !names.contains(&run.text.as_str()) {
                continue;
            }
            // Both ends of the turned run, in page coordinates.
            for u in [0.0, run.width] {
                let (x, y) = (a * u + c * 0.0 + e, b * u + d * 0.0 + f);
                assert!(
                    x >= frame.x - 1e-6 && x <= frame.right() + 1e-6,
                    "`{}` sai pela lateral: {x} fora de {frame:?}",
                    run.text,
                );
                assert!(
                    y <= frame.bottom() + 1e-6,
                    "`{}` cai abaixo do pé: {y} > {}",
                    run.text,
                    frame.bottom(),
                );
            }
        }
    }

    #[test]
    fn a_vertical_axis_of_huge_numbers_is_shortened_rather_than_left_wide() {
        // Down the side, labels never collide — they are stacked. What goes
        // wrong there is width: written out, `1000000000` spent a fifth of a
        // small chart saying what `1 bi` says, and the drawing paid for it.
        let mut spec = chart(bare());
        spec.mark = Mark::Line;
        spec.encoding.x = Channel { field: "t".into(), ..Channel::default() };
        let data: Vec<Row> = [(0.0, 0.0), (100.0, 1e9)]
            .iter()
            .map(|(t, v)| {
                Row::from([
                    ("t".to_string(), Value::Number(*t)),
                    ("v".to_string(), Value::Number(*v)),
                ])
            })
            .collect();

        let frame = Rect::new(0.0, 0.0, 235.0, 175.0);
        let out = plot(&spec, &data, &style(), &Ruler, frame);

        let side: Vec<String> = runs(&out.items)
            .into_iter()
            .filter(|run| run.x < out.plot.x)
            .map(|run| run.text)
            .collect();
        assert!(
            side.iter().any(|text| text.ends_with("mi") || text.ends_with("bi")),
            "os números do lado vêm em forma curta: {side:?}",
        );
        assert!(
            side.iter().all(|text| text.len() <= 8),
            "e nenhum fica por extenso: {side:?}",
        );
        assert!(
            out.plot.x <= frame.w * SIDE_SHARE,
            "a margem esquerda cabe no quarto que lhe é dado: {} de {}",
            out.plot.x,
            frame.w,
        );
    }

    #[test]
    fn a_vertical_axis_is_never_turned() {
        // Turned, a name down the side is no narrower and much slower to
        // read. When the escapes run out it is written out and left wide.
        let names: Vec<String> = (0..6).map(|i| format!("categoria-longa-{i}")).collect();
        let data: Vec<Row> = names
            .iter()
            .enumerate()
            .map(|(index, name)| {
                Row::from([
                    ("v".to_string(), Value::Number(index as f64 + 1.0)),
                    ("mes".to_string(), Value::Text(name.clone())),
                ])
            })
            .collect();

        let mut spec = chart(bare());
        spec.encoding.x = Channel { field: "v".into(), ..Channel::default() };
        spec.encoding.y = Channel {
            field: "mes".into(),
            kind: Some(FieldKind::Categorical),
            ..Channel::default()
        };

        let out = plot(&spec, &data, &style(), &Ruler, Rect::new(0.0, 0.0, 200.0, 160.0));
        let written: Vec<String> = runs(&out.items).into_iter().map(|r| r.text).collect();
        for name in &names {
            assert!(written.contains(name), "`{name}` continua escrito: {written:?}");
        }
        assert!(
            !out.items
                .iter()
                .any(|item| matches!(item, DisplayItem::Group(g) if g.transform.is_some_and(
                    |m| m[0] != 0.0
                ))),
            "e nenhum de esguelha",
        );
    }

    #[test]
    fn an_axis_that_fits_is_left_alone() {
        // The escapes are for axes that need them. Four short names across a
        // wide frame need none, and taking one anyway would turn type nobody
        // has to read slowly.
        let out = plot(
            &chart(bare()),
            &rows(&[("jan", 1.0), ("fev", 2.0), ("mar", 3.0), ("abr", 4.0)]),
            &style(),
            &Ruler,
            FRAME,
        );
        assert!(
            !out.items
                .iter()
                .any(|item| matches!(item, DisplayItem::Group(g) if g.transform.is_some_and(
                    |m| m[0] != 0.0
                ))),
            "nada virado de esguelha",
        );
        let written: Vec<String> = runs(&out.items).into_iter().map(|r| r.text).collect();
        for name in ["jan", "fev", "mar", "abr"] {
            assert!(written.contains(&name.to_string()), "`{name}` continua lá: {written:?}");
        }
    }
}
