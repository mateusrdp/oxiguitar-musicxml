// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Mateus Rocha de Paula and the OxiGuitar contributors

//! Reading MusicXML into OxiGuitar's score (docs/design.md, "Reading").
//!
//! Each part becomes a track. A part's notation staff and its tablature
//! staff (as MuseScore and Guitar Pro write a guitar) become one staff
//! that shows both, its notes taking their strings and frets from the
//! tablature; any other staff is a staff of its own (a piano's two staves
//! make a grand staff). Notes that come with no string keep their pitch
//! (`NotePlace::Pitched`), and OxiGuitar fingers them as the file opens
//! (#157). Time is counted in whole notes from MusicXML's divisions, so
//! voices that skip time (`backup`, `forward`) are laid out exactly.

use std::collections::BTreeMap;

use musicxml::datatypes as dt;
use musicxml::elements as mx;
use oxiguitar_model::{
    Accent, Bar, Beat, BeatKind, BeatTechnique, Bend, BendPoint, ChordDiagram, Clef, Direction,
    Duration, Dynamic, Fermata, FermataKind, GraceKind, Hairpin, Harmonic, Instrument, KeyMode,
    KeySignature, LyricLine, Marker, MasterBar, Mix, Note, NoteEffects, NotePlace, NoteValue,
    Ornament, Ottava, Pedal, Pitch, Ratio, Score, ScoreInfo, Slide, Spelling, Staff,
    StrokeDirection, TempoChange, TimeSignature, Track, Tuplet, Voice,
};

use crate::instruments::{self, Clues, Kind};
use crate::log::{Log, place};
use crate::text::{decode, unescape};
use crate::time;

/// A score read from a file, with what the file held that it couldn't.
#[derive(Debug)]
pub(crate) struct Imported {
    /// The score.
    pub(crate) score: Score,
    /// The import log.
    pub(crate) log: Log,
}

/// Reads MusicXML from `xml`, the score's XML (already out of its archive).
pub(crate) fn read(xml: Vec<u8>) -> Result<Imported, String> {
    // UTF-8 for the crate, whatever the file came in (see `text`).
    let xml = decode(&xml)?.into_bytes();
    let file = musicxml::read_score_data_partwise(xml).map_err(|e| {
        if e.is_empty() {
            "not a MusicXML score".to_owned()
        } else {
            format!("not a MusicXML score that can be read: {e}")
        }
    })?;
    let mut log = Log::default();
    let score = Reader::new(&file, &mut log).read();
    score.check_structure()?;
    Ok(Imported { score, log })
}

/// Semitones above C of each step, C first.
const NATURALS: [i32; 7] = [0, 2, 4, 5, 7, 9, 11];

/// A step's index, C = 0 … B = 6.
fn step_index(step: &dt::Step) -> u8 {
    match step {
        dt::Step::C => 0,
        dt::Step::D => 1,
        dt::Step::E => 2,
        dt::Step::F => 3,
        dt::Step::G => 4,
        dt::Step::A => 5,
        dt::Step::B => 6,
    }
}

/// The MIDI number of a step, alteration and octave, if in range.
fn midi(step: &dt::Step, alter: i32, octave: i32) -> Option<u8> {
    let natural = NATURALS.get(usize::from(step_index(step)))?;
    let p = (octave + 1) * 12 + natural + alter;
    u8::try_from(p).ok().filter(|p| *p <= 127)
}

/// What a part's staff is, as far as its attributes have said.
#[derive(Clone, Debug)]
struct StaffState {
    /// Its clef.
    clef: Clef,
    /// A tablature staff (clef sign TAB).
    tab: bool,
    /// A percussion clef.
    percussion: bool,
    /// Its strings, highest first, when the file tunes them.
    tuning: Option<Vec<Pitch>>,
    /// Its number of lines, when given.
    lines: Option<usize>,
    /// Its capo fret.
    capo: u8,
}

impl Default for StaffState {
    fn default() -> Self {
        StaffState {
            clef: Clef::Treble,
            tab: false,
            percussion: false,
            tuning: None,
            lines: None,
            capo: 0,
        }
    }
}

/// One note as read, before it has a place in a voice.
#[derive(Clone, Debug)]
struct RawNote {
    /// Sounding pitch, if pitched (drums: their MIDI key).
    pitch: Option<u8>,
    /// The written spelling (step, alteration).
    spelling: Option<(u8, i8)>,
    /// String and fret (from the nut) from the file.
    fretted: Option<(u8, u8)>,
    /// Tied to the note before.
    tie: bool,
    /// Its dynamic.
    dynamic: Dynamic,
    /// Its effects.
    effects: NoteEffects,
}

/// One beat as read: a note or chord, a rest, or a grace note.
#[derive(Clone, Debug)]
struct RawBeat {
    /// Where it starts in the bar, in whole notes.
    start: Ratio,
    /// How long it lasts (zero for a grace note).
    len: Ratio,
    /// Its written duration, when the file gives one that fits its length.
    duration: Option<Duration>,
    /// A grace note.
    grace: Option<GraceKind>,
    /// A rest (no notes).
    rest: bool,
    /// A whole-bar rest.
    measure_rest: bool,
    /// Its notes.
    notes: Vec<RawNote>,
    /// Slurs starting and stopping here.
    slur_start: u8,
    slur_stop: u8,
    /// Arpeggio.
    arpeggio: Option<StrokeDirection>,
    /// Pick stroke (up-bow, down-bow).
    pick_stroke: Option<StrokeDirection>,
    /// Lyrics: (verse, syllable, the word goes on).
    lyrics: Vec<(String, String, bool)>,
    /// A fermata over it.
    fermata: Option<FermataKind>,
    /// Tapped with the right hand.
    tap: bool,
    /// A dynamics marking on the note.
    dynamic: Option<Dynamic>,
}

/// A mark that spans beats, from one place in a bar to another.
#[derive(Clone, Copy, Debug)]
enum Span {
    Hairpin(Hairpin),
    Ottava(Ottava),
}

/// What a part's measure holds, beyond its beats.
#[derive(Debug, Default)]
struct RawMeasure {
    /// How far its music reaches, in whole notes.
    len: Ratio,
    /// Each staff's voices, in the order they first appear: (voice id,
    /// beats).
    staves: BTreeMap<usize, Vec<(String, Vec<RawBeat>)>>,
    /// Each staff's clef in this measure.
    clefs: BTreeMap<usize, Clef>,
    /// Chord symbols: where, and which diagram.
    chords: Vec<(Ratio, usize)>,
    /// Words: where, on which staff, the text.
    words: Vec<(Ratio, usize, String)>,
    /// Spans starting (`Some`) and stopping (`None`), by staff.
    spans: Vec<(Ratio, usize, Option<Span>, bool)>,
    /// Pedal marks, by staff.
    pedals: Vec<(Ratio, usize, Pedal)>,
    /// Dynamics markings, by staff.
    dynamics: Vec<(Ratio, usize, Dynamic)>,
}

/// The parts' shared bars, as read.
#[derive(Debug, Default)]
struct RawMaster {
    bar: MasterBar,
    /// The time signature was set in this measure.
    time_set: bool,
    /// The key was set in this measure.
    key_set: bool,
}

/// Reads a whole file.
struct Reader<'a> {
    file: &'a mx::ScorePartwise,
    log: &'a mut Log,
    masters: Vec<RawMaster>,
    /// Tempo marks: (bar, where, quarter notes a minute, label).
    tempos: Vec<(usize, Ratio, f64, String)>,
    /// Chord diagrams for the track being read.
    diagrams: Vec<ChordDiagram>,
}

/// Facts about a part from the part list.
#[derive(Debug, Default)]
struct PartInfo<'a> {
    name: String,
    abbreviation: String,
    instrument_names: Vec<String>,
    sounds: Vec<&'a str>,
    channel: Option<u8>,
    program: Option<u8>,
    volume: Option<f64>,
    pan: Option<f32>,
    /// Drum keys by instrument id (MIDI, 0-based).
    unpitched: BTreeMap<&'a str, u8>,
}

