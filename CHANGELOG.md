# Changelog

All notable changes to this project are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the project
follows [Semantic Versioning](https://semver.org/). A release's number is
the plugin's own; the plugin interface it uses is in `manifest.json` and in
the release notes.

## [Unreleased]

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
