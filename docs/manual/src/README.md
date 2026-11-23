# MusicXML plugin

This plugin lets OxiGuitar swap scores with other notation programs. With it, OxiGuitar opens what MuseScore, Finale, Sibelius, Dorico or Guitar Pro export as MusicXML, and saves scores those programs can open.

It reads and writes `.musicxml` and `.xml` files, and compressed `.mxl` files. It only handles files. It makes no sound.

## What MusicXML is

Every notation program saves in its own format, and none of them reads all the others. **MusicXML** is the common language they agreed on for moving a score from one to another. It was created by Michael Good at Recordare around 2004, is now looked after by the W3C's Music Notation Community Group, and almost every notation program can import and export it.

As the name says, it is **XML**: a text format of nested tags, readable (just about) by people and easy for programs to check. A note looks something like this:

```xml
<note>
  <pitch><step>E</step><octave>4</octave></pitch>
  <duration>2</duration>
  <type>quarter</type>
  <notations><technical><string>1</string><fret>0</fret></technical></notations>
</note>
```

Because it spells everything out, a MusicXML file is big; a **`.mxl`** file is the same thing packed in a ZIP archive, a fraction of the size.

## What survives the trip

MusicXML is a general-purpose format. It is excellent at standard notation: pitches, rhythms, voices, key and time signatures, dynamics, lyrics, repeats. Guitar-specific details are another matter: string and fret numbers, bends and slides are in the standard, but programs write them in different ways, and some things (a whammy bar, for instance) have no place at all. So:

- **Guitar and bass parts** with tablature open as tablature. When a file has only the notes, OxiGuitar works out a fingering as it opens.
- **When saving**, each guitar is written as a notation staff and a tablature staff, so the frets survive in programs that read tablature.
- **Before saving**, OxiGuitar tells you if anything in your score can't be written to MusicXML.

To keep everything, save in a Guitar Pro format; use MusicXML to share with notation programs.