impl<'a> Reader<'a> {
    fn new(file: &'a mx::ScorePartwise, log: &'a mut Log) -> Reader<'a> {
        Reader {
            file,
            log,
            masters: Vec::new(),
            tempos: Vec::new(),
            diagrams: Vec::new(),
        }
    }

    fn read(mut self) -> Score {
        let info = self.info();
        let parts = self.part_list();
        let mut tracks = Vec::new();
        for (index, part) in self.file.content.part.iter().enumerate() {
            let id = part.attributes.id.0.as_str();
            let empty = PartInfo {
                name: unescape(id),
                ..PartInfo::default()
            };
            let info = parts.get(id).unwrap_or(&empty);
            let measures: Vec<&mx::Measure> = part
                .content
                .iter()
                .filter_map(|e| match e {
                    mx::PartElement::Measure(m) => Some(m),
                    _ => None,
                })
                .collect();
            let track = self.read_part(index, info, &measures);
            tracks.push(track);
        }
        let bars = self.masters.len();
        // Every staff has one bar per master bar.
        for track in &mut tracks {
            for staff in &mut track.staves {
                while staff.bars.len() < bars {
                    staff.bars.push(Bar {
                        clef: staff.bars.last().map_or(Clef::Treble, |b| b.clef),
                        voices: vec![Voice::default()],
                        simile: None,
                    });
                }
            }
        }
        let mut master_bars: Vec<MasterBar> = self.masters.into_iter().map(|m| m.bar).collect();
        let mut initial_tempo = 120.0;
        let mut tempo_label = String::new();
        self.tempos.sort_by_key(|a| (a.0, a.1));
        self.tempos.dedup_by(|a, b| a.0 == b.0 && a.1 == b.1);
        for (i, (bar, at, bpm, label)) in self.tempos.into_iter().enumerate() {
            if i == 0 && bar == 0 && at == Ratio::ZERO {
                initial_tempo = bpm;
                tempo_label = label;
            } else if let Some(m) = master_bars.get_mut(bar) {
                m.tempo_changes.push(TempoChange {
                    position: at,
                    bpm,
                    label,
                    linear: false,
                });
            }
        }
        Score {
            info,
            initial_tempo,
            tempo_label,
            master_bars,
            tracks,
            layout: Vec::new(),
        }
    }

    /// The header texts.
    fn info(&mut self) -> ScoreInfo {
        let c = &self.file.content;
        let work = c
            .work
            .as_ref()
            .and_then(|w| w.content.work_title.as_ref())
            .map(|t| unescape(t.content.trim()))
            .filter(|t| !t.is_empty());
        let movement = c
            .movement_title
            .as_ref()
            .map(|t| unescape(t.content.trim()))
            .filter(|t| !t.is_empty());
        let (title, subtitle) = match (work, movement) {
            (Some(w), Some(m)) if w != m => (w, m),
            (Some(w), _) => (w, String::new()),
            (None, Some(m)) => (m, String::new()),
            (None, None) => (String::new(), String::new()),
        };
        let mut info = ScoreInfo {
            title,
            subtitle,
            ..ScoreInfo::default()
        };
        if let Some(id) = &c.identification {
            let join = |field: &mut String, text: &str| {
                let text = text.trim();
                if text.is_empty() {
                    return;
                }
                if !field.is_empty() {
                    field.push_str(", ");
                }
                field.push_str(text);
            };
            for creator in &id.content.creator {
                let kind = creator.attributes.r#type.as_ref().map(|t| t.0.as_str());
                match kind {
                    Some("composer") | None => join(&mut info.music, &unescape(&creator.content)),
                    Some("lyricist" | "poet" | "librettist" | "writer") => {
                        join(&mut info.words, &unescape(&creator.content))
                    }
                    Some("transcriber") => join(&mut info.transcriber, &unescape(&creator.content)),
                    Some("artist") => join(&mut info.artist, &unescape(&creator.content)),
                    Some(other) => {
                        let text = unescape(creator.content.trim());
                        if !text.is_empty() {
                            let mut k = other.to_owned();
                            if let Some(first) = k.get_mut(..1) {
                                first.make_ascii_uppercase();
                            }
                            info.notices.push(format!("{k}: {text}"));
                        }
                    }
                }
            }
            if let Some(misc) = &id.content.miscellaneous {
                for f in &misc.content.miscellaneous_field {
                    let text = unescape(f.content.trim());
                    if text.is_empty() {
                        continue;
                    }
                    match f.attributes.name.0.as_str() {
                        "album" => info.album = text,
                        "instructions" => info.instructions = text,
                        "notice" => info.notices.push(text),
                        // Other programs' fields (MuseScore's "source",
                        // say) aren't texts of the score.
                        _ => {}
                    }
                }
            }
            for rights in &id.content.rights {
                join(&mut info.copyright, &unescape(&rights.content));
            }
        }
        info
    }

    /// What the part list says about each part, by id.
    fn part_list(&mut self) -> BTreeMap<&'a str, PartInfo<'a>> {
        let mut out = BTreeMap::new();
        for e in &self.file.content.part_list.content.content {
            let mx::PartListElement::ScorePart(p) = e else {
                continue;
            };
            let c = &p.content;
            let mut info = PartInfo {
                name: unescape(c.part_name.content.trim()),
                abbreviation: c
                    .part_abbreviation
                    .as_ref()
                    .map_or_else(String::new, |a| unescape(a.content.trim())),
                ..PartInfo::default()
            };
            for i in &c.score_instrument {
                info.instrument_names
                    .push(unescape(&i.content.instrument_name.content));
                if let Some(s) = &i.content.instrument_sound {
                    info.sounds.push(s.content.as_str());
                }
            }
            for m in &c.midi_instrument {
                let mc = &m.content;
                if let Some(ch) = &mc.midi_channel {
                    info.channel.get_or_insert(ch.content.0);
                }
                if let Some(p) = &mc.midi_program {
                    info.program.get_or_insert(p.content.0.saturating_sub(1));
                }
                if let Some(u) = &mc.midi_unpitched {
                    info.unpitched
                        .insert(m.attributes.id.0.as_str(), u.content.0.saturating_sub(1));
                }
                if let Some(v) = &mc.volume {
                    info.volume.get_or_insert(v.content.0);
                }
                if let Some(p) = &mc.pan {
                    info.pan.get_or_insert(p.content.0);
                }
            }
            out.insert(p.attributes.id.0.as_str(), info);
        }
        out
    }

    /// The master bar for measure `m`, made (carrying the time and key
    /// before it) if this is the first part to reach it.
    fn master(&mut self, m: usize) -> &mut RawMaster {
        while self.masters.len() <= m {
            let bar = self
                .masters
                .last()
                .map_or_else(MasterBar::default, |prev| MasterBar {
                    time_signature: prev.bar.time_signature,
                    key_signature: prev.bar.key_signature,
                    free_time: prev.bar.free_time,
                    ..MasterBar::default()
                });
            self.masters.push(RawMaster {
                bar,
                time_set: false,
                key_set: false,
            });
        }
        let len = self.masters.len();
        &mut self.masters[len.min(m + 1) - 1]
    }

    /// Reads a part into a track.
    fn read_part(&mut self, part: usize, info: &PartInfo<'_>, measures: &[&mx::Measure]) -> Track {
        self.diagrams.clear();
        let mut staves: Vec<StaffState> = vec![StaffState::default()];
        let mut divisions: i64 = 1;
        // Written to sounding, in semitones, and its octave part.
        let mut transpose: i32 = 0;
        let mut octave_change: i32 = 0;
        let mut dynamic = Dynamic::default();
        let mut unpitched_seen = false;
        let mut raw: Vec<RawMeasure> = Vec::new();
        let mut key = KeySignature::default();
        // Open slurs and spans carry across measures through the beats.
        for (m, measure) in measures.iter().enumerate() {
            let mut rm = RawMeasure::default();
            let mut cursor: i64 = 0;
            let mut reach: i64 = 0;
            // The last note's start, for chord notes.
            let mut last_start: i64 = 0;
            let at = |div: i64, divisions: i64| {
                time::from_divisions(div.max(0), divisions).unwrap_or(Ratio::ZERO)
            };
            for e in &measure.content {
                match e {
                    mx::MeasureElement::Attributes(a) => {
                        let a = &a.content;
                        if let Some(d) = &a.divisions {
                            let d = i64::from(d.content.0);
                            if d > 0 {
                                divisions = d;
                            }
                        }
                        if let Some(k) = a.key.first() {
                            if let mx::KeyContents::Explicit(k) = &k.content {
                                key = KeySignature {
                                    fifths: k.fifths.content.0.clamp(-7, 7),
                                    mode: match k.mode.as_ref().map(|m| &m.content) {
                                        Some(dt::Mode::Minor | dt::Mode::Aeolian) => KeyMode::Minor,
                                        _ => KeyMode::Major,
                                    },
                                };
                            } else {
                                self.log.lost(
                                    "keys not on the circle of fifths are left out",
                                    place(m, part),
                                );
                            }
                            if part == 0 || !self.master(m).key_set {
                                let master = self.master(m);
                                master.bar.key_signature = key;
                                master.key_set = true;
                            }
                        }
                        if let Some(t) = a.time.first() {
                            self.time(m, part, t);
                        }
                        if let Some(n) = &a.staves {
                            let n = usize::try_from(n.content.0).unwrap_or(1).clamp(1, 8);
                            staves.resize(n, StaffState::default());
                        }
                        for c in &a.clef {
                            let s = staff_index(c.attributes.number.as_ref().map(|n| n.0));
                            if let Some(st) = staves.get_mut(s) {
                                let (clef, tab, percussion) = clef_of(&c.content);
                                st.clef = clef;
                                st.tab = tab;
                                st.percussion = percussion;
                                rm.clefs.entry(s).or_insert(clef);
                            }
                        }
                        for d in &a.staff_details {
                            let numbered = d.attributes.number.as_ref().map(|n| n.0);
                            for (s, st) in staves.iter_mut().enumerate() {
                                if numbered.is_some_and(|n| staff_index(Some(n)) != s) {
                                    continue;
                                }
                                staff_details(st, &d.content);
                            }
                        }
                        if let Some(t) = a.transpose.first() {
                            octave_change = t
                                .content
                                .octave_change
                                .as_ref()
                                .map_or(0, |o| i32::from(o.content));
                            transpose =
                                i32::from(t.content.chromatic.content.0) + 12 * octave_change;
                        }
                    }
                    mx::MeasureElement::Note(n) => {
                        let read =
                            self.note(n, divisions, transpose, &mut dynamic, info, (m, part));
                        let Some((staff, voice, chord, len_div, mut beat)) = read else {
                            continue;
                        };
                        if beat.notes.iter().any(|n| n.pitch.is_some())
                            && staves.get(staff).is_some_and(|s| s.percussion)
                        {
                            unpitched_seen = true;
                        }
                        let start = if chord { last_start } else { cursor };
                        beat.start = at(start, divisions);
                        if let Some(kind) = beat.fermata {
                            let position = beat.start;
                            let master = &mut self.master(m).bar;
                            if !master.fermatas.iter().any(|f| f.position == position) {
                                master.fermatas.push(Fermata {
                                    position,
                                    kind,
                                    length: 0.0,
                                });
                                master.fermatas.sort_by_key(|f| f.position);
                            }
                        }
                        if !chord && beat.grace.is_none() {
                            last_start = cursor;
                            cursor = cursor.saturating_add(len_div);
                            reach = reach.max(cursor);
                        }
                        if let Some(st) = staves.get(staff) {
                            rm.clefs.entry(staff).or_insert(st.clef);
                        }
                        push_beat(&mut rm, staff, voice, chord, beat);
                    }
                    mx::MeasureElement::Backup(b) => {
                        cursor = cursor
                            .saturating_sub(i64::from(b.content.duration.content.0))
                            .max(0);
                    }
                    mx::MeasureElement::Forward(f) => {
                        cursor = cursor.saturating_add(i64::from(f.content.duration.content.0));
                        reach = reach.max(cursor);
                    }
                    mx::MeasureElement::Direction(d) => {
                        let offset = d
                            .content
                            .offset
                            .as_ref()
                            .map_or(0, |o| i64::from(o.content.0));
                        let here = at(cursor.saturating_add(offset), divisions);
                        let staff = staff_index(d.content.staff.as_ref().map(|s| s.content.0));
                        self.direction(&d.content, here, staff, m, &mut rm, &mut dynamic);
                    }
                    mx::MeasureElement::Harmony(h) => {
                        let offset = h
                            .content
                            .offset
                            .as_ref()
                            .map_or(0, |o| i64::from(o.content.0));
                        let here = at(cursor.saturating_add(offset), divisions);
                        if let Some(d) = self.harmony(h) {
                            rm.chords.push((here, d));
                        }
                    }
                    mx::MeasureElement::Barline(b) => self.barline(b, m),
                    mx::MeasureElement::Sound(s) => {
                        self.sound(&s.attributes, at(cursor, divisions), m, String::new());
                    }
                    _ => {}
                }
            }
            rm.len = at(reach, divisions);
            self.master(m);
            raw.push(rm);
        }
        self.build_track(
            part,
            info,
            &staves,
            &raw,
            transpose,
            octave_change,
            unpitched_seen,
        )
    }

    /// A time signature for measure `m`.
    fn time(&mut self, m: usize, part: usize, t: &mx::Time) {
        let c = &t.content;
        if c.senza_misura.is_some() {
            let master = self.master(m);
            master.bar.free_time = true;
            master.time_set = true;
            return;
        }
        // Composite (3+2/8) and several signatures at once: the sum of the
        // first.
        let Some(first) = c.beats.first() else {
            return;
        };
        let numerator: u32 = first
            .beats
            .content
            .split('+')
            .filter_map(|p| p.trim().parse::<u32>().ok())
            .sum();
        let denominator: u32 = first.beat_type.content.trim().parse().unwrap_or(4);
        if first.beats.content.contains('+') || c.beats.len() > 1 {
            self.log.note(
                "composite time signatures are shown as their sum",
                place(m, part),
            );
        }
        let (Ok(numerator), Ok(denominator)) = (u8::try_from(numerator), u8::try_from(denominator))
        else {
            self.log
                .lost("time signatures out of range are left out", place(m, part));
            return;
        };
        if numerator == 0 || !matches!(denominator, 1 | 2 | 4 | 8 | 16 | 32 | 64) {
            self.log
                .lost("time signatures out of range are left out", place(m, part));
            return;
        }
        if part == 0 || !self.master(m).time_set {
            let master = self.master(m);
            master.bar.time_signature = TimeSignature {
                numerator,
                denominator,
            };
            master.bar.free_time = false;
            master.time_set = true;
        }
    }

    /// Reads a note: its staff, voice, whether it joins the chord before,
    /// its length in divisions, and itself as a beat.
    #[allow(clippy::too_many_arguments)]
    fn note(
        &mut self,
        n: &mx::Note,
        divisions: i64,
        transpose: i32,
        dynamic: &mut Dynamic,
        info: &PartInfo<'_>,
        (m, part): (usize, usize),
    ) -> Option<(usize, String, bool, i64, RawBeat)> {
        let c = &n.content;
        let (grace, chord, audible, len_div, ties) = match &c.info {
            mx::NoteType::Normal(i) => (
                None,
                i.chord.is_some(),
                &i.audible,
                i64::from(i.duration.content.0),
                i.tie.as_slice(),
            ),
            mx::NoteType::Grace(g) => {
                let kind = if g.grace.attributes.steal_time_following.is_some() {
                    GraceKind::OnBeat
                } else {
                    GraceKind::BeforeBeat
                };
                match &g.info {
                    mx::GraceType::Normal(i) => (
                        Some(kind),
                        i.chord.is_some(),
                        &i.audible,
                        0,
                        i.tie.as_slice(),
                    ),
                    mx::GraceType::Cue(i) => {
                        (Some(kind), i.chord.is_some(), &i.audible, 0, &[][..])
                    }
                }
            }
            mx::NoteType::Cue(i) => {
                // Cue notes are another part's, shown small and not played:
                // their time passes, nothing is written.
                self.log.lost("cue notes are left out", place(m, part));
                let staff = staff_index(c.staff.as_ref().map(|s| s.content.0));
                let voice = c
                    .voice
                    .as_ref()
                    .map_or("1".to_owned(), |v| v.content.clone());
                let len = time::from_divisions(i64::from(i.duration.content.0), divisions)
                    .unwrap_or(Ratio::ZERO);
                let beat = RawBeat::rest(len);
                return Some((
                    staff,
                    voice,
                    i.chord.is_some(),
                    i64::from(i.duration.content.0),
                    beat,
                ));
            }
        };
        let staff = staff_index(c.staff.as_ref().map(|s| s.content.0));
        let voice = c
            .voice
            .as_ref()
            .map_or("1".to_owned(), |v| v.content.trim().to_owned());
        let len = time::from_divisions(len_div.max(0), divisions).unwrap_or(Ratio::ZERO);
        let tuplet = c.time_modification.as_ref().and_then(|t| {
            let notes = u8::try_from(t.content.actual_notes.content.0).ok()?;
            let in_time_of = u8::try_from(t.content.normal_notes.content.0).ok()?;
            (notes > 0 && in_time_of > 0 && notes != in_time_of)
                .then_some(Tuplet { notes, in_time_of })
        });
        let written = c.r#type.as_ref().map(|t| {
            let (value, clamped) = time::note_value(&t.content);
            if clamped {
                self.log.note(
                    "note values beyond a double whole or a 256th are clamped",
                    place(m, part),
                );
            }
            Duration {
                value,
                dots: u8::try_from(c.dot.len()).unwrap_or(3).min(3),
                tuplets: tuplet.into_iter().collect(),
            }
        });
        // The written value counts when it lasts what the divisions say;
        // otherwise the length decides (and the note may be split).
        let duration = match (grace, written) {
            (Some(_), Some(w)) => Some(w),
            (Some(_), None) => Some(Duration::new(NoteValue::Eighth)),
            (None, Some(w)) if w.length() == Some(len) => Some(w),
            (None, w) => {
                let exact = time::exact(len, tuplet);
                if w.is_some() && len > Ratio::ZERO {
                    self.log.note(
                        "notes whose written value doesn't match their length take their length",
                        place(m, part),
                    );
                }
                exact
            }
        };
        let mut beat = RawBeat {
            start: Ratio::ZERO,
            len: if grace.is_some() { Ratio::ZERO } else { len },
            duration,
            grace,
            rest: false,
            measure_rest: false,
            notes: Vec::new(),
            slur_start: 0,
            slur_stop: 0,
            arpeggio: None,
            pick_stroke: None,
            lyrics: Vec::new(),
            fermata: None,
            tap: false,
            dynamic: None,
        };
        let mut note = RawNote {
            pitch: None,
            spelling: None,
            fretted: None,
            tie: ties
                .iter()
                .any(|t| t.attributes.r#type == dt::StartStop::Stop),
            dynamic: *dynamic,
            effects: NoteEffects::default(),
        };
        match audible {
            mx::AudibleType::Rest(r) => {
                beat.rest = true;
                beat.measure_rest = r.attributes.measure == Some(dt::YesNo::Yes);
            }
            mx::AudibleType::Pitch(p) => {
                let p = &p.content;
                let alter = p.alter.as_ref().map_or(0, |a| i32::from(a.content.0));
                let octave = i32::from(p.octave.content.0);
                let written = midi(&p.step.content, alter, octave);
                let sounding = written.and_then(|w| u8::try_from(i32::from(w) + transpose).ok());
                match sounding.filter(|p| *p <= 127) {
                    Some(s) => note.pitch = Some(s),
                    None => {
                        self.log
                            .lost("notes outside the MIDI range are left out", place(m, part));
                        beat.rest = true;
                    }
                }
                if let Ok(alter) = i8::try_from(alter) {
                    note.spelling = Some((step_index(&p.step.content), alter));
                }
            }
            mx::AudibleType::Unpitched(u) => {
                let id = c.instrument.first().map(|i| i.attributes.id.0.as_str());
                let key = id
                    .and_then(|id| info.unpitched.get(id))
                    .copied()
                    .or_else(|| info.unpitched.values().next().copied());
                let key = key.or_else(|| {
                    midi(
                        &u.content.display_step.content,
                        0,
                        i32::from(u.content.display_octave.content.0),
                    )
                });
                note.pitch = key;
                if note.pitch.is_none() {
                    beat.rest = true;
                }
            }
        }
        if let Some(head) = &c.notehead {
            match head.content {
                dt::NoteheadValue::X | dt::NoteheadValue::Cross => note.effects.dead = true,
                _ => {}
            }
            if head.attributes.parentheses == Some(dt::YesNo::Yes) {
                note.effects.ghost = true;
            }
        }
        for notations in &c.notations {
            for e in &notations.content.notations {
                self.notation(e, &mut note, &mut beat, dynamic, (m, part));
            }
        }
        note.dynamic = *dynamic;
        for l in &c.lyric {
            if let mx::LyricContents::Text(t) = &l.content {
                let verse = l
                    .attributes
                    .number
                    .as_ref()
                    .map_or("1".to_owned(), |n| n.0.clone());
                let goes_on = matches!(
                    t.syllabic.as_ref().map(|s| &s.content),
                    Some(dt::Syllabic::Begin | dt::Syllabic::Middle)
                );
                let mut text = unescape(t.text.content.trim());
                for more in &t.additional {
                    text.push('+');
                    text.push_str(&unescape(more.text.content.trim()));
                }
                beat.lyrics.push((verse, text, goes_on));
            }
        }
        if !beat.rest {
            beat.notes.push(note);
        }
        Some((staff, voice, chord, len_div, beat))
    }

    /// One notation of a note.
    fn notation(
        &mut self,
        e: &mx::NotationContentTypes,
        note: &mut RawNote,
        beat: &mut RawBeat,
        dynamic: &mut Dynamic,
        (m, part): (usize, usize),
    ) {
        match e {
            mx::NotationContentTypes::Tied(t) => {
                if matches!(
                    t.attributes.r#type,
                    dt::StartStopContinue::Stop | dt::StartStopContinue::Continue
                ) {
                    note.tie = true;
                }
            }
            mx::NotationContentTypes::Slur(s) => match s.attributes.r#type {
                dt::StartStopContinue::Start => beat.slur_start = beat.slur_start.saturating_add(1),
                dt::StartStopContinue::Stop => beat.slur_stop = beat.slur_stop.saturating_add(1),
                dt::StartStopContinue::Continue => {}
            },
            mx::NotationContentTypes::Glissando(g) => {
                if g.attributes.r#type == dt::StartStop::Start {
                    note.effects.slides.push(Slide::Shift);
                }
            }
            mx::NotationContentTypes::Slide(s) => {
                if s.attributes.r#type == dt::StartStop::Start {
                    note.effects.slides.push(Slide::Shift);
                }
            }
            mx::NotationContentTypes::Articulations(a) => {
                for a in &a.content {
                    match a {
                        mx::ArticulationsType::Accent(_) => {
                            note.effects.accent = Some(Accent::Normal)
                        }
                        mx::ArticulationsType::StrongAccent(_) => {
                            note.effects.accent = Some(Accent::Heavy)
                        }
                        mx::ArticulationsType::Staccato(_)
                        | mx::ArticulationsType::Staccatissimo(_) => note.effects.staccato = true,
                        _ => self.log.lost(
                            "articulations other than accents and staccato are left out",
                            place(m, part),
                        ),
                    }
                }
            }
            mx::NotationContentTypes::Fermata(f) => {
                let kind = match f.content {
                    dt::FermataShape::Angled | dt::FermataShape::DoubleAngled => FermataKind::Short,
                    dt::FermataShape::Square | dt::FermataShape::DoubleSquare => FermataKind::Long,
                    _ => FermataKind::Medium,
                };
                // On its master bar once the beat's start is known.
                beat.fermata = Some(kind);
            }
            mx::NotationContentTypes::Ornaments(o) => {
                for o in &o.content.ornaments {
                    match o {
                        mx::OrnamentType::Mordent(_) => {
                            note.effects.ornament = Some(Ornament::LowerMordent)
                        }
                        mx::OrnamentType::InvertedMordent(_) => {
                            note.effects.ornament = Some(Ornament::UpperMordent)
                        }
                        mx::OrnamentType::Turn(_) | mx::OrnamentType::DelayedTurn(_) => {
                            note.effects.ornament = Some(Ornament::Turn)
                        }
                        mx::OrnamentType::InvertedTurn(_)
                        | mx::OrnamentType::DelayedInvertedTurn(_) => {
                            note.effects.ornament = Some(Ornament::InvertedTurn)
                        }
                        mx::OrnamentType::Tremolo(t) => {
                            let single = !matches!(
                                t.attributes.r#type,
                                Some(dt::TremoloType::Start | dt::TremoloType::Stop)
                            );
                            if single {
                                note.effects.tremolo_picking = Some(match t.content.0 {
                                    0 | 1 => NoteValue::Eighth,
                                    2 => NoteValue::Sixteenth,
                                    _ => NoteValue::ThirtySecond,
                                });
                            } else {
                                self.log
                                    .lost("two-note tremolos are left out", place(m, part));
                            }
                        }
                        mx::OrnamentType::WavyLine(_) => {}
                        _ => self.log.lost(
                            "trills and ornaments other than mordents and turns are left out",
                            place(m, part),
                        ),
                    }
                }
            }
            mx::NotationContentTypes::Technical(t) => {
                let mut string = None;
                let mut fret = None;
                for t in &t.content {
                    match t {
                        mx::TechnicalContents::StringNumber(s) => string = Some(s.content.0),
                        mx::TechnicalContents::Fret(f) => fret = u8::try_from(f.content.0).ok(),
                        mx::TechnicalContents::HammerOn(h) => {
                            if h.attributes.r#type == dt::StartStop::Start {
                                note.effects.hammer_pull = true;
                            }
                        }
                        mx::TechnicalContents::PullOff(p) => {
                            if p.attributes.r#type == dt::StartStop::Start {
                                note.effects.hammer_pull = true;
                            }
                        }
                        mx::TechnicalContents::Harmonic(h) => {
                            note.effects.harmonic = Some(if h.content.artificial.is_some() {
                                Harmonic::Artificial { semitones: 12 }
                            } else {
                                Harmonic::Natural
                            });
                        }
                        mx::TechnicalContents::Bend(b) => {
                            let cents =
                                i32::from(b.content.bend_alter.content.0).saturating_mul(100);
                            let at = |n, d| Ratio::new(n, d).unwrap_or(Ratio::ZERO);
                            let points = if b.content.pre_bend.is_some() {
                                vec![(at(0, 1), cents), (at(1, 1), cents)]
                            } else if b.content.release.is_some() {
                                vec![
                                    (at(0, 1), 0),
                                    (at(1, 3), cents),
                                    (at(2, 3), cents),
                                    (at(1, 1), 0),
                                ]
                            } else {
                                vec![(at(0, 1), 0), (at(1, 2), cents), (at(1, 1), cents)]
                            };
                            note.effects.bend = Some(Bend {
                                points: points
                                    .into_iter()
                                    .map(|(offset, cents)| BendPoint { offset, cents })
                                    .collect(),
                            });
                        }
                        mx::TechnicalContents::Fingering(f) => {
                            note.effects.left_finger = match f.content.trim() {
                                "0" | "t" | "T" => Some(0),
                                "1" => Some(1),
                                "2" => Some(2),
                                "3" => Some(3),
                                "4" => Some(4),
                                _ => note.effects.left_finger,
                            };
                        }
                        mx::TechnicalContents::Pluck(p) => {
                            note.effects.right_finger = match p.content.trim() {
                                "p" | "P" => Some(0),
                                "i" | "I" => Some(1),
                                "m" | "M" => Some(2),
                                "a" | "A" => Some(3),
                                "c" | "C" | "e" | "x" => Some(4),
                                _ => note.effects.right_finger,
                            };
                        }
                        mx::TechnicalContents::Tap(t) => {
                            if t.attributes.hand == Some(dt::TapHand::Left) {
                                note.effects.left_hand_tap = true;
                            } else {
                                beat.tap = true;
                            }
                        }
                        mx::TechnicalContents::UpBow(_) => {
                            beat.pick_stroke = Some(StrokeDirection::Up)
                        }
                        mx::TechnicalContents::DownBow(_) => {
                            beat.pick_stroke = Some(StrokeDirection::Down)
                        }
                        _ => {}
                    }
                }
                if let (Some(s), Some(f)) = (string, fret) {
                    note.fretted = Some((s, f));
                } else if string.is_some() {
                    note.effects.string_number = true;
                }
            }
            mx::NotationContentTypes::Arpeggiate(a) => {
                // MusicXML rolls upwards (from the lowest note) unless told
                // otherwise; OxiGuitar's Down rolls from the lowest string.
                beat.arpeggio = Some(match a.attributes.direction {
                    Some(dt::UpDown::Down) => StrokeDirection::Up,
                    _ => StrokeDirection::Down,
                });
            }
            mx::NotationContentTypes::Dynamics(d) => {
                if let Some(new) = dynamics(d) {
                    *dynamic = new;
                    beat.dynamic = Some(new);
                }
            }
            _ => {}
        }
    }

    /// A direction at `here` in measure `m`.
    #[allow(clippy::too_many_arguments)]
    fn direction(
        &mut self,
        d: &mx::DirectionContents,
        here: Ratio,
        staff: usize,
        m: usize,
        rm: &mut RawMeasure,
        dynamic: &mut Dynamic,
    ) {
        let mut words_here = Vec::new();
        for t in &d.direction_type {
            match &t.content {
                mx::DirectionTypeContents::Words(w) => {
                    for w in w {
                        let text = unescape(w.content.trim());
                        if !text.is_empty() {
                            words_here.push(text);
                        }
                    }
                }
                mx::DirectionTypeContents::Rehearsal(r) => {
                    let text: Vec<String> = r.iter().map(|r| unescape(r.content.trim())).collect();
                    let text = text.join(" ");
                    let master = &mut self.master(m).bar;
                    if master.marker.is_none() && !text.is_empty() {
                        // A rehearsal letter ("A", "B2") in its box; a
                        // section's name ("Verse") as text.
                        let letter =
                            text.chars().count() <= 3 && text.chars().all(char::is_alphanumeric);
                        master.marker = Some(if letter {
                            Marker {
                                letter: text,
                                ..Marker::default()
                            }
                        } else {
                            Marker {
                                text,
                                ..Marker::default()
                            }
                        });
                    }
                }
                mx::DirectionTypeContents::Segno(_) => self.add_direction(m, Direction::Segno),
                mx::DirectionTypeContents::Coda(_) => self.add_direction(m, Direction::Coda),
                mx::DirectionTypeContents::Dynamics(ds) => {
                    for d in ds {
                        if let Some(new) = dynamics(d) {
                            *dynamic = new;
                            rm.dynamics.push((here, staff, new));
                        }
                    }
                }
                mx::DirectionTypeContents::Wedge(w) => {
                    let span = match w.attributes.r#type {
                        dt::WedgeType::Crescendo => Some(Span::Hairpin(Hairpin::Crescendo)),
                        dt::WedgeType::Diminuendo => Some(Span::Hairpin(Hairpin::Decrescendo)),
                        dt::WedgeType::Stop => None,
                        dt::WedgeType::Continue => continue,
                    };
                    rm.spans.push((here, staff, span, true));
                }
                mx::DirectionTypeContents::OctaveShift(o) => {
                    let size = o.attributes.size.as_ref().map_or(8, |s| s.0);
                    let span = match (&o.attributes.r#type, size) {
                        (dt::UpDownStopContinue::Down, 15) => Some(Span::Ottava(Ottava::Alta15)),
                        (dt::UpDownStopContinue::Down, _) => Some(Span::Ottava(Ottava::Alta8)),
                        (dt::UpDownStopContinue::Up, 15) => Some(Span::Ottava(Ottava::Bassa15)),
                        (dt::UpDownStopContinue::Up, _) => Some(Span::Ottava(Ottava::Bassa8)),
                        (dt::UpDownStopContinue::Stop, _) => None,
                        (dt::UpDownStopContinue::Continue, _) => continue,
                    };
                    rm.spans.push((here, staff, span, false));
                }
                mx::DirectionTypeContents::Pedal(p) => {
                    let pedal = match p.attributes.r#type {
                        dt::PedalType::Start | dt::PedalType::Resume => Some(Pedal::Down),
                        dt::PedalType::Stop | dt::PedalType::Discontinue => Some(Pedal::Up),
                        _ => None,
                    };
                    if let Some(p) = pedal {
                        rm.pedals.push((here, staff, p));
                    }
                }
                mx::DirectionTypeContents::Metronome(mt) => {
                    if let mx::MetronomeContents::BeatBased(b) = &mt.content
                        && let mx::BeatEquation::BPM(per) = &b.equals
                        && let Ok(n) = per.content.trim().parse::<f64>()
                    {
                        let (value, _) = time::note_value(&b.beat_unit.content);
                        let unit = Duration {
                            value,
                            dots: u8::try_from(b.beat_unit_dot.len()).unwrap_or(1).min(3),
                            tuplets: Vec::new(),
                        };
                        if let Some(len) = unit.length() {
                            let quarters = n * len.to_f64() * 4.0;
                            if quarters.is_finite()
                                && quarters > 0.0
                                && d.sound
                                    .as_ref()
                                    .is_none_or(|s| s.attributes.tempo.is_none())
                            {
                                self.tempos.push((
                                    m,
                                    here,
                                    quarters.clamp(1.0, 999.0),
                                    String::new(),
                                ));
                            }
                        }
                    }
                }
                _ => {}
            }
        }
        if let Some(s) = &d.sound {
            let jump = self.sound(&s.attributes, here, m, words_here.join(" "));
            let tempo_label = s.attributes.tempo.is_some();
            if jump || tempo_label {
                words_here.clear();
            }
        }
        for w in words_here {
            rm.words.push((here, staff, w));
        }
    }

    /// A `sound` element's tempo and jumps; whether it held a jump (whose
    /// words are then the jump's, not a text).
    fn sound(&mut self, s: &mx::SoundAttributes, here: Ratio, m: usize, words: String) -> bool {
        if let Some(t) = &s.tempo {
            let bpm = t.0;
            if bpm.is_finite() && bpm > 0.0 {
                let label = if words.chars().any(|c| c.is_alphabetic()) {
                    words.clone()
                } else {
                    String::new()
                };
                self.tempos.push((m, here, bpm.clamp(1.0, 999.0), label));
            }
        }
        let lower = words.to_lowercase();
        let al_fine = lower.contains("fine");
        let al_coda = lower.contains("coda");
        let mut jump = false;
        if s.dacapo == Some(dt::YesNo::Yes) {
            self.add_direction(
                m,
                if al_fine {
                    Direction::DaCapoAlFine
                } else if al_coda {
                    Direction::DaCapoAlCoda
                } else {
                    Direction::DaCapo
                },
            );
            jump = true;
        }
        if s.dalsegno.is_some() {
            self.add_direction(
                m,
                if al_fine {
                    Direction::DalSegnoAlFine
                } else if al_coda {
                    Direction::DalSegnoAlCoda
                } else {
                    Direction::DalSegno
                },
            );
            jump = true;
        }
        if s.fine.is_some() {
            self.add_direction(m, Direction::Fine);
            jump = true;
        }
        if s.tocoda.is_some() {
            self.add_direction(m, Direction::ToCoda);
            jump = true;
        }
        if s.segno.is_some() {
            self.add_direction(m, Direction::Segno);
        }
        if s.coda.is_some() {
            self.add_direction(m, Direction::Coda);
        }
        jump
    }

    fn add_direction(&mut self, m: usize, d: Direction) {
        let bar = &mut self.master(m).bar;
        if !bar.directions.contains(&d) {
            bar.directions.push(d);
        }
    }

    /// A barline of measure `m`: repeats, endings and double bars.
    fn barline(&mut self, b: &mx::Barline, m: usize) {
        let c = &b.content;
        let master = &mut self.master(m).bar;
        if let Some(r) = &c.repeat {
            match r.attributes.direction {
                dt::BackwardForward::Forward => master.repeat_start = true,
                dt::BackwardForward::Backward => {
                    // Both count the plays in all: 2 for an ordinary repeat.
                    let times = r.attributes.times.as_ref().map_or(2, |t| t.0).clamp(2, 99);
                    master.repeat_end = Some(u8::try_from(times).unwrap_or(2));
                }
            }
        }
        if let Some(e) = &c.ending
            && e.attributes.r#type == dt::StartStopDiscontinue::Start
        {
            for n in e.attributes.number.0.split([',', ' ']) {
                if let Ok(n) = n.trim().parse::<u32>()
                    && (1..=16).contains(&n)
                {
                    master.alternate_endings |= 1 << (n - 1);
                }
            }
        }
        if let Some(style) = &c.bar_style
            && matches!(style.content, dt::BarStyle::LightLight)
            && b.attributes.location != Some(dt::RightLeftMiddle::Left)
        {
            master.double_bar = true;
        }
        if c.segno.is_some() {
            self.add_direction(m, Direction::Segno);
        }
        if c.coda.is_some() {
            self.add_direction(m, Direction::Coda);
        }
    }

    /// A chord symbol: the index of its diagram for this track.
    fn harmony(&mut self, h: &mx::Harmony) -> Option<usize> {
        let first = h.content.harmony.first()?;
        let root = first.root.as_ref()?;
        let mut name = step_name(&root.content.root_step.content).to_owned();
        name.push_str(accidental(
            root.content.root_alter.as_ref().map_or(0, |a| a.content.0),
        ));
        name.push_str(&kind_suffix(&first.kind));
        for d in &first.degree {
            let c = &d.content;
            let value = c.degree_value.content.0;
            let alter = c.degree_alter.content.0;
            match c.degree_type.content {
                dt::DegreeTypeValue::Add => {
                    name.push_str(&format!("(add{}{value})", accidental(alter)))
                }
                dt::DegreeTypeValue::Alter => {
                    name.push_str(&format!("({}{value})", accidental(alter)))
                }
                dt::DegreeTypeValue::Subtract => name.push_str(&format!("(no{value})")),
            }
        }
        if let Some(b) = &first.bass {
            name.push('/');
            name.push_str(step_name(&b.content.bass_step.content));
            name.push_str(accidental(
                b.content.bass_alter.as_ref().map_or(0, |a| a.content.0),
            ));
        }
        let mut diagram = ChordDiagram {
            name,
            first_fret: 1,
            frets: Vec::new(),
        };
        if let Some(f) = &h.content.frame {
            let strings = usize::try_from(f.content.frame_strings.content.0)
                .unwrap_or(0)
                .min(12);
            diagram.frets = vec![None; strings];
            diagram.first_fret = f
                .content
                .first_fret
                .as_ref()
                .and_then(|f| u8::try_from(f.content.0).ok())
                .unwrap_or(1);
            for n in &f.content.frame_note {
                let s = usize::from(n.content.string.content.0);
                if let (Some(slot), Ok(fret)) = (
                    s.checked_sub(1).and_then(|i| diagram.frets.get_mut(i)),
                    u8::try_from(n.content.fret.content.0),
                ) {
                    *slot = Some(fret);
                }
            }
        }
        if let Some(i) = self.diagrams.iter().position(|d| *d == diagram) {
            return Some(i);
        }
        self.diagrams.push(diagram);
        Some(self.diagrams.len() - 1)
    }

    /// Makes the track from what was read of its part.
    #[allow(clippy::too_many_arguments)]
    fn build_track(
        &mut self,
        part: usize,
        info: &PartInfo<'_>,
        states: &[StaffState],
        raw: &[RawMeasure],
        transpose: i32,
        octave_change: i32,
        unpitched_seen: bool,
    ) -> Track {
        // Which MusicXML staves make which of OxiGuitar's: a notation staff
        // with the part's tablature staff, and every other staff alone.
        let tab_staves: Vec<usize> = (0..states.len()).filter(|&s| states[s].tab).collect();
        let notation: Vec<usize> = (0..states.len()).filter(|&s| !states[s].tab).collect();
        let mut groups: Vec<(Option<usize>, Option<usize>)> = Vec::new();
        let mut tabs = tab_staves.iter().copied();
        for &n in &notation {
            groups.push((Some(n), if groups.is_empty() { tabs.next() } else { None }));
        }
        for t in tabs {
            groups.push((None, Some(t)));
        }
        let percussion =
            info.channel == Some(10) || unpitched_seen || states.iter().any(|s| s.percussion);
        let string_count = states.iter().find_map(|s| {
            s.tuning
                .as_ref()
                .map(Vec::len)
                .or(if s.tab { s.lines } else { None })
        });
        let clues = Clues {
            names: std::iter::once(info.name.as_str())
                .chain(info.instrument_names.iter().map(String::as_str))
                .collect(),
            sounds: info.sounds.clone(),
            program: info.program,
            percussion,
            strings: !tab_staves.is_empty() || states.iter().any(|s| s.tuning.is_some()),
            string_count,
        };
        let kind = instruments::kind(&clues);
        let instrument = Instrument {
            midi_program: info
                .program
                .unwrap_or_else(|| kind.default_program(&info.name)),
            percussion: kind == Kind::Drums,
        };
        let mut staves = Vec::new();
        for (n, t) in groups {
            let state = |s: Option<usize>| s.and_then(|s| states.get(s));
            let tuning_from = state(t)
                .and_then(|s| s.tuning.clone())
                .or_else(|| state(n).and_then(|s| s.tuning.clone()))
                .or_else(|| {
                    // A tablature staff with only its lines: the usual
                    // strings for that many.
                    state(t)
                        .and_then(|s| s.lines)
                        .and_then(|lines| match (kind, lines) {
                            (Kind::Bass, 4) => Some(Kind::Bass.default_tuning()),
                            (Kind::Bass, 5) => Some([43, 38, 33, 28, 23].map(Pitch).to_vec()),
                            (_, 6) => Some(Kind::Guitar.default_tuning()),
                            (_, 7) => Some([64, 59, 55, 50, 45, 40, 35].map(Pitch).to_vec()),
                            _ => None,
                        })
                });
            let tuning = if kind == Kind::Drums {
                Vec::new()
            } else {
                tuning_from.unwrap_or_else(|| kind.default_tuning())
            };
            let capo = state(t).or(state(n)).map_or(0, |s| s.capo);
            // The octave a guitar's 8vb clef carries is not a
            // transposition in OxiGuitar's model.
            let clef_of_staff = state(n).map_or(Clef::Treble, |s| s.clef);
            // A guitar or bass written an octave up on a plain clef gets
            // OxiGuitar's 8vb clef, as Guitar Pro shows them.
            let guitar_octave = !matches!(clef_of_staff, Clef::Treble8vb | Clef::Bass8vb)
                && octave_change == -1
                && matches!(kind, Kind::Guitar | Kind::Bass);
            let octave_clef =
                matches!(clef_of_staff, Clef::Treble8vb | Clef::Bass8vb) || guitar_octave;
            // OxiGuitar's transposition is sounding to written. On an 8vb
            // clef the clef carries an octave of it: a guitar written an
            // octave up (transpose -12) is plainly OxiGuitar's 8vb guitar,
            // and so is one written at its sounding pitch (no transpose).
            // `write` does the reverse.
            let written_shift = match (transpose, octave_clef) {
                (0, _) => 0,
                (t, true) => t + 12,
                (t, false) => t,
            };
            let mut staff = Staff {
                tuning,
                capo,
                fret_count: 24,
                transposition: i8::try_from(-written_shift).unwrap_or(0),
                show_standard: n.is_some(),
                show_tablature: t.is_some() || (kind.shows_tablature() && n.is_some()),
                bars: Vec::new(),
            };
            if kind == Kind::Drums {
                staff.show_tablature = false;
                staff.show_standard = true;
            }
            if !staff.show_standard && !staff.show_tablature {
                staff.show_standard = true;
            }
            let source = n.or(t).unwrap_or(0);
            for (m, rm) in raw.iter().enumerate() {
                let mut clef = rm
                    .clefs
                    .get(&source)
                    .copied()
                    .or_else(|| staff.bars.last().map(|b: &Bar| b.clef))
                    .unwrap_or(clef_of_staff);
                if guitar_octave && clef == Clef::Treble {
                    clef = Clef::Treble8vb;
                }
                if kind == Kind::Drums {
                    clef = Clef::Neutral;
                }
                let frets = t.filter(|_| n.is_some()).map(|t| fret_lookup(rm, t));
                let bar = self.build_bar(rm, source, m, part, &staff, frets.as_ref(), clef);
                staff.bars.push(bar);
            }
            staves.push(staff);
        }
        if staves.is_empty() {
            staves.push(Staff {
                tuning: kind.default_tuning(),
                ..Staff::default()
            });
        }
        // Marks spanning beats, chord symbols, texts and pedals, on the first
        // staff's first voice of each bar, and the lyrics.
        let lyrics = Self::place_marks(&mut staves, raw);
        Track {
            name: if info.name.is_empty() {
                format!("Track {}", part + 1)
            } else {
                info.name.clone()
            },
            short_name: info.abbreviation.clone(),
            color: oxiguitar_model::Color { r: 255, g: 0, b: 0 },
            instrument,
            mix: Mix {
                volume: info
                    .volume
                    .map_or(0.8, |v| (v / 100.0).clamp(0.0, 1.0) as f32),
                pan: info.pan.map_or(0.0, |p| (p / 90.0).clamp(-1.0, 1.0)),
                ..Mix::default()
            },
            staves,
            chords: std::mem::take(&mut self.diagrams),
            lyrics,
            plugins: BTreeMap::new(),
            effects: Vec::new(),
            play_chords: false,
            layout: Vec::new(),
        }
    }

    /// One bar of a staff from a measure's voices.
    #[allow(clippy::too_many_arguments)]
    fn build_bar(
        &mut self,
        rm: &RawMeasure,
        source: usize,
        m: usize,
        part: usize,
        staff: &Staff,
        frets: Option<&FretMap>,
        clef: Clef,
    ) -> Bar {
        let key = self.master(m).bar.key_signature;
        let bar_len = self.master(m).bar.expected_length().unwrap_or(Ratio::ONE);
        let mut frets = frets.cloned();
        let mut voices = Vec::new();
        let empty = Vec::new();
        let raw_voices = rm.staves.get(&source).unwrap_or(&empty);
        if raw_voices.len() > 4 {
            self.log.lost(
                "voices past the fourth of a staff are left out",
                place(m, part),
            );
        }
        for (_, beats) in raw_voices.iter().take(4) {
            let mut sorted: Vec<&RawBeat> = beats.iter().collect();
            sorted.sort_by_key(|b| b.start);
            let mut voice = Voice::default();
            let mut pos = Ratio::ZERO;
            for b in sorted {
                if b.start > pos {
                    let gap = b.start.checked_sub(pos).unwrap_or(Ratio::ZERO);
                    for d in time::fill(gap) {
                        voice.beats.push(Beat {
                            kind: BeatKind::Empty,
                            duration: d,
                            ..Beat::default()
                        });
                    }
                    pos = b.start;
                } else if b.start < pos && b.grace.is_none() {
                    self.log.lost(
                        "notes that overlap others in their voice are left out",
                        place(m, part),
                    );
                    continue;
                }
                let lens: Vec<Duration> = if b.measure_rest {
                    let len = if b.len > Ratio::ZERO { b.len } else { bar_len };
                    time::fill(len)
                } else if let Some(d) = &b.duration {
                    vec![d.clone()]
                } else {
                    let pieces = time::fill(b.len);
                    if pieces.is_empty() && b.len > Ratio::ZERO {
                        self.log.lost(
                            "notes of a length no note value can show are left out",
                            place(m, part),
                        );
                        continue;
                    }
                    pieces
                };
                for (i, d) in lens.into_iter().enumerate() {
                    let mut beat = Beat {
                        kind: if b.rest {
                            BeatKind::Rest
                        } else {
                            BeatKind::Notes
                        },
                        duration: d,
                        grace: b.grace,
                        ..Beat::default()
                    };
                    beat.effects.arpeggio = b.arpeggio;
                    if b.tap {
                        beat.effects.technique = Some(BeatTechnique::Tap);
                    }
                    if i == 0 {
                        beat.effects.dynamic = b.dynamic;
                    }
                    beat.effects.pick_stroke = b.pick_stroke;
                    for n in &b.notes {
                        beat.notes.push(self.place_note(
                            n,
                            i > 0,
                            staff,
                            key,
                            b.start,
                            frets.as_mut(),
                        ));
                    }
                    voice.beats.push(beat);
                }
                pos = pos.checked_add(b.len).unwrap_or(pos);
            }
            voices.push(voice);
        }
        if voices.is_empty() {
            voices.push(Voice::default());
        }
        // Fermatas read on this measure's notes: at their beats' starts.
        Bar {
            clef,
            voices,
            simile: None,
        }
    }

    /// A note in its place: on its string from the tablature or the file,
    /// or as a pitch.
    fn place_note(
        &mut self,
        n: &RawNote,
        continued: bool,
        staff: &Staff,
        key: KeySignature,
        start: Ratio,
        frets: Option<&mut FretMap>,
    ) -> Note {
        let pitch = n.pitch.unwrap_or(0);
        let from_tab = frets.and_then(|f| {
            let list = f.get_mut(&(start, pitch))?;
            (!list.is_empty()).then(|| list.remove(0))
        });
        let place = from_tab
            .or(n.fretted)
            .and_then(|(string, fret)| fretted(staff, string, fret, n.pitch))
            .unwrap_or(NotePlace::Pitched(Pitch(pitch)));
        let spelling = n.spelling.and_then(|(step, alter)| {
            let s = Spelling { step, alter };
            let written = u8::try_from(i32::from(pitch) + i32::from(staff.transposition))
                .ok()
                .map(Pitch)?;
            (s.spells(written) && s != Spelling::automatic(written, key)).then_some(s)
        });
        let mut effects = n.effects.clone();
        if continued {
            effects = NoteEffects::default();
        }
        Note {
            place,
            tie: n.tie || continued,
            dynamic: n.dynamic,
            effects,
            spelling,
        }
    }

    /// Spans, chord symbols, texts, pedals and fermatas onto the beats of
    /// each bar's first staff and voice; the lyrics as lines.
    fn place_marks(staves: &mut [Staff], raw: &[RawMeasure]) -> Vec<LyricLine> {
        let Some(staff) = staves.first_mut() else {
            return Vec::new();
        };
        let mut open: Option<Span> = None;
        let mut open_ottava: Option<Ottava> = None;
        let mut slur_depth: u32 = 0;
        let mut verses: BTreeMap<String, (usize, String)> = BTreeMap::new();
        for (m, (rm, bar)) in raw.iter().zip(staff.bars.iter_mut()).enumerate() {
            // Beats of the first voice with their starts.
            let mut starts = Vec::new();
            let mut pos = Ratio::ZERO;
            if let Some(v) = bar.voices.first() {
                for b in &v.beats {
                    starts.push(pos);
                    if b.grace.is_none() {
                        pos = b
                            .duration
                            .length()
                            .and_then(|l| pos.checked_add(l))
                            .unwrap_or(pos);
                    }
                }
            }
            let beat_at = |at: Ratio| -> Option<usize> {
                starts
                    .iter()
                    .rposition(|s| *s <= at)
                    .or(if starts.is_empty() { None } else { Some(0) })
            };
            let Some(voice) = bar.voices.first_mut() else {
                continue;
            };
            // Hairpins and ottavas: a beat is under one when its start
            // came at or before the beat's and its stop didn't; what is
            // left after the last beat (a stop at the bar's end) closes
            // after it.
            let mut spans: Vec<&(Ratio, usize, Option<Span>, bool)> = rm.spans.iter().collect();
            spans.sort_by_key(|s| s.0);
            let mut next = 0;
            let apply = |s: &(Ratio, usize, Option<Span>, bool),
                         open: &mut Option<Span>,
                         open_ottava: &mut Option<Ottava>| match (s.2, s.3)
            {
                (Some(Span::Hairpin(h)), _) => *open = Some(Span::Hairpin(h)),
                (Some(Span::Ottava(o)), _) => *open_ottava = Some(o),
                (None, true) => *open = None,
                (None, false) => *open_ottava = None,
            };
            for (bi, beat) in voice.beats.iter_mut().enumerate() {
                let here = starts.get(bi).copied().unwrap_or(Ratio::ZERO);
                while let Some(s) = spans.get(next).filter(|s| s.0 <= here) {
                    apply(s, &mut open, &mut open_ottava);
                    next += 1;
                }
                if let Some(Span::Hairpin(h)) = open {
                    beat.hairpin = Some(h);
                }
                beat.ottava = open_ottava;
            }
            for s in spans.iter().skip(next) {
                apply(s, &mut open, &mut open_ottava);
            }
            let _ = &mut slur_depth;
            for (at, d) in &rm.chords {
                if let Some(b) = beat_at(*at)
                    && let Some(beat) = voice.beats.get_mut(b)
                {
                    beat.chord = Some(*d);
                }
            }
            for (at, _, text) in &rm.words {
                if let Some(b) = beat_at(*at)
                    && let Some(beat) = voice.beats.get_mut(b)
                {
                    if !beat.text.is_empty() {
                        beat.text.push(' ');
                    }
                    beat.text.push_str(text);
                }
            }
            for (at, _, d) in &rm.dynamics {
                if let Some(b) = beat_at(*at)
                    && let Some(beat) = voice.beats.get_mut(b)
                {
                    beat.effects.dynamic = Some(*d);
                }
            }
            for (at, _, pedal) in &rm.pedals {
                if let Some(b) = beat_at(*at)
                    && let Some(beat) = voice.beats.get_mut(b)
                {
                    beat.effects.pedal = Some(*pedal);
                }
            }
            // Slurs and lyrics come with the beats of the first voice.
            if let Some(raw_voice) = rm
                .staves
                .get(&0)
                .or_else(|| rm.staves.values().next())
                .and_then(|v| v.first())
            {
                let mut sorted: Vec<&RawBeat> = raw_voice.1.iter().collect();
                sorted.sort_by_key(|b| b.start);
                for rb in sorted {
                    let Some(b) = beat_at(rb.start) else { continue };
                    let Some(beat) = voice.beats.get_mut(b) else {
                        continue;
                    };
                    // OxiGuitar's slur on a beat ties it to the next: every
                    // beat from a slur's start up to the one before its end.
                    slur_depth += u32::from(rb.slur_start);
                    slur_depth = slur_depth.saturating_sub(u32::from(rb.slur_stop));
                    if slur_depth > 0 {
                        beat.slur = true;
                    }
                    for (verse, syllable, goes_on) in &rb.lyrics {
                        let line = verses.entry(verse.clone()).or_insert((m, String::new()));
                        if syllable.is_empty() {
                            continue;
                        }
                        // Beats sung before this syllable in the line are
                        // filled by `lyric_line` below.
                        line.1.push_str(&format!(
                            "\u{0}{m}\u{1}{b}\u{1}{syllable}\u{1}{}",
                            u8::from(*goes_on)
                        ));
                    }
                }
            }
        }
        let mut lines = Vec::new();
        for (_, (start_bar, coded)) in verses {
            lines.push(lyric_line(staff, start_bar, &coded));
        }
        lines
    }
}

