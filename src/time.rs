// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Mateus Rocha de Paula and the OxiGuitar contributors

//! Durations between MusicXML's note types and divisions and OxiGuitar's
//! note values, dots and tuplets.

use musicxml::datatypes::NoteTypeValue;
use oxiguitar_model::{Duration, NoteValue, Ratio, Tuplet};

/// OxiGuitar's note value for a MusicXML note type, and whether it had to
/// be clamped (longer than a double whole, shorter than a 256th).
pub(crate) fn note_value(t: &NoteTypeValue) -> (NoteValue, bool) {
    match t {
        NoteTypeValue::Maxima | NoteTypeValue::Long => (NoteValue::DoubleWhole, true),
        NoteTypeValue::Breve => (NoteValue::DoubleWhole, false),
        NoteTypeValue::Whole => (NoteValue::Whole, false),
        NoteTypeValue::Half => (NoteValue::Half, false),
        NoteTypeValue::Quarter => (NoteValue::Quarter, false),
        NoteTypeValue::Eighth => (NoteValue::Eighth, false),
        NoteTypeValue::Sixteenth => (NoteValue::Sixteenth, false),
        NoteTypeValue::ThirtySecond => (NoteValue::ThirtySecond, false),
        NoteTypeValue::SixtyFourth => (NoteValue::SixtyFourth, false),
        NoteTypeValue::OneHundredTwentyEighth => (NoteValue::OneTwentyEighth, false),
        NoteTypeValue::TwoHundredFiftySixth => (NoteValue::TwoFiftySixth, false),
        NoteTypeValue::FiveHundredTwelfth | NoteTypeValue::OneThousandTwentyFourth => {
            (NoteValue::TwoFiftySixth, true)
        }
    }
}

/// MusicXML's note type for an OxiGuitar note value.
pub(crate) fn note_type(v: NoteValue) -> NoteTypeValue {
    match v {
        NoteValue::DoubleWhole => NoteTypeValue::Breve,
        NoteValue::Whole => NoteTypeValue::Whole,
        NoteValue::Half => NoteTypeValue::Half,
        NoteValue::Quarter => NoteTypeValue::Quarter,
        NoteValue::Eighth => NoteTypeValue::Eighth,
        NoteValue::Sixteenth => NoteTypeValue::Sixteenth,
        NoteValue::ThirtySecond => NoteTypeValue::ThirtySecond,
        NoteValue::SixtyFourth => NoteTypeValue::SixtyFourth,
        NoteValue::OneTwentyEighth => NoteTypeValue::OneHundredTwentyEighth,
        NoteValue::TwoFiftySixth => NoteTypeValue::TwoHundredFiftySixth,
    }
}

/// Every note value, longest first.
const VALUES: [NoteValue; 10] = [
    NoteValue::DoubleWhole,
    NoteValue::Whole,
    NoteValue::Half,
    NoteValue::Quarter,
    NoteValue::Eighth,
    NoteValue::Sixteenth,
    NoteValue::ThirtySecond,
    NoteValue::SixtyFourth,
    NoteValue::OneTwentyEighth,
    NoteValue::TwoFiftySixth,
];

/// The one duration (a value with up to three dots, in `tuplet` if given)
/// that lasts exactly `len`, if there is one.
pub(crate) fn exact(len: Ratio, tuplet: Option<Tuplet>) -> Option<Duration> {
    VALUES.iter().find_map(|&value| {
        (0..=3).find_map(|dots| {
            let d = Duration {
                value,
                dots,
                tuplets: tuplet.into_iter().collect(),
            };
            (d.length() == Some(len)).then_some(d)
        })
    })
}

/// Durations, longest first, that together last exactly `len`: for rests
/// and gaps the file leaves, where only the length is known. Lengths that
/// need a tuplet (a third of a beat) get triplets, quintuplets, sextuplets
/// or septuplets. Empty when `len` is zero or can't be made of at most 16
/// pieces.
pub(crate) fn fill(len: Ratio) -> Vec<Duration> {
    if len <= Ratio::ZERO {
        return Vec::new();
    }
    let mut out = Vec::new();
    // The dyadic part plainly; what is left in the first tuplet that
    // makes it dyadic.
    let tuplets = [
        None,
        Some(Tuplet::TRIPLET),
        Some(Tuplet {
            notes: 5,
            in_time_of: 4,
        }),
        Some(Tuplet {
            notes: 6,
            in_time_of: 4,
        }),
        Some(Tuplet {
            notes: 7,
            in_time_of: 4,
        }),
    ];
    for tuplet in tuplets {
        // The length as written inside the tuplet.
        let factor = match tuplet {
            None => Ratio::ONE,
            Some(t) => match t.factor() {
                Some(f) => f,
                None => continue,
            },
        };
        let Some(written) = len.checked_div(factor) else {
            continue;
        };
        if !is_dyadic(written) {
            continue;
        }
        let mut left = written;
        while left > Ratio::ZERO && out.len() < 16 {
            let Some(value) = VALUES.iter().copied().find(|v| v.length() <= left) else {
                break;
            };
            // A dot or two where they fit exactly in what is left.
            let mut d = Duration::new(value);
            for dots in [2u8, 1] {
                let dotted = Duration::new(value);
                let dotted = Duration { dots, ..dotted };
                if dotted.length().is_some_and(|l| l <= left) {
                    d = dotted;
                    break;
                }
            }
            let Some(next) = d.length().and_then(|l| left.checked_sub(l)) else {
                break;
            };
            left = next;
            out.push(Duration {
                tuplets: tuplet.into_iter().collect(),
                ..d
            });
        }
        if left == Ratio::ZERO {
            return out;
        }
        out.clear();
    }
    out
}

/// Whether `r`'s denominator is a power of two (and so a sum of plain
/// note values).
fn is_dyadic(r: Ratio) -> bool {
    let d = r.denom();
    d > 0 && d & (d - 1) == 0
}

/// `divisions` of a quarter note, as a whole-note ratio: MusicXML counts
/// time in divisions per quarter note.
pub(crate) fn from_divisions(amount: i64, per_quarter: i64) -> Option<Ratio> {
    Ratio::new(amount, per_quarter.checked_mul(4)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(n: i64, d: i64) -> Ratio {
        Ratio::new(n, d).unwrap()
    }

    #[test]
    fn fills_lengths() {
        let lens = |v: Vec<Duration>| v.iter().map(|d| d.length().unwrap()).collect::<Vec<_>>();
        assert_eq!(lens(fill(r(1, 1))), vec![r(1, 1)]);
        assert_eq!(fill(r(3, 4)).len(), 1, "a dotted half");
        assert_eq!(lens(fill(r(5, 8))), vec![r(1, 2), r(1, 8)]);
        let triplet = fill(r(1, 12));
        assert_eq!(triplet.len(), 1);
        assert_eq!(triplet[0].tuplets, vec![Tuplet::TRIPLET]);
        assert!(fill(Ratio::ZERO).is_empty());
        assert!(fill(r(1, 11)).is_empty(), "no tuplet here makes elevenths");
        for n in 1..64 {
            let len = r(n, 32);
            assert_eq!(
                fill(len).iter().map(|d| d.length().unwrap()).sum::<Ratio>(),
                len
            );
        }
    }

    #[test]
    fn exact_durations() {
        let d = exact(r(3, 8), None).unwrap();
        assert_eq!((d.value, d.dots), (NoteValue::Quarter, 1));
        assert!(exact(r(5, 8), None).is_none());
        let t = exact(r(1, 12), Some(Tuplet::TRIPLET)).unwrap();
        assert_eq!(t.value, NoteValue::Eighth);
    }
}
