# The MusicXML plugin: design

MusicXML is the format notation programs exchange scores in. This plugin (#223 in github.com/mateusrdp/oxiguitar) reads and writes it, uncompressed (`.musicxml`, `.xml`) and compressed (`.mxl`), so a score from MuseScore, Finale, Sibelius, Dorico or Guitar Pro opens in OxiGuitar as tablature, and OxiGuitar's scores open in them.

## Decided by Mateus (8 Nov 2026)

- **Which parts get tablature.** Parts the file marks as guitars or basses get strings and show their tablature. Every other pitched part (a melody, a piano, a voice) also gets a six-string guitar's strings, so OxiGuitar fingers it as it opens (#157), but shows notation only until the user turns its tablature on in the Track tab. Drums get no strings.
- **The library.** MusicXML is read and written with the musicxml crate (1.1.2, MIT), as MusicXML 4.0 is the format's latest release.
- **What is written.** MusicXML 4.0, uncompressed and compressed, each fretted staff as a notation staff and a tablature staff with the string and fret of every note, so programs that read tablature (OxiGuitar itself included) keep the fingering.
- **Recognising the files** needed a core change, plugin interface 1.7 (oxiguitar#253): signatures that look at an XML file's first element (`xml_root`) and at the first element of a ZIP container's root file (`container_root`), since `.mxl` and MuseScore's `.mscz` have the same ZIP layout. A small core release, v0.18.0, comes before this plugin's first release.

## The library, and what is added on top of it

The musicxml crate is used exactly as published (OxiGuitar's rule). What it lacks is added in this plugin's own code around it:

- **Compressed files are opened with the `zip` crate** (`src/mxl.rs`), not with the crate's own ZIP reader, which panics on some damaged archives (found by cutting a valid `.mxl` short). Writing goes the same way.
- **Text is decoded and escaped here** (`src/text.rs`). The crate reads and writes XML characters as they are: it doesn't decode `&amp;` and the other entities, doesn't escape `&`, `<`, `>` or `"` when writing (a title holding an e-mail address in angle brackets made a file that couldn't be read back), and indexes two bytes of any input that isn't UTF-8 (a panic on a one-byte file). So the file's bytes become UTF-8 first (with or without a byte order mark, UTF-16 with one, or the ISO-8859-1 and Windows-1252 that older programs declare), every text taken from the crate is unescaped and every text given to it escaped.
- **The document type** the crate writes names MusicXML 3.0 on a 4.0 score; the writer puts 4.0 there.
- Its XML parser survived 100,000 levels of nesting, every truncation of a real file and thousands of random byte changes without a panic (spike, 8 Nov 2026); the fuzz targets keep checking (`fuzz/`).

The crate's numbers are whole where MusicXML allows decimals in a few places: an `alter` of a quarter tone or a fractional `bend-alter` can't be read or written. A file with a microtonal pitch is refused by the crate as a whole.

## Reading

- **Parts are tracks.** A part's notation staff and its tablature staff (as MuseScore and Guitar Pro write a guitar) become one staff that shows both, its notes taking their strings and frets from the tablature, matched by moment and pitch. Any other staff is a staff of its own (a piano's two staves make a grand staff). Notes with no string keep their pitch, `NotePlace::Pitched`, and OxiGuitar fingers them as the file opens.
- **Strings.** From `staff-details` (`staff-tuning` counts lines from the bottom, so line 1 is the lowest string), or the usual strings for a tablature staff's line count, or the kind's default (six-string guitar, four-string bass). A fret is counted from the nut or from the capo, whichever gives the note's pitch; a tablature that disagrees with the pitch loses to it.
- **Which instrument.** From the instrument's sound (`pluck.guitar…`, `pluck.bass…`), its General MIDI program, or words in its name in the languages MusicXML files usually come in (Guitar, Violão, Gitarre …; "Bass" alone is not enough, it is also a voice). Drums: MIDI channel 10, a percussion clef or unpitched notes; each note's key comes from its instrument's `midi-unpitched`.
- **Pitch and transposition.** Sounding pitch is the written pitch plus the part's `transpose`. On an 8vb clef the clef carries an octave of it: a guitar written an octave up (transpose −12) is OxiGuitar's usual 8vb guitar, and so is one written at its sounding pitch with no transpose. A guitar or bass written an octave up on a plain treble clef gets the 8vb clef too, as Guitar Pro shows it.
- **Time.** Counted in whole notes from MusicXML's divisions, so voices that `backup` and `forward` are laid out exactly; gaps become empty beats. A note's written value counts when it lasts what its divisions say; otherwise its length decides (and the import log says so). A bar that is over- or underfull in the file stays so (OxiGuitar shows it red): show what the file says.
- **Voices.** Up to four a staff, in the order they first appear; more are named in the log.
- **Spelling.** A note's written step and alteration is kept where it isn't the one OxiGuitar would choose.
- **Bars.** Time signatures (a composite one as its sum), keys, repeats (`times` is the plays in all, as OxiGuitar counts them), endings, double bars, rehearsal marks (a letter in its box, a name as the marker's text), segno, coda, Fine, To Coda, D.C. and D.S. with their "al Fine" and "al Coda" from the words, tempo from `sound` or the metronome mark, fermatas.
- **Beats and notes.** Chords, grace notes (slashed before the beat, `steal-time-following` on it), tuplets (one level), rests and whole-bar rests, ties, slurs, accents, staccato, mordents and turns, single-note tremolo, hammer-ons and pull-offs, natural and artificial harmonics, bends (with pre-bend and release), left- and right-hand fingering, taps, up- and down-strokes (bowings), arpeggios, slides and glissandos, dead and ghost notes (an X notehead, a parenthesised one), dynamics, hairpins, 8va and 15ma lines, the sustain pedal, words above the staff.
- **Chord symbols** become chord diagrams named from the root, the kind (its own text when the file gives one), degrees and bass, with the frets of a `frame` when there is one.
- **Lyrics** become lines, one a verse, a syllable a sung beat of the first voice, as OxiGuitar's lyrics are.
- **Header.** Work and movement titles, the creators (composer, lyricist, transcriber, artist; others such as the arranger as notices, "Arranger: …"), rights, and the album, instructions and notices OxiGuitar wrote in `miscellaneous-field`s.
- **What is left out**, named in the import log: cue notes, keys not on the circle of fifths, two-note tremolos, trills, articulations other than accents and staccato, voices past the fourth. Layout (`print`, `defaults`, positions) is not read.

## Writing

- Each track is a part; each staff a notation staff, a tablature staff, or both, the tablature's notes carrying their string and fret (from the nut) and its `staff-details` the strings and the capo. Voices 1 to 4 of MusicXML staff *n* are numbered from 4(*n* − 1) + 1, as MuseScore numbers them.
- An 8vb staff with no transposition is written at sounding pitch on the 8vb clef; any other transposition as a `transpose`, the reverse of reading, so a file OxiGuitar writes reads back the same.
- Marks that belong to a staff (directions, chord symbols, lyrics, slurs, fermatas) go on its first MusicXML staff: the notation, or the tablature when it is shown alone. Tempo changes belong to the bar, at its start with an `offset` to where they are.
- Divisions per quarter note are the least that makes every beat's length whole.
- **Losses**, named before anything is written (each kind once, counted, with its first place): strums, whammy-bar bends and vibrato, slaps and pops, fades and swells, tremolo picking on whole beats, let ring, wah, rasgueado, pick scrapes, dead slaps, slash noteheads, barres, beaming choices, vibrato, palm mutes, trills, slides into and out of notes, string numbers, triplet feel, simile signs (their bars are written out), Design Mode's bars per line, a track's sound settings, bends of less than a semitone, nested tuplets (written as the outer one); bends other than the three plain shapes are written as their highest point; tapped, pinch, semi- and feedback harmonics as artificial ones.

## Checked

- The repository's own file (`fixtures/oxiguitar-test.musicxml`) reads as it should and writes back exactly, plain and compressed (`tests/read_write.rs`).
- The private corpus, locally (`tests/corpus.rs`, with the files unpacked into `corpus/files/`, which git ignores): the MuseScore export in `scores.tar.gz`, and all 104 Guitar Pro files of `tabs.tar.gz` converted to MusicXML with `oxiguitar-cli convert`, read back with the same tracks, note counts, bars and bar check, and, written again, read back exactly when the writer named no losses and stable from the second round trip otherwise.
- The contract suite through OxiGuitar's own plugin host (`tests/contract.rs`), damaged and hostile input, and the fuzz targets.

## Later, not asked for

Layout (system and page breaks) both ways; `.mxl` files with several scores; microtones; MusicXML's `wavy-line` for vibrato and words for palm mutes and let ring, which Guitar Pro writes; percussion display positions from the file rather than the drum key's name.