/// The text of a lyric line from its syllables, coded as
/// `\0bar\u{1}beat\u{1}syllable\u{1}hyphen` each: one syllable a sung beat,
/// spaces for sung beats without one.
fn lyric_line(staff: &Staff, start_bar: usize, coded: &str) -> LyricLine {
    let sung = oxiguitar_model::lyrics::sung_beats(staff, start_bar);
    let mut text = String::new();
    let mut next = 0;
    for item in coded.split('\u{0}').filter(|s| !s.is_empty()) {
        let mut f = item.split('\u{1}');
        let (Some(bar), Some(beat), Some(syl), Some(hyphen)) =
            (f.next(), f.next(), f.next(), f.next())
        else {
            continue;
        };
        let (Ok(bar), Ok(beat)) = (bar.parse::<usize>(), beat.parse::<usize>()) else {
            continue;
        };
        let Some(index) = sung.iter().position(|&(b, i)| b == bar && i == beat) else {
            continue;
        };
        if index < next {
            continue;
        }
        // An empty beat for each sung beat skipped.
        for _ in next..index {
            text.push(' ');
        }
        let word: String = syl
            .chars()
            .map(|c| if c == ' ' || c == '-' { '+' } else { c })
            .collect();
        text.push_str(&word);
        text.push(if hyphen == "1" { '-' } else { ' ' });
        next = index + 1;
    }
    LyricLine {
        start_bar,
        text: text.trim_end().to_owned(),
    }
}

