// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Mateus Rocha de Paula and the OxiGuitar contributors

//! The contract suite: builds this crate as an `.oxp` file and checks it
//! through OxiGuitar's own plugin host, as the app would load it. `cargo
//! test` runs it with the rest.

use std::path::Path;

#[test]
fn the_plugin_file_keeps_the_contract() {
    let crate_folder = Path::new(env!("CARGO_MANIFEST_DIR"));
    let oxp = oxiguitar_plugin_test::build(crate_folder, env!("CARGO_PKG_NAME"), "musicxml", false)
        .expect("the .oxp file builds");
    let report = oxiguitar_plugin_test::check(&oxp);
    for passed in &report.passed {
        println!("ok    {passed}");
    }
    assert!(report.ok(), "failed: {:#?}", report.failed);
}

#[test]
fn its_signatures_recognise_musicxml_only() {
    use oxiguitar_plugin::api::manifest::Manifest;
    use oxiguitar_plugin::api::signature::Sniff;
    let manifest: Manifest =
        serde_json::from_str(include_str!("../manifest.json")).expect("the manifest reads");
    let plain = std::fs::read(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/oxiguitar-test.musicxml"),
    )
    .unwrap();
    let score = oxiguitar_musicxml_plugin::read(&plain).unwrap().score;
    let mxl = oxiguitar_musicxml_plugin::write(&score, true).unwrap().data;
    let recognises = |format: usize, data: &[u8]| {
        let sniff = Sniff::new(data);
        manifest.formats[format]
            .signatures
            .iter()
            .any(|s| s.matches(&sniff))
    };
    assert!(recognises(0, &plain) && !recognises(1, &plain));
    assert!(recognises(1, &mxl) && !recognises(0, &mxl));
    let other = b"<?xml version=\"1.0\"?><museScore version=\"4.60\"/>";
    assert!(!recognises(0, other) && !recognises(1, other));
}
