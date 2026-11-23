# Architecture and strategy

The plugin is one file with a reader and a writer on top of the `musicxml` Rust library. We wrote the parts around it: compressed archives, text encodings, and the decision about which parts are guitars, basses, drums or something else.

The strategy is to use the library as published and handle its gaps in our own code, instead of forking it. The file type is decided from the first element of the XML, never from the extension. The repository has a test file of our own, tests for damaged files and edge cases, and fuzzing for the reader and the writer.

`docs/design.md` in the repository says what is read and written, what is left out, and why.

## Current limitations

- **Left out when reading**, and named in the import log: cue notes, keys that aren't on the circle of fifths, two-note tremolos, trills, articulations other than accents and staccato, and voices past the fourth.
- **No layout.** System and page breaks, positions and spacing aren't read or written: OxiGuitar lays out the score its own way.
- **One score per `.mxl`.** Archives holding several scores aren't supported yet.
- **No microtones.**
- **Some guitar marks Guitar Pro writes as text**, such as palm mute and let ring as words, or vibrato as a wavy line, aren't read as marks yet.

## Left out on purpose

- **A parser of our own.** The [musicxml](https://github.com/hedgetechllc/musicxml) Rust library reads and writes the format; we use it as published and handle its gaps (ZIP oddities, text encodings, older DOCTYPEs) in our code, not in a fork.
- **Guessing from the extension.** The file type comes from the XML's first element. A `.xml` file that isn't MusicXML is left to other plugins.
- **Writing everything into MusicXML.** Things MusicXML can't hold are named before saving, rather than squeezed into text that other programs would misread.

## Where you can help

- **Text marks to real marks**: palm mute, let ring and vibrato as Guitar Pro writes them (`src/`, `docs/design.md`).
- **Layout**: system and page breaks both ways.
- **Files from Finale, Sibelius and Dorico** to test with, especially with tablature. Never attach files you aren't allowed to share.

Every change needs its tests and a row on the [Features](features.md) page; the [Testing](testing.md) page says how to run them.

