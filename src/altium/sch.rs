//! Altium .SchLib parsing.
//!
//! The library stores each symbol in its own OLE storage `<Name>/Data` as a
//! stream of length-prefixed records. Text records are `KEY=VALUE` property
//! lists (`RECORD=n` selects the kind); pins are stored as binary blobs.
//! Schematic coordinates are in 10-mil units with an optional fractional part.

use super::records::{parse_stream_records, Reader, StreamRecord};
use std::io::Read;
use std::path::Path;

/// Internal schematic units per mil.
#[allow(dead_code)]
pub const UNITS_PER_MIL: f64 = 0.1;

/// Default ink color used when COLOR is absent (black, matching Altium).
pub const SCH_DEFAULT_INK: u32 = 0x000000;

#[derive(Debug, Default, Clone)]
pub struct SchLib {
    pub symbols: Vec<Symbol>,
    /// Point sizes per font id from the library header (index 0 = font 1).
    pub font_sizes: Vec<f64>,
}

#[derive(Debug, Default, Clone)]
pub struct Symbol {
    pub name: String,
    pub lib_reference: String,
    pub part_count: i64,
    pub prims: Vec<SchPrim>,
    pub pins: Vec<Pin>,
}

#[derive(Debug, Clone, Copy)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}

impl Point {
    /// Convert from record values: integer part plus 1/100000 fraction, in
    /// 10-mil schematic units, into mils.
    fn from_parts(xi: i64, xf: i64, yi: i64, yf: i64) -> Self {
        Point {
            x: (xi as f64 + xf as f64 / 100_000.0) * 10.0,
            y: (yi as f64 + yf as f64 / 100_000.0) * 10.0,
        }
    }
}

#[derive(Debug, Clone)]
pub struct SchPrim {
    pub owner_part: i64,
    pub kind: SchPrimKind,
}

#[derive(Debug, Clone)]
pub enum SchPrimKind {
    Polyline {
        points: Vec<Point>,
        width: u8,
        color: u32,
    },
    Polygon {
        points: Vec<Point>,
        color: u32,
        area_color: u32,
        solid: bool,
    },
    Ellipse {
        center: Point,
        radius_x: f64,
        radius_y: f64,
        color: u32,
        area_color: u32,
        solid: bool,
    },
    Pie {
        center: Point,
        radius_x: f64,
        radius_y: f64,
        start_angle: f64,
        end_angle: f64,
        color: u32,
        area_color: u32,
        solid: bool,
    },
    RoundRect {
        p1: Point,
        p2: Point,
        corner: Point,
        color: u32,
        area_color: u32,
        solid: bool,
    },
    EllipticalArc {
        center: Point,
        radius_x: f64,
        radius_y: f64,
        start_angle: f64,
        end_angle: f64,
        width: u8,
        color: u32,
    },
    Arc {
        center: Point,
        radius: f64,
        start_angle: f64,
        end_angle: f64,
        width: u8,
        color: u32,
    },
    /// RECORD=3: IEEE symbol marker drawn at a location.
    IeeeMarker {
        pos: Point,
        color: u32,
    },
    /// RECORD=30: embedded image; rendered as a placeholder frame.
    Image {
        p1: Point,
        p2: Point,
    },
    Bezier {
        points: Vec<Point>,
        width: u8,
        color: u32,
    },
    Label {
        pos: Point,
        text: String,
        color: u32,
        font_id: i64,
        orientation: i64,
    },
}

#[derive(Debug, Clone)]
pub struct Pin {
    #[allow(dead_code)]
    pub owner_part_id: i16,
    #[allow(dead_code)]
    pub description: String,
    #[allow(dead_code)]
    pub electrical: u8,
    pub orientation: u8,
    pub hidden: bool,
    #[allow(dead_code)]
    pub show_name: bool,
    #[allow(dead_code)]
    pub show_designator: bool,
    /// Length in mils.
    pub length: f64,
    /// Body attachment point in mils.
    pub x: f64,
    pub y: f64,
    #[allow(dead_code)]
    pub color: u32,
    pub name: String,
    pub designator: String,
}

