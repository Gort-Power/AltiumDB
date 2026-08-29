//! Low-level Altium record stream parsing shared by SchLib and PcbLib.
//!
//! Records in schematic-style streams are length-prefixed: a 4-byte
//! little-endian value whose highest byte is zero for text records and
//! non-zero (mode 0x01) for binary records. Text records are null-terminated
//! pipe-separated `KEY=VALUE` pairs.

pub struct Reader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        Reader { data, pos: 0 }
    }

    pub fn remaining(&self) -> usize {
        self.data.len().saturating_sub(self.pos)
    }

    pub fn pos(&self) -> usize {
        self.pos
    }

    pub fn seek(&mut self, pos: usize) {
        self.pos = pos.min(self.data.len());
    }

    /// Peek at a pending little-endian u32 without consuming it.
    pub fn peek_u32(&self) -> Option<u32> {
        if self.remaining() < 4 {
            return None;
        }
        let b = &self.data[self.pos..self.pos + 4];
        Some(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }

    /// Peek at the pending byte without consuming it.
    pub fn peek_u8(&self) -> Option<u8> {
        self.data.get(self.pos).copied()
    }

    pub fn u8(&mut self) -> Option<u8> {
        let v = *self.data.get(self.pos)?;
        self.pos += 1;
        Some(v)
    }

    pub fn i16(&mut self) -> Option<i16> {
        let b = self.bytes(2)?;
        Some(i16::from_le_bytes([b[0], b[1]]))
    }

    #[allow(dead_code)]
    pub fn u16(&mut self) -> Option<u16> {
        let b = self.bytes(2)?;
        Some(u16::from_le_bytes([b[0], b[1]]))
    }

    pub fn i32(&mut self) -> Option<i32> {
        let b = self.bytes(4)?;
        Some(i32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }

    pub fn u32(&mut self) -> Option<u32> {
        let b = self.bytes(4)?;
        Some(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }

    pub fn f64(&mut self) -> Option<f64> {
        let b = self.bytes(8)?;
        Some(f64::from_le_bytes([b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]]))
    }

    pub fn bytes(&mut self, n: usize) -> Option<&'a [u8]> {
        if self.remaining() < n {
            return None;
        }
        let s = &self.data[self.pos..self.pos + n];
        self.pos += n;
        Some(s)
    }

    /// Pascal string: 1-byte length followed by that many bytes.
    pub fn pascal_string(&mut self) -> Option<String> {
        let len = self.u8()? as usize;
        let bytes = self.bytes(len)?;
        Some(decode_text(bytes))
    }
}

/// Decode an Altium text payload (cp1252 with `%UTF8%` sidecars).
pub fn decode_text(bytes: &[u8]) -> String {
    if let Ok(s) = std::str::from_utf8(bytes) {
        return s.to_string();
    }
    bytes.iter().map(|&b| cp1252_byte(b)).collect()
}

fn cp1252_byte(b: u8) -> char {
    match b {
        0x80 => '\u{20AC}',
        0x82 => '\u{201A}',
        0x83 => '\u{0192}',
        0x84 => '\u{201E}',
        0x85 => '\u{2026}',
        0x86 => '\u{2020}',
        0x87 => '\u{2021}',
        0x88 => '\u{02C6}',
        0x89 => '\u{2030}',
        0x8A => '\u{0160}',
        0x8B => '\u{2039}',
        0x8C => '\u{0152}',
        0x8E => '\u{017D}',
        0x91 => '\u{2018}',
        0x92 => '\u{2019}',
        0x93 => '\u{201C}',
        0x94 => '\u{201D}',
        0x95 => '\u{2022}',
        0x96 => '\u{2013}',
        0x97 => '\u{2014}',
        0x98 => '\u{02DC}',
        0x99 => '\u{2122}',
        0x9A => '\u{0161}',
        0x9B => '\u{203A}',
        0x9C => '\u{0153}',
        0x9E => '\u{017E}',
        0x9F => '\u{0178}',
        _ => b as char,
    }
}

/// One record from a schematic-style stream.
#[derive(Debug)]
pub enum StreamRecord {
    Text(Vec<(String, String)>),
    Binary(Vec<u8>),
}

impl StreamRecord {
    pub fn prop(&self, key: &str) -> Option<&str> {
        match self {
            StreamRecord::Text(pairs) => pairs
                .iter()
                .find(|(k, _)| k.eq_ignore_ascii_case(key))
                .map(|(_, v)| v.as_str()),
            _ => None,
        }
    }

    pub fn prop_i64(&self, key: &str) -> Option<i64> {
        self.prop(key).and_then(|v| parse_int(v))
    }

    pub fn prop_f64(&self, key: &str) -> Option<f64> {
        self.prop(key).and_then(|v| v.trim().parse::<f64>().ok())
    }

    #[allow(dead_code)]
    pub fn binary_data(&self) -> Option<&[u8]> {
        match self {
            StreamRecord::Binary(d) => Some(d),
            _ => None,
        }
    }
}

pub fn parse_int(v: &str) -> Option<i64> {
    let t = v.trim();
    if let Some(hex) = t.strip_prefix('$') {
        return i64::from_str_radix(hex, 16).ok();
    }
    t.parse::<i64>().ok()
}

/// Parse a full schematic-style stream into records.
pub fn parse_stream_records(data: &[u8]) -> Vec<StreamRecord> {
    let mut records = Vec::new();
    let mut r = Reader::new(data);
    while r.remaining() >= 4 {
        let raw_len = r.u32().unwrap();
        let is_binary = (raw_len >> 24) != 0;
        let len = (raw_len & 0x00FF_FFFF) as usize;
        if r.remaining() < len || len == 0 {
            break;
        }
        // Guard against absurd lengths caused by desync.
        if !is_binary && len > r.remaining() {
            break;
        }
        let payload = r.bytes(len).unwrap().to_vec();
        if is_binary {
            records.push(StreamRecord::Binary(payload));
        } else {
            let body = payload.strip_suffix(&[0u8]).unwrap_or(&payload);
            records.push(StreamRecord::Text(parse_pairs(body)));
        }
    }
    records
}

fn parse_pairs(body: &[u8]) -> Vec<(String, String)> {
    let mut pairs = Vec::new();
    for chunk in split_pipes(body) {
        if chunk.is_empty() {
            continue;
        }
        if let Some(eq) = chunk.iter().position(|&b| b == b'=') {
            let key = decode_text(&chunk[..eq]);
            let val = decode_text(&chunk[eq + 1..]);
            pairs.push((key, val));
        }
    }
    pairs
}

fn split_pipes(body: &[u8]) -> Vec<&[u8]> {
    let mut parts = Vec::new();
    let mut start = 0usize;
    for (i, &b) in body.iter().enumerate() {
        if b == b'|' {
            parts.push(&body[start..i]);
            start = i + 1;
        }
    }
    parts.push(&body[start..]);
    if let Some(first) = parts.first() {
        if first.is_empty() {
            parts.remove(0);
        }
    }
    parts
}
