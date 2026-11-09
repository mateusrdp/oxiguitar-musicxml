// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Mateus Rocha de Paula and the OxiGuitar contributors

//! Whatever a file reads into is written back, both ways, and what was
//! written reads again.

#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Ok(read) = oxiguitar_musicxml_plugin::read(data) else {
        return;
    };
    for compressed in [false, true] {
        let written = oxiguitar_musicxml_plugin::write(&read.score, compressed)
            .expect("a score that was read can be written");
        oxiguitar_musicxml_plugin::read(&written.data).expect("what was written reads");
    }
});
