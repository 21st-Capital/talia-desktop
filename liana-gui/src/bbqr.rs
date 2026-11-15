//! BBQr, the moving QR code Talia on the iPhone shows and scans (bbqr.org).
//!
//! Each frame is `B$`, the encoding, the file type, the frame count and the
//! frame's index (two base-36 digits each), then the payload. Talia Desktop
//! writes encoding `2` (base 32, RFC 4648, no padding) exactly as the iPhone
//! app does: the bytes are cut into parts of `FRAME_BYTES`, a multiple of 5,
//! so every frame decodes on its own (decided 2026-11-15).

use std::fmt;

/// Raw bytes per frame: 800 base-32 characters, the iPhone app's own size.
pub const FRAME_BYTES: usize = 500;

/// BBQr counts frames in two base-36 digits.
const MAX_FRAMES: usize = 36 * 36 - 1;

const BASE32: &[u8; 32] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";
const BASE36: &[u8; 36] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ";

/// What a frame holds, as BBQr names it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileType {
    /// JSON, such as a wallet backup.
    Json,
    /// Plain text, such as a descriptor.
    Text,
}

impl FileType {
    fn code(self) -> char {
        match self {
            FileType::Json => 'J',
            FileType::Text => 'U',
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    Empty,
    TooLarge,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Empty => write!(f, "Nothing to show"),
            Error::TooLarge => write!(f, "Too large for a QR code"),
        }
    }
}

/// The frames of `data`, in order.
pub fn encode(data: &[u8], file_type: FileType) -> Result<Vec<String>, Error> {
    if data.is_empty() {
        return Err(Error::Empty);
    }
    let total = data.len().div_ceil(FRAME_BYTES);
    if total > MAX_FRAMES {
        return Err(Error::TooLarge);
    }
    Ok(data
        .chunks(FRAME_BYTES)
        .enumerate()
        .map(|(index, part)| {
            format!(
                "B$2{}{}{}{}",
                file_type.code(),
                base36(total),
                base36(index),
                base32(part)
            )
        })
        .collect())
}

fn base36(n: usize) -> String {
    [BASE36[n / 36] as char, BASE36[n % 36] as char]
        .iter()
        .collect()
}

/// RFC 4648 base 32, upper case, no padding.
fn base32(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().div_ceil(5) * 8);
    let mut buffer: u32 = 0;
    let mut bits = 0;
    for &byte in bytes {
        buffer = (buffer << 8) | u32::from(byte);
        bits += 8;
        while bits >= 5 {
            bits -= 5;
            out.push(BASE32[((buffer >> bits) & 31) as usize] as char);
        }
    }
    if bits > 0 {
        out.push(BASE32[((buffer << (5 - bits)) & 31) as usize] as char);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Reads frames back as the iPhone app does: any order, base 32.
    fn decode(frames: &[String]) -> Vec<u8> {
        let mut parts: Vec<(usize, &str)> = frames
            .iter()
            .map(|f| {
                assert!(f.starts_with("B$2"));
                let digit = |c: u8| BASE36.iter().position(|&d| d == c).unwrap();
                let b = f.as_bytes();
                let total = digit(b[4]) * 36 + digit(b[5]);
                assert_eq!(total, frames.len());
                (digit(b[6]) * 36 + digit(b[7]), &f[8..])
            })
            .collect();
        parts.sort();
        let mut out = Vec::new();
        for (_, payload) in parts {
            let mut buffer: u64 = 0;
            let mut bits = 0;
            for c in payload.bytes() {
                let v = BASE32.iter().position(|&d| d == c).unwrap() as u64;
                buffer = (buffer << 5) | v;
                bits += 5;
                if bits >= 8 {
                    bits -= 8;
                    out.push((buffer >> bits) as u8);
                }
            }
        }
        out
    }

    #[test]
    fn base32_follows_rfc_4648() {
        for (raw, coded) in [
            ("", ""),
            ("f", "MY"),
            ("fo", "MZXQ"),
            ("foo", "MZXW6"),
            ("foob", "MZXW6YQ"),
            ("fooba", "MZXW6YTB"),
            ("foobar", "MZXW6YTBOI"),
        ] {
            assert_eq!(base32(raw.as_bytes()), coded);
        }
    }

    #[test]
    fn frames_carry_their_count_and_place() {
        let one = encode(br#"{"a":1}"#, FileType::Json).unwrap();
        assert_eq!(one, vec!["B$2J0100PMRGCIR2GF6Q".to_string()]);

        let data: Vec<u8> = (0..1234u32).map(|i| (i * 7 % 251) as u8).collect();
        let frames = encode(&data, FileType::Json).unwrap();
        assert_eq!(frames.len(), 3);
        assert!(frames[0].starts_with("B$2J0300"));
        assert!(frames[2].starts_with("B$2J0302"));
        // A full part is 800 characters after the header.
        assert_eq!(frames[0].len(), 8 + 800);
        assert_eq!(decode(&frames), data);
        // Every frame is QR alphanumeric, the densest QR mode.
        assert!(frames
            .iter()
            .all(|f| f.bytes().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == b'$')));
    }

    #[test]
    fn nothing_or_too_much_is_refused() {
        assert_eq!(encode(b"", FileType::Text), Err(Error::Empty));
        let huge = vec![0u8; FRAME_BYTES * MAX_FRAMES + 1];
        assert_eq!(encode(&huge, FileType::Text), Err(Error::TooLarge));
    }
}
