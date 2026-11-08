// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Mateus Rocha de Paula and the OxiGuitar contributors

//! The MusicXML format plugin (`musicxml.oxp`, id `oxiguitar.musicxml`):
//! reads and writes MusicXML (`.musicxml`, `.xml`) and compressed MusicXML
//! (`.mxl`), the format most notation programs exchange scores in (#223).
//!
//! MusicXML itself is read and written with the musicxml crate (Mateus,
//! 8 Nov 2026); [`read()`] turns it into OxiGuitar's score and [`write()`] back.
//! The host recognises the files from the signatures in `manifest.json`
//! (the first element, `score-partwise` or `score-timewise`, in the file or
//! in a compressed file's root file: plugin interface 1.7), without loading
//! the plugin. Design and decisions: `docs/design.md`.

mod instruments;
mod log;
mod mxl;
mod read;
mod text;
mod time;
mod write;

use oxiguitar_model::wire;
use oxiguitar_plugin::{
    Error, ErrorKind, FormatReader2, FormatWriter2, Host, Plugin, Read2, Written, score2,
};

/// The id of uncompressed MusicXML in `manifest.json`.
pub const MUSICXML: &str = "musicxml";

/// The id of compressed MusicXML in `manifest.json`.
pub const MXL: &str = "mxl";

/// Reads a MusicXML or compressed MusicXML file (whichever `data` is) into
/// OxiGuitar's score and its import log.
pub fn read(data: &[u8]) -> Result<Read2, Error> {
    let xml = if mxl::is_archive(data) {
        mxl::score_xml(data).map_err(invalid)?
    } else {
        data.to_vec()
    };
    let imported = read::read(xml).map_err(invalid)?;
    Ok(Read2 {
        score: wire::to_wire2(&imported.score),
        log: imported.log.into_entries(),
    })
}

/// Writes `score` as MusicXML 4.0, compressed (`.mxl`) when `compressed`,
/// with what the file can't hold.
pub fn write(score: &score2::Score2, compressed: bool) -> Result<Written, Error> {
    let score = wire::from_wire2(score.clone()).map_err(invalid)?;
    let (xml, losses) = write::write(&score).map_err(invalid)?;
    let data = if compressed {
        mxl::archive(&xml).map_err(invalid)?
    } else {
        xml
    };
    Ok(Written {
        data,
        losses: losses.into_entries(),
    })
}

/// A file or score that can't be read or written, as the interface carries
/// it.
fn invalid(message: String) -> Error {
    Error {
        kind: ErrorKind::Invalid,
        message,
        offset: None,
        context: String::new(),
    }
}

/// The plugin.
#[derive(Debug)]
pub struct MusicXml;

impl Plugin for MusicXml {
    fn start(_host: Host) -> Result<MusicXml, Error> {
        Ok(MusicXml)
    }

    // The score version 2 (interface 1.2 and later), which holds the
    // palette's marks (1.6).
    fn reader2(&self, format: &str) -> Option<&dyn FormatReader2> {
        matches!(format, MUSICXML | MXL).then_some(self as &dyn FormatReader2)
    }

    fn writer2(&self, format: &str) -> Option<&dyn FormatWriter2> {
        match format {
            MUSICXML => Some(&Plain),
            MXL => Some(&Compressed),
            _ => None,
        }
    }
}

impl FormatReader2 for MusicXml {
    fn read(&self, data: &[u8]) -> Result<Read2, Error> {
        read(data)
    }
}

/// The writer of uncompressed MusicXML.
struct Plain;

/// The writer of compressed MusicXML.
struct Compressed;

impl FormatWriter2 for Plain {
    fn write(&self, score: &score2::Score2) -> Result<Written, Error> {
        write(score, false)
    }
}

impl FormatWriter2 for Compressed {
    fn write(&self, score: &score2::Score2) -> Result<Written, Error> {
        write(score, true)
    }
}

oxiguitar_plugin::export_plugin!(MusicXml);
