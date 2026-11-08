// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Mateus Rocha de Paula and the OxiGuitar contributors

//! Writing OxiGuitar's score as MusicXML 4.0 (docs/design.md, "Writing"),
//! built from the musicxml crate's elements.
//!
//! Each track is a part. Each of its staves is written as a notation staff,
//! a tablature staff, or both (as MuseScore and Guitar Pro write a guitar),
//! the tablature's notes carrying their string and fret and its
//! `staff-details` the strings' tuning and the capo, so the fingering
//! survives a round trip through any program that reads tablature. A
//! guitar's usual octave (OxiGuitar's treble 8vb clef) is written as other
//! programs write it: an 8vb clef, pitches an octave up, and a `transpose`
//! of an octave down.

use std::collections::BTreeMap;

use musicxml::datatypes as dt;
use musicxml::elements as mx;
use oxiguitar_model::{
    Accent, BeatKind, BeatTechnique, Clef, Direction, Dynamic, FermataKind, GraceKind, Hairpin,
    Harmonic, KeyMode, NotePlace, Ornament, Ottava, Pedal, Pitch, Ratio, Score, Spelling, Staff,
    StrokeDirection, Track, lyrics,
};

use crate::log::{Log, place};
use crate::text::escape;
use crate::time::note_type;

/// The score as MusicXML, with what the file can't hold.
pub(crate) fn write(score: &Score) -> Result<(Vec<u8>, Log), String> {
    let mut log = Log::default();
    let file = Writer {
        score,
        log: &mut log,
    }
    .file();
    let xml = musicxml::write_partwise_score_data(&file, false, false)?;
    // The crate names MusicXML 3.0's document type on every file; this one
    // is 4.0, as its version says.
    let xml = replace_once(
        &xml,
        b"-//Recordare//DTD MusicXML 3.0 Partwise//EN",
        b"-//Recordare//DTD MusicXML 4.0 Partwise//EN",
    );
    Ok((xml, log))
}

/// `data` with the first `from` replaced by `to`.
fn replace_once(data: &[u8], from: &[u8], to: &[u8]) -> Vec<u8> {
    match data.windows(from.len()).position(|w| w == from) {
        Some(at) => [&data[..at], to, &data[at + from.len()..]].concat(),
        None => data.to_vec(),
    }
}

/// A note value's MusicXML, an OxiGuitar staff's place among MusicXML's.
#[derive(Clone, Copy, Debug)]
struct Layout {
    /// MusicXML's staff number of its notation staff, if shown.
    notation: Option<u32>,
    /// MusicXML's staff number of its tablature staff, if written.
    tab: Option<u32>,
    /// Pitches are written an octave above OxiGuitar's (an 8vb clef with a
    /// transposition of an octave down).
    octave_up: bool,
}

struct Writer<'a> {
    score: &'a Score,
    log: &'a mut Log,
}

