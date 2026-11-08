// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Mateus Rocha de Paula and the OxiGuitar contributors

//! Reading the repository's own MusicXML file, writing it back, and
//! reading damaged files.

use oxiguitar_model::{
    BeatKind, Clef, Direction, Dynamic, GraceKind, Hairpin, NotePlace, Pitch, Score,
    StrokeDirection, Tuplet, wire,
};
use oxiguitar_musicxml_plugin::{read, write};

fn fixture() -> Vec<u8> {
    let path =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/oxiguitar-test.musicxml");
    std::fs::read(path).unwrap()
}

fn score(data: &[u8]) -> Score {
    wire::from_wire2(read(data).unwrap().score).unwrap()
}

#[test]
fn reads_the_header_parts_and_bars() {
    let read = read(&fixture()).unwrap();
    assert!(read.log.is_empty(), "{:#?}", read.log);
    let s = wire::from_wire2(read.score).unwrap();
    assert_eq!(s.info.title, "OxiGuitar Test Song");
    assert_eq!(s.info.subtitle, "For the MusicXML reader");
    assert_eq!(s.info.music, "The OxiGuitar contributors");
    assert_eq!(s.info.words, "Nobody in particular");
    assert_eq!(s.info.notices, vec!["Arranger: A. Ranger"]);
    assert_eq!(s.info.copyright, "CC0");
    assert_eq!(s.initial_tempo, 96.0);
    assert_eq!(s.master_bars.len(), 4);
    let m = &s.master_bars;
    assert_eq!(
        (
            m[0].time_signature.numerator,
            m[0].time_signature.denominator
        ),
        (3, 4)
    );
    assert_eq!(m[0].key_signature.fifths, -1);
    assert_eq!(m[0].marker.as_ref().unwrap().letter, "A");
    assert!(m[0].double_bar);
    assert!(m[1].repeat_start);
    assert_eq!(m[2].repeat_end, Some(2));
    assert_eq!((m[2].alternate_endings, m[3].alternate_endings), (1, 2));
    assert_eq!(m[0].fermatas.len(), 1);
    assert_eq!(s.tracks.len(), 3);
    let names: Vec<&str> = s.tracks.iter().map(|t| t.name.as_str()).collect();
    assert_eq!(names, ["Guitar", "Flute", "Drumset"]);
    // Bars add up everywhere.
    for t in &s.tracks {
        for st in &t.staves {
            for (b, (bar, master)) in st.bars.iter().zip(&s.master_bars).enumerate() {
                let len = bar.voices[0].length().unwrap();
                assert!(
                    len == master.expected_length().unwrap(),
                    "{} bar {}",
                    t.name,
                    b + 1
                );
            }
        }
    }
}

