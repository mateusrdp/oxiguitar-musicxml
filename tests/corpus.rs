// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Mateus Rocha de Paula and the OxiGuitar contributors

//! The test corpus (`tests/support/corpus.rs`: `OXIGUITAR_CORPUS`, or
//! `corpus/files/`; never committed): every MusicXML file in it, plain or
//! compressed, recognised by its contents as the manifest's signatures do,
//! reads, writes and reads back the same. Any other file is left alone.
//! `cargo test --test corpus -- --nocapture`; with no corpus it checks
//! nothing (unless `OXIGUITAR_CORPUS_REQUIRED` is set).

#[path = "support/corpus.rs"]
mod corpus;

use std::io::Read;
use std::path::Path;

use corpus::Outcome;
use oxiguitar_model::wire;

/// The XML's root is a MusicXML score.
fn musicxml_root(text: &[u8]) -> bool {
    let head = &text[..text.len().min(64 * 1024)];
    let head = head.strip_prefix(b"\xef\xbb\xbf").unwrap_or(head);
    let head = String::from_utf8_lossy(head);
    head.trim_start().starts_with('<')
        && (head.contains("<score-partwise") || head.contains("<score-timewise"))
}

/// A compressed MusicXML file: a ZIP whose `META-INF/container.xml` names
/// a root file that is a MusicXML score (a MuseScore file is a ZIP with a
/// container too, but its root file isn't one).
fn compressed_musicxml(data: &[u8]) -> bool {
    let Ok(mut zip) = zip::ZipArchive::new(std::io::Cursor::new(data)) else {
        return false;
    };
    let mut container = String::new();
    let Ok(mut entry) = zip.by_name("META-INF/container.xml") else {
        return false;
    };
    if entry.read_to_string(&mut container).is_err() {
        return false;
    }
    drop(entry);
    let Some(root) = container
        .split("full-path=\"")
        .nth(1)
        .and_then(|rest| rest.split('"').next())
    else {
        return false;
    };
    let Ok(member) = zip.by_name(root) else {
        return false;
    };
    let mut head = Vec::new();
    member.take(64 * 1024).read_to_end(&mut head).is_ok() && musicxml_root(&head)
}

fn is_musicxml(data: &[u8]) -> bool {
    if data.starts_with(b"PK") {
        compressed_musicxml(data)
    } else {
        musicxml_root(data)
    }
}

/// One file: exact when the writer named no losses; otherwise what it
/// wrote writes and reads back the same from then on. Plain and compressed.
fn round_trip(data: &[u8]) -> Result<Outcome, String> {
    if !is_musicxml(data) {
        return Ok(Outcome::NotTaken);
    }
    let read = oxiguitar_musicxml_plugin::read(data).map_err(|e| format!("doesn't read: {e}"))?;
    for compressed in [false, true] {
        let written = oxiguitar_musicxml_plugin::write(&read.score, compressed)
            .map_err(|e| format!("doesn't write (compressed: {compressed}): {e}"))?;
        let again = oxiguitar_musicxml_plugin::read(&written.data)
            .map_err(|e| format!("what it wrote doesn't read (compressed: {compressed}): {e}"))?;
        let (expected, got) = if written.losses.is_empty() {
            (read.score.clone(), again.score)
        } else {
            let third = oxiguitar_musicxml_plugin::write(&again.score, compressed)
                .map_err(|e| format!("doesn't write a second time: {e}"))?;
            let third = oxiguitar_musicxml_plugin::read(&third.data)
                .map_err(|e| format!("the second writing doesn't read: {e}"))?;
            (again.score, third.score)
        };
        let expected = wire::from_wire2(expected).map_err(|e| e.to_string())?;
        let got = wire::from_wire2(got).map_err(|e| e.to_string())?;
        if expected != got {
            return Err(format!(
                "doesn't round-trip (compressed: {compressed}; {} losses named)",
                written.losses.len()
            ));
        }
    }
    Ok(Outcome::Passed)
}

#[test]
fn every_corpus_musicxml_file_round_trips() {
    let fallback = Path::new(env!("CARGO_MANIFEST_DIR")).join("corpus/files");
    corpus::check("musicxml round trip", &fallback, |_, data| round_trip(data));
}

/// The picking itself, on files made here: what the signatures take and
/// what they leave.
#[test]
fn musicxml_is_recognised_by_its_contents() {
    assert!(is_musicxml(
        b"\xef\xbb\xbf<?xml version=\"1.0\"?>\n<!DOCTYPE score-partwise>\n<score-partwise/>"
    ));
    assert!(is_musicxml(b"<score-timewise version=\"4.0\"/>"));
    assert!(!is_musicxml(b"<museScore version=\"4.70\"/>"));
    assert!(!is_musicxml(b"MThd\0\0\0\x06"));
    let compressed = oxiguitar_musicxml_plugin::write(&Default::default(), true).unwrap();
    assert!(is_musicxml(&compressed.data));
    assert!(!is_musicxml(b"PK\x03\x04 not a zip"));
}