impl Writer<'_> {
    fn file(&mut self) -> mx::ScorePartwise {
        let info = &self.score.info;
        let mut creators = Vec::new();
        let mut creator = |kind: &str, text: &str| {
            if !text.trim().is_empty() {
                creators.push(mx::Creator {
                    attributes: mx::CreatorAttributes {
                        r#type: Some(dt::Token(escape(kind))),
                    },
                    content: escape(text.trim()),
                });
            }
        };
        creator("composer", &info.music);
        creator("lyricist", &info.words);
        creator("transcriber", &info.transcriber);
        if !info.artist.trim().is_empty() {
            creator("artist", &info.artist);
        }
        // Notices the reader made of other creators ("Arranger: …") go
        // back as those creators; the album, the instructions and other
        // notices as named fields, where MuseScore keeps such texts too.
        let mut fields: Vec<(&str, &str)> = Vec::new();
        if !info.album.trim().is_empty() {
            fields.push(("album", info.album.trim()));
        }
        if !info.instructions.trim().is_empty() {
            fields.push(("instructions", info.instructions.trim()));
        }
        for notice in &info.notices {
            match notice.split_once(": ") {
                Some((kind, text))
                    if !kind.is_empty() && kind.chars().all(|c| c.is_alphabetic() || c == ' ') =>
                {
                    creator(&kind.to_lowercase(), text)
                }
                _ => fields.push(("notice", notice.as_str())),
            }
        }
        let rights = if info.copyright.trim().is_empty() {
            Vec::new()
        } else {
            vec![mx::Rights {
                attributes: mx::RightsAttributes { r#type: None },
                content: escape(info.copyright.trim()),
            }]
        };
        let identification = mx::Identification {
            attributes: (),
            content: mx::IdentificationContents {
                creator: creators,
                rights,
                encoding: Some(mx::Encoding {
                    attributes: (),
                    content: vec![mx::EncodingContents::Software(mx::Software {
                        attributes: (),
                        content: "OxiGuitar".into(),
                    })],
                }),
                source: None,
                relation: Vec::new(),
                miscellaneous: (!fields.is_empty()).then(|| mx::Miscellaneous {
                    attributes: (),
                    content: mx::MiscellaneousContents {
                        miscellaneous_field: fields
                            .into_iter()
                            .map(|(name, text)| mx::MiscellaneousField {
                                attributes: mx::MiscellaneousFieldAttributes {
                                    name: dt::Token(name.into()),
                                },
                                content: escape(text),
                            })
                            .collect(),
                    },
                }),
            },
        };
        let mut part_list = Vec::new();
        let mut parts = Vec::new();
        for (t, track) in self.score.tracks.iter().enumerate() {
            part_list.push(mx::PartListElement::ScorePart(self.score_part(t, track)));
            parts.push(self.part(t, track));
        }
        if !self.score.layout.is_empty() || self.score.tracks.iter().any(|t| !t.layout.is_empty()) {
            self.log
                .lost("Design Mode's bars per line are left out", "the score");
        }
        mx::ScorePartwise {
            attributes: mx::ScorePartwiseAttributes {
                version: Some(dt::Token("4.0".into())),
            },
            content: mx::ScorePartwiseContents {
                work: (!info.title.is_empty()).then(|| mx::Work {
                    attributes: (),
                    content: mx::WorkContents {
                        work_number: None,
                        work_title: Some(mx::WorkTitle {
                            attributes: (),
                            content: escape(&info.title),
                        }),
                        opus: None,
                    },
                }),
                movement_number: None,
                movement_title: (!info.subtitle.is_empty()).then(|| mx::MovementTitle {
                    attributes: (),
                    content: escape(&info.subtitle),
                }),
                identification: Some(identification),
                defaults: None,
                credit: Vec::new(),
                part_list: mx::PartList {
                    attributes: (),
                    content: mx::PartListContents { content: part_list },
                },
                part: parts,
            },
        }
    }

    fn score_part(&mut self, t: usize, track: &Track) -> mx::ScorePart {
        let id = part_id(t);
        let instruments: Vec<(String, String, Option<u8>)> = if track.instrument.percussion {
            drum_keys(track)
                .into_iter()
                .map(|k| (format!("{id}-I{}", u16::from(k) + 1), drum_name(k), Some(k)))
                .collect()
        } else {
            vec![(format!("{id}-I1"), track.name.clone(), None)]
        };
        let instruments = if instruments.is_empty() {
            vec![(format!("{id}-I1"), track.name.clone(), None)]
        } else {
            instruments
        };
        mx::ScorePart {
            attributes: mx::ScorePartAttributes {
                id: dt::Id(id.clone()),
            },
            content: mx::ScorePartContents {
                identification: None,
                part_link: Vec::new(),
                part_name: mx::PartName {
                    attributes: mx::PartNameAttributes::default(),
                    content: escape(&track.name),
                },
                part_name_display: None,
                part_abbreviation: (!track.short_name.is_empty()).then(|| mx::PartAbbreviation {
                    attributes: mx::PartAbbreviationAttributes::default(),
                    content: escape(&track.short_name),
                }),
                part_abbreviation_display: None,
                group: Vec::new(),
                score_instrument: instruments
                    .iter()
                    .map(|(iid, name, _)| mx::ScoreInstrument {
                        attributes: mx::ScoreInstrumentAttributes {
                            id: dt::Id(iid.clone()),
                        },
                        content: mx::ScoreInstrumentContents {
                            instrument_name: mx::InstrumentName {
                                attributes: (),
                                content: if name.is_empty() {
                                    "Instrument".into()
                                } else {
                                    escape(name)
                                },
                            },
                            instrument_abbreviation: None,
                            instrument_sound: None,
                            solo: None,
                            ensemble: None,
                            virtual_instrument: None,
                        },
                    })
                    .collect(),
                player: Vec::new(),
                midi_device: Vec::new(),
                midi_instrument: instruments
                    .iter()
                    .map(|(iid, _, key)| mx::MidiInstrument {
                        attributes: mx::MidiInstrumentAttributes {
                            id: dt::IdRef(iid.clone()),
                        },
                        content: mx::MidiInstrumentContents {
                            midi_channel: Some(mx::MidiChannel {
                                attributes: (),
                                content: dt::Midi16(if track.instrument.percussion {
                                    10
                                } else {
                                    u8::try_from(t % 15 + 1)
                                        .unwrap_or(1)
                                        .min(9 + u8::from(t % 15 >= 9))
                                }),
                            }),
                            midi_name: None,
                            midi_bank: None,
                            midi_program: Some(mx::MidiProgram {
                                attributes: (),
                                content: dt::Midi128(
                                    track.instrument.midi_program.saturating_add(1).min(128),
                                ),
                            }),
                            midi_unpitched: key.map(|k| mx::MidiUnpitched {
                                attributes: (),
                                content: dt::Midi128(k.saturating_add(1).min(128)),
                            }),
                            volume: Some(mx::Volume {
                                attributes: (),
                                content: dt::Percent(
                                    (f64::from(track.mix.volume.clamp(0.0, 1.0)) * 1_000_000.0)
                                        .round()
                                        / 10_000.0,
                                ),
                            }),
                            pan: Some(mx::Pan {
                                attributes: (),
                                content: dt::RotationDegrees(
                                    (track.mix.pan.clamp(-1.0, 1.0) * 90.0).round(),
                                ),
                            }),
                            elevation: None,
                        },
                    })
                    .collect(),
            },
        }
    }

    /// The MusicXML staves of each of a track's staves.
    fn layouts(&self, track: &Track) -> Vec<Layout> {
        let mut next = 1u32;
        track
            .staves
            .iter()
            .map(|s| {
                let has_tab =
                    !track.instrument.percussion && !s.tuning.is_empty() && s.show_tablature;
                let notation = (s.show_standard || !has_tab).then(|| {
                    next += 1;
                    next - 1
                });
                let tab = has_tab.then(|| {
                    next += 1;
                    next - 1
                });
                let octave_up = s
                    .bars
                    .first()
                    .is_some_and(|b| matches!(b.clef, Clef::Treble8vb | Clef::Bass8vb));
                Layout {
                    notation,
                    tab,
                    octave_up,
                }
            })
            .collect()
    }

    fn part(&mut self, t: usize, track: &Track) -> mx::Part {
        let layouts = self.layouts(track);
        let divisions = divisions(track);
        let staves: u32 = layouts
            .iter()
            .map(|l| u32::from(l.notation.is_some()) + u32::from(l.tab.is_some()))
            .sum();
        let mut measures = Vec::new();
        for m in 0..self.score.master_bars.len() {
            measures.push(mx::PartElement::Measure(
                self.measure(t, track, m, &layouts, divisions, staves),
            ));
        }
        if track.play_chords || !track.plugins.is_empty() || !track.effects.is_empty() {
            self.log.lost(
                "a track's sound settings (its plugins, effects and played chords) are left out",
                place(0, t),
            );
        }
        mx::Part {
            attributes: mx::PartAttributes {
                id: dt::IdRef(part_id(t)),
            },
            content: measures,
        }
    }

    #[allow(clippy::too_many_lines)]
    fn measure(
        &mut self,
        t: usize,
        track: &Track,
        m: usize,
        layouts: &[Layout],
        divisions: i64,
        staves: u32,
    ) -> mx::Measure {
        let master = &self.score.master_bars[m];
        let prev = m.checked_sub(1).and_then(|p| self.score.master_bars.get(p));
        let mut out: Vec<mx::MeasureElement> = Vec::new();
        // The left barline: a repeat's start, an ending's start.
        let ending_start = master.alternate_endings != 0
            && prev.is_none_or(|p| p.alternate_endings != master.alternate_endings);
        if master.repeat_start || ending_start {
            out.push(mx::MeasureElement::Barline(barline(
                dt::RightLeftMiddle::Left,
                master.repeat_start.then_some(dt::BarStyle::HeavyLight),
                master
                    .repeat_start
                    .then_some((dt::BackwardForward::Forward, None)),
                ending_start.then(|| {
                    (
                        endings(master.alternate_endings),
                        dt::StartStopDiscontinue::Start,
                    )
                }),
            )));
        }
        // Attributes: everything in the first measure, then what changes.
        let first = m == 0;
        let mut attrs = mx::AttributesContents::default();
        if first {
            attrs.divisions = Some(mx::Divisions {
                attributes: (),
                content: dt::PositiveDivisions(u32::try_from(divisions).unwrap_or(1)),
            });
        }
        if first || prev.is_some_and(|p| p.key_signature != master.key_signature) {
            attrs.key.push(mx::Key {
                attributes: mx::KeyAttributes::default(),
                content: mx::KeyContents::Explicit(mx::ExplicitKeyContents {
                    cancel: None,
                    fifths: mx::Fifths {
                        attributes: (),
                        content: dt::Fifths(master.key_signature.fifths),
                    },
                    mode: Some(mx::Mode {
                        attributes: (),
                        content: match master.key_signature.mode {
                            KeyMode::Minor => dt::Mode::Minor,
                            KeyMode::Major => dt::Mode::Major,
                        },
                    }),
                    key_octave: Vec::new(),
                }),
            });
        }
        let time_changed = prev.is_some_and(|p| {
            p.time_signature != master.time_signature || p.free_time != master.free_time
        });
        if first || time_changed {
            attrs.time.push(mx::Time {
                attributes: mx::TimeAttributes::default(),
                content: mx::TimeContents {
                    beats: if master.free_time {
                        Vec::new()
                    } else {
                        vec![mx::TimeBeatContents {
                            beats: mx::Beats {
                                attributes: (),
                                content: master.time_signature.numerator.to_string(),
                            },
                            beat_type: mx::BeatType {
                                attributes: (),
                                content: master.time_signature.denominator.to_string(),
                            },
                        }]
                    },
                    interchangeable: None,
                    senza_misura: master.free_time.then(|| mx::SenzaMisura {
                        attributes: (),
                        content: String::new(),
                    }),
                },
            });
        }
        if first && staves > 1 {
            attrs.staves = Some(mx::Staves {
                attributes: (),
                content: dt::NonNegativeInteger(staves),
            });
        }
        for (s, (staff, layout)) in track.staves.iter().zip(layouts).enumerate() {
            let clef = staff.bars.get(m).map_or(Clef::Treble, |b| b.clef);
            let prev_clef = m
                .checked_sub(1)
                .and_then(|p| staff.bars.get(p))
                .map(|b| b.clef);
            if let Some(n) = layout.notation
                && (first || prev_clef != Some(clef))
            {
                attrs.clef.push(clef_element(
                    if track.instrument.percussion {
                        Clef::Neutral
                    } else {
                        clef
                    },
                    n,
                    staves > 1,
                ));
            }
            if first && let Some(tab) = layout.tab {
                attrs.clef.push(tab_clef(tab));
                attrs.staff_details.push(staff_details(staff, tab));
            }
            if first {
                // Written to sounding: OxiGuitar's transposition is sounding
                // to written; the 8vb clef's octave is written as other
                // programs write it.
                let total = written_to_sounding(staff, *layout);
                if total != 0 {
                    for number in [layout.notation, layout.tab].into_iter().flatten() {
                        attrs.transpose.push(mx::Transpose {
                            attributes: mx::TransposeAttributes {
                                id: None,
                                number: (staves > 1)
                                    .then(|| dt::StaffNumber(u8::try_from(number).unwrap_or(1))),
                            },
                            content: mx::TransposeContents {
                                diatonic: None,
                                chromatic: mx::Chromatic {
                                    attributes: (),
                                    content: dt::Semitones(i16::try_from(total % 12).unwrap_or(0)),
                                },
                                octave_change: (total / 12 != 0).then(|| mx::OctaveChange {
                                    attributes: (),
                                    content: i8::try_from(total / 12).unwrap_or(0),
                                }),
                                double: None,
                            },
                        });
                    }
                }
            }
            if staff.bars.get(m).and_then(|b| b.simile).is_some() {
                self.log.lost(
                    "simile signs are left out (their bars are written out)",
                    place(m, t),
                );
            }
            let _ = s;
        }
        if attrs != mx::AttributesContents::default() {
            out.push(mx::MeasureElement::Attributes(mx::Attributes {
                attributes: (),
                content: attrs,
            }));
        }
        if master.triplet_feel != oxiguitar_model::TripletFeel::None {
            self.log.lost("triplet feel is left out", place(m, t));
        }
        // Directions at the bar's start (on the first part only, as they
        // belong to every track).
        if t == 0 {
            if let Some(marker) = &master.marker {
                let text = if marker.letter.is_empty() {
                    &marker.text
                } else {
                    &marker.letter
                };
                if !text.is_empty() {
                    out.push(direction(
                        vec![mx::DirectionTypeContents::Rehearsal(vec![mx::Rehearsal {
                            attributes: mx::RehearsalAttributes::default(),
                            content: escape(text),
                        }])],
                        None,
                        None,
                    ));
                }
                if !marker.letter.is_empty() && !marker.text.is_empty() {
                    out.push(words(&marker.text, None));
                }
            }
            for d in &master.directions {
                if let Some(e) = start_direction(*d) {
                    out.push(e);
                }
            }
            if m == 0 {
                out.push(tempo(self.score.initial_tempo, &self.score.tempo_label, 0));
            }
            // Tempo changes belong to the bar, whatever its voices hold:
            // at its start, offset to where they are.
            for c in &master.tempo_changes {
                if m == 0 && c.position == Ratio::ZERO {
                    continue;
                }
                out.push(tempo(c.bpm, &c.label, to_divisions(c.position, divisions)));
            }
        }
        // The music, staff by staff, voice by voice, each from the bar's
        // start.
        // What the voice just written lasted: the next goes back by it.
        let mut written: Option<Ratio> = None;
        for (s, (staff, layout)) in track.staves.iter().zip(layouts).enumerate() {
            let Some(bar) = staff.bars.get(m) else {
                continue;
            };
            for number in [layout.notation, layout.tab].into_iter().flatten() {
                let is_tab = Some(number) == layout.tab;
                for (v, voice) in bar.voices.iter().enumerate().take(4) {
                    if voice.is_empty() && v > 0 {
                        continue;
                    }
                    if let Some(len) = written {
                        let back = to_divisions(len.max(Ratio::ZERO), divisions);
                        if back > 0 {
                            out.push(mx::MeasureElement::Backup(mx::Backup {
                                attributes: (),
                                content: mx::BackupContents {
                                    duration: mx::Duration {
                                        attributes: (),
                                        content: dt::PositiveDivisions(
                                            u32::try_from(back).unwrap_or(1),
                                        ),
                                    },
                                    footnote: None,
                                    level: None,
                                },
                            }));
                        }
                    }
                    let voice_no = (number - 1) * 4 + u32::try_from(v).unwrap_or(0) + 1;
                    // The staff's marks (directions, chord symbols, lyrics,
                    // slurs, fermatas) go on its first MusicXML staff: the
                    // notation, or the tablature when it is shown alone.
                    let primary = Some(number) == layout.notation.or(layout.tab);
                    let first_voice = s == 0 && v == 0 && primary;
                    self.voice(
                        &mut out,
                        VoiceAt {
                            t,
                            m,
                            track,
                            staff,
                            layout: *layout,
                            staff_number: if staves > 1 { Some(number) } else { None },
                            voice: voice_no,
                            index: v,
                            is_tab,
                            primary,
                            first_voice,
                            divisions,
                        },
                    );
                    written = Some(voice_len(voice));
                }
            }
        }
        // Jumps at the bar's end.
        if t == 0 {
            for d in &master.directions {
                if let Some(e) = end_direction(*d) {
                    out.push(e);
                }
            }
        }
        // The right barline: a repeat's end, an ending's end, a double bar,
        // the final bar.
        let next = self.score.master_bars.get(m + 1);
        let ending_end = master.alternate_endings != 0
            && next.is_none_or(|n| n.alternate_endings != master.alternate_endings);
        let last = next.is_none();
        let style = if master.repeat_end.is_some() || last {
            Some(dt::BarStyle::LightHeavy)
        } else if master.double_bar {
            Some(dt::BarStyle::LightLight)
        } else {
            None
        };
        if style.is_some() || ending_end {
            out.push(mx::MeasureElement::Barline(barline(
                dt::RightLeftMiddle::Right,
                style,
                master.repeat_end.map(|times| {
                    (
                        dt::BackwardForward::Backward,
                        (times != 2).then_some(u32::from(times)),
                    )
                }),
                ending_end.then(|| {
                    (
                        endings(master.alternate_endings),
                        if master.repeat_end.is_some() {
                            dt::StartStopDiscontinue::Stop
                        } else {
                            dt::StartStopDiscontinue::Discontinue
                        },
                    )
                }),
            )));
        }
        mx::Measure {
            attributes: mx::MeasureAttributes {
                number: dt::Token((m + 1).to_string()),
                id: None,
                implicit: None,
                non_controlling: None,
                text: None,
                width: None,
            },
            content: out,
        }
    }

    /// One voice of a bar, on one MusicXML staff.
    #[allow(clippy::too_many_lines)]
    fn voice(&mut self, out: &mut Vec<mx::MeasureElement>, at: VoiceAt<'_>) {
        let VoiceAt {
            t,
            m,
            track,
            staff,
            layout,
            staff_number,
            voice,
            index,
            is_tab,
            primary,
            first_voice,
            divisions,
        } = at;
        let master = &self.score.master_bars[m];
        let Some(bar) = staff.bars.get(m) else { return };
        let Some(v) = bar.voices.get(index) else {
            return;
        };
        let key = master.key_signature;
        let mut sung = if first_voice {
            syllables_here(track, staff, m)
        } else {
            BTreeMap::new()
        };
        let mut pos = Ratio::ZERO;
        let mut prev_hairpin: Option<Hairpin> = None;
        let mut prev_ottava: Option<Ottava> = None;
        if let Some(pb) = m.checked_sub(1).and_then(|p| staff.bars.get(p))
            && let Some(last) = pb.voices.get(index).and_then(|v| v.beats.last())
        {
            prev_hairpin = last.hairpin;
            prev_ottava = last.ottava;
        }
        let prev_slur = m
            .checked_sub(1)
            .and_then(|p| staff.bars.get(p))
            .and_then(|pb| pb.voices.get(index))
            .and_then(|v| v.beats.iter().rev().find(|b| b.grace.is_none()))
            .is_some_and(|b| b.slur);
        let mut slur_open = prev_slur;
        for (bi, beat) in v.beats.iter().enumerate() {
            let len = if beat.grace.is_some() {
                Ratio::ZERO
            } else {
                beat.duration.length().unwrap_or(Ratio::ZERO)
            };
            let dur = to_divisions(len, divisions);
            // Marks before the beat, on the first voice of the first staff.
            if first_voice && beat.grace.is_none() {
                if let Some(d) = beat.effects.dynamic {
                    out.push(direction(
                        vec![mx::DirectionTypeContents::Dynamics(vec![dynamics_element(
                            d,
                        )])],
                        staff_number,
                        Some(dt::AboveBelow::Below),
                    ));
                }
                if beat.hairpin != prev_hairpin {
                    let mut types = Vec::new();
                    if prev_hairpin.is_some() {
                        types.push(wedge(dt::WedgeType::Stop));
                    }
                    if let Some(h) = beat.hairpin {
                        types.push(wedge(match h {
                            Hairpin::Crescendo => dt::WedgeType::Crescendo,
                            Hairpin::Decrescendo => dt::WedgeType::Diminuendo,
                        }));
                    }
                    out.push(direction(types, staff_number, Some(dt::AboveBelow::Below)));
                    prev_hairpin = beat.hairpin;
                }
                if beat.ottava != prev_ottava {
                    let mut types = Vec::new();
                    if let Some(o) = prev_ottava {
                        types.push(octave_shift(dt::UpDownStopContinue::Stop, o));
                    }
                    if let Some(o) = beat.ottava {
                        let kind = match o {
                            Ottava::Alta8 | Ottava::Alta15 => dt::UpDownStopContinue::Down,
                            Ottava::Bassa8 | Ottava::Bassa15 => dt::UpDownStopContinue::Up,
                        };
                        types.push(octave_shift(kind, o));
                    }
                    out.push(direction(types, staff_number, None));
                    prev_ottava = beat.ottava;
                }
                if let Some(p) = beat.effects.pedal {
                    out.push(direction(
                        vec![mx::DirectionTypeContents::Pedal(mx::Pedal {
                            attributes: mx::PedalAttributes {
                                r#type: match p {
                                    Pedal::Down => dt::PedalType::Start,
                                    Pedal::Up => dt::PedalType::Stop,
                                },
                                abbreviated: None,
                                color: None,
                                default_x: None,
                                default_y: None,
                                font_family: None,
                                font_size: None,
                                font_style: None,
                                font_weight: None,
                                id: None,
                                line: None,
                                number: None,
                                relative_x: None,
                                relative_y: None,
                                sign: Some(dt::YesNo::Yes),
                            },
                            content: (),
                        })],
                        staff_number,
                        Some(dt::AboveBelow::Below),
                    ));
                }
                if !beat.text.trim().is_empty() {
                    out.push(words(beat.text.trim(), staff_number));
                }
                if let Some(chord) = beat.chord.and_then(|c| track.chords.get(c)) {
                    if let Some(h) = harmony(chord, staff_number) {
                        out.push(mx::MeasureElement::Harmony(h));
                    } else {
                        self.log.lost(
                            "chord names that aren't chord symbols are left out",
                            place(m, t),
                        );
                    }
                }
            }
            self.beat_losses(beat, m, t);
            if beat.kind == BeatKind::Empty {
                if dur > 0 {
                    out.push(mx::MeasureElement::Forward(mx::Forward {
                        attributes: (),
                        content: mx::ForwardContents {
                            duration: duration(dur),
                            footnote: None,
                            level: None,
                            voice: Some(voice_element(voice)),
                            staff: staff_number.map(staff_element),
                        },
                    }));
                }
                pos = pos.checked_add(len).unwrap_or(pos);
                continue;
            }
            let fermata = (first_voice && t == 0 || first_voice)
                .then(|| {
                    master
                        .fermatas
                        .iter()
                        .find(|f| f.position == pos && beat.grace.is_none())
                })
                .flatten()
                .map(|f| f.kind);
            // The slur: started on the first slurred beat, stopped on the
            // beat after the last.
            let slur_start = beat.slur && !slur_open;
            let slur_stop = slur_open && !beat.slur && beat.grace.is_none();
            if beat.grace.is_none() {
                slur_open = beat.slur;
            }
            let rest = beat.kind == BeatKind::Rest || beat.notes.is_empty();
            let count = if rest { 1 } else { beat.notes.len() };
            for ni in 0..count {
                let note = beat.notes.get(ni);
                let mut notations: Vec<mx::NotationContentTypes> = Vec::new();
                let mut technical: Vec<mx::TechnicalContents> = Vec::new();
                let mut ornaments: Vec<mx::OrnamentType> = Vec::new();
                let mut articulations: Vec<mx::ArticulationsType> = Vec::new();
                let mut ties = Vec::new();
                let mut audible = mx::AudibleType::Rest(mx::Rest {
                    attributes: mx::RestAttributes::default(),
                    content: mx::RestContents {
                        display_step: None,
                        display_octave: None,
                    },
                });
                let mut head = None;
                let mut instrument = Vec::new();
                if let Some(note) = note {
                    let sounding = note_pitch(note, staff);
                    if track.instrument.percussion {
                        let (step, octave) = drum_display(sounding);
                        audible = mx::AudibleType::Unpitched(mx::Unpitched {
                            attributes: (),
                            content: mx::UnpitchedContents {
                                display_step: mx::DisplayStep {
                                    attributes: (),
                                    content: step,
                                },
                                display_octave: mx::DisplayOctave {
                                    attributes: (),
                                    content: dt::Octave(octave),
                                },
                            },
                        });
                        instrument.push(mx::Instrument {
                            attributes: mx::InstrumentAttributes {
                                id: dt::IdRef(format!(
                                    "{}-I{}",
                                    part_id(t),
                                    u16::from(sounding.0) + 1
                                )),
                            },
                            content: (),
                        });
                    } else {
                        let written = i32::from(sounding.0) - written_to_sounding(staff, layout);
                        let written = Pitch(u8::try_from(written.clamp(0, 127)).unwrap_or(60));
                        let spelling = note
                            .spelling
                            .filter(|s| s.spells(written))
                            .unwrap_or_else(|| Spelling::automatic(written, key));
                        audible = mx::AudibleType::Pitch(pitch_element(spelling, written));
                    }
                    if note.tie {
                        ties.push(tie(dt::StartStop::Stop));
                        notations.push(mx::NotationContentTypes::Tied(tied(
                            dt::StartStopContinue::Stop,
                        )));
                    }
                    // A tie into the next beat: the next note on this
                    // string (or of this pitch) is tied.
                    if tied_to_next(staff, m, index, bi, note) {
                        ties.push(tie(dt::StartStop::Start));
                        notations.push(mx::NotationContentTypes::Tied(tied(
                            dt::StartStopContinue::Start,
                        )));
                    }
                    let fx = &note.effects;
                    if fx.dead {
                        head = Some(dt::NoteheadValue::X);
                    }
                    if let Some(a) = fx.accent {
                        articulations.push(match a {
                            Accent::Normal => mx::ArticulationsType::Accent(mx::Accent {
                                attributes: mx::AccentAttributes::default(),
                                content: (),
                            }),
                            Accent::Heavy => {
                                mx::ArticulationsType::StrongAccent(mx::StrongAccent {
                                    attributes: mx::StrongAccentAttributes::default(),
                                    content: (),
                                })
                            }
                        });
                    }
                    if fx.staccato {
                        articulations.push(mx::ArticulationsType::Staccato(mx::Staccato {
                            attributes: mx::StaccatoAttributes::default(),
                            content: (),
                        }));
                    }
                    if let Some(o) = fx.ornament {
                        ornaments.push(match o {
                            Ornament::UpperMordent => {
                                mx::OrnamentType::InvertedMordent(mx::InvertedMordent {
                                    attributes: mx::InvertedMordentAttributes::default(),
                                    content: (),
                                })
                            }
                            Ornament::LowerMordent => mx::OrnamentType::Mordent(mx::Mordent {
                                attributes: mx::MordentAttributes::default(),
                                content: (),
                            }),
                            Ornament::Turn => mx::OrnamentType::Turn(mx::Turn {
                                attributes: mx::TurnAttributes::default(),
                                content: (),
                            }),
                            Ornament::InvertedTurn => {
                                mx::OrnamentType::InvertedTurn(mx::InvertedTurn {
                                    attributes: mx::InvertedTurnAttributes::default(),
                                    content: (),
                                })
                            }
                        });
                    }
                    if let Some(v) = fx.tremolo_picking {
                        let marks = match v {
                            oxiguitar_model::NoteValue::Eighth => 1,
                            oxiguitar_model::NoteValue::Sixteenth => 2,
                            _ => 3,
                        };
                        ornaments.push(mx::OrnamentType::Tremolo(mx::Tremolo {
                            attributes: mx::TremoloAttributes {
                                r#type: Some(dt::TremoloType::Single),
                                ..mx::TremoloAttributes::default()
                            },
                            content: dt::TremoloMarks(marks),
                        }));
                    }
                    if is_tab && let NotePlace::Fretted { string, fret } = note.place {
                        technical.push(mx::TechnicalContents::StringNumber(mx::StringNumber {
                            attributes: mx::StringAttributes::default(),
                            content: dt::StringNumber(string),
                        }));
                        technical.push(mx::TechnicalContents::Fret(mx::Fret {
                            attributes: mx::FretAttributes::default(),
                            content: dt::NonNegativeInteger(
                                u32::from(fret) + u32::from(staff.capo),
                            ),
                        }));
                    }
                    if fx.hammer_pull {
                        let up =
                            next_pitch(staff, m, index, bi, note).is_none_or(|n| n >= sounding.0);
                        technical.push(if up {
                            mx::TechnicalContents::HammerOn(mx::HammerOn {
                                attributes: mx::HammerOnAttributes {
                                    r#type: dt::StartStop::Start,
                                    color: None,
                                    default_x: None,
                                    default_y: None,
                                    font_family: None,
                                    font_size: None,
                                    font_style: None,
                                    font_weight: None,
                                    number: None,
                                    placement: None,
                                    relative_x: None,
                                    relative_y: None,
                                },
                                content: "H".into(),
                            })
                        } else {
                            mx::TechnicalContents::PullOff(mx::PullOff {
                                attributes: mx::PullOffAttributes {
                                    r#type: dt::StartStop::Start,
                                    color: None,
                                    default_x: None,
                                    default_y: None,
                                    font_family: None,
                                    font_size: None,
                                    font_style: None,
                                    font_weight: None,
                                    number: None,
                                    placement: None,
                                    relative_x: None,
                                    relative_y: None,
                                },
                                content: "P".into(),
                            })
                        });
                    }
                    if let Some(h) = fx.harmonic {
                        technical.push(mx::TechnicalContents::Harmonic(mx::Harmonic {
                            attributes: mx::HarmonicAttributes::default(),
                            content: mx::HarmonicContents {
                                natural: matches!(h, Harmonic::Natural).then(|| mx::Natural {
                                    attributes: (),
                                    content: (),
                                }),
                                artificial: (!matches!(h, Harmonic::Natural)).then_some({
                                    mx::Artificial {
                                        attributes: (),
                                        content: (),
                                    }
                                }),
                                base_pitch: None,
                                touching_pitch: None,
                                sounding_pitch: None,
                            },
                        }));
                        if !matches!(h, Harmonic::Natural | Harmonic::Artificial { .. }) {
                            self.log.lost(
                                "tapped, pinch, semi- and feedback harmonics are written as artificial ones",
                                place(m, t),
                            );
                        }
                    }
                    let whole_bend = fx.bend.as_ref().filter(|b| {
                        let peak = b
                            .points
                            .iter()
                            .map(|p| p.cents)
                            .max_by_key(|c| c.abs())
                            .unwrap_or(0);
                        peak.abs() >= 100
                    });
                    if fx.bend.is_some() && whole_bend.is_none() {
                        self.log
                            .lost("bends of less than a semitone are left out", place(m, t));
                    }
                    if let Some(b) = whole_bend {
                        let peak = b
                            .points
                            .iter()
                            .map(|p| p.cents)
                            .max_by_key(|c| c.abs())
                            .unwrap_or(0);
                        let pre = b.points.first().is_some_and(|p| p.cents != 0);
                        let release = b.points.last().is_some_and(|p| p.cents == 0) && peak != 0;
                        technical.push(mx::TechnicalContents::Bend(mx::Bend {
                            attributes: mx::BendAttributes::default(),
                            content: mx::BendContents {
                                bend_alter: mx::BendAlter {
                                    attributes: (),
                                    content: dt::Semitones(i16::try_from(peak / 100).unwrap_or(0)),
                                },
                                pre_bend: pre.then_some(mx::PreBend {
                                    attributes: (),
                                    content: (),
                                }),
                                release: (release && !pre).then(|| mx::Release {
                                    attributes: mx::ReleaseAttributes::default(),
                                    content: (),
                                }),
                                with_bar: None,
                            },
                        }));
                        if b.points.len() > 4 || peak % 100 != 0 {
                            self.log
                                .lost("bends are written as their highest point", place(m, t));
                        }
                    }
                    if let Some(f) = fx.left_finger {
                        technical.push(mx::TechnicalContents::Fingering(mx::Fingering {
                            attributes: mx::FingeringAttributes::default(),
                            content: if f == 0 { "t".into() } else { f.to_string() },
                        }));
                    }
                    if let Some(f) = fx.right_finger {
                        technical.push(mx::TechnicalContents::Pluck(mx::Pluck {
                            attributes: mx::PluckAttributes::default(),
                            content: ["p", "i", "m", "a", "c"]
                                .get(usize::from(f))
                                .unwrap_or(&"c")
                                .to_string(),
                        }));
                    }
                    if fx.left_hand_tap {
                        technical.push(mx::TechnicalContents::Tap(mx::Tap {
                            attributes: mx::TapAttributes {
                                hand: Some(dt::TapHand::Left),
                                ..mx::TapAttributes::default()
                            },
                            content: String::new(),
                        }));
                    }
                    if !fx.slides.is_empty() {
                        notations.push(mx::NotationContentTypes::Slide(mx::Slide {
                            attributes: mx::SlideAttributes {
                                r#type: dt::StartStop::Start,
                                accelerate: None,
                                beats: None,
                                color: None,
                                dash_length: None,
                                default_x: None,
                                default_y: None,
                                first_beat: None,
                                font_family: None,
                                font_size: None,
                                font_style: None,
                                font_weight: None,
                                id: None,
                                last_beat: None,
                                line_type: None,
                                number: None,
                                relative_x: None,
                                relative_y: None,
                                space_length: None,
                            },
                            content: String::new(),
                        }));
                    }
                    self.note_losses(note, m, t);
                    if ni == 0 {
                        if beat.effects.technique == Some(BeatTechnique::Tap) {
                            technical.push(mx::TechnicalContents::Tap(mx::Tap {
                                attributes: mx::TapAttributes::default(),
                                content: String::new(),
                            }));
                        }
                        match beat.effects.pick_stroke {
                            Some(StrokeDirection::Up) => {
                                technical.push(mx::TechnicalContents::UpBow(mx::UpBow {
                                    attributes: mx::UpBowAttributes::default(),
                                    content: (),
                                }))
                            }
                            Some(StrokeDirection::Down) => {
                                technical.push(mx::TechnicalContents::DownBow(mx::DownBow {
                                    attributes: mx::DownBowAttributes::default(),
                                    content: (),
                                }))
                            }
                            None => {}
                        }
                        if beat.effects.golpe.is_some() {
                            technical.push(mx::TechnicalContents::Golpe(mx::Golpe {
                                attributes: mx::GolpeAttributes::default(),
                                content: (),
                            }));
                        }
                    }
                    if let Some(d) = beat.effects.arpeggio {
                        notations.push(mx::NotationContentTypes::Arpeggiate(mx::Arpeggiate {
                            attributes: mx::ArpeggiateAttributes {
                                direction: Some(match d {
                                    StrokeDirection::Down => dt::UpDown::Up,
                                    StrokeDirection::Up => dt::UpDown::Down,
                                }),
                                ..mx::ArpeggiateAttributes::default()
                            },
                            content: (),
                        }));
                    }
                    if fx.ghost {
                        head = Some(head.unwrap_or(dt::NoteheadValue::Normal));
                    }
                }
                if ni == 0 && primary {
                    if slur_stop {
                        notations.push(mx::NotationContentTypes::Slur(slur(
                            dt::StartStopContinue::Stop,
                        )));
                    }
                    if slur_start {
                        notations.push(mx::NotationContentTypes::Slur(slur(
                            dt::StartStopContinue::Start,
                        )));
                    }
                    if let Some(kind) = fermata {
                        notations.push(mx::NotationContentTypes::Fermata(mx::Fermata {
                            attributes: mx::FermataAttributes {
                                r#type: Some(dt::UprightInverted::Upright),
                                ..mx::FermataAttributes::default()
                            },
                            content: match kind {
                                FermataKind::Short => dt::FermataShape::Angled,
                                FermataKind::Medium => dt::FermataShape::Normal,
                                FermataKind::Long => dt::FermataShape::Square,
                            },
                        }));
                    }
                }
                if !articulations.is_empty() {
                    notations.push(mx::NotationContentTypes::Articulations(mx::Articulations {
                        attributes: mx::ArticulationsAttributes::default(),
                        content: articulations,
                    }));
                }
                if !ornaments.is_empty() {
                    notations.push(mx::NotationContentTypes::Ornaments(mx::Ornaments {
                        attributes: mx::OrnamentsAttributes::default(),
                        content: mx::OrnamentContents {
                            ornaments,
                            accidental_mark: Vec::new(),
                        },
                    }));
                }
                if !technical.is_empty() {
                    notations.push(mx::NotationContentTypes::Technical(mx::Technical {
                        attributes: mx::TechnicalAttributes::default(),
                        content: technical,
                    }));
                }
                let chord = (ni > 0).then_some(mx::Chord {
                    attributes: (),
                    content: (),
                });
                let info = match beat.grace {
                    Some(g) => mx::NoteType::Grace(mx::GraceInfo {
                        grace: mx::Grace {
                            attributes: mx::GraceAttributes {
                                make_time: None,
                                slash: (g == GraceKind::BeforeBeat).then_some(dt::YesNo::Yes),
                                steal_time_following: (g == GraceKind::OnBeat)
                                    .then_some(dt::Percent(50.0)),
                                steal_time_previous: None,
                            },
                            content: (),
                        },
                        info: mx::GraceType::Normal(mx::GraceNormalInfo {
                            chord,
                            audible,
                            tie: ties,
                        }),
                    }),
                    None => mx::NoteType::Normal(mx::NormalInfo {
                        chord,
                        audible,
                        duration: duration(dur.max(1)),
                        tie: ties,
                    }),
                };
                let tuplet = beat.duration.tuplets.first();
                if beat.duration.tuplets.len() > 1 {
                    self.log.lost(
                        "nested tuplets are written as their outer tuplet",
                        place(m, t),
                    );
                }
                let lyric = if ni == 0 && !rest {
                    sung.remove(&bi).unwrap_or_default()
                } else {
                    Vec::new()
                };
                out.push(mx::MeasureElement::Note(mx::Note {
                    attributes: mx::NoteAttributes::default(),
                    content: mx::NoteContents {
                        info,
                        instrument,
                        footnote: None,
                        level: None,
                        voice: Some(voice_element(voice)),
                        r#type: Some(mx::Type {
                            attributes: mx::TypeAttributes::default(),
                            content: note_type(beat.duration.value),
                        }),
                        dot: (0..beat.duration.dots)
                            .map(|_| mx::Dot {
                                attributes: mx::DotAttributes::default(),
                                content: (),
                            })
                            .collect(),
                        accidental: None,
                        time_modification: tuplet.map(|t| mx::TimeModification {
                            attributes: (),
                            content: mx::TimeModificationContents {
                                actual_notes: mx::ActualNotes {
                                    attributes: (),
                                    content: dt::NonNegativeInteger(u32::from(t.notes)),
                                },
                                normal_notes: mx::NormalNotes {
                                    attributes: (),
                                    content: dt::NonNegativeInteger(u32::from(t.in_time_of)),
                                },
                                normal_type: None,
                                normal_dot: Vec::new(),
                            },
                        }),
                        stem: None,
                        notehead: head.map(|h| mx::Notehead {
                            attributes: mx::NoteheadAttributes {
                                parentheses: note
                                    .is_some_and(|n| n.effects.ghost)
                                    .then_some(dt::YesNo::Yes),
                                ..mx::NoteheadAttributes::default()
                            },
                            content: h,
                        }),
                        notehead_text: None,
                        staff: staff_number.map(staff_element),
                        beam: Vec::new(),
                        notations: if notations.is_empty() {
                            Vec::new()
                        } else {
                            vec![mx::Notations {
                                attributes: mx::NotationsAttributes::default(),
                                content: mx::NotationsContents {
                                    footnote: None,
                                    level: None,
                                    notations,
                                },
                            }]
                        },
                        lyric,
                        play: None,
                        listen: None,
                    },
                }));
            }
            pos = pos.checked_add(len).unwrap_or(pos);
        }
        let _ = divisions;
    }

    /// Losses on a beat, named once each.
    fn beat_losses(&mut self, beat: &oxiguitar_model::Beat, m: usize, t: usize) {
        let fx = &beat.effects;
        let mut lost = |what: &str| self.log.lost(format!("{what} are left out"), place(m, t));
        if fx.stroke.is_some() {
            lost("strums (brush strokes)");
        }
        if fx.whammy.is_some() {
            lost("whammy-bar bends");
        }
        if matches!(fx.technique, Some(BeatTechnique::Slap | BeatTechnique::Pop)) {
            lost("slaps and pops");
        }
        if fx.fade.is_some() {
            lost("fade-ins, fade-outs and volume swells");
        }
        if fx.bridge_vibrato.is_some() {
            lost("whammy-bar vibratos");
        }
        if fx.tremolo_picking.is_some() {
            lost("tremolo picking on whole beats");
        }
        if fx.let_ring {
            lost("let-ring marks");
        }
        if fx.wah.is_some() {
            lost("wah marks");
        }
        if fx.rasgueado.is_some() {
            lost("rasgueados");
        }
        if fx.pick_scrape.is_some() {
            lost("pick scrapes");
        }
        if fx.dead_slap {
            lost("dead slaps");
        }
        if beat.slash {
            lost("slash noteheads");
        }
        if beat.barre.is_some() {
            lost("barres");
        }
        if beat.beaming != oxiguitar_model::Beaming::Automatic || beat.tuplet_group {
            lost("beaming choices");
        }
    }

    /// Losses on a note.
    fn note_losses(&mut self, note: &oxiguitar_model::Note, m: usize, t: usize) {
        let fx = &note.effects;
        let mut lost = |what: &str| self.log.lost(format!("{what} are left out"), place(m, t));
        if fx.vibrato.is_some() {
            lost("vibratos");
        }
        if fx.palm_mute {
            lost("palm mutes");
        }
        if fx.let_ring {
            lost("let-ring marks");
        }
        if fx.trill.is_some() {
            lost("trills");
        }
        if fx.slides.iter().any(|s| {
            !matches!(
                s,
                oxiguitar_model::Slide::Shift | oxiguitar_model::Slide::Legato
            )
        }) {
            lost("slides into and out of notes");
        }
        if fx.string_number {
            lost("string numbers on the staff");
        }
    }
}

