//! Altium .PcbLib parsing (binary PCB records).
//!
//! Stream layout per footprint (`<Name>/Data`):
//! `[u32 pascal_len][u8 name_len][name bytes]` followed by records of the form
//! `[u8 type][u32 len][payload]`.

use super::records::{decode_text, Reader};
use std::io::Read;
use std::path::Path;

const TYPE_ARC: u8 = 1;
const TYPE_PAD: u8 = 2;
const TYPE_VIA: u8 = 3;
const TYPE_TRACK: u8 = 4;
const TYPE_TEXT: u8 = 5;
const TYPE_FILL: u8 = 6;
const TYPE_REGION: u8 = 11;
const TYPE_COMPONENT_BODY: u8 = 12;

/// Internal PCB units per mil.
pub const UNITS_PER_MIL: f64 = 10_000.0;

// Pad shape codes (APAD6).
pub const PAD_SHAPE_CIRCLE: u8 = 1;
#[allow(dead_code)]
pub const PAD_SHAPE_RECTANGLE: u8 = 2;
pub const PAD_SHAPE_OCTAGONAL: u8 = 3;
pub const PAD_SHAPE_ROUNDED_RECTANGLE: u8 = 4;

// PCB layer ids used for rendering.
pub const LAYER_TOP: u8 = 1;
pub const LAYER_BOTTOM: u8 = 32;
pub const LAYER_TOP_OVERLAY: u8 = 33;
pub const LAYER_BOTTOM_OVERLAY: u8 = 34;
pub const LAYER_TOP_COURTYARD: u8 = 55;
pub const LAYER_BOTTOM_COURTYARD: u8 = 56;
pub const LAYER_MULTI: u8 = 74;

#[derive(Debug, Default, Clone)]
pub struct Footprint {
    pub name: String,
    pub prims: Vec<Prim>,
}

#[derive(Debug, Clone)]
pub enum Prim {
    Track(Track),
    Arc(Arc),
    Pad(Pad),
    Via(Via),
    Text(Text),
    Fill(Fill),
    Region(Region),
}

