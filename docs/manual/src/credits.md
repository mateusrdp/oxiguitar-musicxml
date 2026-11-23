# Credits

Thank you to the people whose work this plugin is built on.

## Code we use

| Project | Authors | What it does here | Licence |
| --- | --- | --- | --- |
| [OxiGuitar's plugin SDK](https://github.com/mateusrdp/oxiguitar) (`oxiguitar-plugin`, `oxiguitar-model`, and `oxiguitar-plugin-test` for the tests) | Mateus Rocha de Paula and the OxiGuitar contributors | The plugin interface and the score model | MIT OR Apache-2.0 for the interface and SDK; GPL-3.0-or-later for the model |
| [musicxml](https://github.com/hedgetechllc/musicxml) | Will Hedgecock | Reading and writing MusicXML | MIT |
| [zip](https://github.com/zip-rs/zip2) | Mathijs van de Nes, Marli Frost, Ryan Levick, Chris Hennick and contributors | Compressed `.mxl` archives | MIT |
| [zlib-rs](https://github.com/trifectatechfoundation/zlib-rs) | The Trifecta Tech Foundation | Deflate compression, in pure Rust, under zip | Zlib |
| [serde_json](https://github.com/serde-rs/json) | David Tolnay, Erick Tryzelaar and contributors | Tests only | MIT OR Apache-2.0 |
| [Rust](https://www.rust-lang.org/) and its standard library | The Rust Project | Everything else | MIT OR Apache-2.0 |

## Standards

[MusicXML](https://www.w3.org/2021/06/musicxml40/), created by Michael Good and Recordare, maintained by the W3C Music Notation Community Group.

## Tools

| Tool | Use |
| --- | --- |
| [cargo-fuzz](https://github.com/rust-fuzz/cargo-fuzz) and [libfuzzer-sys](https://github.com/rust-fuzz/libfuzzer) | Fuzzing the reader and the writer (`fuzz/`) |
| [mdBook](https://github.com/rust-lang/mdBook) | Builds this manual |
| [cargo-deny](https://github.com/EmbarkStudios/cargo-deny) | Checks the licences of everything we build with |

## Thanks

To Will Hedgecock for the musicxml library, and to Michael Good and the W3C community group for keeping a common language for notation alive.
