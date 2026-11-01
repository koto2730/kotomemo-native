use crate::entity::{Charset, LineEnding};
use std::fs;
use std::io;
use std::path::Path;

const BOM_UTF8: [u8; 3] = [0xEF, 0xBB, 0xBF];
const BOM_UTF16BE: [u8; 2] = [0xFE, 0xFF];
const BOM_UTF16LE: [u8; 2] = [0xFF, 0xFE];

pub fn read_bytes(path: &Path) -> io::Result<Vec<u8>> {
    fs::read(path)
}

pub fn write_bytes(path: &Path, bytes: &[u8]) -> io::Result<()> {
    fs::write(path, bytes)
}

pub fn detect_encoding(bytes: &[u8]) -> (Charset, bool) {
    if bytes.starts_with(&BOM_UTF8) {
        (Charset::Utf8, true)
    } else if bytes.starts_with(&BOM_UTF16BE) {
        (Charset::Utf16Be, true)
    } else if bytes.starts_with(&BOM_UTF16LE) {
        (Charset::Utf16Le, true)
    } else {
        (Charset::Utf8, false)
    }
}

fn bom_for(charset: Charset) -> &'static [u8] {
    match charset {
        Charset::Utf8 => &BOM_UTF8,
        Charset::Utf16Be => &BOM_UTF16BE,
        Charset::Utf16Le => &BOM_UTF16LE,
    }
}

pub fn decode(bytes: &[u8], charset: Charset, has_bom: bool) -> String {
    let skip = if has_bom { bom_for(charset).len() } else { 0 };
    let body = bytes.get(skip..).unwrap_or(&[]);
    match charset {
        Charset::Utf8 => String::from_utf8_lossy(body).into_owned(),
        Charset::Utf16Be => decode_utf16(body, true),
        Charset::Utf16Le => decode_utf16(body, false),
    }
}

fn decode_utf16(bytes: &[u8], big_endian: bool) -> String {
    let units: Vec<u16> = bytes
        .chunks_exact(2)
        .map(|c| {
            if big_endian {
                u16::from_be_bytes([c[0], c[1]])
            } else {
                u16::from_le_bytes([c[0], c[1]])
            }
        })
        .collect();
    String::from_utf16_lossy(&units)
}

pub fn encode(text: &str, charset: Charset, line_ending: LineEnding, with_bom: bool) -> Vec<u8> {
    // Buffer is always logical LF in memory; CRLF is applied only here, at
    // encode time, regardless of what stray \r sequences the incoming text
    // might already contain.
    let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
    let converted = normalized.replace('\n', line_ending.sequence());
    let mut body = match charset {
        Charset::Utf8 => converted.into_bytes(),
        Charset::Utf16Be => encode_utf16(&converted, true),
        Charset::Utf16Le => encode_utf16(&converted, false),
    };
    if with_bom {
        let mut out = bom_for(charset).to_vec();
        out.append(&mut body);
        out
    } else {
        body
    }
}

fn encode_utf16(text: &str, big_endian: bool) -> Vec<u8> {
    let mut out = Vec::with_capacity(text.len() * 2);
    for unit in text.encode_utf16() {
        let bytes = if big_endian { unit.to_be_bytes() } else { unit.to_le_bytes() };
        out.extend_from_slice(&bytes);
    }
    out
}

/// Loads a file into a normalized in-memory form: LF-only text plus the
/// (charset, hasBom, lineEnding) metadata needed to write it back
/// byte-for-byte equivalent to how it was found. Mirrors
/// `FileContentsRepository.load` + `DefaultFileManager` combined.
pub struct LoadedFile {
    pub text: String,
    pub charset: Charset,
    pub has_bom: bool,
    pub line_ending: LineEnding,
}

pub fn load(path: &Path) -> io::Result<LoadedFile> {
    let raw = read_bytes(path)?;
    let (charset, has_bom) = detect_encoding(&raw);
    let raw_text = decode(&raw, charset, has_bom);
    let line_ending = LineEnding::detect(&raw_text);
    let normalized = raw_text.replace("\r\n", "\n").replace('\r', "\n");
    Ok(LoadedFile {
        text: normalized,
        charset,
        has_bom,
        line_ending,
    })
}

pub fn save(
    path: &Path,
    text: &str,
    charset: Charset,
    line_ending: LineEnding,
    with_bom: bool,
) -> io::Result<()> {
    let bytes = encode(text, charset, line_ending, with_bom);
    write_bytes(path, &bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;
    use std::fs;

    fn temp_path(name: &str) -> std::path::PathBuf {
        let mut p = env::temp_dir();
        p.push(format!("kotomemo-test-{}-{name}", std::process::id()));
        p
    }

    #[test]
    fn save_then_open_round_trips_text_with_lf() {
        let tmp = temp_path("roundtrip-lf.txt");
        save(&tmp, "hello\nworld", Charset::Utf8, LineEnding::Lf, false).unwrap();
        let loaded = load(&tmp).unwrap();
        assert_eq!(loaded.text, "hello\nworld");
        assert_eq!(loaded.line_ending, LineEnding::Lf);
        fs::remove_file(&tmp).ok();
    }

    #[test]
    fn save_with_crlf_writes_crlf_on_disk() {
        let tmp = temp_path("crlf.txt");
        save(&tmp, "a\nb\nc", Charset::Utf8, LineEnding::Crlf, false).unwrap();
        let raw = fs::read_to_string(&tmp).unwrap();
        assert!(raw.contains("\r\n"), "expected CRLF in {raw:?}");
        fs::remove_file(&tmp).ok();
    }

    #[test]
    fn bom_round_trips_when_enabled() {
        let tmp = temp_path("bom.txt");
        save(&tmp, "xyz", Charset::Utf8, LineEnding::Lf, true).unwrap();
        let bytes = fs::read(&tmp).unwrap();
        assert_eq!(bytes[0], 0xEF);
        assert_eq!(bytes[1], 0xBB);
        assert_eq!(bytes[2], 0xBF);

        let loaded = load(&tmp).unwrap();
        assert!(loaded.has_bom);
        assert_eq!(loaded.text, "xyz");
        fs::remove_file(&tmp).ok();
    }

    #[test]
    fn open_crlf_file_normalises_buffer_to_lf_but_keeps_crlf_metadata() {
        let tmp = temp_path("open-crlf.txt");
        fs::write(&tmp, "a\r\nb\r\nc").unwrap();
        let loaded = load(&tmp).unwrap();
        assert_eq!(loaded.text, "a\nb\nc");
        assert_eq!(loaded.line_ending, LineEnding::Crlf);
        fs::remove_file(&tmp).ok();
    }

    #[test]
    fn open_file_with_stray_cr_is_normalised_to_lf() {
        let tmp = temp_path("stray-cr.txt");
        fs::write(&tmp, "a\rb\rc").unwrap();
        let loaded = load(&tmp).unwrap();
        assert_eq!(loaded.text, "a\nb\nc");
        // Bare CR (legacy Mac Classic) is detected as LF since detect() gates
        // on the presence of CRLF specifically.
        assert_eq!(loaded.line_ending, LineEnding::Lf);
        fs::remove_file(&tmp).ok();
    }
}
