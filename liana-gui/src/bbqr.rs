//! BBQr, the moving QR code Talia on the iPhone shows and scans (bbqr.org).
//!
//! Each frame is `B$`, the encoding, the file type, the frame count and the
//! frame's index (two base-36 digits each), then the payload. Talia Desktop
//! writes encoding `2` (base 32, RFC 4648, no padding) exactly as the iPhone
//! app does: the bytes are cut into parts of `FRAME_BYTES`, a multiple of 5,
//! so every frame decodes on its own. It reads all three encodings, `H` (hex),
//! `2` and `Z` (raw deflate, then base 32), as the iPhone app does
//! (decided 2026-11-15).

use std::{collections::BTreeMap, fmt};

/// Raw bytes per frame: 800 base-32 characters, the iPhone app's own size.
pub const FRAME_BYTES: usize = 500;

/// BBQr counts frames in two base-36 digits.
const MAX_FRAMES: usize = 36 * 36 - 1;

/// The most a scanned code may unpack to: a wallet backup is a few kilobytes,
/// and a code that inflates further must not freeze the app.
pub const MAX_PAYLOAD_BYTES: usize = 4 * 1024 * 1024;

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
    /// Not a BBQr frame at all.
    NotBbqr,
    /// A frame of another code than the frames read so far.
    OtherCode,
    /// A frame whose payload cannot be read.
    BadFrame,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Empty => write!(f, "Nothing to show"),
            Error::TooLarge => write!(f, "Too large for a QR code"),
            Error::NotBbqr => write!(f, "Not a BBQr code"),
            Error::OtherCode => write!(f, "A part of another QR code"),
            Error::BadFrame => write!(f, "This QR code cannot be read"),
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

/// Whether a scanned text is a BBQr frame.
pub fn is_frame(text: &str) -> bool {
    let b = text.as_bytes();
    b.len() >= 8
        && b.starts_with(b"B$")
        && matches!(b[2], b'H' | b'2' | b'Z')
        && b[3].is_ascii_uppercase()
        && b[4..8].iter().all(|c| BASE36.contains(c))
}

/// Gathers the frames of one code, in any order and any number of times,
/// as a camera sees them.
#[derive(Debug, Default)]
pub struct Joiner {
    /// Encoding, file type and frame count of the code being read.
    code: Option<(u8, u8, usize)>,
    parts: BTreeMap<usize, Vec<u8>>,
}

impl Joiner {
    /// Takes one frame; the frames held so far and the count.
    pub fn add(&mut self, text: &str) -> Result<(usize, usize), Error> {
        if !is_frame(text) {
            return Err(Error::NotBbqr);
        }
        let b = text.as_bytes();
        let total = base36_value(b[4], b[5]);
        let index = base36_value(b[6], b[7]);
        if total == 0 || index >= total {
            return Err(Error::BadFrame);
        }
        let code = (b[2], b[3], total);
        match self.code {
            None => self.code = Some(code),
            Some(held) if held != code => return Err(Error::OtherCode),
            Some(_) => {}
        }
        let payload = &text[8..];
        let bytes = match b[2] {
            b'H' => from_hex(payload),
            _ => from_base32(payload),
        }
        .ok_or(Error::BadFrame)?;
        if self.parts.values().map(Vec::len).sum::<usize>() + bytes.len() > MAX_PAYLOAD_BYTES {
            return Err(Error::TooLarge);
        }
        self.parts.insert(index, bytes);
        Ok((self.parts.len(), total))
    }

    /// Whether no frame is held.
    pub fn is_empty(&self) -> bool {
        self.code.is_none()
    }

    /// The whole content once every frame is in.
    pub fn result(&self) -> Option<Result<Vec<u8>, Error>> {
        let (encoding, _, total) = self.code?;
        if self.parts.len() != total {
            return None;
        }
        let joined: Vec<u8> = self.parts.values().flatten().copied().collect();
        Some(if encoding == b'Z' {
            inflate(&joined)
        } else {
            Ok(joined)
        })
    }
}

#[cfg(unix)]
fn inflate(data: &[u8]) -> Result<Vec<u8>, Error> {
    use std::io::Read;
    let mut out = Vec::new();
    flate2::read::DeflateDecoder::new(data)
        .take(MAX_PAYLOAD_BYTES as u64 + 1)
        .read_to_end(&mut out)
        .map_err(|_| Error::BadFrame)?;
    if out.len() > MAX_PAYLOAD_BYTES {
        return Err(Error::TooLarge);
    }
    Ok(out)
}