/// Where a voice is written.
struct VoiceAt<'a> {
    t: usize,
    m: usize,
    track: &'a Track,
    staff: &'a Staff,
    layout: Layout,
    staff_number: Option<u32>,
    voice: u32,
    index: usize,
    is_tab: bool,
    /// The first MusicXML staff of its staff, which carries its marks.
    primary: bool,
    first_voice: bool,
    divisions: i64,
}

/// Semitones from written to sounding, as MusicXML's `transpose` gives
/// them: the reverse of `read`'s rule. An 8vb staff with no transposition
/// is written at its sounding pitch on the 8vb clef; otherwise the clef's
/// octave is part of the transposition.
fn written_to_sounding(staff: &Staff, layout: Layout) -> i32 {
    match (i32::from(staff.transposition), layout.octave_up) {
        (0, _) => 0,
        (t, true) => -t - 12,
        (t, false) => -t,
    }
}

/// A part's id.
fn part_id(t: usize) -> String {
    format!("P{}", t + 1)
}

/// The drum keys a drum track plays.
fn drum_keys(track: &Track) -> Vec<u8> {
    let mut keys: Vec<u8> = track
        .staves
        .iter()
        .flat_map(|s| &s.bars)
        .flat_map(|b| &b.voices)
        .flat_map(|v| &v.beats)
        .flat_map(|b| &b.notes)
        .filter_map(|n| match n.place {
            NotePlace::Pitched(p) => Some(p.0),
            NotePlace::Fretted { .. } => None,
        })
        .collect();
    keys.sort_unstable();
    keys.dedup();
    keys
}

