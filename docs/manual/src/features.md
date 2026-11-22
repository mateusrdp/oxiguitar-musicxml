# Features and their tests

Everything the MusicXML plugin does, and the tests that check it. The test names are the ones `cargo test` prints; a `stage:` is a step of `scripts/test-all.py`. The script fails when a feature here names no test, or names a test that doesn't exist, so a new feature gets its row and its tests in the same pull request (the [Testing](testing.md) page says how to run them, on your own files too).

| Feature | What it does | Tests |
| --- | --- | --- |
| Recognises MusicXML by its contents | Plain files by their root (`score-partwise`, `score-timewise`), compressed `.mxl` by the root file their container names; nothing else. | `contract::its_signatures_recognise_musicxml_only`, `corpus::musicxml_is_recognised_by_its_contents`, `mxl::tests::the_container_names_the_score` |
| Reads the header, parts and bars | Titles, creators, rights; parts as tracks; time and key signatures, repeats, endings, rehearsal marks, tempo; exact durations from divisions, gaps filled. | `read_write::reads_the_header_parts_and_bars`, `time::tests::exact_durations`, `time::tests::fills_lengths`, `read_write::timewise_files_read_as_partwise_ones` |
| Jumps and signs | Segno, coda, Fine, To Coda, D.C. and D.S. with their "al Fine" and "al Coda". | `read_write::reads_jumps_and_signs` |
| Guitars open as tablature | A guitar takes its strings and frets from its tablature staff; other pitched parts get a guitar's strings, drums none; the instrument from its sound, program or name. | `read_write::a_guitar_takes_its_strings_and_frets_from_its_tablature`, `read_write::other_parts_get_a_guitars_strings_drums_none`, `instruments::tests::kinds_from_clues` |
| Text as files carry it | Encodings and entities both ways; texts with markup characters survive. | `text::tests::decodes_what_files_come_in`, `text::tests::entities_both_ways`, `read_write::texts_with_markup_characters_survive` |
| Writes MusicXML, plain and compressed | Each guitar as a notation staff and a tablature staff; what it writes reads back the same; an empty score too. | `read_write::writes_what_it_reads`, `read_write::writes_an_empty_score_and_reads_it_back`, `mxl::tests::an_archive_round_trips` |
| Names what it can't write | Each kind of loss once, counted, with its first place, before anything is written. | `read_write::names_what_it_cant_write`, `log::tests::one_entry_per_kind_counted` |
| Damaged files | Errors, never panics. | `read_write::damaged_files_are_errors_never_panics` |
| Your MusicXML files | Every MusicXML file of the corpus reads, writes (plain and compressed) and reads back the same, or stable from the second trip when losses were named. | `corpus::every_corpus_musicxml_file_round_trips` |
| Through the newest OxiGuitar | Every corpus file OxiGuitar opens (MusicXML by this plugin, Guitar Pro by the released Guitar Pro plugins) is written as `.musicxml` and `.mxl` and read back by this plugin, a second round trip changing nothing. | `stage:corpus through the core` |
| A working plugin file | The `.oxp` file keeps the plugin contract, with this repository's SDK version and with the newest release of OxiGuitar. | `contract::the_plugin_file_keeps_the_contract`, `oxiguitar_plugin_checks::the_manifest_matches_the_plugin`, `stage:contract (latest core)` |