/// Talia Desktop reads codes only on the Mac for now.
#[cfg(not(unix))]
fn inflate(_data: &[u8]) -> Result<Vec<u8>, Error> {
    Err(Error::BadFrame)
}

fn base36(n: usize) -> String {
    [BASE36[n / 36] as char, BASE36[n % 36] as char]
        .iter()
        .collect()
}

fn base36_value(hi: u8, lo: u8) -> usize {
    let digit = |c: u8| BASE36.iter().position(|&d| d == c).unwrap_or(0);
    digit(hi) * 36 + digit(lo)
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

fn from_base32(text: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(text.len() * 5 / 8);
    let mut buffer: u32 = 0;
    let mut bits = 0;
    for c in text.bytes() {
        let value = BASE32.iter().position(|&d| d == c)? as u32;
        buffer = ((buffer << 5) | value) & 0xFFFF;
        bits += 5;
        if bits >= 8 {
            bits -= 8;
            out.push((buffer >> bits) as u8);
        }
    }
    Some(out)
}

fn from_hex(text: &str) -> Option<Vec<u8>> {
    if text.len() % 2 != 0 {
        return None;
    }
    (0..text.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(text.get(i..i + 2)?, 16).ok())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn join(frames: &[String]) -> Vec<u8> {
        let mut joiner = Joiner::default();
        // Any order, and a frame seen twice.
        for frame in frames.iter().rev().chain(frames.first()) {
            joiner.add(frame).unwrap();
        }
        joiner.result().unwrap().unwrap()
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
            assert_eq!(from_base32(coded).unwrap(), raw.as_bytes());
        }
    }

    #[test]
    fn frames_carry_their_count_and_place() {
        let one = encode(br#"{"a":1}"#, FileType::Json).unwrap();
        assert_eq!(one, vec!["B$2J0100PMRGCIR2GF6Q".to_string()]);
        assert_eq!(join(&one), br#"{"a":1}"#);

        let data: Vec<u8> = (0..1234u32).map(|i| (i * 7 % 251) as u8).collect();
        let frames = encode(&data, FileType::Json).unwrap();
        assert_eq!(frames.len(), 3);
        assert!(frames[0].starts_with("B$2J0300"));
        assert!(frames[2].starts_with("B$2J0302"));
        // A full part is 800 characters after the header.
        assert_eq!(frames[0].len(), 8 + 800);
        assert_eq!(join(&frames), data);
        // Every frame is QR alphanumeric, the densest QR mode.
        assert!(frames.iter().all(|f| f
            .bytes()
            .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == b'$')));
    }

    #[test]
    fn the_iphone_apps_frames_and_other_encodings_are_read() {
        // Frames as the iPhone app's encodeBbqr writes them (base 32).
        let mut joiner = Joiner::default();
        assert_eq!(joiner.add("B$2U0200MFRGG"), Ok((1, 2)));
        assert!(joiner.result().is_none());
        assert_eq!(joiner.add("B$2U0201MRSQ"), Ok((2, 2)));
        assert_eq!(joiner.result().unwrap().unwrap(), b"abcde");

        // Hex.
        let mut hex = Joiner::default();
        hex.add("B$HU01006869").unwrap();
        assert_eq!(hex.result().unwrap().unwrap(), b"hi");

        // Raw deflate, then base 32: "hello hello hello" deflated.
        let mut z = Joiner::default();
        z.add("B$ZU0100ZNEM3SOJK7EEBEAA").unwrap();
        assert_eq!(z.result().unwrap().unwrap(), b"hello hello hello");
    }

    #[test]
    fn foreign_and_broken_frames_are_refused() {
        let mut joiner = Joiner::default();
        assert_eq!(joiner.add("wsh(pk(xpub))"), Err(Error::NotBbqr));
        joiner.add("B$2U0200MFRGG").unwrap();
        // A frame of another code: three frames, not two.
        assert_eq!(joiner.add("B$2U0300MFRGG"), Err(Error::OtherCode));
        assert_eq!(joiner.add("B$2U0201mrsq"), Err(Error::BadFrame));
        assert_eq!(Joiner::default().add("B$2U0202MFRGG"), Err(Error::BadFrame));
    }

    #[test]
    fn nothing_or_too_much_is_refused() {
        assert_eq!(encode(b"", FileType::Text), Err(Error::Empty));
        let huge = vec![0u8; FRAME_BYTES * MAX_FRAMES + 1];
        assert_eq!(encode(&huge, FileType::Text), Err(Error::TooLarge));
    }
}