/// A note's sounding pitch.
fn note_pitch(note: &oxiguitar_model::Note, staff: &Staff) -> Pitch {
    match note.place {
        NotePlace::Pitched(p) => p,
        NotePlace::Fretted { string, fret } => staff.pitch_of(string, fret).unwrap_or(Pitch(60)),
    }
}

/// Where a drum note sits on the staff: its key's name, as a stand-in.
fn drum_display(key: Pitch) -> (dt::Step, u8) {
    let s = Spelling::automatic(key, oxiguitar_model::KeySignature::default());
    let octave = u8::try_from(s.octave(key).clamp(0, 9)).unwrap_or(4);
    (step(s.step), octave)
}

fn step(i: u8) -> dt::Step {
    match i {
        0 => dt::Step::C,
        1 => dt::Step::D,
        2 => dt::Step::E,
        3 => dt::Step::F,
        4 => dt::Step::G,
        5 => dt::Step::A,
        _ => dt::Step::B,
    }
}

fn pitch_element(s: Spelling, written: Pitch) -> mx::Pitch {
    mx::Pitch {
        attributes: (),
        content: mx::PitchContents {
            step: mx::Step {
                attributes: (),
                content: step(s.step),
            },
            alter: (s.alter != 0).then(|| mx::Alter {
                attributes: (),
                content: dt::Semitones(i16::from(s.alter)),
            }),
            octave: mx::Octave {
                attributes: (),
                content: dt::Octave(u8::try_from(s.octave(written).clamp(0, 9)).unwrap_or(4)),
            },
        },
    }
}

