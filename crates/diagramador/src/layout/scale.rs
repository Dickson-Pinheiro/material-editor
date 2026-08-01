//! Scales: from data to the page.
//!
//! A scale is a function with a domain and a range. The domain is what the
//! data says — a span of numbers, or a list of categories. The range is where
//! it goes on the paper, in points.
//!
//! Nothing here draws. A chart asks a scale where a value belongs and then
//! emits a rectangle there, which is what keeps the drawing honest: change
//! the scale and every mark moves together, because there is one place that
//! decides.
//!
//! Nothing outside the tests calls this yet: the chart is what it exists for.
#![allow(dead_code)]

use super::ticks;

/// How a value finds its place.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Scale {
    /// Proportional. The workhorse.
    Linear { domain: (f64, f64), range: (f64, f64), clamp: bool },
    /// Proportional in the logarithm. Orders of magnitude.
    Log { domain: (f64, f64), range: (f64, f64), base: f64 },
    /// Categories, each given an interval of its own. The scale of bars.
    Band {
        categories: Vec<String>,
        range: (f64, f64),
        /// Share of each step given up to separate neighbours, `0..1`.
        padding_inner: f64,
        /// Share of a step held back at each end, `0..1`.
        padding_outer: f64,
        /// Where any leftover goes: 0 at the start, 1 at the end.
        align: f64,
    },
    /// Categories, each given a point rather than an interval. The scale of
    /// lines, where a series is drawn through the middle of each category.
    Point { categories: Vec<String>, range: (f64, f64), padding: f64 },
}

/// Why a scale could not be built or used.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ScaleError {
    /// A logarithmic scale cannot cross or touch zero: the logarithm has no
    /// value there, and no amount of clamping invents one.
    LogDomainCrossesZero,
    /// A category asked for that the scale never heard of.
    UnknownCategory,
}

impl Scale {
    /// Where `value` belongs on the page, for the scales that take numbers.
    pub(crate) fn map(&self, value: f64) -> Result<f64, ScaleError> {
        match self {
            Scale::Linear { domain, range, clamp } => {
                let t = normalise(value, *domain);
                let t = if *clamp { t.clamp(0.0, 1.0) } else { t };
                Ok(range.0 + t * (range.1 - range.0))
            }
            Scale::Log { domain, range, base } => {
                if domain.0 <= 0.0 || domain.1 <= 0.0 {
                    return Err(ScaleError::LogDomainCrossesZero);
                }
                if value <= 0.0 {
                    return Err(ScaleError::LogDomainCrossesZero);
                }
                let ln = |v: f64| v.log(*base);
                let t = normalise(ln(value), (ln(domain.0), ln(domain.1)));
                Ok(range.0 + t * (range.1 - range.0))
            }
            Scale::Band { .. } | Scale::Point { .. } => Err(ScaleError::UnknownCategory),
        }
    }

    /// Where `category` begins, for the scales that take names.
    pub(crate) fn map_category(&self, category: &str) -> Result<f64, ScaleError> {
        let index = self
            .categories()
            .and_then(|list| list.iter().position(|name| name == category))
            .ok_or(ScaleError::UnknownCategory)?;

        match self {
            Scale::Band { .. } => Ok(self.band_start(index)),
            Scale::Point { .. } => Ok(self.band_start(index)),
            _ => Err(ScaleError::UnknownCategory),
        }
    }

    /// How wide one category's interval is. Zero for a point scale, which is
    /// what makes a line pass through a category rather than across it.
    pub(crate) fn bandwidth(&self) -> f64 {
        match self {
            Scale::Band { padding_inner, .. } => self.step().abs() * (1.0 - clamp01(*padding_inner)),
            Scale::Point { .. } => 0.0,
            _ => 0.0,
        }
    }

    /// Distance from one category to the next, sign included.
    pub(crate) fn step(&self) -> f64 {
        let (categories, range, inner, outer) = match self {
            Scale::Band { categories, range, padding_inner, padding_outer, .. } => {
                (categories, range, clamp01(*padding_inner), clamp01(*padding_outer))
            }
            // A point scale is a band scale whose bands have no width, so the
            // same arithmetic serves both and there is one formula to trust.
            Scale::Point { categories, range, padding } => {
                (categories, range, 1.0, clamp01(*padding))
            }
            _ => return 0.0,
        };

        let count = categories.len() as f64;
        if count == 0.0 {
            return 0.0;
        }
        let span = range.1 - range.0;
        let divisor = (count - inner + outer * 2.0).max(1.0);
        span / divisor
    }