/// The note on `string` at `fret` (from the nut) of `staff`, its fret
/// counted from the capo as OxiGuitar counts it: from the nut when that
/// gives the note's pitch, from the capo when that does.
fn fretted(staff: &Staff, string: u8, fret: u8, pitch: Option<u8>) -> Option<NotePlace> {
    let open = staff.tuning.get(usize::from(string).checked_sub(1)?)?.0;
    let from_nut = u16::from(open) + u16::from(fret);
    let place = |fret| Some(NotePlace::Fretted { string, fret });
    match pitch {
        Some(p) if u16::from(p) == from_nut && fret >= staff.capo => place(fret - staff.capo),
        Some(p) if u16::from(p) == from_nut + u16::from(staff.capo) => place(fret),
        // A tablature that disagrees with the pitch: the pitch wins.
        Some(_) => None,
        None if fret >= staff.capo => place(fret - staff.capo),
        None => None,
    }
}

/// A tablature staff's strings and frets by (start, sounding pitch).
type FretMap = BTreeMap<(Ratio, u8), Vec<(u8, u8)>>;

/// The strings and frets of a tablature staff's notes in a measure.
fn fret_lookup(rm: &RawMeasure, tab: usize) -> FretMap {
    let mut out = FretMap::new();
    for (_, beats) in rm.staves.get(&tab).into_iter().flatten() {
        for b in beats {
            for n in &b.notes {
                if let (Some(p), Some(sf)) = (n.pitch, n.fretted) {
                    out.entry((b.start, p)).or_default().push(sf);
                }
            }
        }
    }
    out
}

