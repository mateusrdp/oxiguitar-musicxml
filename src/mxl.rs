// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Mateus Rocha de Paula and the OxiGuitar contributors

//! Compressed MusicXML (`.mxl`): a ZIP container whose
//! `META-INF/container.xml` names the score as its first root file
//! (MusicXML 4.0, "Compressed MusicXML files").
//!
//! The musicxml crate reads these too, but its own ZIP reader panics on
//! some damaged archives (found by cutting a valid `.mxl` short), and a
//! reader must answer any file with an error, never a panic. So archives
//! are opened here with the `zip` crate, and the crate is handed the score's
//! XML. Writing goes the same way, so one ZIP library does both.

use std::io::{Cursor, Read, Write};

use zip::write::SimpleFileOptions;

/// The largest score accepted inside an archive, uncompressed: far beyond
/// any real score (a 300-bar, 8-track one is a few megabytes of XML), and
/// small enough that a file claiming more can't exhaust memory.
const MAX_SCORE: u64 = 256 * 1024 * 1024;

/// The container file of a compressed MusicXML archive.
const CONTAINER: &str = "META-INF/container.xml";

/// The MIME type a compressed MusicXML archive starts with.
const MIMETYPE: &str = "application/vnd.recordare.musicxml";

/// Whether `data` is a ZIP archive (a compressed MusicXML file, if it is
/// MusicXML at all).
pub(crate) fn is_archive(data: &[u8]) -> bool {
    data.starts_with(b"PK\x03\x04")
}

/// The score's XML inside a compressed MusicXML archive: the first root
/// file its container names, or, without a container, its one `.musicxml`
/// or `.xml` member outside `META-INF`.
pub(crate) fn score_xml(data: &[u8]) -> Result<Vec<u8>, String> {
    let mut archive = zip::ZipArchive::new(Cursor::new(data))
        .map_err(|e| format!("not a readable compressed MusicXML archive: {e}"))?;
    let path = match read_member(&mut archive, CONTAINER) {
        Ok(container) => first_rootfile(&String::from_utf8_lossy(&container))
            .ok_or_else(|| format!("{CONTAINER} names no root file"))?,
        Err(_) => {
            let candidates: Vec<String> = archive
                .file_names()
                .filter_map(Result::ok)
                .map(|n| n.into_owned())
                .filter(|n| !n.starts_with("META-INF/"))
                .filter(|n| {
                    let n = n.to_ascii_lowercase();
                    n.ends_with(".musicxml") || n.ends_with(".xml")
                })
                .collect();
            match candidates.as_slice() {
                [one] => one.clone(),
                [] => return Err("the archive holds no MusicXML file".into()),
                _ => {
                    return Err(format!(
                        "the archive has no {CONTAINER} to say which file is the score"
                    ));
                }
            }
        }
    };
    read_member(&mut archive, &path)
}

/// One member's contents, at most [`MAX_SCORE`] bytes.
fn read_member(
    archive: &mut zip::ZipArchive<Cursor<&[u8]>>,
    name: &str,
) -> Result<Vec<u8>, String> {
    let file = archive
        .by_name(name)
        .map_err(|e| format!("the archive's {name}: {e}"))?;
    let mut out = Vec::new();
    file.take(MAX_SCORE + 1)
        .read_to_end(&mut out)
        .map_err(|e| format!("the archive's {name}: {e}"))?;
    if out.len() as u64 > MAX_SCORE {
        return Err(format!("the archive's {name} is larger than 256 MiB"));
    }
    Ok(out)
}

/// The `full-path` of a container's first `rootfile`, its predefined
/// entities resolved.
fn first_rootfile(container: &str) -> Option<String> {
    let mut rest = container;
    loop {
        let at = rest.find("<rootfile")?;
        rest = rest.get(at + "<rootfile".len()..)?;
        // `<rootfiles>` is the list, not a root file.
        if rest.starts_with(|c: char| c.is_whitespace() || c == '/' || c == '>') {
            break;
        }
    }
    let tag = rest.get(..rest.find('>')?)?;
    let mut attrs = tag;
    while let Some(at) = attrs.find("full-path") {
        let after = attrs.get(at + "full-path".len()..)?;
        let starts_word = at == 0 || attrs.get(..at)?.ends_with(char::is_whitespace);
        let value = after.trim_start().strip_prefix('=').map(str::trim_start);
        if let (true, Some(value)) = (starts_word, value) {
            let quote = value.chars().next().filter(|c| matches!(c, '"' | '\''))?;
            let inner = value.get(1..)?;
            let raw = inner.get(..inner.find(quote)?)?;
            return Some(
                raw.replace("&lt;", "<")
                    .replace("&gt;", ">")
                    .replace("&quot;", "\"")
                    .replace("&apos;", "'")
                    .replace("&amp;", "&"),
            );
        }
        attrs = after;
    }
    None
}

/// A compressed MusicXML archive holding `xml` as `score.musicxml`: the
/// MIME type first and stored, then the container, then the score.
pub(crate) fn archive(xml: &[u8]) -> Result<Vec<u8>, String> {
    let mut out = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let stored = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    let deflated =
        SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    let container = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
        <container>\n  <rootfiles>\n    \
        <rootfile full-path=\"score.musicxml\" media-type=\"application/vnd.recordare.musicxml+xml\"/>\n  \
        </rootfiles>\n</container>\n";
    let mut put = |name: &str, options: SimpleFileOptions, data: &[u8]| -> Result<(), String> {
        out.start_file(name, options).map_err(|e| e.to_string())?;
        out.write_all(data).map_err(|e| e.to_string())
    };
    put("mimetype", stored, MIMETYPE.as_bytes())?;
    put(CONTAINER, deflated, container.as_bytes())?;
    put("score.musicxml", deflated, xml)?;
    let cursor = out.finish().map_err(|e| e.to_string())?;
    Ok(cursor.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_archive_round_trips() {
        let xml = b"<score-partwise version=\"4.0\"><part-list/></score-partwise>";
        let z = archive(xml).unwrap();
        assert!(is_archive(&z));
        assert_eq!(score_xml(&z).unwrap(), xml);
        // Cut anywhere or damaged: an error, never a panic.
        for cut in 0..z.len() {
            let _ = score_xml(&z[..cut]);
        }
        let mut flipped = z.clone();
        for i in (0..flipped.len()).step_by(5) {
            flipped[i] ^= 0xa5;
            let _ = score_xml(&flipped);
        }
    }

    #[test]
    fn the_container_names_the_score() {
        assert_eq!(
            first_rootfile("<rootfiles><rootfile media-type='x' full-path='a &amp; b.xml'/>")
                .as_deref(),
            Some("a & b.xml")
        );
        assert_eq!(first_rootfile("<rootfiles></rootfiles>"), None);
    }
}