// ---------------------------------------------------------------------------
// Library access
// ---------------------------------------------------------------------------

pub fn open(path: &Path) -> Result<SchLib, String> {
    let file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let mut comp = cfb::CompoundFile::open(file).map_err(|e| format!("OLE: {}", e))?;

    // Font table from the FileHeader record.
    let font_sizes = read_stream(&mut comp, "FileHeader")
        .ok()
        .map(|data| parse_font_sizes(&data))
        .unwrap_or_default();

    // Symbols are top-level storages other than FileHeader, each with Data.
    let mut names: Vec<String> = Vec::new();
    for entry in comp.walk() {
        if entry.is_storage()
            && entry
                .path()
                .iter()
                .filter(|c| c.to_string_lossy() != "/" && c.to_string_lossy() != "\\")
                .count()
                == 1
        {
            let n = entry.name();
            if !n.eq_ignore_ascii_case("FileHeader") && !n.eq_ignore_ascii_case("Storage") {
                names.push(n.to_string());
            }
        }
    }
    names.retain(|name| comp.exists(Path::new(name).join("Data")));

    let mut symbols = Vec::new();
    for name in names {
        let data = match read_stream(&mut comp, Path::new(&name).join("Data")) {
            Ok(d) => d,
            Err(_) => continue,
        };
        if let Some(sym) = parse_symbol(&name, &data) {
            symbols.push(sym);
        }
    }
    Ok(SchLib { symbols, font_sizes })
}

