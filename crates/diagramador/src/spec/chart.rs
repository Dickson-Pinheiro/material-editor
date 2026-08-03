//! What a chart is, as the author writes it.
//!
//! A frame rather than a block: a chart has a size of its own and is placed,
//! the way a photograph is, not stacked in a column of text.
//!
//! The shape is Vega-Lite's, minus everything this engine will not honour.
//! Data are rows of named values; an *encoding* says which field drives which
//! visual channel; the field's kind picks the scale unless the author says
//! otherwise. That last part is the whole ergonomic argument for the grammar:
//! `{"field": "mes", "kind": "categorical"}` on an axis is a band scale
//! without anyone having to say the words "band scale".
//!
//! Nothing here computes anything. It is the vocabulary; `layout/chart.rs`
//! is what reads it.

use serde::{Deserialize, Serialize};

use super::style::Style;
use crate::color::Color;

/// One cell of data.
///
/// Untagged, so a row is written the way anyone would write it: `120` is a
/// number, `"jan"` is text, `null` is a hole. Holes are kept rather than
/// rejected — real data has them, and a chart that refuses to load because one
/// month is missing is less useful than one that draws the other eleven.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Value {
    Number(f64),
    Text(String),
    Null,
}

impl Value {
    pub fn as_number(&self) -> Option<f64> {
        match self {
            Value::Number(number) => Some(*number),
            _ => None,
        }
    }

    /// The value as a category label. A number can be one; a hole cannot.
    pub fn as_category(&self) -> Option<String> {
        match self {
            Value::Text(text) => Some(text.clone()),
            Value::Number(number) => Some(format!("{number}")),
            Value::Null => None,
        }
    }

    pub fn is_null(&self) -> bool {
        matches!(self, Value::Null)
    }
}

/// One observation: a field name to a value.
///
/// A map rather than a positional row, and rows rather than columns, because
/// that is how a person writes data by hand — and hand-written data is what a
/// teaching document has. Column-oriented storage would be denser and would
/// make every fixture unreadable.
pub type Row = std::collections::BTreeMap<String, Value>;

/// How the data are drawn.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Mark {
    #[default]
    Bar,
    Line,
    Area,
    Point,
}

/// What a field means, which is what decides its scale.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum FieldKind {
    /// A number, on a continuous scale.
    #[default]
    Quantitative,
    /// A name, one band or one position each.
    Categorical,
}

/// Which scale to build, when the field's kind is not the answer wanted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ScaleKind {
    Linear,
    Log,
    Band,
    Point,
}

/// The author overriding what the field's kind would have chosen.
///
/// Every field optional, and an absent one means "decide for me". A chart that
/// has to state its scale to draw at all would be a chart nobody writes twice.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ScaleSpec {
    pub kind: Option<ScaleKind>,
    /// Lower and upper bound, for a continuous scale.
    pub domain: Option<(f64, f64)>,
    /// The categories, in the order they should appear. Absent means the order
    /// they are met in the data, which is the order the author typed them.
    pub categories: Option<Vec<String>>,
    /// Whether the scale must reach zero.
    ///
    /// Absent means the mark decides, and a bar decides yes: a bar chart that
    /// does not start at zero lies about the proportions, and teaching
    /// material is the worst place for that.
    pub zero: Option<bool>,
    /// Widen the domain to round numbers. On by default for a continuous axis.
    pub nice: Option<bool>,
    /// Base of a logarithmic scale.
    pub base: Option<f64>,
    /// Gap between neighbouring bands, as a share of the step.
    pub padding_inner: Option<f64>,
    /// Gap before the first band and after the last.
    pub padding_outer: Option<f64>,
}

/// A field, and what it drives.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Channel {
    /// Name of the field in the data.
    pub field: String,
    /// What the field means. Absent means "read it off the data", which is
    /// only ever asked when the answer is not in doubt: a field holding no
    /// numbers at all cannot go on a continuous scale, so it is categorical.
    ///
    /// Optional rather than defaulting to quantitative, because the default
    /// was wrong in the commonest case there is. `{"field": "mes"}` over
    /// months is what anyone writes first, and reading it as a quantity gave
    /// a numeric axis over text and a bar chart with nothing to stand on.
    pub kind: Option<FieldKind>,
    pub scale: Option<ScaleSpec>,
    /// Shown beside the axis. Absent means the field's own name.
    pub title: Option<String>,
}

impl Channel {
    /// A quantitative channel on `field`, which is the common case.
    pub fn of(field: impl Into<String>) -> Self {
        Channel { field: field.into(), ..Channel::default() }
    }
}

/// Which field drives which visual channel.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Encoding {
    pub x: Channel,
    pub y: Channel,
    /// Splits the data into series, one colour each.
    pub color: Option<Channel>,
}

/// One axis.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Axis {
    pub visible: bool,
    /// Overrides the channel's title.
    pub title: Option<String>,
    /// Roughly how many marks to aim for. The real count is whatever lands on
    /// round numbers near it.
    pub ticks: Option<u32>,
    /// Rules across the plot at every mark.
    pub grid: bool,
}

