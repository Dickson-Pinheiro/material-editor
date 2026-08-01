//! Where to put the marks on an axis.
//!
//! The numbers a reader expects to see are 1, 2, 5 and their multiples of ten
//! — never 3, never 7, never 0.375. This is d3's `tickIncrement`, which picks
//! among those four by how far the ideal step falls from each, using the
//! geometric midpoints √50, √10 and √2 as the thresholds.
//!
//! There is better. Talbot, Lin and Hanrahan (2010) optimise simplicity,
//! coverage, density and legibility together, and will even choose the label
//! format and orientation. That is the answer when an axis is cramped; this
//! is the answer for every other axis, and it is twenty lines.
//!
//! Nothing outside the tests calls this yet: the chart's axes are what it
//! exists for, and the scales' `nice` already leans on it.
#![allow(dead_code)]

/// Geometric midpoint between 5 and 10 — above it, 10 is the closer step.
const E10: f64 = 7.071_067_811_865_476; // sqrt(50)
/// Between 2 and 5.
const E5: f64 = 3.162_277_660_168_379_5; // sqrt(10)
/// Between 1 and 2.
const E2: f64 = std::f64::consts::SQRT_2;

/// The step between marks, for roughly `count` of them across `start..stop`.
///
/// Negative when the span is smaller than one unit — the caller divides by
/// the magnitude instead of multiplying, which is what keeps the arithmetic
/// exact for steps like 0.1 that no binary float can hold.
pub(crate) fn increment(start: f64, stop: f64, count: usize) -> f64 {
    let step = (stop - start) / count.max(1) as f64;
    if !step.is_finite() || step == 0.0 {
        return 0.0;
    }

    let power = (step.abs().log10()).floor();
    let error = step.abs() / 10f64.powf(power);

    let factor = if error >= E10 {
        10.0
    } else if error >= E5 {
        5.0
    } else if error >= E2 {
        2.0
    } else {
        1.0
    };

    let magnitude = if power >= 0.0 {
        factor * 10f64.powf(power)
    } else {
        // Return the divisor, negated, so the caller never multiplies by a
        // value like 0.1 that cannot be represented exactly.
        -(10f64.powf(-power)) / factor
    };

    if step < 0.0 { -magnitude } else { magnitude }
}

/// Marks across `start..stop`, roughly `count` of them, on round numbers.
///
/// Always ascending in value order, but reversed when the domain is: an axis
/// that counts down gets marks that count down.
pub(crate) fn ticks(start: f64, stop: f64, count: usize) -> Vec<f64> {
    if !start.is_finite() || !stop.is_finite() {
        return Vec::new();
    }
    if start == stop {
        return vec![start];
    }

    let reversed = stop < start;
    let (low, high) = if reversed { (stop, start) } else { (start, stop) };

    let step = increment(low, high, count);
    if step == 0.0 {
        return Vec::new();
    }

    let mut out = Vec::new();
    if step > 0.0 {
        let first = (low / step).ceil();
        let last = (high / step).floor();
        let n = (last - first) as i64;
        for index in 0..=n.max(-1) {
            out.push((first + index as f64) * step);
        }
    } else {
        // Fractional steps: divide rather than multiply, so 0.1 lands on 0.1
        // and not on 0.30000000000000004.
        let divisor = -step;
        let first = (low * divisor).ceil();
        let last = (high * divisor).floor();
        let n = (last - first) as i64;
        for index in 0..=n.max(-1) {
            out.push((first + index as f64) / divisor);
        }
    }

    if reversed {
        out.reverse();
    }
    out
}