/// Adds a beat to its staff and voice, or a chord note to the beat before.
fn push_beat(rm: &mut RawMeasure, staff: usize, voice: String, chord: bool, beat: RawBeat) {
    let voices = rm.staves.entry(staff).or_default();
    let index = match voices.iter().position(|(v, _)| *v == voice) {
        Some(i) => i,
        None => {
            voices.push((voice, Vec::new()));
            voices.len() - 1
        }
    };
    let Some((_, beats)) = voices.get_mut(index) else {
        return;
    };
    if chord && let Some(last) = beats.last_mut() {
        last.notes.extend(beat.notes);
        last.lyrics.extend(beat.lyrics);
        last.slur_start = last.slur_start.saturating_add(beat.slur_start);
        last.slur_stop = last.slur_stop.saturating_add(beat.slur_stop);
        last.arpeggio = last.arpeggio.or(beat.arpeggio);
        last.tap |= beat.tap;
        last.dynamic = last.dynamic.or(beat.dynamic);
        if last.rest && !last.notes.is_empty() {
            last.rest = false;
        }
        return;
    }
    beats.push(beat);
}

impl RawBeat {
    /// A rest of `len`.
    fn rest(len: Ratio) -> RawBeat {
        RawBeat {
            start: Ratio::ZERO,
            len,
            duration: time::exact(len, None),
            grace: None,
            rest: true,
            measure_rest: false,
            notes: Vec::new(),
            slur_start: 0,
            slur_stop: 0,
            arpeggio: None,
            pick_stroke: None,
            lyrics: Vec::new(),
            fermata: None,
            tap: false,
            dynamic: None,
        }
    }
}

