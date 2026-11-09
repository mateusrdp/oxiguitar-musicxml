// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Mateus Rocha de Paula and the OxiGuitar contributors

//! The private test corpus, when it is here (never committed): every
//! MusicXML file in `corpus/files/` reads, writes and reads back the same.
//! `cargo test --test corpus -- --nocapture` locally; with no corpus it
//! checks nothing.

use oxiguitar_model::wire;

#[test]
fn every_corpus_file_round_trips() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("corpus/files");
    let Ok(entries) = std::fs::read_dir(&dir) else {
        println!("no corpus in {}", dir.display());
        return;
    };
    let mut checked = 0;
    for e in entries.flatten() {
        let path = e.path();
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_lowercase();
        if !matches!(ext.as_str(), "musicxml" | "xml" | "mxl") {
            continue;
        }
        let data = std::fs::read(&path).unwrap();
        let read = oxiguitar_musicxml_plugin::read(&data)
            .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        println!("{}: {} log entries", path.display(), read.log.len());
        // Exact when the writer named no losses; otherwise what it wrote
        // writes and reads back the same from then on.
        let written = oxiguitar_musicxml_plugin::write(&read.score, false).unwrap();
        let again = oxiguitar_musicxml_plugin::read(&written.data).unwrap();
        let (expected, got) = if written.losses.is_empty() {
            (read.score, again.score)
        } else {
            let third = oxiguitar_musicxml_plugin::write(&again.score, false).unwrap();
            let third = oxiguitar_musicxml_plugin::read(&third.data).unwrap();
            (again.score, third.score)
        };
        assert!(
            wire::from_wire2(got).unwrap() == wire::from_wire2(expected).unwrap(),
            "{} doesn't round-trip",
            path.display()
        );
        checked += 1;
    }
    println!("{checked} files");
}