/// Extract `SizeN` point sizes from the FileHeader stream.
fn parse_font_sizes(data: &[u8]) -> Vec<f64> {
    let mut sizes = Vec::new();
    for rec in parse_stream_records(data) {
        if let StreamRecord::Text(pairs) = rec {
            for (k, v) in pairs {
                if let Some(idx) = k.strip_prefix("Size").and_then(|s| s.parse::<usize>().ok()) {
                    if let Ok(pt) = v.trim().parse::<f64>() {
                        if sizes.len() <= idx {
                            sizes.resize(idx + 1, 10.0);
                        }
                        sizes[idx] = pt;
                    }
                }
            }
        }
        break; // Only the first (header) record carries the font table.
    }
    sizes
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

pub fn parse_symbol(name: &str, data: &[u8]) -> Option<Symbol> {
    let records = parse_stream_records(data);
    if records.is_empty() {
        return None;
    }

    let mut sym = Symbol {
        name: name.to_string(),
        lib_reference: name.to_string(),
        part_count: 1,
        ..Default::default()
    };

    for rec in &records {
        match rec {
            StreamRecord::Binary(blob) => {
                if blob.first() == Some(&2) {
                    if let Some(pin) = parse_pin_blob(blob) {
                        sym.pins.push(pin);
                    }
                }
            }
            StreamRecord::Text(_) => {
                let rt = rec.prop_i64("RECORD").unwrap_or(0);
                match rt {
                    1 => {
                        if let Some(v) = rec.prop("LibReference") {
                            sym.lib_reference = v.to_string();
                        }
                        if let Some(v) = rec.prop_i64("PartCount") {
                            sym.part_count = v;
                        }
                    }
                    // 3 = IEEE marker, 13 = line, 14 = rectangle, 12 = arc,
                    // 30 = image frame. Structural records (1 component,
                    // 25/33 parameters, 34/41/44 implementations) are
                    // parsed separately or intentionally not rendered.
                    3 | 5 | 6 | 7 | 8 | 9 | 10 | 11 | 12 | 13 | 14 | 30 => {
                        if let Some(kind) = parse_graphic(rec, rt) {
                            sym.prims.push(SchPrim {
                                owner_part: rec.prop_i64("OWNERPARTID").unwrap_or(-1),
                                kind,
                            });
                        }
                    }
                    4 => {
                        if let Some(kind) = parse_label(rec) {
                            sym.prims.push(SchPrim { owner_part: rec.prop_i64("OWNERPARTID").unwrap_or(-1), kind });
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    Some(sym)
}

/// Filter helper: does this record belong to the requested part?
/// `owner_part_id` of -1 (or missing) applies to every part.
pub fn belongs_to_part(owner: i64, part: Option<i64>) -> bool {
    match part {
        None => true,
        Some(requested) => owner == -1 || owner == requested,
    }
}

fn rec_point(rec: &StreamRecord, idx: usize) -> Option<Point> {
    // Missing Xn/Yn keys mean 0 (Altium omits zero components).
    let xi = rec.prop_i64(&format!("X{idx}")).unwrap_or(0);
    let yi = rec.prop_i64(&format!("Y{idx}")).unwrap_or(0);
    let xf = rec.prop_i64(&format!("X{idx}_Frac")).unwrap_or(0);
    let yf = rec.prop_i64(&format!("Y{idx}_Frac")).unwrap_or(0);
    Some(Point::from_parts(xi, xf, yi, yf))
}

fn rec_xy(rec: &StreamRecord, key: &str) -> Option<Point> {
    // Missing X/Y keys mean 0; the point exists if the LOCATION key itself
    // is present (checked by the caller via any component).
    if rec.prop(&format!("{key}.X")).is_none() && rec.prop(&format!("{key}.Y")).is_none() {
        return None;
    }
    Some(rec_xy_zero(rec, key))
}

fn rec_xy_zero(rec: &StreamRecord, key: &str) -> Point {
    let xi = rec.prop_i64(&format!("{key}.X")).unwrap_or(0);
    let yi = rec.prop_i64(&format!("{key}.Y")).unwrap_or(0);
    let xf = rec.prop_i64(&format!("{key}.X_Frac")).unwrap_or(0);
    let yf = rec.prop_i64(&format!("{key}.Y_Frac")).unwrap_or(0);
    Point::from_parts(xi, xf, yi, yf)
}

/// Number of location points: explicit `LOCATIONCOUNT`, or the highest
/// `Xn`/`Yn` index present (older files omit the count and zero components).
fn rec_point_count(rec: &StreamRecord) -> usize {
    if let Some(n) = rec.prop_i64("LOCATIONCOUNT") {
        return n.max(0) as usize;
    }
    let mut max = 0usize;
    for n in 1..=100 {
        if rec.prop(&format!("X{n}")).is_some() || rec.prop(&format!("Y{n}")).is_some() {
            max = n;
        }
    }
    max
}

fn parse_graphic(rec: &StreamRecord, rt: i64) -> Option<SchPrimKind> {
    match rt {
        6 => {
            let n = rec_point_count(rec);
            let mut points = Vec::with_capacity(n);
            for i in 1..=n {
                points.push(rec_point(rec, i)?);
            }
            if points.is_empty() {
                return None;
            }
            Some(SchPrimKind::Polyline {
                points,
                width: rec.prop_i64("LINEWIDTH").unwrap_or(1) as u8,
                color: color_of(rec, "COLOR").unwrap_or(SCH_DEFAULT_INK),
            })
        }
        7 => {
            let n = rec_point_count(rec);
            let mut points = Vec::with_capacity(n);
            for i in 1..=n {
                points.push(rec_point(rec, i)?);
            }
            if points.is_empty() {
                return None;
            }
            Some(SchPrimKind::Polygon {
                points,
                color: color_of(rec, "COLOR").unwrap_or(SCH_DEFAULT_INK),
                area_color: color_of(rec, "AREACOLOR")
                    .or_else(|| color_of(rec, "COLOR"))
                    .unwrap_or(SCH_DEFAULT_INK),
                solid: prop_bool(rec, "ISSOLID"),
            })
        }
        8 => {
            let center = rec_xy(rec, "LOCATION")?;
            let rx = mils(rec.prop_f64("RADIUS")?);
            let ry = mils(rec.prop_f64("SECONDARYRADIUS")?);
            Some(SchPrimKind::Ellipse {
                center,
                radius_x: rx,
                radius_y: ry,
                color: color_of(rec, "COLOR").unwrap_or(SCH_DEFAULT_INK),
                area_color: color_of(rec, "AREACOLOR")
                    .or_else(|| color_of(rec, "COLOR"))
                    .unwrap_or(SCH_DEFAULT_INK),
                solid: prop_bool(rec, "ISSOLID"),
            })
        }
        9 => {
            let center = rec_xy(rec, "LOCATION")?;
            Some(SchPrimKind::Pie {
                center,
                radius_x: mils(rec.prop_f64("RADIUS")?),
                radius_y: mils(rec.prop_f64("SECONDARYRADIUS").unwrap_or(0.0)),
                start_angle: rec.prop_f64("STARTANGLE")?,
                end_angle: rec.prop_f64("ENDANGLE")?,
                color: color_of(rec, "COLOR").unwrap_or(SCH_DEFAULT_INK),
                area_color: color_of(rec, "AREACOLOR")
                    .or_else(|| color_of(rec, "COLOR"))
                    .unwrap_or(SCH_DEFAULT_INK),
                solid: prop_bool(rec, "ISSOLID"),
            })
        }
        10 => {
            let p1 = rec_point(rec, 1)?;
            let p2 = rec_point(rec, 2)?;
            let corner = Point {
                x: mils(rec.prop_f64("CORNERXRADIUS")?),
                y: mils(rec.prop_f64("CORNERYRADIUS")?),
            };
            Some(SchPrimKind::RoundRect {
                p1,
                p2,
                corner,
                color: color_of(rec, "COLOR").unwrap_or(SCH_DEFAULT_INK),
                area_color: color_of(rec, "AREACOLOR")
                    .or_else(|| color_of(rec, "COLOR"))
                    .unwrap_or(SCH_DEFAULT_INK),
                solid: prop_bool(rec, "ISSOLID"),
            })
        }
        11 => {
            let center = rec_xy(rec, "LOCATION")?;
            Some(SchPrimKind::EllipticalArc {
                center,
                 radius_x: mils(rec.prop_f64("RADIUS")?),
                radius_y: mils(rec.prop_f64("SECONDARYRADIUS")?),
                start_angle: rec.prop_f64("STARTANGLE").unwrap_or(0.0),
                end_angle: rec.prop_f64("ENDANGLE").unwrap_or(360.0),
                width: rec.prop_i64("LINEWIDTH").unwrap_or(1) as u8,
                color: color_of(rec, "COLOR").unwrap_or(SCH_DEFAULT_INK),
            })
        }
        // 12: circular arc (center, radius, angles). Full-circle / semicircle
        // arcs (connector bubbles, transistor bodies, inductor half-coils)
        // store only `ENDANGLE` and rely on Altium's implicit STARTANGLE of 0.
        12 => {
            let center = rec_xy(rec, "LOCATION")?;
            let radius = mils(rec.prop_f64("RADIUS").or_else(|| rec.prop_f64("SECONDARYRADIUS"))?);
            Some(SchPrimKind::Arc {
                center,
                radius,
                start_angle: rec.prop_f64("STARTANGLE").unwrap_or(0.0),
                end_angle: rec.prop_f64("ENDANGLE")?,
                width: rec.prop_i64("LINEWIDTH").unwrap_or(1) as u8,
                color: color_of(rec, "COLOR").unwrap_or(SCH_DEFAULT_INK),
            })
        }
        // 3: IEEE symbol marker (rendered as a small placeholder glyph).
        3 => {
            let pos = rec_xy(rec, "LOCATION")?;
            Some(SchPrimKind::IeeeMarker {
                pos,
                color: color_of(rec, "COLOR").unwrap_or(SCH_DEFAULT_INK),
            })
        }
        // 30: embedded image (LOCATION + WIDTH/HEIGHT); rendered as a frame.
        30 => {
            let p1 = rec_xy(rec, "LOCATION")?;
            let w = mils(rec.prop_f64("WIDTH")?);
            let h = mils(rec.prop_f64("HEIGHT")?);
            Some(SchPrimKind::Image {
                p1,
                p2: Point { x: p1.x + w, y: p1.y + h },
            })
        }
        // 13: line segment from Location to Corner (zero components omitted).
        13 => {
            let p1 = rec_xy_zero(rec, "LOCATION");
            let p2 = rec_xy_zero(rec, "CORNER");
            Some(SchPrimKind::Polyline {
                points: vec![p1, p2],
                width: rec.prop_i64("LINEWIDTH").unwrap_or(1) as u8,
                color: color_of(rec, "COLOR").unwrap_or(SCH_DEFAULT_INK),
            })
        }
        // 14: plain rectangle given by Location and Corner corners; missing
        // components (sometimes all of them) are zero.
        14 => {
            let p1 = rec_xy_zero(rec, "LOCATION");
            let p2 = rec_xy_zero(rec, "CORNER");
            Some(SchPrimKind::RoundRect {
                p1,
                p2,
                corner: Point { x: 0.0, y: 0.0 },
                color: color_of(rec, "COLOR").unwrap_or(SCH_DEFAULT_INK),
                area_color: color_of(rec, "AREACOLOR")
                    .or_else(|| color_of(rec, "COLOR"))
                    .unwrap_or(SCH_DEFAULT_INK),
                solid: prop_bool(rec, "ISSOLID"),
            })
        }
        5 => {
            let n = rec_point_count(rec);
            let mut points = Vec::with_capacity(n);
            for i in 1..=n {
                points.push(rec_point(rec, i)?);
            }
            if points.is_empty() {
                return None;
            }
            Some(SchPrimKind::Bezier {
                points,
                width: rec.prop_i64("LINEWIDTH").unwrap_or(1) as u8,
                color: color_of(rec, "COLOR").unwrap_or(SCH_DEFAULT_INK),
            })
        }
        _ => None,
    }
}

fn parse_label(rec: &StreamRecord) -> Option<SchPrimKind> {
    Some(SchPrimKind::Label {
        pos: rec_xy(rec, "LOCATION")?,
        text: rec.prop("TEXT").unwrap_or_default().to_string(),
        color: color_of(rec, "COLOR").unwrap_or(SCH_DEFAULT_INK),
        font_id: rec.prop_i64("FONTID").unwrap_or(1),
        orientation: rec.prop_i64("ORIENTATION").unwrap_or(0),
    })
}

fn mils(units: f64) -> f64 {
    units * 10.0
}

fn color_of(rec: &StreamRecord, key: &str) -> Option<u32> {
    rec.prop_i64(key).map(|v| (v as u32) & 0xFFFFFF)
}

fn prop_bool(rec: &StreamRecord, key: &str) -> bool {
    matches!(rec.prop(key).map(|s| s.to_ascii_uppercase()).as_deref(), Some("T") | Some("TRUE"))
}

/// Binary PIN blob layout (first byte 0x02).
fn parse_pin_blob(blob: &[u8]) -> Option<Pin> {
    if blob.len() < 30 {
        return None;
    }
    let mut r = Reader::new(blob);
    r.u8()?; // type byte 0x02
    let owner_index = r.u32()?;
    let _ = owner_index;
    let owner_part_id = r.i16()?;
    let display_mode = r.u8()?;
    let _ = display_mode;
    r.bytes(4)?; // IEEE symbol decorations
    let description = r.pascal_string().unwrap_or_default();
    let _formal_type = r.u8()?;
    let electrical = r.u8()? & 0x0F;
    let conglomerate = r.u8()?;
    let length_units = r.i16()?;
    let x_units = r.i16()?;
    let y_units = r.i16()?;
    let color = r.u32()?;
    let name = r.pascal_string().unwrap_or_default();
    let designator = r.pascal_string().unwrap_or_default();

    Some(Pin {
        owner_part_id,
        description,
        electrical,
        orientation: conglomerate & 0x03,
        hidden: conglomerate & 0x04 != 0,
        show_name: conglomerate & 0x08 != 0,
        show_designator: conglomerate & 0x10 != 0,
        length: length_units as f64 * 10.0,
        x: x_units as f64 * 10.0,
        y: y_units as f64 * 10.0,
        color,
        name,
        designator,
    })
}