/// A staff's index from MusicXML's staff number (1 when absent).
fn staff_index(number: Option<impl Into<u64>>) -> usize {
    number
        .map(Into::into)
        .and_then(|n| usize::try_from(n).ok())
        .unwrap_or(1)
        .clamp(1, 8)
        - 1
}

/// A clef's model, and whether it is a tablature or percussion clef.
fn clef_of(c: &mx::ClefContents) -> (Clef, bool, bool) {
    let line = c.line.as_ref().map(|l| l.content.0);
    let octave = c.clef_octave_change.as_ref().map_or(0, |o| o.content);
    match (&c.sign.content, line) {
        (dt::ClefSign::G, _) if octave == -1 => (Clef::Treble8vb, false, false),
        (dt::ClefSign::G, _) => (Clef::Treble, false, false),
        (dt::ClefSign::F, _) if octave == -1 => (Clef::Bass8vb, false, false),
        (dt::ClefSign::F, _) => (Clef::Bass, false, false),
        (dt::ClefSign::C, Some(4)) => (Clef::Tenor, false, false),
        (dt::ClefSign::C, _) => (Clef::Alto, false, false),
        (dt::ClefSign::TAB, _) => (Clef::Treble, true, false),
        (dt::ClefSign::Percussion, _) => (Clef::Neutral, false, true),
        _ => (Clef::Treble, false, false),
    }
}

