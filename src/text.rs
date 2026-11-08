// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Mateus Rocha de Paula and the OxiGuitar contributors

//! Text into and out of the musicxml crate, which reads and writes XML
//! characters as they are: it doesn't decode `&amp;` and the other
//! entities when it reads, doesn't escape `&`, `<`, `>` or `"` when it
//! writes, and expects its input as UTF-8 or UTF-16 (indexing two bytes of
//! anything else, which can panic on a one-byte file). So the file's bytes
//! become UTF-8 here first ([`decode`]), every text taken from the crate is
//! unescaped ([`unescape`]) and every text handed to it escaped
//! ([`escape`]), as the library rule asks: what it lacks, added on top.

/// The file's text as UTF-8, from UTF-8 (with or without its byte order
/// mark), UTF-16 with a byte order mark, or the single-byte encodings
/// older programs declare (ISO-8859-1, Windows-1252, read as Latin-1).
pub(crate) fn decode(data: &[u8]) -> Result<String, String> {
    match data {
        [0xef, 0xbb, 0xbf, rest @ ..] => return utf8(rest),
        [0xff, 0xfe, rest @ ..] | [0xfe, 0xff, rest @ ..] => {
            let be = data.first() == Some(&0xfe);
            let units = rest.as_chunks::<2>().0.iter().map(|&pair| {
                if be {
                    u16::from_be_bytes(pair)
                } else {
                    u16::from_le_bytes(pair)
                }
            });
            return char::decode_utf16(units)
                .collect::<Result<String, _>>()
                .map_err(|_| "damaged UTF-16 text".to_owned());
        }
        _ => {}
    }
    // The XML declaration names any other encoding.
    let head = String::from_utf8_lossy(data.get(..200).unwrap_or(data)).to_ascii_lowercase();
    let declared = head
        .split_once("encoding=")
        .and_then(|(_, rest)| {
            let q = rest.chars().next().filter(|c| matches!(c, '"' | '\''))?;
            let rest = rest.get(1..)?;
            Some(rest.get(..rest.find(q)?)?.to_owned())
        })
        .unwrap_or_default();
    if matches!(
        declared.as_str(),
        "iso-8859-1" | "iso8859-1" | "latin1" | "latin-1" | "windows-1252" | "cp1252"
    ) {
        return Ok(data.iter().map(|&b| char::from(b)).collect());
    }
    utf8(data)
}

fn utf8(data: &[u8]) -> Result<String, String> {
    String::from_utf8(data.to_vec())
        .map_err(|e| format!("not UTF-8 text (byte {})", e.utf8_error().valid_up_to()))
}

/// XML text with its entities decoded: the five predefined ones and
/// character references. Anything else stays as written.
pub(crate) fn unescape(s: &str) -> String {
    if !s.contains('&') {
        return s.to_owned();
    }
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(at) = rest.find('&') {
        out.push_str(&rest[..at]);
        rest = &rest[at..];
        let decoded = rest.find(';').filter(|&end| end <= 12).and_then(|end| {
            let name = &rest[1..end];
            let c = match name {
                "amp" => '&',
                "lt" => '<',
                "gt" => '>',
                "quot" => '"',
                "apos" => '\'',
                _ => {
                    let code = if let Some(hex) =
                        name.strip_prefix("#x").or_else(|| name.strip_prefix("#X"))
                    {
                        u32::from_str_radix(hex, 16).ok()?
                    } else {
                        name.strip_prefix('#')?.parse().ok()?
                    };
                    char::from_u32(code)?
                }
            };
            Some((c, end + 1))
        });
        match decoded {
            Some((c, len)) => {
                out.push(c);
                rest = &rest[len..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// Text made safe for XML, as element text or an attribute's value.
pub(crate) fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            // XML 1.0 has no other control characters.
            c if c.is_control() && !matches!(c, '\n' | '\t') => {}
            c => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_what_files_come_in() {
        assert_eq!(decode(b"\xef\xbb\xbf<a/>").unwrap(), "<a/>");
        assert_eq!(decode(&[0xff, 0xfe, b'<', 0, b'a', 0]).unwrap(), "<a");
        assert_eq!(decode(&[0xfe, 0xff, 0, b'<', 0, b'a']).unwrap(), "<a");
        let latin = b"<?xml version=\"1.0\" encoding=\"ISO-8859-1\"?><a>\xc1gua</a>";
        assert!(decode(latin).unwrap().ends_with("<a>\u{c1}gua</a>"));
        assert!(decode(b"\xff").is_err());
        assert!(decode(b"").unwrap().is_empty());
    }

    #[test]
    fn entities_both_ways() {
        let s = "Tom & Jerry <tom@example.org> \"live\"";
        assert_eq!(unescape(&escape(s)), s);
        assert_eq!(
            unescape("&#233;&#xE9;&amp;&bogus;&"),
            "\u{e9}\u{e9}&&bogus;&"
        );
    }
}
