//! Track sizing: how a length is shared out among columns or rows.
//!
//! One question, asked by both consumers that need it: *given this much room
//! and these declarations, how wide is each track?* A table asks it of its
//! columns; a chart will ask it of the bands an axis is divided into.
//!
//! Nothing here knows about cells, text or data. It takes declarations and
//! intrinsic sizes and gives back lengths.
//!
//! The table's column resolver is what it exists for; a chart's axis bands
//! will ask the same question.

use super::text::Intrinsic;

/// What a track asks for.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub(crate) enum Track {
    /// Exactly this many points.
    Fixed(f64),
    /// This share of the available length, before anything else is taken.
    Relative(f64),
    /// This share of whatever is left once the others are served.
    Fraction(f64),
    /// As much as the content wants, between its own minimum and maximum.
    #[default]
    Auto,
}

/// Lengths, and whether they fit.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Resolved {
    pub(crate) lengths: Vec<f64>,
    /// By how much the tracks exceed the room, `0.0` when they fit.
    ///
    /// Reported rather than swallowed: a table whose columns cannot be made
    /// to fit is a thing the author has to be told about, and silently
    /// shrinking text into a sliver is not telling them.
    pub(crate) overflow: f64,
}

/// Share `available` out among `tracks`.
///
/// `content` carries one intrinsic per track and is only read for `Auto`;
/// the others may pass anything.
///
/// The order is the one CSS Grid settled on, and it matters. An `Auto` track
/// takes its content's natural width **before** the fractions divide what is
/// left — the other way round, `[auto, 1fr]` would squeeze the first column
/// to its longest word and hand everything else to the second, which is the
/// opposite of what anyone declaring it means.
pub(crate) fn resolve(tracks: &[Track], content: &[Intrinsic], available: f64, gap: f64) -> Resolved {
    if tracks.is_empty() {
        return Resolved { lengths: Vec::new(), overflow: 0.0 };
    }

    // Gaps are not negotiable, so they come off the top. Dividing first and
    // subtracting after would over-promise every track by a share of the gap.
    let gaps = gap * (tracks.len() - 1) as f64;
    let room = (available - gaps).max(0.0);

    let intrinsic = |index: usize| content.get(index).copied().unwrap_or_default();

    // ── Everything that is not a fraction takes what it asks for ────────────
    let mut lengths: Vec<f64> = tracks
        .iter()
        .enumerate()
        .map(|(index, track)| match *track {
            Track::Fixed(length) => length.max(0.0),
            Track::Relative(share) => (room * share).max(0.0),
            Track::Auto => {
                let want = intrinsic(index);
                want.max.max(want.min).max(0.0)
            }
            Track::Fraction(_) => 0.0,
        })
        .collect();

    let taken: f64 = lengths.iter().sum();
    let fractions: f64 = tracks
        .iter()
        .map(|track| match *track {
            Track::Fraction(share) => share.max(0.0),
            _ => 0.0,
        })
        .sum();

    // ── What is left goes to the fractions ──────────────────────────────────
    if taken <= room && fractions > 0.0 {
        let spare = room - taken;
        for (index, track) in tracks.iter().enumerate() {
            if let Track::Fraction(share) = *track {
                lengths[index] = spare * share.max(0.0) / fractions;
            }
        }
        return Resolved { lengths, overflow: 0.0 };
    }

    if taken <= room {
        // No fractions to absorb the slack. The tracks keep their own sizes
        // and the leftover simply is not used — a table that stretched to
        // fill would silently disagree with the widths it was given.
        return Resolved { lengths, overflow: 0.0 };
    }

    // ── Over-subscribed: give back, in the order that hurts least ───────────
    //
    // Fractions asked for leftovers and there are none, so they are already
    // at zero. Autos are asked to give back down to their minimum, in
    // proportion to how much slack each has — a column with one long word
    // has nothing to give and should not be asked.
    let mut excess = taken - room;

    let slack: f64 = tracks
        .iter()
        .enumerate()
        .map(|(index, track)| match *track {
            Track::Auto => (lengths[index] - intrinsic(index).min).max(0.0),
            _ => 0.0,
        })
        .sum();

    if slack > 0.0 {
        let give = excess.min(slack);
        for (index, track) in tracks.iter().enumerate() {
            if matches!(track, Track::Auto) {
                let mine = (lengths[index] - intrinsic(index).min).max(0.0);
                lengths[index] -= give * mine / slack;
            }
        }
        excess -= give;
    }

    Resolved { lengths, overflow: excess.max(0.0) }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wants(min: f64, max: f64) -> Intrinsic {
        Intrinsic { min, max }
    }

    fn none(count: usize) -> Vec<Intrinsic> {
        vec![Intrinsic::default(); count]
    }

    #[test]
    fn fixed_tracks_get_exactly_what_they_ask() {
        let out = resolve(
            &[Track::Fixed(100.0), Track::Fixed(50.0), Track::Fixed(30.0)],
            &none(3),
            500.0,
            0.0,
        );
        assert_eq!(out.lengths, vec![100.0, 50.0, 30.0]);
        assert_eq!(out.overflow, 0.0);
    }

    #[test]
    fn fractions_split_what_is_left_in_proportion() {
        let out = resolve(&[Track::Fraction(1.0), Track::Fraction(2.0)], &none(2), 300.0, 0.0);
        assert_eq!(out.lengths, vec![100.0, 200.0]);
    }

    #[test]
    fn a_fraction_takes_what_the_fixed_tracks_leave() {
        let out = resolve(
            &[Track::Fixed(120.0), Track::Fraction(1.0), Track::Fixed(80.0)],
            &none(3),
            400.0,
            0.0,
        );
        assert_eq!(out.lengths, vec![120.0, 200.0, 80.0]);
    }

    #[test]
    fn an_auto_track_takes_its_content_before_the_fractions_divide() {
        // The order that matters: auto first, fraction second. Reversed, the
        // auto column would be squeezed to its longest word.
        let out = resolve(
            &[Track::Auto, Track::Fraction(1.0)],
            &[wants(40.0, 150.0), Intrinsic::default()],
            400.0,
            0.0,
        );
        assert_eq!(out.lengths, vec![150.0, 250.0], "auto leva o seu máximo, fr leva o resto");
    }

    #[test]
    fn an_auto_track_never_goes_below_its_longest_word() {
        let out = resolve(
            &[Track::Auto, Track::Fixed(300.0)],
            &[wants(80.0, 200.0), Intrinsic::default()],
            320.0,
            0.0,
        );
        assert_eq!(out.lengths[0], 80.0, "encolhe até ao mínimo");
        assert_eq!(out.lengths[1], 300.0, "a fixa não cede");
        assert!(
            (out.overflow - 60.0).abs() < 0.01,
            "e o que falta é reportado, não escondido: {}",
            out.overflow,
        );
    }

    #[test]
    fn autos_give_back_in_proportion_to_what_they_have_to_spare() {
        // 200 asked for, 140 available. The roomy track gives more.
        let out = resolve(
            &[Track::Auto, Track::Auto],
            &[wants(10.0, 100.0), wants(90.0, 100.0)],
            140.0,
            0.0,
        );
        assert!(out.lengths[0] < out.lengths[1], "a que tinha folga cede mais");
        assert!((out.lengths.iter().sum::<f64>() - 140.0).abs() < 0.01, "e o total fecha");
        assert_eq!(out.overflow, 0.0);
        assert!(out.lengths[1] >= 90.0, "sem passar abaixo do próprio mínimo");
    }

    #[test]
    fn the_gap_is_taken_off_before_anything_is_shared() {
        // Three tracks, two gaps of 10: 300 minus 20 leaves 280 to divide.
        let out = resolve(
            &[Track::Fraction(1.0), Track::Fraction(1.0), Track::Fraction(1.0)],
            &none(3),
            300.0,
            10.0,
        );
        for length in &out.lengths {
            assert!((length - 280.0 / 3.0).abs() < 0.01, "veio {length}");
        }
    }

    #[test]
    fn a_relative_track_is_a_share_of_the_room_not_of_the_leftovers() {
        let out = resolve(
            &[Track::Relative(0.25), Track::Fraction(1.0)],
            &none(2),
            400.0,
            0.0,
        );
        assert_eq!(out.lengths, vec![100.0, 300.0]);
    }

    #[test]
    fn slack_with_no_fraction_to_absorb_it_stays_unused() {
        let out = resolve(&[Track::Fixed(50.0), Track::Fixed(50.0)], &none(2), 400.0, 0.0);
        assert_eq!(out.lengths, vec![50.0, 50.0], "as fixas não esticam para preencher");
        assert_eq!(out.overflow, 0.0);
    }

    #[test]
    fn no_tracks_is_not_a_crash() {
        let out = resolve(&[], &[], 400.0, 10.0);
        assert!(out.lengths.is_empty());
        assert_eq!(out.overflow, 0.0);
    }

    #[test]
    fn a_gap_wider_than_the_room_does_not_go_negative() {
        let out = resolve(&[Track::Fraction(1.0), Track::Fraction(1.0)], &none(2), 10.0, 40.0);
        assert!(out.lengths.iter().all(|l| *l >= 0.0), "nenhuma pista negativa");
    }
}