#[test]
fn a_guitar_takes_its_strings_and_frets_from_its_tablature() {
    let s = score(&fixture());
    let g = &s.tracks[0];
    assert_eq!(g.instrument.midi_program, 24, "nylon, as the file says");
    assert_eq!(g.staves.len(), 1, "notation and tablature are one staff");
    let st = &g.staves[0];
    assert!(st.show_standard && st.show_tablature);
    let drop_d: Vec<Pitch> = [64, 59, 55, 50, 45, 38].map(Pitch).to_vec();
    assert_eq!(st.tuning, drop_d);
    assert_eq!(st.transposition, 0, "the 8vb clef carries the octave");
    assert_eq!(st.bars[0].clef, Clef::Treble8vb);
    let beats = &st.bars[0].voices[0].beats;
    // D3, written D4 an octave up: string 4, open.
    assert_eq!(
        beats[0].notes[0].place,
        NotePlace::Fretted { string: 4, fret: 0 }
    );
    assert_eq!(beats[0].duration.dots, 1);
    assert!(beats[0].slur, "slurred to the E");
    assert!(!beats[1].slur);
    assert!(beats[1].notes[0].effects.hammer_pull);
    assert_eq!(beats[0].effects.dynamic, Some(Dynamic::MP));
    let chord: Vec<NotePlace> = beats[2].notes.iter().map(|n| n.place).collect();
    assert_eq!(
        chord,
        [
            NotePlace::Fretted { string: 5, fret: 0 },
            NotePlace::Fretted { string: 4, fret: 3 }
        ]
    );
    assert_eq!(g.chords[0].name, "Dm");
    assert_eq!(
        g.chords[0].frets,
        vec![Some(1), Some(3), Some(2), Some(0), None, None]
    );
    assert_eq!(beats[0].chord, Some(0));
    assert_eq!(g.lyrics[0].text, "Gui-tar song");
    // Bar 2: a grace note, a triplet with a tie into it, a sharp kept as
    // written, and a second voice after a forward.
    let b2 = &st.bars[1];
    assert_eq!(b2.voices.len(), 2);
    let v1 = &b2.voices[0].beats;
    assert_eq!(v1[0].grace, Some(GraceKind::BeforeBeat));
    assert_eq!(v1[1].duration.tuplets, vec![Tuplet::TRIPLET]);
    assert!(v1[1].notes[0].tie);
    assert_eq!(v1[4].kind, BeatKind::Rest);
    let v2 = &b2.voices[1].beats;
    assert_eq!(v2[0].kind, BeatKind::Empty, "the forward");
    assert_eq!(v2[1].notes[0].place, NotePlace::Pitched(Pitch(38)));
    assert_eq!(
        st.bars[2].voices[0].beats[0].hairpin,
        Some(Hairpin::Crescendo)
    );
    assert_eq!(st.bars[3].voices[0].beats[0].text, "let ring");
}

#[test]
fn other_parts_get_a_guitars_strings_drums_none() {
    let s = score(&fixture());
    let flute = &s.tracks[1];
    assert_eq!(flute.instrument.midi_program, 73);
    let st = &flute.staves[0];
    assert_eq!(st.tuning.len(), 6, "fingered as it opens");
    assert!(st.show_standard && !st.show_tablature, "Mateus, 8 Nov 2026");
    let drums = &s.tracks[2];
    assert!(drums.instrument.percussion);
    assert!(drums.staves[0].tuning.is_empty());
    let keys: Vec<NotePlace> = drums.staves[0].bars[0].voices[0]
        .beats
        .iter()
        .map(|b| b.notes[0].place)
        .collect();
    assert_eq!(keys, [36, 38, 36].map(|k| NotePlace::Pitched(Pitch(k))));
    assert_eq!(drums.staves[0].bars[0].clef, Clef::Neutral);
}

#[test]
fn writes_what_it_reads() {
    let first = read(&fixture()).unwrap().score;
    for compressed in [false, true] {
        let written = write(&first, compressed).unwrap();
        assert!(written.losses.is_empty(), "{:#?}", written.losses);
        assert_eq!(written.data.starts_with(b"PK\x03\x04"), compressed);
        let again = read(&written.data).unwrap();
        assert!(again.log.is_empty(), "{:#?}", again.log);
        assert_eq!(
            wire::from_wire2(again.score).unwrap(),
            wire::from_wire2(first.clone()).unwrap(),
            "compressed {compressed}"
        );
    }
}

#[test]
fn texts_with_markup_characters_survive() {
    let mut s = score(&fixture());
    s.info.title = "Tom & Jerry <live> \"at last\"".into();
    s.info.album = "An album".into();
    s.tracks[0].name = "Gtr <1> & co".into();
    s.tracks[0].staves[0].bars[0].voices[0].beats[0].text = "a < b".into();
    let written = write(&wire::to_wire2(&s), false).unwrap();
    let back = score(&written.data);
    assert_eq!(back.info.title, s.info.title);
    assert_eq!(back.info.album, "An album");
    assert_eq!(back.tracks[0].name, s.tracks[0].name);
    assert_eq!(
        back.tracks[0].staves[0].bars[0].voices[0].beats[0].text,
        "a < b"
    );
}