/// Divisions a quarter note for a track: the least that makes every beat's
/// length whole.
fn divisions(track: &Track) -> i64 {
    let mut div: i64 = 1;
    for len in track
        .staves
        .iter()
        .flat_map(|s| &s.bars)
        .flat_map(|b| &b.voices)
        .flat_map(|v| &v.beats)
        .filter(|b| b.grace.is_none())
        .filter_map(|b| b.duration.length())
    {
        // `len` whole notes are `len * 4` quarters.
        let Some(q) = len.checked_mul(Ratio::from_integer(4)) else {
            continue;
        };
        let d = q.denom();
        let lcm = div / gcd(div, d) * d;
        if lcm <= 1_000_000 {
            div = lcm;
        }
    }
    div
}

fn gcd(a: i64, b: i64) -> i64 {
    if b == 0 {
        a.abs().max(1)
    } else {
        gcd(b, a % b)
    }
}

/// A length in whole notes as divisions, rounded.
fn to_divisions(len: Ratio, divisions: i64) -> i64 {
    let q = len.to_f64() * 4.0 * divisions as f64;
    q.round().max(0.0) as i64
}

/// How long a voice's beats last.
fn voice_len(v: &oxiguitar_model::Voice) -> Ratio {
    v.length().unwrap_or(Ratio::ZERO)
}

