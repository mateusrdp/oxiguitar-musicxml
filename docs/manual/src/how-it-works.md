# Architecture and strategy

The plugin is one file with a reader and a writer on top of the `musicxml` Rust library. We wrote the parts around it: compressed archives, text encodings, and the decision about which parts are guitars, basses, drums or something else.

The strategy is to use the library as published and handle its gaps in our own code, instead of forking it. The file type is decided from the first element of the XML, never from the extension. The repository has a test file of our own, tests for damaged files and edge cases, and fuzzing for the reader and the writer.

`docs/design.md` in the repository says what is read and written, what is left out, and why.
