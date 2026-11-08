// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Mateus Rocha de Paula and the OxiGuitar contributors

//! What kind of instrument a MusicXML part is, and so which strings it
//! gets. Mateus, 8 Nov 2026: parts the file marks as guitars or basses get
//! strings and show their tablature; every other pitched part also gets a
//! six-string guitar's strings (OxiGuitar fingers it as it opens, #157) but
//! shows notation only until the user turns its tablature on; drums get no
//! strings.

use oxiguitar_model::Pitch;

/// What a part is played on, as far as its strings go.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Kind {
    /// A guitar: six strings unless the file says otherwise, tablature
    /// shown.
    Guitar,
    /// A bass: four strings unless the file says otherwise, tablature
    /// shown.
    Bass,
    /// Any other pitched instrument: a guitar's strings, tablature hidden.
    Other,
    /// Drums and other unpitched percussion: no strings.
    Drums,
}

impl Kind {
    /// The strings given when the file names none, highest first.
    pub(crate) fn default_tuning(self) -> Vec<Pitch> {
        match self {
            Kind::Guitar | Kind::Other => [64, 59, 55, 50, 45, 40].map(Pitch).to_vec(),
            Kind::Bass => [43, 38, 33, 28].map(Pitch).to_vec(),
            Kind::Drums => Vec::new(),
        }
    }

    /// Whether its tablature is shown.
    pub(crate) fn shows_tablature(self) -> bool {
        matches!(self, Kind::Guitar | Kind::Bass)
    }

    /// The General MIDI program (0-based) it plays with when the file gives
    /// none: steel-string guitar, fingered electric bass, or piano.
    pub(crate) fn default_program(self, name: &str) -> u8 {
        match self {
            Kind::Guitar if is_nylon(name) => 24,
            Kind::Guitar => 25,
            Kind::Bass => 33,
            Kind::Other | Kind::Drums => 0,
        }
    }
}

/// What the file says about a part.
#[derive(Debug, Default)]
pub(crate) struct Clues<'a> {
    /// Its name and its instruments' names.
    pub(crate) names: Vec<&'a str>,
    /// Its instruments' sounds (MusicXML's sound ids, such as
    /// `pluck.guitar.nylon-string`).
    pub(crate) sounds: Vec<&'a str>,
    /// Its General MIDI program, 0-based.
    pub(crate) program: Option<u8>,
    /// It plays on MIDI channel 10, or has a percussion clef or unpitched
    /// notes.
    pub(crate) percussion: bool,
    /// It has a tablature staff or a staff with tuned strings.
    pub(crate) strings: bool,
    /// The number of strings, when a staff gives them.
    pub(crate) string_count: Option<usize>,
}

/// The kind of instrument the clues point to.
pub(crate) fn kind(clues: &Clues<'_>) -> Kind {
    if clues.percussion {
        return Kind::Drums;
    }
    let sound_is = |prefix: &str| clues.sounds.iter().any(|s| s.starts_with(prefix));
    let named = |words: &[&str]| {
        clues.names.iter().any(|n| {
            let n = n.to_lowercase();
            words.iter().any(|w| n.contains(w))
        })
    };
    let bass = sound_is("pluck.bass")
        || clues.program.is_some_and(|p| (32..=39).contains(&p))
        || named(BASS_NAMES);
    let guitar = sound_is("pluck.guitar")
        || clues.program.is_some_and(|p| (24..=31).contains(&p))
        || named(GUITAR_NAMES);
    match (guitar, bass) {
        (_, true) if !guitar || clues.string_count.is_some_and(|n| n <= 5) => Kind::Bass,
        (true, _) => Kind::Guitar,
        _ if clues.strings && clues.string_count.is_some_and(|n| n <= 5) => Kind::Bass,
        _ if clues.strings => Kind::Guitar,
        _ => Kind::Other,
    }
}

/// Words in a part's or instrument's name that make it a guitar, in the
/// languages MusicXML files usually come in.
const GUITAR_NAMES: &[&str] = &[
    "guitar",
    "guit.",
    "gtr",
    "violão",
    "violao",
    "guitarra",
    "gitarre",
    "chitarra",
    "guitare",
    "gitaar",
    "gitara",
    "cavaquinho",
    "ukulele",
    "banjo",
    "mandolin",
];

/// Words that make it a bass guitar ("bass" alone is also a voice, a
/// clarinet or a double bass).
const BASS_NAMES: &[&str] = &[
    "bass guitar",
    "electric bass",
    "acoustic bass",
    "fretless bass",
    "e-bass",
    "el. bass",
    "baixo",
    "bajo eléctrico",
    "basse électrique",
];

/// Whether a guitar's name says nylon strings.
fn is_nylon(name: &str) -> bool {
    let n = name.to_lowercase();
    [
        "classical",
        "nylon",
        "violão",
        "violao",
        "spanish",
        "klassik",
    ]
    .iter()
    .any(|w| n.contains(w))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kinds_from_clues() {
        let named = |n: &'static str| Clues {
            names: vec![n],
            ..Clues::default()
        };
        assert_eq!(kind(&named("Violão")), Kind::Guitar);
        assert_eq!(kind(&named("Electric Bass")), Kind::Bass);
        assert_eq!(kind(&named("Bass")), Kind::Other, "a singer, maybe");
        assert_eq!(kind(&named("Flute")), Kind::Other);
        let program = Clues {
            program: Some(33),
            ..Clues::default()
        };
        assert_eq!(kind(&program), Kind::Bass);
        let drums = Clues {
            percussion: true,
            names: vec!["Guitar"],
            ..Clues::default()
        };
        assert_eq!(kind(&drums), Kind::Drums);
        let tab = Clues {
            strings: true,
            string_count: Some(4),
            ..Clues::default()
        };
        assert_eq!(kind(&tab), Kind::Bass);
        let sound = Clues {
            sounds: vec!["pluck.guitar.nylon-string"],
            ..Clues::default()
        };
        assert_eq!(kind(&sound), Kind::Guitar);
        assert_eq!(Kind::Guitar.default_program("Classical Guitar"), 24);
    }
}