/// Widen a domain outward to the nearest round numbers.
///
/// Never inward: a domain that shrank would hide data, which is a worse sin
/// than an axis that reaches a little past the last point.
pub(crate) fn nice(start: f64, stop: f64, count: usize) -> (f64, f64) {
    if !start.is_finite() || !stop.is_finite() || start == stop {
        return (start, stop);
    }

    let reversed = stop < start;
    let (mut low, mut high) = if reversed { (stop, start) } else { (start, stop) };

    // Two rounds: widening can change the magnitude, and the step that suits
    // the wider domain may be coarser than the one that produced it.
    for _ in 0..2 {
        let step = increment(low, high, count);
        if step == 0.0 {
            break;
        }
        let (next_low, next_high) = if step > 0.0 {
            ((low / step).floor() * step, (high / step).ceil() * step)
        } else {
            let divisor = -step;
            ((low * divisor).floor() / divisor, (high * divisor).ceil() / divisor)
        };
        if next_low == low && next_high == high {
            break;
        }
        low = next_low;
        high = next_high;
    }

    if reversed { (high, low) } else { (low, high) }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn near(actual: f64, expected: f64) -> bool {
        (actual - expected).abs() < 1e-9
    }

    #[test]
    fn a_unit_span_is_marked_in_tenths() {
        let out = ticks(0.0, 1.0, 10);
        assert_eq!(out.len(), 11, "de 0 a 1 inclusive: {out:?}");
        assert!(near(out[1], 0.1), "e em décimos exactos, não 0,1000000001: {}", out[1]);
        assert!(near(out[3], 0.3), "veio {}", out[3]);
        assert!(near(out[10], 1.0));
    }

    #[test]
    fn seven_gets_whole_numbers_not_sevenths() {
        // The ideal step is 1.4, and 1.4 falls just below √2 — so the mark
        // lands on 1, not on 2. An axis of 0 to 7 reads in whole numbers.
        let out = ticks(0.0, 7.0, 5);
        assert_eq!(out, vec![0.0, 1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0]);
    }

    #[test]
    fn a_million_is_marked_in_round_hundreds_of_thousands() {
        // Asking for four gives five, and that is the point: the step is
        // rounded to something a reader recognises rather than the exact
        // 250 000 that four marks would need. 2.5 is not one of 1, 2, 5, 10.
        let out = ticks(0.0, 1_000_000.0, 4);
        assert_eq!(out.first(), Some(&0.0));
        assert_eq!(out.last(), Some(&1_000_000.0));
        for pair in out.windows(2) {
            assert!(near(pair[1] - pair[0], 200_000.0), "passo irregular: {out:?}");
        }
    }

    #[test]
    fn a_descending_domain_gets_descending_marks() {
        let out = ticks(10.0, 0.0, 5);
        assert!(out.first() > out.last(), "as marcas seguem o eixo: {out:?}");
        assert_eq!(out.first(), Some(&10.0));
        assert_eq!(out.last(), Some(&0.0));
    }

    #[test]
    fn asking_for_no_marks_does_not_divide_by_zero() {
        let out = ticks(0.0, 10.0, 0);
        assert!(out.iter().all(|v| v.is_finite()), "{out:?}");
    }

    #[test]
    fn a_span_of_nothing_gives_one_mark() {
        assert_eq!(ticks(5.0, 5.0, 10), vec![5.0]);
    }

    #[test]
    fn nice_widens_and_never_narrows() {
        let (low, high) = nice(0.3, 9.4, 5);
        assert!(low <= 0.3, "o mínimo não pode subir: {low}");
        assert!(high >= 9.4, "nem o máximo descer: {high}");
        assert!(near(low, 0.0), "veio {low}");
        assert!(near(high, 10.0), "veio {high}");
    }

    #[test]
    fn nice_leaves_an_already_round_domain_alone() {
        assert_eq!(nice(0.0, 100.0, 5), (0.0, 100.0));
    }

    #[test]
    fn nice_keeps_the_direction_of_a_descending_domain() {
        let (start, stop) = nice(9.4, 0.3, 5);
        assert!(start > stop, "a direcção sobrevive: {start}..{stop}");
        assert!(start >= 9.4 && stop <= 0.3);
    }

    #[test]
    fn tiny_spans_still_land_on_round_numbers() {
        let out = ticks(0.001, 0.01, 5);
        assert!(!out.is_empty());
        for value in &out {
            // Every mark is a whole number of thousandths.
            let thousandths = value * 1000.0;
            assert!(
                (thousandths - thousandths.round()).abs() < 1e-6,
                "{value} não é redondo",
            );
        }
    }
}
