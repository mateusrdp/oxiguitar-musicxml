# Changelog

All notable changes to this project are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the project
follows [Semantic Versioning](https://semver.org/). A release's number is
the plugin's own; the plugin interface it uses is in `manifest.json` and in
the release notes.

## [Unreleased]

### Added

- **Every test with one command** (mateusrdp/oxiguitar#273): `scripts/test-all.py` builds and runs all of this repository's tests on your own corpus (`--corpus DIR` or `OXIGUITAR_CORPUS`), and tests the plugin against the newest OxiGuitar release, with a progress bar, a summary to paste into an issue and a log to diagnose from. The manual has a Features page naming the tests of every feature (CI checks it) and a Testing page.

## [0.1.1] - 2026-11-16

### Added

- **A Flatpak extension**, `io.github.mateusrdp.OxiGuitar.Plugin.musicxml`, in
  the release and in OxiGuitar's Flatpak repository, for OxiGuitar
  installed as a Flatpak. 0.1.0 was released without it: its release was
  built before the packaging gained Flatpak extensions.

## [0.1.0] - 2026-11-09

### Added

- **The MusicXML plugin** (`musicxml.oxp`, `oxiguitar.musicxml`, #223 in
  OxiGuitar): reads and writes MusicXML 4.0, uncompressed (.musicxml, .xml)
  and compressed (.mxl), partwise and timewise. Guitars and basses open as
  tablature, their strings, frets and tuning taken from the file's
  tablature staff when it has one; other pitched parts get a guitar's
  strings with their tablature hidden; drums open as drums. Saving writes
  each guitar as a notation staff and a tablature staff and names what
  MusicXML can't hold. Built on the musicxml crate, with compressed files,
  text encodings and XML escaping handled around it. Needs OxiGuitar 0.18
  (plugin interface 1.7), which recognises the files by their first
  element. Design: `docs/design.md`.