#[test]
fn names_what_it_cant_write() {
    let mut s = score(&fixture());
    let beat = &mut s.tracks[0].staves[0].bars[0].voices[0].beats[0];
    beat.notes[0].effects.palm_mute = true;
    beat.effects.stroke = Some(oxiguitar_model::Stroke {
        direction: StrokeDirection::Down,
        speed: oxiguitar_model::NoteValue::Sixteenth,
    });
    let written = write(&wire::to_wire2(&s), false).unwrap();
    let messages: Vec<&str> = written.losses.iter().map(|l| l.message.as_str()).collect();
    assert!(
        messages.contains(&"palm mutes are left out"),
        "{messages:?}"
    );
    assert!(
        messages.iter().any(|m| m.starts_with("strums")),
        "{messages:?}"
    );
    assert_eq!(written.losses[0].place, "bar 1, track 1");
}

#[test]
fn writes_an_empty_score_and_reads_it_back() {
    let empty = oxiguitar_plugin::score2::Score2::default();
    let written = write(&empty, false).unwrap();
    let back = score(&written.data);
    assert!(back.tracks.is_empty());
}

#[test]
fn timewise_files_read_as_partwise_ones() {
    let timewise = r#"<?xml version="1.0" encoding="UTF-8"?>
<score-timewise version="4.0">
  <part-list><score-part id="P1"><part-name>Guitar</part-name></score-part></part-list>
  <measure number="1">
    <part id="P1">
      <attributes><divisions>1</divisions><time><beats>2</beats><beat-type>4</beat-type></time></attributes>
      <note><pitch><step>E</step><octave>4</octave></pitch><duration>2</duration><type>half</type></note>
    </part>
  </measure>
</score-timewise>"#;
    let s = score(timewise.as_bytes());
    assert_eq!(s.tracks[0].name, "Guitar");
    assert_eq!(s.tracks[0].staves[0].bars[0].voices[0].beats.len(), 1);
}

#[test]
fn damaged_files_are_errors_never_panics() {
    let data = fixture();
    for cut in (0..data.len()).step_by(37) {
        let _ = read(&data[..cut]);
    }
    let mut seed = 0x2545_f491_4f6c_dd1du64;
    for _ in 0..300 {
        let mut d = data.clone();
        for _ in 0..3 {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            let i = (seed as usize) % d.len();
            d[i] = (seed >> 32) as u8;
        }
        let _ = read(&d);
    }
    let mxl = write(&read(&data).unwrap().score, true).unwrap().data;
    for cut in 0..mxl.len() {
        let _ = read(&mxl[..cut]);
    }
    for bad in [&b""[..], b"\xff", b"<", b"<score-partwise>", b"PK\x03\x04"] {
        assert!(read(bad).is_err(), "{bad:?}");
    }
}

#[test]
fn reads_jumps_and_signs() {
    let xml = r#"<?xml version="1.0"?>
<score-partwise version="4.0">
  <part-list><score-part id="P1"><part-name>Guitar</part-name></score-part></part-list>
  <part id="P1">
    <measure number="1">
      <attributes><divisions>1</divisions><time><beats>1</beats><beat-type>4</beat-type></time></attributes>
      <direction><direction-type><segno/></direction-type><sound segno="s"/></direction>
      <note><rest/><duration>1</duration><type>quarter</type></note>
    </measure>
    <measure number="2">
      <note><rest/><duration>1</duration><type>quarter</type></note>
      <direction><direction-type><words>D.S. al Fine</words></direction-type><sound dalsegno="s"/></direction>
    </measure>
  </part>
</score-partwise>"#;
    let s = score(xml.as_bytes());
    assert_eq!(s.master_bars[0].directions, vec![Direction::Segno]);
    assert_eq!(s.master_bars[1].directions, vec![Direction::DalSegnoAlFine]);
    assert!(
        s.tracks[0].staves[0].bars[1].voices[0].beats[0]
            .text
            .is_empty(),
        "the jump's words"
    );
    let again = score(&write(&wire::to_wire2(&s), false).unwrap().data);
    assert_eq!(again.master_bars, s.master_bars);
}