impl Default for Axis {
    fn default() -> Self {
        // Visible, because an axis nobody asked to hide is an axis they want;
        // no grid, because a grid nobody asked for is ink over the data.
        Axis { visible: true, title: None, ticks: None, grid: false }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Axes {
    pub x: Axis,
    pub y: Axis,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum LegendPosition {
    #[default]
    Right,
    Bottom,
    Top,
    Left,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Legend {
    pub visible: bool,
    pub position: LegendPosition,
    pub title: Option<String>,
}

impl Default for Legend {
    fn default() -> Self {
        Legend { visible: true, position: LegendPosition::Right, title: None }
    }
}

/// A chart.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ChartFrame {
    /// Inline observations. Ignored when `dataset` is set.
    pub data: Vec<Row>,
    /// Sugar: pull the rows from `resources.data` instead of `data`.
    ///
    /// The same pair `blocks` and `story` already are on a text frame, and for
    /// the same reason: two charts of the same numbers should not be two
    /// copies of the numbers. Called `dataset` and not `series`, because a
    /// series is one line inside a chart and confusing the two would cost an
    /// explanation every time.
    pub dataset: Option<String>,

    pub mark: Mark,
    pub encoding: Encoding,
    pub axes: Axes,
    pub legend: Option<Legend>,

    /// Colours for the series, in order. Absent means the built-in palette.
    pub palette: Vec<Color>,
    pub style: Option<Style>,
}

impl ChartFrame {
    /// The rows this chart draws, wherever they live.
    ///
    /// `None` when it names a dataset that is not there — which is a
    /// diagnostic and an empty frame, never a refusal to read the document.
    pub fn rows<'a>(
        &'a self,
        datasets: &'a std::collections::BTreeMap<String, Vec<Row>>,
    ) -> Option<&'a [Row]> {
        match &self.dataset {
            Some(name) => datasets.get(name).map(Vec::as_slice),
            None => Some(&self.data),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn round_trip(json: &str) -> ChartFrame {
        let parsed: ChartFrame = serde_json::from_str(json).expect("lê");
        let written = serde_json::to_string(&parsed).expect("escreve");
        let again: ChartFrame = serde_json::from_str(&written).expect("volta a ler");
        assert_eq!(parsed, again, "o que se grava volta a ler-se igual:\n{written}");
        parsed
    }

    #[test]
    fn a_chart_survives_being_written_and_read_again() {
        let chart = round_trip(
            r#"{
                "data": [
                    { "mes": "jan", "vendas": 120, "regiao": "norte" },
                    { "mes": "fev", "vendas": 98.5, "regiao": "norte" },
                    { "mes": "jan", "vendas": null, "regiao": "sul" }
                ],
                "mark": "bar",
                "encoding": {
                    "x": { "field": "mes", "kind": "categorical" },
                    "y": { "field": "vendas", "title": "Vendas (mil)" },
                    "color": { "field": "regiao", "kind": "categorical" }
                },
                "axes": { "y": { "grid": true, "ticks": 6 } },
                "legend": { "position": "bottom" }
            }"#,
        );
        assert_eq!(chart.mark, Mark::Bar);
        assert_eq!(chart.data.len(), 3);
        assert_eq!(chart.encoding.x.kind, Some(FieldKind::Categorical));
        assert_eq!(
            chart.encoding.y.kind, None,
            "o que não foi dito fica por dizer, e quem lê os dados decide",
        );
        assert!(chart.axes.y.grid);
        assert!(!chart.axes.x.grid, "a grelha não se liga sozinha no outro eixo");
        assert!(chart.axes.x.visible, "e o eixo não se apaga sozinho");
    }

    #[test]
    fn a_hole_in_the_data_is_a_hole_and_not_an_error() {
        let chart = round_trip(r#"{ "data": [{ "v": null }] }"#);
        assert!(chart.data[0]["v"].is_null());
        assert_eq!(chart.data[0]["v"].as_number(), None);
        assert_eq!(chart.data[0]["v"].as_category(), None);
    }

    #[test]
    fn a_number_can_stand_in_for_a_category() {
        // A year is written as a number and read as a label. Refusing it would
        // make `{"ano": 2024}` a thing the author has to quote.
        let value = Value::Number(2024.0);
        assert_eq!(value.as_category().as_deref(), Some("2024"));
    }

    #[test]
    fn the_scale_can_be_overridden_field_by_field() {
        let chart = round_trip(
            r#"{
                "encoding": {
                    "x": { "field": "t" },
                    "y": { "field": "v", "scale": {
                        "kind": "log", "base": 2, "domain": [1, 1024], "zero": false
                    } }
                }
            }"#,
        );
        let scale = chart.encoding.y.scale.expect("escala declarada");
        assert_eq!(scale.kind, Some(ScaleKind::Log));
        assert_eq!(scale.base, Some(2.0));
        assert_eq!(scale.domain, Some((1.0, 1024.0)));
        assert_eq!(scale.zero, Some(false));
        assert_eq!(scale.nice, None, "o que não foi dito continua por dizer");
    }

    #[test]
    fn a_chart_without_a_dataset_draws_what_it_carries() {
        let chart: ChartFrame =
            serde_json::from_str(r#"{ "data": [{ "v": 1 }] }"#).expect("lê");
        let none = std::collections::BTreeMap::new();
        assert_eq!(chart.rows(&none).map(<[Row]>::len), Some(1));
    }

    #[test]
    fn a_chart_naming_a_dataset_that_is_not_there_says_so_rather_than_guessing() {
        let chart: ChartFrame =
            serde_json::from_str(r#"{ "dataset": "vendas", "data": [{ "v": 1 }] }"#)
                .expect("lê");
        let none = std::collections::BTreeMap::new();
        assert_eq!(
            chart.rows(&none),
            None,
            "não cai para os dados em linha: quem nomeou uma série quer essa série",
        );
    }

    #[test]
    fn a_named_dataset_wins_over_the_rows_written_in_place() {
        let chart: ChartFrame =
            serde_json::from_str(r#"{ "dataset": "vendas", "data": [{ "v": 1 }] }"#)
                .expect("lê");
        let mut datasets = std::collections::BTreeMap::new();
        datasets.insert(
            "vendas".to_string(),
            vec![Row::from([("v".to_string(), Value::Number(9.0))])],
        );
        let rows = chart.rows(&datasets).expect("a série");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0]["v"].as_number(), Some(9.0));
    }
}