    fn categories(&self) -> Option<&Vec<String>> {
        match self {
            Scale::Band { categories, .. } | Scale::Point { categories, .. } => Some(categories),
            _ => None,
        }
    }

    fn band_start(&self, index: usize) -> f64 {
        // The outer padding is not read here: it is already inside `step`,
        // and what is left over after the bands is what `align` distributes.
        let (range, count, inner, align) = match self {
            Scale::Band { categories, range, padding_inner, align, .. } => (
                *range,
                categories.len() as f64,
                clamp01(*padding_inner),
                clamp01(*align),
            ),
            Scale::Point { categories, range, .. } => {
                (*range, categories.len() as f64, 1.0, 0.5)
            }
            _ => return 0.0,
        };

        let step = self.step();
        // Whatever the bands do not use is shared out by `align`: 0 pins them
        // to the start, 1 to the end, 0.5 centres them.
        let used = step * (count - inner);
        let start = range.0 + (range.1 - range.0 - used) * align;
        start + step * index as f64
    }

    /// Marks for an axis on this scale.
    pub(crate) fn ticks(&self, count: usize) -> Vec<f64> {
        match self {
            Scale::Linear { domain, .. } => ticks::ticks(domain.0, domain.1, count),
            Scale::Log { domain, base, .. } => {
                if domain.0 <= 0.0 || domain.1 <= 0.0 {
                    return Vec::new();
                }
                // Powers of the base, which is the only marking a reader can
                // interpret on a log axis. Linear marks there would be a lie
                // told in even spacing.
                let first = domain.0.log(*base).floor() as i32;
                let last = domain.1.log(*base).ceil() as i32;
                (first..=last)
                    .map(|power| base.powi(power))
                    .filter(|v| *v >= domain.0 && *v <= domain.1)
                    .collect()
            }
            Scale::Band { .. } | Scale::Point { .. } => Vec::new(),
        }
    }

    /// Widen a numeric domain to round numbers.
    pub(crate) fn nice(&mut self, count: usize) {
        if let Scale::Linear { domain, .. } = self {
            *domain = ticks::nice(domain.0, domain.1, count);
        }
    }

    /// Pull the domain down to include zero.
    ///
    /// A bar chart whose axis starts above zero misstates every proportion it
    /// draws, so bars ask for this and the author has to say otherwise.
    pub(crate) fn include_zero(&mut self) {
        if let Scale::Linear { domain, .. } = self {
            if domain.0 > 0.0 {
                domain.0 = 0.0;
            }
            if domain.1 < 0.0 {
                domain.1 = 0.0;
            }
        }
    }
}

fn normalise(value: f64, domain: (f64, f64)) -> f64 {
    let span = domain.1 - domain.0;
    if span == 0.0 { 0.0 } else { (value - domain.0) / span }
}