fn duration(d: i64) -> mx::Duration {
    mx::Duration {
        attributes: (),
        content: dt::PositiveDivisions(u32::try_from(d.max(1)).unwrap_or(1)),
    }
}

fn voice_element(v: u32) -> mx::Voice {
    mx::Voice {
        attributes: (),
        content: v.to_string(),
    }
}

fn staff_element(n: u32) -> mx::Staff {
    mx::Staff {
        attributes: (),
        content: dt::PositiveInteger(n),
    }
}

fn tie(kind: dt::StartStop) -> mx::Tie {
    mx::Tie {
        attributes: mx::TieAttributes {
            r#type: kind,
            time_only: None,
        },
        content: (),
    }
}

fn tied(kind: dt::StartStopContinue) -> mx::Tied {
    mx::Tied {
        attributes: mx::TiedAttributes {
            r#type: kind,
            bezier_offset: None,
            bezier_offset2: None,
            bezier_x: None,
            bezier_x2: None,
            bezier_y: None,
            bezier_y2: None,
            color: None,
            dash_length: None,
            default_x: None,
            default_y: None,
            id: None,
            line_type: None,
            number: None,
            orientation: None,
            placement: None,
            relative_x: None,
            relative_y: None,
            space_length: None,
        },
        content: (),
    }
}

fn slur(kind: dt::StartStopContinue) -> mx::Slur {
    mx::Slur {
        attributes: mx::SlurAttributes {
            r#type: kind,
            bezier_offset: None,
            bezier_offset2: None,
            bezier_x: None,
            bezier_x2: None,
            bezier_y: None,
            bezier_y2: None,
            color: None,
            dash_length: None,
            default_x: None,
            default_y: None,
            id: None,
            line_type: None,
            number: Some(dt::NumberLevel(1)),
            orientation: None,
            placement: None,
            relative_x: None,
            relative_y: None,
            space_length: None,
        },
        content: (),
    }
}

/// The notes of the beat after `bi` in its voice (into the next bar).
fn next_beat(staff: &Staff, m: usize, index: usize, bi: usize) -> Option<&oxiguitar_model::Beat> {
    let here = staff.bars.get(m)?.voices.get(index)?;
    here.beats
        .iter()
        .skip(bi + 1)
        .find(|b| b.grace.is_none())
        .or_else(|| {
            staff
                .bars
                .get(m + 1)?
                .voices
                .get(index)?
                .beats
                .iter()
                .find(|b| b.grace.is_none())
        })
}

/// Whether the beat after has a note tied to `note`.
fn tied_to_next(
    staff: &Staff,
    m: usize,
    index: usize,
    bi: usize,
    note: &oxiguitar_model::Note,
) -> bool {
    let Some(next) = next_beat(staff, m, index, bi) else {
        return false;
    };
    let here = note_pitch(note, staff);
    next.notes.iter().any(|n| {
        n.tie
            && match (n.place, note.place) {
                (NotePlace::Fretted { string: a, .. }, NotePlace::Fretted { string: b, .. }) => {
                    a == b
                }
                _ => note_pitch(n, staff) == here,
            }
    })
}

/// The pitch of the note after `note` on its string, for hammer-on or
/// pull-off.
fn next_pitch(
    staff: &Staff,
    m: usize,
    index: usize,
    bi: usize,
    note: &oxiguitar_model::Note,
) -> Option<u8> {
    let next = next_beat(staff, m, index, bi)?;
    let n = next
        .notes
        .iter()
        .find(|n| n.string().is_some() && n.string() == note.string())
        .or_else(|| next.notes.first())?;
    Some(note_pitch(n, staff).0)
}

fn clef_element(clef: Clef, number: u32, numbered: bool) -> mx::Clef {
    let (sign, line, octave) = match clef {
        Clef::Treble => (dt::ClefSign::G, 2, 0),
        Clef::Treble8vb => (dt::ClefSign::G, 2, -1),
        Clef::Bass => (dt::ClefSign::F, 4, 0),
        Clef::Bass8vb => (dt::ClefSign::F, 4, -1),
        Clef::Alto => (dt::ClefSign::C, 3, 0),
        Clef::Tenor => (dt::ClefSign::C, 4, 0),
        Clef::Neutral => (dt::ClefSign::Percussion, 0, 0),
    };
    mx::Clef {
        attributes: mx::ClefAttributes {
            number: numbered.then(|| dt::StaffNumber(u8::try_from(number).unwrap_or(1))),
            ..mx::ClefAttributes::default()
        },
        content: mx::ClefContents {
            sign: mx::Sign {
                attributes: (),
                content: sign,
            },
            line: (line != 0).then_some(mx::Line {
                attributes: (),
                content: dt::StaffLinePosition(line),
            }),
            clef_octave_change: (octave != 0).then_some(mx::ClefOctaveChange {
                attributes: (),
                content: octave,
            }),
        },
    }
}

fn tab_clef(number: u32) -> mx::Clef {
    mx::Clef {
        attributes: mx::ClefAttributes {
            number: Some(dt::StaffNumber(u8::try_from(number).unwrap_or(1))),
            ..mx::ClefAttributes::default()
        },
        content: mx::ClefContents {
            sign: mx::Sign {
                attributes: (),
                content: dt::ClefSign::TAB,
            },
            line: Some(mx::Line {
                attributes: (),
                content: dt::StaffLinePosition(5),
            }),
            clef_octave_change: None,
        },
    }
}

/// The strings of a tablature staff: line 1 is the lowest string.
fn staff_details(staff: &Staff, number: u32) -> mx::StaffDetails {
    let strings = staff.tuning.len();
    mx::StaffDetails {
        attributes: mx::StaffDetailsAttributes {
            number: Some(dt::StaffNumber(u8::try_from(number).unwrap_or(1))),
            ..mx::StaffDetailsAttributes::default()
        },
        content: mx::StaffDetailsContents {
            staff_type: None,
            staff_lines: Some(mx::StaffLines {
                attributes: (),
                content: dt::NonNegativeInteger(u32::try_from(strings).unwrap_or(6)),
            }),
            line_detail: Vec::new(),
            staff_tuning: staff
                .tuning
                .iter()
                .rev()
                .enumerate()
                .map(|(i, p)| {
                    let s = Spelling::automatic(*p, oxiguitar_model::KeySignature::default());
                    mx::StaffTuning {
                        attributes: mx::StaffTuningAttributes {
                            line: dt::StaffLine(u8::try_from(i + 1).unwrap_or(1)),
                        },
                        content: mx::StaffTuningContents {
                            tuning_step: mx::TuningStep {
                                attributes: (),
                                content: step(s.step),
                            },
                            tuning_alter: (s.alter != 0).then(|| mx::TuningAlter {
                                attributes: (),
                                content: dt::Semitones(i16::from(s.alter)),
                            }),
                            tuning_octave: mx::TuningOctave {
                                attributes: (),
                                content: dt::Octave(
                                    u8::try_from(s.octave(*p).clamp(0, 9)).unwrap_or(2),
                                ),
                            },
                        },
                    }
                })
                .collect(),
            capo: (staff.capo > 0).then(|| mx::Capo {
                attributes: (),
                content: dt::NonNegativeInteger(u32::from(staff.capo)),
            }),
            staff_size: None,
        },
    }
}