/// What a `staff-details` says: lines, tuning, capo.
fn staff_details(st: &mut StaffState, d: &mx::StaffDetailsContents) {
    if let Some(l) = &d.staff_lines {
        st.lines = usize::try_from(l.content.0).ok();
    }
    if let Some(c) = &d.capo {
        st.capo = u8::try_from(c.content.0).unwrap_or(0).min(24);
    }
    if !d.staff_tuning.is_empty() {
        // Lines count from the bottom: line 1 is the lowest string.
        let mut tuned: Vec<(u8, u8)> = d
            .staff_tuning
            .iter()
            .filter_map(|t| {
                let c = &t.content;
                let alter = c
                    .tuning_alter
                    .as_ref()
                    .map_or(0, |a| i32::from(a.content.0));
                let p = midi(
                    &c.tuning_step.content,
                    alter,
                    i32::from(c.tuning_octave.content.0),
                )?;
                Some((t.attributes.line.0, p))
            })
            .collect();
        tuned.sort_by_key(|(line, _)| std::cmp::Reverse(*line));
        tuned.dedup_by_key(|(line, _)| *line);
        if !tuned.is_empty() {
            st.tuning = Some(tuned.into_iter().map(|(_, p)| Pitch(p)).collect());
        }
    }
}

/// A dynamics mark's value, for the marks OxiGuitar has.
fn dynamics(d: &mx::Dynamics) -> Option<Dynamic> {
    d.content.iter().find_map(|t| {
        Some(match t {
            mx::DynamicsType::Pppppp(_)
            | mx::DynamicsType::Ppppp(_)
            | mx::DynamicsType::Pppp(_)
            | mx::DynamicsType::Ppp(_) => Dynamic::PPP,
            mx::DynamicsType::Pp(_) => Dynamic::PP,
            mx::DynamicsType::P(_) => Dynamic::P,
            mx::DynamicsType::Mp(_) => Dynamic::MP,
            mx::DynamicsType::Mf(_) => Dynamic::MF,
            mx::DynamicsType::F(_)
            | mx::DynamicsType::Sf(_)
            | mx::DynamicsType::Sfz(_)
            | mx::DynamicsType::Fz(_)
            | mx::DynamicsType::Rf(_)
            | mx::DynamicsType::Rfz(_)
            | mx::DynamicsType::Fp(_) => Dynamic::F,
            mx::DynamicsType::Ff(_) | mx::DynamicsType::Sffz(_) => Dynamic::FF,
            mx::DynamicsType::Fff(_)
            | mx::DynamicsType::Ffff(_)
            | mx::DynamicsType::Fffff(_)
            | mx::DynamicsType::Ffffff(_) => Dynamic::FFF,
            _ => return None,
        })
    })
}