fn clamp01(value: f64) -> f64 {
    value.clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn near(actual: f64, expected: f64) -> bool {
        (actual - expected).abs() < 1e-6
    }

    fn linear(domain: (f64, f64), range: (f64, f64)) -> Scale {
        Scale::Linear { domain, range, clamp: false }
    }

    fn band(count: usize, range: (f64, f64), inner: f64, outer: f64) -> Scale {
        Scale::Band {
            categories: (0..count).map(|i| format!("c{i}")).collect(),
            range,
            padding_inner: inner,
            padding_outer: outer,
            align: 0.5,
        }
    }

    #[test]
    fn linear_maps_the_ends_and_the_middle() {
        let s = linear((0.0, 10.0), (0.0, 200.0));
        assert!(near(s.map(0.0).unwrap(), 0.0));
        assert!(near(s.map(5.0).unwrap(), 100.0));
        assert!(near(s.map(10.0).unwrap(), 200.0));
    }

    #[test]
    fn a_range_that_runs_backwards_maps_backwards() {
        // The y axis of every chart: value grows upward, the page grows down.
        let s = linear((0.0, 10.0), (300.0, 100.0));
        assert!(near(s.map(0.0).unwrap(), 300.0), "zero fica em baixo");
        assert!(near(s.map(10.0).unwrap(), 100.0), "o máximo em cima");
    }

    #[test]
    fn clamping_holds_a_stray_value_inside_the_range() {
        let solto = linear((0.0, 10.0), (0.0, 100.0));
        let preso = Scale::Linear { domain: (0.0, 10.0), range: (0.0, 100.0), clamp: true };
        assert!(near(solto.map(20.0).unwrap(), 200.0), "sem prender, sai fora");
        assert!(near(preso.map(20.0).unwrap(), 100.0), "preso, para na borda");
    }

    #[test]
    fn four_bands_share_the_range_with_gaps_between_them() {
        let s = band(4, (0.0, 100.0), 0.1, 0.0);
        let width = s.bandwidth();
        let step = s.step();

        assert!(near(s.map_category("c0").unwrap(), 0.0), "a primeira encosta ao início");
        assert!(
            near(s.map_category("c3").unwrap() + width, 100.0),
            "e a última ao fim: {}",
            s.map_category("c3").unwrap() + width,
        );
        for index in 0..4 {
            let start = s.map_category(&format!("c{index}")).unwrap();
            assert!(near(start, step * index as f64), "as bandas são regulares");
        }
        assert!(near(step - width, step * 0.1), "o intervalo é o padding pedido");
    }

    #[test]
    fn outer_padding_pulls_the_bands_off_the_ends() {
        let colado = band(3, (0.0, 90.0), 0.0, 0.0);
        let solto = band(3, (0.0, 90.0), 0.0, 0.5);
        assert!(near(colado.map_category("c0").unwrap(), 0.0));
        assert!(
            solto.map_category("c0").unwrap() > 0.0,
            "com folga externa a primeira banda afasta-se do início",
        );
        assert!(solto.bandwidth() < colado.bandwidth(), "e todas ficam mais estreitas");
    }

    #[test]
    fn a_point_scale_has_no_width() {
        let s = Scale::Point {
            categories: vec!["a".into(), "b".into(), "c".into()],
            range: (0.0, 100.0),
            padding: 0.0,
        };
        assert_eq!(s.bandwidth(), 0.0);
        assert!(near(s.map_category("a").unwrap(), 0.0), "a primeira no início");
        assert!(near(s.map_category("c").unwrap(), 100.0), "a última no fim");
        assert!(near(s.map_category("b").unwrap(), 50.0), "e a do meio no meio");
    }

    #[test]
    fn a_category_nobody_declared_is_an_error_not_a_zero() {
        let s = band(2, (0.0, 100.0), 0.0, 0.0);
        assert_eq!(s.map_category("inexistente"), Err(ScaleError::UnknownCategory));
    }

    #[test]
    fn a_log_scale_refuses_zero_instead_of_returning_infinity() {
        let s = Scale::Log { domain: (0.0, 100.0), range: (0.0, 200.0), base: 10.0 };
        assert_eq!(s.map(10.0), Err(ScaleError::LogDomainCrossesZero));
        assert!(s.ticks(5).is_empty(), "e não inventa marcas");
    }

    #[test]
    fn a_log_scale_spaces_by_order_of_magnitude() {
        let s = Scale::Log { domain: (1.0, 1000.0), range: (0.0, 300.0), base: 10.0 };
        assert!(near(s.map(1.0).unwrap(), 0.0));
        assert!(near(s.map(10.0).unwrap(), 100.0), "cada década ocupa o mesmo");
        assert!(near(s.map(100.0).unwrap(), 200.0));
        assert_eq!(s.ticks(5), vec![1.0, 10.0, 100.0, 1000.0]);
    }

    #[test]
    fn nice_widens_a_linear_domain_without_hiding_data() {
        let mut s = linear((0.3, 9.4), (0.0, 100.0));
        s.nice(5);
        match s {
            Scale::Linear { domain, .. } => {
                assert!(domain.0 <= 0.3 && domain.1 >= 9.4, "veio {domain:?}");
            }
            _ => unreachable!(),
        }
    }

    #[test]
    fn including_zero_only_ever_extends() {
        let mut acima = linear((20.0, 80.0), (0.0, 100.0));
        acima.include_zero();
        let mut abaixo = linear((-80.0, -20.0), (0.0, 100.0));
        abaixo.include_zero();
        let mut atravessa = linear((-10.0, 10.0), (0.0, 100.0));
        atravessa.include_zero();

        assert_eq!(domain_of(&acima), (0.0, 80.0));
        assert_eq!(domain_of(&abaixo), (-80.0, 0.0));
        assert_eq!(domain_of(&atravessa), (-10.0, 10.0), "já continha o zero");
    }

    #[test]
    fn no_categories_is_not_a_division_by_zero() {
        let s = band(0, (0.0, 100.0), 0.1, 0.1);
        assert_eq!(s.step(), 0.0);
        assert_eq!(s.bandwidth(), 0.0);
    }

    fn domain_of(scale: &Scale) -> (f64, f64) {
        match scale {
            Scale::Linear { domain, .. } | Scale::Log { domain, .. } => *domain,
            _ => (0.0, 0.0),
        }
    }
}