impl Prim {
    #[allow(dead_code)]
    pub fn layer(&self) -> u8 {
        match self {
            Prim::Track(p) => p.layer,
            Prim::Arc(p) => p.layer,
            Prim::Pad(p) => p.layer,
            Prim::Via(_) => LAYER_MULTI,
            Prim::Text(p) => p.layer,
            Prim::Fill(p) => p.layer,
            Prim::Region(p) => p.layer,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Track {
    pub layer: u8,
    pub x1: i32,
    pub y1: i32,
    pub x2: i32,
    pub y2: i32,
    pub width: i32,
}

#[derive(Debug, Clone)]
pub struct Arc {
    pub layer: u8,
    pub cx: i32,
    pub cy: i32,
    pub radius: i32,
    pub start_angle: f64,
    pub end_angle: f64,
    pub width: i32,
}

impl Arc {
    /// True for a full circle (start == end).
    pub fn is_full_circle(&self) -> bool {
        (self.end_angle - self.start_angle).abs() >= 360.0 - 1e-6
            || (self.start_angle % 360.0) == (self.end_angle % 360.0)
    }
}

#[derive(Debug, Clone)]
pub struct Pad {
    pub designator: String,
    pub layer: u8,
    pub x: i32,
    pub y: i32,
    pub top_width: i32,
    pub top_height: i32,
    pub bot_width: i32,
    pub bot_height: i32,
    pub hole_size: u32,
    pub top_shape: u8,
    pub bot_shape: u8,
    /// Rotation in degrees.
    pub rotation: f64,
    pub hole_shape: u8,
    pub slot_size: i32,
    pub slot_rotation: f64,
}

impl Pad {
    pub fn is_through_hole(&self) -> bool {
        self.hole_size > 0
    }

    /// Shape effective on the given copper side.
    pub fn shape_on(&self, top: bool) -> u8 {
        if self.is_through_hole() || top {
            self.top_shape
        } else {
            self.bot_shape
        }
    }

    pub fn size_on(&self, top: bool) -> (i32, i32) {
        if self.is_through_hole() || top {
            (self.top_width, self.top_height)
        } else {
            (self.bot_width, self.bot_height)
        }
    }

    /// True when the hole is a slot rather than a round drill.
    pub fn has_slot(&self) -> bool {
        self.hole_shape == 2 && self.slot_size > 0
    }
}

#[derive(Debug, Clone)]
pub struct Via {
    pub x: i32,
    pub y: i32,
    pub diameter: i32,
    pub hole_size: i32,
}

#[derive(Debug, Clone)]
pub struct Text {
    pub layer: u8,
    pub x: i32,
    pub y: i32,
    pub height: i32,
    pub rotation: f64,
    pub mirrored: bool,
    pub content: String,
}

#[derive(Debug, Clone)]
pub struct Fill {
    pub layer: u8,
    pub x1: i32,
    pub y1: i32,
    pub x2: i32,
    pub y2: i32,
    pub rotation: f64,
}

#[derive(Debug, Clone)]
pub struct Region {
    pub layer: u8,
    pub is_body: bool,
    pub outline: Vec<RegionVertex>,
}

/// Extended region vertex; when `round` is set the segment arriving at this
/// vertex is an arc with the stored center/radius/angles.
#[derive(Debug, Clone, Copy)]
pub struct RegionVertex {
    pub x: f64,
    pub y: f64,
    pub round: bool,
    pub cx: f64,
    pub cy: f64,
    pub radius: f64,
    pub start_angle: f64,
    pub end_angle: f64,
}

pub struct PcbLib {
    pub footprints: Vec<Footprint>,
    /// Layer ids that the library's layer table flags as Courtyard. These
    /// vary between libraries (standard Altium uses 55/56, but this library
    /// places them on 71/72), so they are discovered per-file.
    pub courtyard_layers: Vec<u8>,
}

// ---------------------------------------------------------------------------
// Library access
// ---------------------------------------------------------------------------

pub fn open(path: &Path) -> Result<PcbLib, String> {
    let file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let mut comp = cfb::CompoundFile::open(file).map_err(|e| format!("OLE: {}", e))?;

    // Footprint storages are top-level directories containing a "Data" stream.
    let mut names: Vec<String> = Vec::new();
    for entry in comp.walk() {
        if entry.is_stream() && entry.name().eq_ignore_ascii_case("Data") {
            let comps: Vec<String> = entry
                .path()
                .iter()
                .map(|c| c.to_string_lossy().to_string())
                .filter(|c| c != "/" && c != "\\")
                .collect();
            if comps.len() == 2 && comps[0] != "Library" && comps[0] != "FileVersionInfo" {
                names.push(comps[0].clone());
            }
        }
    }
    names.sort_by_key(|n| n.to_lowercase());

    let mut footprints = Vec::new();
    for name in names {
        let data = match read_stream(&mut comp, Path::new(&name).join("Data")) {
            Ok(d) => d,
            Err(_) => continue,
        };
        let wide = read_stream(&mut comp, Path::new(&name).join("WideStrings")).ok();
        if let Some(fp) = parse_footprint(&name, &data, wide.as_deref()) {
            footprints.push(fp);
        }
    }

    let courtyard_layers = read_stream(&mut comp, Path::new("Library/Data"))
        .ok()
        .map(|d| parse_courtyard_layers(&d))
        .unwrap_or_default();

    Ok(PcbLib {
        footprints,
        courtyard_layers,
    })
}

/// Discover which layer ids the library's `Library/Data` table flags as
/// Courtyard (by name or MECHKIND), e.g. `LAYER71MECHKIND=CourtyardTop`.
fn parse_courtyard_layers(data: &[u8]) -> Vec<u8> {
    use std::collections::HashMap;
    let text = String::from_utf8_lossy(data);
    let mut map: HashMap<u8, (String, String)> = HashMap::new();
    for tok in text.split('|') {
        let Some(rest) = tok.strip_prefix("LAYER") else {
            continue;
        };
        // rest is "<digits>NAME=..." or "<digits>MECHKIND=...".
        let digits_end = rest
            .find(|c: char| !c.is_ascii_digit())
            .unwrap_or(rest.len());
        let Ok(id) = rest[..digits_end].parse::<u8>() else {
            continue;
        };
        let suffix = &rest[digits_end..];
        let Some(eq) = suffix.find('=') else {
            continue;
        };
        let val = &suffix[eq + 1..];
        let entry = map
            .entry(id)
            .or_insert_with(|| (String::new(), String::new()));
        if suffix[..eq + 1].ends_with("NAME=") {
            entry.0 = val.to_string();
        } else if suffix[..eq + 1].ends_with("MECHKIND=") {
            entry.1 = val.to_string();
        }
    }
    let mut out = Vec::new();
    for (id, (name, kind)) in map {
        if name.to_lowercase().contains("courtyard") || kind.to_lowercase().contains("courtyard") {
            out.push(id);
        }
    }
    out
}

fn read_stream<P: AsRef<Path>>(
    comp: &mut cfb::CompoundFile<std::fs::File>,
    path: P,
) -> Result<Vec<u8>, String> {
    let mut stream = comp.open_stream(path).map_err(|e| e.to_string())?;
    let mut buf = Vec::new();
    stream.read_to_end(&mut buf).map_err(|e| e.to_string())?;
    Ok(buf)
}

/// Read a length-prefixed subrecord payload as an owned vector.
fn subrecord_vec(r: &mut Reader) -> Option<Vec<u8>> {
    let len = r.u32()? as usize;
    if len == 0 || len > r.remaining() {
        return None;
    }
    Some(r.bytes(len)?.to_vec())
}

pub fn parse_footprint(name: &str, data: &[u8], wide_strings: Option<&[u8]>) -> Option<Footprint> {
    let strings = wide_strings.map(parse_wide_strings).unwrap_or_default();

    let mut r = Reader::new(data);
    // Header: [u32 pascal_len][pascal string]
    let header_len = r.u32()? as usize;
    if header_len == 0 || header_len > r.remaining() {
        return None;
    }
    r.bytes(header_len)?;

    let mut fp = Footprint {
        name: name.to_string(),
        prims: Vec::new(),
    };

    while r.remaining() >= 5 {
        let type_byte = r.u8().unwrap();
        let parsed = match type_byte {
            TYPE_TRACK => subrecord_vec(&mut r).and_then(|c| parse_track(&c)),
            TYPE_ARC => subrecord_vec(&mut r).and_then(|c| parse_arc(&c)),
            TYPE_PAD => parse_pad(&mut r),
            TYPE_VIA => subrecord_vec(&mut r).and_then(|c| parse_via(&c)),
            TYPE_TEXT => parse_text_record(&mut r, &strings),
            TYPE_FILL => subrecord_vec(&mut r).and_then(|c| parse_fill(&c)),
            TYPE_REGION => subrecord_vec(&mut r).and_then(|c| parse_region(&c, false)),
            TYPE_COMPONENT_BODY => subrecord_vec(&mut r).and_then(|c| parse_region(&c, true)),
            _ => None, // Unknown record: cannot know its extent, stop parsing.
        };
        match parsed {
            Some(prim) => fp.prims.push(prim),
            None => break,
        }
    }

    if fp.prims.is_empty() {
        None
    } else {
        Some(fp)
    }
}

fn parse_track(c: &[u8]) -> Option<Prim> {
    let mut r = Reader::new(c);
    let layer = r.u8()?;
    r.bytes(12)?; // flags1/flags2/net/poly/component/union
    let x1 = r.i32()?;
    let y1 = r.i32()?;
    let x2 = r.i32()?;
    let y2 = r.i32()?;
    let width = r.i32()?;
    Some(Prim::Track(Track {
        layer,
        x1,
        y1,
        x2,
        y2,
        width,
    }))
}

fn parse_arc(c: &[u8]) -> Option<Prim> {
    let mut r = Reader::new(c);
    let layer = r.u8()?;
    r.bytes(12)?;
    let cx = r.i32()?;
    let cy = r.i32()?;
    let radius = r.i32()?;
    let start_angle = r.f64()?;
    let end_angle = r.f64()?;
    let width = r.i32()?;
    Some(Prim::Arc(Arc {
        layer,
        cx,
        cy,
        radius,
        start_angle,
        end_angle,
        width,
    }))
}

fn parse_via(c: &[u8]) -> Option<Prim> {
    let mut r = Reader::new(c);
    r.bytes(13)?;
    let x = r.i32()?;
    let y = r.i32()?;
    let diameter = r.i32()?;
    let hole_size = r.i32()?;
    Some(Prim::Via(Via {
        x,
        y,
        diameter,
        hole_size,
    }))
}

fn parse_pad(r: &mut Reader) -> Option<Prim> {
    // SubRecord 1: designator (pascal string inside a length-prefixed block).
    let designator = {
        let c = subrecord_vec(r)?;
        let mut cr = Reader::new(&c);
        cr.pascal_string().unwrap_or_default()
    };
    // SubRecords 2..4: opaque payloads.
    for _ in 0..3 {
        subrecord_vec(r)?;
    }
    // SubRecord 5: SizeAndShape.
    let sr5 = subrecord_vec(r)?;
    {
        let mut c = Reader::new(&sr5);
        let layer = c.u8()?;
        c.bytes(12)?; // flags u16 + net + poly + component + union
        let x = c.i32()?;
        let y = c.i32()?;
        let top_width = c.i32()?;
        let top_height = c.i32()?;
        c.bytes(8)?; // mid width/height (SR6 overrides win when present)
        let bot_width = c.i32()?;
        let bot_height = c.i32()?;
        let hole_size = c.u32()?;
        let top_shape_raw = c.u8()?;
        let _mid_shape = c.u8()?;
        let bot_shape_raw = c.u8()?;
        let rotation = c.f64()?;

        // SubRecord 6 (optional): per-layer overrides incl. hole shape/slot.
        // Known payload lengths: 596, 628, 651; a length of 0 is a placeholder
        // that must still be consumed.
        let mut hole_shape = 0u8;
        let mut slot_size = 0i32;
        let mut slot_rotation = 0f64;
        let mut alt_top: Option<u8> = None;
        let mut alt_bot: Option<u8> = None;
        if let Some(sr6len) = r.peek_u32() {
            let l = sr6len as usize;
            if l == 0 {
                r.bytes(4)?;
            } else if matches!(l, 596 | 628 | 651) && 4 + l <= r.remaining() {
                let sr6 = subrecord_vec(r)?;
                let mut s6 = Reader::new(&sr6);
                s6.bytes(232)?; // inner X/Y sizes (29 * 2 * i32)
                s6.bytes(29)?; // inner shapes
                s6.bytes(1)?; // padding
                hole_shape = s6.u8()?;
                slot_size = s6.i32()?;
                slot_rotation = s6.f64()?;
                s6.bytes(256)?; // offsets X/Y (32 * 2 * i32)
                s6.bytes(1)?; // padding
                let alt0 = s6.u8()?;
                alt_top = Some(alt0);
                for i in 0..32 {
                    let a = s6.u8()?;
                    if i == 31 {
                        alt_bot = Some(a);
                    }
                }
            }
        }

        // Alt-shape override: 9 means rounded rectangle.
        let fix = |s: u8| {
            if s == 9 {
                PAD_SHAPE_ROUNDED_RECTANGLE
            } else {
                s
            }
        };
        let top_shape = fix(alt_top.unwrap_or(top_shape_raw));
        let bot_shape = fix(alt_bot.unwrap_or(bot_shape_raw));

        Some(Prim::Pad(Pad {
            designator,
            layer,
            x,
            y,
            top_width,
            top_height,
            bot_width,
            bot_height,
            hole_size,
            top_shape,
            bot_shape,
            rotation,
            hole_shape,
            slot_size,
            slot_rotation,
        }))
    }
}

fn parse_text_record(r: &mut Reader, strings: &[String]) -> Option<Prim> {
    let sr1 = subrecord_vec(r)?;
    let mut c = Reader::new(&sr1);
    let layer = c.u8()?;
    c.bytes(12)?; // flags/keepout/net/poly/component/union
    let x = c.i32()?;
    let y = c.i32()?;
    let height = c.i32()?;
    c.bytes(2)?; // stroke font type
    let rotation = c.f64()?;
    let mirrored = c.u8()? != 0;
    c.bytes(4)?; // stroke width
    c.bytes(2)?; // is_comment, is_designator

    let mut content = String::new();

    // Extended fields (when long enough): byte42, font_type, bold, italic,
    // fontname[64], inverted, margin, widestring_index @115..119.
    if sr1.len() >= 119 {
        c.bytes(77)?;
        let wide_index = c.u32()? as usize;
        if let Some(s) = strings.get(wide_index) {
            if !s.is_empty() {
                content = s.clone();
            }
        }
    }

    // SubRecord 2: ASCII text payload, always present in PCB text records.
    if r.remaining() >= 4 {
        let save = r.pos();
        let len_raw = r.u32().unwrap() as usize;
        if len_raw <= r.remaining() {
            let bytes = r.bytes(len_raw)?;
            if content.is_empty() && len_raw > 0 {
                let body = if bytes[0] as usize == len_raw - 1 {
                    &bytes[1..]
                } else {
                    bytes
                };
                let t = decode_text(body);
                let trimmed = t.trim_end_matches('\0');
                if !trimmed.is_empty() {
                    content = trimmed.trim().to_string();
                }
            }
        } else {
            r.seek(save);
        }
    }

    Some(Prim::Text(Text {
        layer,
        x,
        y,
        height,
        rotation,
        mirrored,
        content,
    }))
}

fn parse_fill(c: &[u8]) -> Option<Prim> {
    let mut r = Reader::new(c);
    let layer = r.u8()?;
    r.bytes(12)?;
    let x1 = r.i32()?;
    let y1 = r.i32()?;
    let x2 = r.i32()?;
    let y2 = r.i32()?;
    let rotation = r.f64()?;
    Some(Prim::Fill(Fill {
        layer,
        x1,
        y1,
        x2,
        y2,
        rotation,
    }))
}

/// Parse REGION / COMPONENT_BODY payloads. Both share a common header and
/// property blob; geometry comes in simple (f64 pairs) or extended
/// (37-byte arc-aware vertices) variants, with or without a closing vertex.
fn parse_region(c: &[u8], is_body: bool) -> Option<Prim> {
    let mut r = Reader::new(c);
    let layer = r.u8()?;
    r.bytes(8)?; // flags1/flags2/net/poly/component
    r.bytes(5)?; // union index + padding (offsets 9..13)
    r.bytes(2)?; // hole count (holes are not rendered)
    r.bytes(2)?; // reserved (offsets 16..17)

    // Properties: [u32 len][ascii text] starting at offset 18.
    let props_len = r.u32()? as usize;
    if props_len > c.len() {
        return None;
    }
    let props = decode_text(r.bytes(props_len.min(r.remaining()))?);
    let shapebased = props.contains("ISSHAPEBASED=TRUE");
    // Shape-based and body payloads carry an extra NUL after the properties
    // blob; plain regions keep it inside the declared length instead.
    if (is_body || shapebased) && r.remaining() >= 1 && r.peek_u8() == Some(0) {
        r.bytes(1)?;
    }

    let count_pos = r.pos();
    let raw_count = r.u32()? as usize;

    // Candidate geometry layouts to try, best-fit wins.
    let mut candidates: Vec<(bool, bool)> = Vec::new(); // (extended, closing)
    if is_body {
        if shapebased {
            candidates.extend([(true, true), (true, false), (false, false)]);
        } else {
            candidates.extend([(false, false), (true, false), (true, true)]);
        }
    } else if shapebased {
        candidates.push((true, true));
    } else {
        candidates.push((false, false));
    }

    // Try each candidate geometry layout and keep the one consuming the
    // payload most completely (bodies may carry a small trailing block).
    let mut best: Option<(Vec<RegionVertex>, usize)> = None;
    for (ext, closing) in candidates {
        let verts = match read_vertices(c, count_pos, raw_count, ext, closing) {
            Some(v) => v,
            None => continue,
        };
        let leftover = match leftover_after(c, count_pos, raw_count, ext, closing) {
            Some(l) => l,
            None => continue,
        };
        if best.as_ref().map(|(_, l)| leftover < *l).unwrap_or(true) {
            best = Some((verts, leftover));
        }
        if leftover <= 3 {
            break;
        }
    }

    let (outline, _) = best?;

    Some(Prim::Region(Region {
        layer,
        is_body,
        outline,
    }))
}

/// Read outline vertices for a given variant.
fn read_vertices(
    c: &[u8],
    count_pos: usize,
    raw_count: usize,
    ext: bool,
    closing: bool,
) -> Option<Vec<RegionVertex>> {
    let mut probe = Reader::new(c);
    probe.seek(count_pos + 4);
    let n = raw_count + usize::from(closing);
    let mut verts = Vec::with_capacity(n.min(4096));
    for _ in 0..n {
        if ext {
            let round = probe.u8()? != 0;
            let x = probe.i32()? as f64;
            let y = probe.i32()? as f64;
            let cx = probe.i32()? as f64;
            let cy = probe.i32()? as f64;
            let radius = probe.i32()? as f64;
            let sa = probe.f64()?;
            let ea = probe.f64()?;
            verts.push(RegionVertex {
                x,
                y,
                round,
                cx,
                cy,
                radius,
                start_angle: sa,
                end_angle: ea,
            });
        } else {
            let x = probe.f64()?;
            let y = probe.f64()?;
            verts.push(RegionVertex {
                x,
                y,
                round: false,
                cx: 0.0,
                cy: 0.0,
                radius: 0.0,
                start_angle: 0.0,
                end_angle: 0.0,
            });
        }
    }
    Some(verts)
}

/// Compute unconsumed byte count after outline+holes for a given variant.
fn leftover_after(
    c: &[u8],
    count_pos: usize,
    raw_count: usize,
    ext: bool,
    closing: bool,
) -> Option<usize> {
    let mut probe = Reader::new(c);
    probe.seek(count_pos + 4);
    let n = raw_count + usize::from(closing);
    for _ in 0..n {
        if ext {
            probe.bytes(37)?;
        } else {
            probe.bytes(16)?;
        }
    }
    while let Some(hn) = probe.peek_u32() {
        let hn = hn as usize;
        if probe.remaining() < 4 + hn * 16 {
            break;
        }
        probe.bytes(4 + hn * 16)?;
    }
    Some(probe.remaining())
}

/// Parse `<Footprint>/WideStrings`: `[u32 len]["|ENCODEDTEXT{n}=b,b,b|..."]`.
fn parse_wide_strings(data: &[u8]) -> Vec<String> {
    let mut out = Vec::new();
    if data.len() < 4 {
        return out;
    }
    let len = u32::from_le_bytes([data[0], data[1], data[2], data[3]]) as usize;
    if len == 0 || 4 + len > data.len() {
        return out;
    }
    let text = String::from_utf8_lossy(&data[4..4 + len]);
    for pair in text.split('|') {
        let Some(rest) = pair.strip_prefix("ENCODEDTEXT") else {
            continue;
        };
        let Some((idx_s, val_s)) = rest.split_once('=') else {
            continue;
        };
        let Ok(idx) = idx_s.parse::<usize>() else {
            continue;
        };
        let mut chars = Vec::new();
        for b in val_s.split(',') {
            if let Ok(v) = b.trim().parse::<u32>() {
                chars.push(char::from_u32(v).unwrap_or('?'));
            }
        }
        if out.len() <= idx {
            out.resize(idx + 1, String::new());
        }
        out[idx] = chars.into_iter().collect();
    }
    out
}