fn step_name(step: &dt::Step) -> &'static str {
    match step {
        dt::Step::A => "A",
        dt::Step::B => "B",
        dt::Step::C => "C",
        dt::Step::D => "D",
        dt::Step::E => "E",
        dt::Step::F => "F",
        dt::Step::G => "G",
    }
}

fn accidental(alter: i16) -> &'static str {
    match alter {
        ..=-2 => "bb",
        -1 => "b",
        1 => "#",
        2.. => "##",
        _ => "",
    }
}

/// A chord kind as written after the root: the file's own text when it
/// gives one, otherwise the usual abbreviation.
fn kind_suffix(kind: &mx::Kind) -> String {
    if let Some(text) = &kind.attributes.text {
        return unescape(&text.0);
    }
    use dt::KindValue as K;
    match kind.content {
        K::Major => "",
        K::Minor => "m",
        K::Augmented => "+",
        K::Diminished => "dim",
        K::Dominant => "7",
        K::MajorSeventh => "maj7",
        K::MinorSeventh => "m7",
        K::DiminishedSeventh => "dim7",
        K::AugmentedSeventh => "+7",
        K::HalfDiminished => "m7b5",
        K::MajorMinor => "m(maj7)",
        K::MajorSixth => "6",
        K::MinorSixth => "m6",
        K::DominantNinth => "9",
        K::MajorNinth => "maj9",
        K::MinorNinth => "m9",
        K::Dominant11th => "11",
        K::Major11th => "maj11",
        K::Minor11th => "m11",
        K::Dominant13th => "13",
        K::Major13th => "maj13",
        K::Minor13th => "m13",
        K::SuspendedSecond => "sus2",
        K::SuspendedFourth => "sus4",
        K::Power => "5",
        K::Neapolitan => "N6",
        K::Italian => "It+6",
        K::French => "Fr+6",
        K::German => "Ger+6",
        K::Pedal => "ped",
        K::Tristan => "Tristan",
        K::None | K::Other => "",
    }
    .to_owned()
}
