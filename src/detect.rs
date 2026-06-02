use std::io::Read;
use std::path::Path;
use anyhow::Result;
use crate::format::Format;

/// detect format from extension, falling back to magic numbers
pub fn detect(path: &Path) -> Result<Option<Format>> {
    if let Some(fmt) = Format::from_extension(path) {
        return Ok(Some(fmt));
    }
    by_magic(path)
}

/// detect format purely from magic numbers (no extension considered)
pub fn by_magic(path: &Path) -> Result<Option<Format>> {
    let mut file = std::fs::File::open(path)?;
    let mut buf = [0u8; 512];
    let n = file.read(&mut buf)?;
    Ok(magic_from_bytes(&buf[..n]))
}

pub fn magic_from_bytes(buf: &[u8]) -> Option<Format> {
    if buf.len() >= 4
        && (buf.starts_with(b"PK\x03\x04")
            || buf.starts_with(b"PK\x05\x06")
            || buf.starts_with(b"PK\x07\x08"))
    {
        return Some(Format::Zip);
    }
    if buf.len() >= 2 && buf[0] == 0x1f && buf[1] == 0x8b {
        return Some(Format::Gz);
    }
    if buf.len() >= 3 && buf.starts_with(b"BZh") {
        return Some(Format::Bz2);
    }
    if buf.len() >= 6 && buf.starts_with(b"\xfd7zXZ\x00") {
        return Some(Format::Xz);
    }
    if buf.len() >= 4 && buf.starts_with(b"\x28\xb5\x2f\xfd") {
        return Some(Format::Zst);
    }
    if buf.len() >= 6 && buf.starts_with(b"7z\xbc\xaf\x27\x1c") {
        return Some(Format::SevenZip);
    }
    // rar4
    if buf.len() >= 7 && buf.starts_with(b"Rar!\x1a\x07\x00") {
        return Some(Format::Rar);
    }
    // rar5
    if buf.len() >= 8 && buf.starts_with(b"Rar!\x1a\x07\x01\x00") {
        return Some(Format::Rar);
    }
    // tar: "ustar" at offset 257
    if buf.len() >= 262 && &buf[257..262] == b"ustar" {
        return Some(Format::Tar);
    }
    None
}

/// check if raw (already decompressed) bytes look like a tar stream
pub fn is_tar_bytes(buf: &[u8]) -> bool {
    buf.len() >= 262 && &buf[257..262] == b"ustar"
}