fn barline(
    location: dt::RightLeftMiddle,
    style: Option<dt::BarStyle>,
    repeat: Option<(dt::BackwardForward, Option<u32>)>,
    ending: Option<(String, dt::StartStopDiscontinue)>,
) -> mx::Barline {
    mx::Barline {
        attributes: mx::BarlineAttributes {
            location: Some(location),
            ..mx::BarlineAttributes::default()
        },
        content: mx::BarlineContents {
            bar_style: style.map(|s| mx::BarStyle {
                attributes: mx::BarStyleAttributes::default(),
                content: s,
            }),
            footnote: None,
            level: None,
            wavy_line: None,
            segno: None,
            coda: None,
            fermata: Vec::new(),
            ending: ending.map(|(number, kind)| mx::Ending {
                attributes: mx::EndingAttributes {
                    number: dt::EndingNumber(number.clone()),
                    r#type: kind,
                    color: None,
                    default_x: None,
                    default_y: None,
                    end_length: None,
                    font_family: None,
                    font_size: None,
                    font_style: None,
                    font_weight: None,
                    print_object: None,
                    relative_x: None,
                    relative_y: None,
                    system: None,
                    text_x: None,
                    text_y: None,
                },
                content: format!("{}.", number.replace(", ", ". ")),
            }),
            repeat: repeat.map(|(direction, times)| mx::Repeat {
                attributes: mx::RepeatAttributes {
                    direction,
                    after_jump: None,
                    times: times.map(dt::NonNegativeInteger),
                    winged: None,
                },
                content: (),
            }),
        },
    }
}

/// "1, 2" for endings 1 and 2.
fn endings(mask: u16) -> String {
    (0..16)
        .filter(|i| mask & (1 << i) != 0)
        .map(|i| (i + 1).to_string())
        .collect::<Vec<_>>()
        .join(", ")
}

fn direction(
    types: Vec<mx::DirectionTypeContents>,
    staff: Option<u32>,
    placement: Option<dt::AboveBelow>,
) -> mx::MeasureElement {
    direction_with_sound(types, staff, placement, None)
}

fn direction_with_sound(
    types: Vec<mx::DirectionTypeContents>,
    staff: Option<u32>,
    placement: Option<dt::AboveBelow>,
    sound: Option<mx::SoundAttributes>,
) -> mx::MeasureElement {
    mx::MeasureElement::Direction(mx::Direction {
        attributes: mx::DirectionAttributes {
            placement,
            ..mx::DirectionAttributes::default()
        },
        content: mx::DirectionContents {
            direction_type: types
                .into_iter()
                .map(|c| mx::DirectionType {
                    attributes: mx::DirectionTypeAttributes::default(),
                    content: c,
                })
                .collect(),
            staff: staff.map(staff_element),
            sound: sound.map(|attributes| mx::Sound {
                attributes,
                content: mx::SoundContents::default(),
            }),
            ..mx::DirectionContents::default()
        },
    })
}

fn words(text: &str, staff: Option<u32>) -> mx::MeasureElement {
    direction(
        vec![mx::DirectionTypeContents::Words(vec![mx::Words {
            attributes: mx::WordsAttributes::default(),
            content: escape(text),
        }])],
        staff,
        Some(dt::AboveBelow::Above),
    )
}

fn tempo(bpm: f64, label: &str, offset: i64) -> mx::MeasureElement {
    let mut types = Vec::new();
    if !label.trim().is_empty() {
        types.push(mx::DirectionTypeContents::Words(vec![mx::Words {
            attributes: mx::WordsAttributes::default(),
            content: escape(label.trim()),
        }]));
    }
    types.push(mx::DirectionTypeContents::Metronome(mx::Metronome {
        attributes: mx::MetronomeAttributes::default(),
        content: mx::MetronomeContents::BeatBased(mx::BeatBased {
            beat_unit: mx::BeatUnit {
                attributes: (),
                content: dt::NoteTypeValue::Quarter,
            },
            beat_unit_dot: Vec::new(),
            beat_unit_tied: Vec::new(),
            equals: mx::BeatEquation::BPM(mx::PerMinute {
                attributes: mx::PerMinuteAttributes::default(),
                content: format_bpm(bpm),
            }),
        }),
    }));
    let mut element = direction_with_sound(
        types,
        None,
        Some(dt::AboveBelow::Above),
        Some(mx::SoundAttributes {
            tempo: Some(dt::NonNegativeDecimal(bpm.max(0.0))),
            ..mx::SoundAttributes::default()
        }),
    );
    if offset > 0
        && let mx::MeasureElement::Direction(d) = &mut element
    {
        d.content.offset = Some(mx::Offset {
            attributes: mx::OffsetAttributes::default(),
            content: dt::Divisions(i32::try_from(offset).unwrap_or(0)),
        });
    }
    element
}

fn format_bpm(bpm: f64) -> String {
    if (bpm - bpm.round()).abs() < 1e-9 {
        format!("{}", bpm.round() as i64)
    } else {
        format!("{bpm:.2}")
    }
}

fn wedge(kind: dt::WedgeType) -> mx::DirectionTypeContents {
    mx::DirectionTypeContents::Wedge(mx::Wedge {
        attributes: mx::WedgeAttributes {
            r#type: kind,
            color: None,
            dash_length: None,
            default_x: None,
            default_y: None,
            id: None,
            line_type: None,
            niente: None,
            number: None,
            relative_x: None,
            relative_y: None,
            space_length: None,
            spread: None,
        },
        content: (),
    })
}

fn octave_shift(kind: dt::UpDownStopContinue, o: Ottava) -> mx::DirectionTypeContents {
    mx::DirectionTypeContents::OctaveShift(mx::OctaveShift {
        attributes: mx::OctaveShiftAttributes {
            r#type: kind,
            color: None,
            dash_length: None,
            default_x: None,
            default_y: None,
            font_family: None,
            font_size: None,
            font_style: None,
            font_weight: None,
            id: None,
            number: None,
            relative_x: None,
            relative_y: None,
            size: Some(dt::PositiveInteger(match o {
                Ottava::Alta15 | Ottava::Bassa15 => 15,
                Ottava::Alta8 | Ottava::Bassa8 => 8,
            })),
            space_length: None,
        },
        content: (),
    })
}

fn dynamics_element(d: Dynamic) -> mx::Dynamics {
    let t = match d {
        Dynamic::PPP => mx::DynamicsType::Ppp(mx::Ppp {
            attributes: (),
            content: (),
        }),
        Dynamic::PP => mx::DynamicsType::Pp(mx::Pp {
            attributes: (),
            content: (),
        }),
        Dynamic::P => mx::DynamicsType::P(mx::P {
            attributes: (),
            content: (),
        }),
        Dynamic::MP => mx::DynamicsType::Mp(mx::Mp {
            attributes: (),
            content: (),
        }),
        Dynamic::MF => mx::DynamicsType::Mf(mx::Mf {
            attributes: (),
            content: (),
        }),
        Dynamic::F => mx::DynamicsType::F(mx::F {
            attributes: (),
            content: (),
        }),
        Dynamic::FF => mx::DynamicsType::Ff(mx::Ff {
            attributes: (),
            content: (),
        }),
        Dynamic::FFF => mx::DynamicsType::Fff(mx::Fff {
            attributes: (),
            content: (),
        }),
    };
    mx::Dynamics {
        attributes: mx::DynamicsAttributes::default(),
        content: vec![t],
    }
}

/// Signs at a bar's start: segno and coda.
fn start_direction(d: Direction) -> Option<mx::MeasureElement> {
    let types = match d {
        Direction::Segno | Direction::SegnoSegno => {
            vec![mx::DirectionTypeContents::Segno(vec![mx::Segno {
                attributes: mx::SegnoAttributes::default(),
                content: (),
            }])]
        }
        Direction::Coda | Direction::DoubleCoda => {
            vec![mx::DirectionTypeContents::Coda(vec![mx::Coda {
                attributes: mx::CodaAttributes::default(),
                content: (),
            }])]
        }
        _ => return None,
    };
    let sound = match d {
        Direction::Segno | Direction::SegnoSegno => mx::SoundAttributes {
            segno: Some(dt::Token("segno".into())),
            ..mx::SoundAttributes::default()
        },
        _ => mx::SoundAttributes {
            coda: Some(dt::Token("coda".into())),
            ..mx::SoundAttributes::default()
        },
    };
    Some(direction_with_sound(
        types,
        None,
        Some(dt::AboveBelow::Above),
        Some(sound),
    ))
}

/// Jumps at a bar's end: D.C., D.S., Fine, To Coda, with their words.
fn end_direction(d: Direction) -> Option<mx::MeasureElement> {
    let (text, sound) = match d {
        Direction::Fine => (
            "Fine",
            mx::SoundAttributes {
                fine: Some(dt::Token("yes".into())),
                ..Default::default()
            },
        ),
        Direction::ToCoda | Direction::ToDoubleCoda => (
            "To Coda",
            mx::SoundAttributes {
                tocoda: Some(dt::Token("coda".into())),
                ..Default::default()
            },
        ),
        Direction::DaCapo => (
            "D.C.",
            mx::SoundAttributes {
                dacapo: Some(dt::YesNo::Yes),
                ..Default::default()
            },
        ),
        Direction::DaCapoAlCoda | Direction::DaCapoAlDoubleCoda => (
            "D.C. al Coda",
            mx::SoundAttributes {
                dacapo: Some(dt::YesNo::Yes),
                ..Default::default()
            },
        ),
        Direction::DaCapoAlFine => (
            "D.C. al Fine",
            mx::SoundAttributes {
                dacapo: Some(dt::YesNo::Yes),
                ..Default::default()
            },
        ),
        Direction::DalSegno | Direction::DalSegnoSegno => (
            "D.S.",
            mx::SoundAttributes {
                dalsegno: Some(dt::Token("segno".into())),
                ..Default::default()
            },
        ),
        Direction::DalSegnoAlCoda
        | Direction::DalSegnoAlDoubleCoda
        | Direction::DalSegnoSegnoAlCoda
        | Direction::DalSegnoSegnoAlDoubleCoda => (
            "D.S. al Coda",
            mx::SoundAttributes {
                dalsegno: Some(dt::Token("segno".into())),
                ..Default::default()
            },
        ),
        Direction::DalSegnoAlFine | Direction::DalSegnoSegnoAlFine => (
            "D.S. al Fine",
            mx::SoundAttributes {
                dalsegno: Some(dt::Token("segno".into())),
                ..Default::default()
            },
        ),
        Direction::Segno | Direction::SegnoSegno | Direction::Coda | Direction::DoubleCoda => {
            return None;
        }
    };
    Some(direction_with_sound(
        vec![mx::DirectionTypeContents::Words(vec![mx::Words {
            attributes: mx::WordsAttributes::default(),
            content: text.into(),
        }])],
        None,
        Some(dt::AboveBelow::Above),
        Some(sound),
    ))
}

