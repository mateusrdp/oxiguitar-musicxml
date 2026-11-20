# Requirements and assumptions

## Requirements

- OxiGuitar 0.18 or later (plugin interface 1.7). See [the OxiGuitar manual](https://mateusrdp.github.io/oxiguitar/).
- Windows, macOS or Linux.
- To hear a score you also need a plugin that plays scores and an audio output plugin.

## Assumptions

- The file is MusicXML, plain or compressed.
- A part marked as guitar or bass in the file is one. A part with no such mark is treated as an ordinary pitched part.
- When a file has no tablature, a computed fingering is acceptable. It may differ from what you would choose.