/// A chord symbol from a diagram's name ("F#m7b5/E"), with its frets.
fn harmony(chord: &oxiguitar_model::ChordDiagram, staff: Option<u32>) -> Option<mx::Harmony> {
    let name = chord.name.trim();
    let mut chars = name.chars();
    let root = chars.next()?;
    let root_step = match root.to_ascii_uppercase() {
        'C' => dt::Step::C,
        'D' => dt::Step::D,
        'E' => dt::Step::E,
        'F' => dt::Step::F,
        'G' => dt::Step::G,
        'A' => dt::Step::A,
        'B' => dt::Step::B,
        _ => return None,
    };
    let rest = &name[root.len_utf8()..];
    let (alter, rest) = accidental_prefix(rest);
    let (suffix, bass) = match rest.rsplit_once('/') {
        Some((s, b)) if b.chars().next().is_some_and(|c| "ABCDEFG".contains(c)) => (s, Some(b)),
        _ => (rest, None),
    };
    let kind = kind_value(suffix);
    let bass = bass.and_then(|b| {
        let mut c = b.chars();
        let s = match c.next()? {
            'C' => dt::Step::C,
            'D' => dt::Step::D,
            'E' => dt::Step::E,
            'F' => dt::Step::F,
            'G' => dt::Step::G,
            'A' => dt::Step::A,
            'B' => dt::Step::B,
            _ => return None,
        };
        let (a, _) = accidental_prefix(c.as_str());
        Some(mx::Bass {
            attributes: mx::BassAttributes::default(),
            content: mx::BassContents {
                bass_separator: None,
                bass_step: mx::BassStep {
                    attributes: mx::BassStepAttributes::default(),
                    content: s,
                },
                bass_alter: (a != 0).then(|| mx::BassAlter {
                    attributes: mx::BassAlterAttributes::default(),
                    content: dt::Semitones(a),
                }),
            },
        })
    });
    let frame = (!chord.frets.is_empty()).then(|| mx::Frame {
        attributes: mx::FrameAttributes::default(),
        content: mx::FrameContents {
            frame_strings: mx::FrameStrings {
                attributes: (),
                content: dt::PositiveInteger(u32::try_from(chord.frets.len()).unwrap_or(6)),
            },
            frame_frets: mx::FrameFrets {
                attributes: (),
                content: dt::PositiveInteger(4),
            },
            first_fret: (chord.first_fret > 1).then(|| mx::FirstFret {
                attributes: mx::FirstFretAttributes::default(),
                content: dt::PositiveInteger(u32::from(chord.first_fret)),
            }),
            frame_note: chord
                .frets
                .iter()
                .enumerate()
                .filter_map(|(i, f)| {
                    let f = (*f)?;
                    Some(mx::FrameNote {
                        attributes: (),
                        content: mx::FrameNoteContents {
                            string: mx::StringNumber {
                                attributes: mx::StringAttributes::default(),
                                content: dt::StringNumber(u8::try_from(i + 1).ok()?),
                            },
                            fret: mx::Fret {
                                attributes: mx::FretAttributes::default(),
                                content: dt::NonNegativeInteger(u32::from(f)),
                            },
                            fingering: None,
                            barre: None,
                        },
                    })
                })
                .collect(),
        },
    });
    Some(mx::Harmony {
        attributes: mx::HarmonyAttributes::default(),
        content: mx::HarmonyContents {
            harmony: vec![mx::HarmonySubcontents {
                root: Some(mx::Root {
                    attributes: (),
                    content: mx::RootContents {
                        root_step: mx::RootStep {
                            attributes: mx::RootStepAttributes::default(),
                            content: root_step,
                        },
                        root_alter: (alter != 0).then(|| mx::RootAlter {
                            attributes: mx::RootAlterAttributes::default(),
                            content: dt::Semitones(alter),
                        }),
                    },
                }),
                numeral: None,
                function: None,
                kind: mx::Kind {
                    attributes: mx::KindAttributes {
                        text: Some(dt::Token(escape(suffix))),
                        ..mx::KindAttributes::default()
                    },
                    content: kind,
                },
                inversion: None,
                bass,
                degree: Vec::new(),
            }],
            frame,
            offset: None,
            footnote: None,
            level: None,
            staff: staff.map(staff_element),
        },
    })
}

/// The sharps or flats at the start of `s`, and what follows.
fn accidental_prefix(s: &str) -> (i16, &str) {
    let mut alter = 0;
    let mut rest = s;
    loop {
        if let Some(r) = rest.strip_prefix('#').or_else(|| rest.strip_prefix('♯')) {
            alter += 1;
            rest = r;
        } else if let Some(r) = rest.strip_prefix('b').or_else(|| rest.strip_prefix('♭')) {
            alter -= 1;
            rest = r;
        } else {
            return (alter, rest);
        }
    }
}

/// MusicXML's kind of chord for a name's suffix; its text keeps the suffix
/// as written.
fn kind_value(suffix: &str) -> dt::KindValue {
    use dt::KindValue as K;
    match suffix {
        "" | "maj" | "M" => K::Major,
        "m" | "min" | "-" => K::Minor,
        "+" | "aug" => K::Augmented,
        "dim" | "°" | "o" => K::Diminished,
        "7" => K::Dominant,
        "maj7" | "M7" | "Δ" | "Δ7" => K::MajorSeventh,
        "m7" | "min7" | "-7" => K::MinorSeventh,
        "dim7" | "°7" | "o7" => K::DiminishedSeventh,
        "+7" | "aug7" | "7#5" => K::AugmentedSeventh,
        "m7b5" | "ø" | "ø7" => K::HalfDiminished,
        "m(maj7)" | "mMaj7" | "m(M7)" => K::MajorMinor,
        "6" => K::MajorSixth,
        "m6" => K::MinorSixth,
        "9" => K::DominantNinth,
        "maj9" => K::MajorNinth,
        "m9" => K::MinorNinth,
        "11" => K::Dominant11th,
        "maj11" => K::Major11th,
        "m11" => K::Minor11th,
        "13" => K::Dominant13th,
        "maj13" => K::Major13th,
        "m13" => K::Minor13th,
        "sus2" => K::SuspendedSecond,
        "sus4" | "sus" => K::SuspendedFourth,
        "5" => K::Power,
        _ => K::Other,
    }
}

/// Each sung beat's lyrics in bar `m` of the first voice, by beat: one
/// `lyric` per line that sings there.
fn syllables_here(track: &Track, staff: &Staff, m: usize) -> BTreeMap<usize, Vec<mx::Lyric>> {
    let mut out: BTreeMap<usize, Vec<mx::Lyric>> = BTreeMap::new();
    for (number, line) in track.lyrics.iter().enumerate() {
        let sung = lyrics::sung_beats(staff, line.start_bar);
        let syllables = lyrics::syllables(&line.text);
        let mut word_goes_on = false;
        for (s, &(bar, beat)) in syllables.iter().zip(&sung) {
            let before = word_goes_on;
            word_goes_on = s.hyphen;
            if bar != m || s.text.is_empty() {
                continue;
            }
            let syllabic = match (before, s.hyphen) {
                (false, false) => dt::Syllabic::Single,
                (false, true) => dt::Syllabic::Begin,
                (true, true) => dt::Syllabic::Middle,
                (true, false) => dt::Syllabic::End,
            };
            out.entry(beat).or_default().push(mx::Lyric {
                attributes: mx::LyricAttributes {
                    number: Some(dt::NmToken((number + 1).to_string())),
                    ..mx::LyricAttributes::default()
                },
                content: mx::LyricContents::Text(mx::TextLyric {
                    syllabic: Some(mx::Syllabic {
                        attributes: (),
                        content: syllabic,
                    }),
                    text: mx::Text {
                        attributes: mx::TextAttributes::default(),
                        content: escape(&s.text),
                    },
                    ..mx::TextLyric::default()
                }),
            });
        }
    }
    out
}

/// General MIDI's name of a drum key (0-based MIDI note).
fn drum_name(key: u8) -> String {
    const NAMES: [&str; 47] = [
        "Acoustic Bass Drum",
        "Bass Drum 1",
        "Side Stick",
        "Acoustic Snare",
        "Hand Clap",
        "Electric Snare",
        "Low Floor Tom",
        "Closed Hi-Hat",
        "High Floor Tom",
        "Pedal Hi-Hat",
        "Low Tom",
        "Open Hi-Hat",
        "Low-Mid Tom",
        "Hi-Mid Tom",
        "Crash Cymbal 1",
        "High Tom",
        "Ride Cymbal 1",
        "Chinese Cymbal",
        "Ride Bell",
        "Tambourine",
        "Splash Cymbal",
        "Cowbell",
        "Crash Cymbal 2",
        "Vibraslap",
        "Ride Cymbal 2",
        "Hi Bongo",
        "Low Bongo",
        "Mute Hi Conga",
        "Open Hi Conga",
        "Low Conga",
        "High Timbale",
        "Low Timbale",
        "High Agogo",
        "Low Agogo",
        "Cabasa",
        "Maracas",
        "Short Whistle",
        "Long Whistle",
        "Short Guiro",
        "Long Guiro",
        "Claves",
        "Hi Wood Block",
        "Low Wood Block",
        "Mute Cuica",
        "Open Cuica",
        "Mute Triangle",
        "Open Triangle",
    ];
    key.checked_sub(35)
        .and_then(|i| NAMES.get(usize::from(i)))
        .map_or_else(|| format!("Drum {key}"), |n| (*n).to_owned())
}
