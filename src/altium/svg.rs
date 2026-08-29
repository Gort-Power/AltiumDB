//! Minimal SVG rendering for parsed Altium libraries.
//!
//! Rendering is two-phase: primitives are collected into layout-independent
//! ops (logical coordinates, Y up, mils) while measuring bounds; ops are then
//! emitted with the final viewBox mapping. Text nodes are plain `<text>` and
//! are rasterized later via resvg.

use super::pcblib::{self, Pad, Prim};
use super::sch::{self, Point, SchPrimKind};

// ---------------------------------------------------------------------------
// Shared helpers
// ---------------------------------------------------------------------------

/// Win32 COLORREF (0x00BBGGRR) to CSS color.
pub fn rgb(c: u32) -> String {
    let r = c & 0xFF;
    let g = (c >> 8) & 0xFF;
    let b = (c >> 16) & 0xFF;
    format!("#{r:02X}{g:02X}{b:02X}")
}

fn fmt(v: f64) -> String {
    if !v.is_finite() {
        return "0".to_string();
    }
    let r = v.round();
    if (v - r).abs() < 1e-9 {
        format!("{}", r as i64)
    } else {
        format!("{v:.2}")
    }
}

fn escape_text(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn wrap_svg(width: f64, height: f64, body: &str, bg: &str) -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <svg version=\"1.1\" xmlns=\"http://www.w3.org/2000/svg\" \
         stroke-linecap=\"round\" stroke-linejoin=\"round\" \
         width=\"{w}\" height=\"{h}\" viewBox=\"0 0 {w} {h}\">\
         <rect x=\"0\" y=\"0\" width=\"{w}\" height=\"{h}\" fill=\"{bg}\"/>{body}</svg>\n",
        w = fmt(width),
        h = fmt(height),
        body = body,
        bg = bg,
    )
}

/// Layout-independent drawing op in logical coordinates (Y up).
enum Op {
    Line {
        p1: Point,
        p2: Point,
        width: f64,
        color: String,
    },
    Polyline {
        pts: Vec<Point>,
        width: f64,
        color: String,
        closed: bool,
        fill: Option<String>,
        fill_opacity: f64,
    },
    Ellipse {
        center: Point,
        rx: f64,
        ry: f64,
        stroke: String,
        fill: Option<String>,
        stroke_width: f64,
    },
    Rect {
        center: Point,
        w: f64,
        h: f64,
        rx: f64,
        rotation: f64,
        stroke: Option<String>,
        fill: Option<String>,
    },
    Text {
        pos: Point,
        size: f64,
        color: String,
        anchor: &'static str,
        content: String,
        rotate: f64,
    },
}

struct Mapper {
    min_x: f64,
    max_y: f64,
}

impl Mapper {
    fn tx(&self, x: f64) -> f64 {
        x - self.min_x
    }

    fn ty(&self, y: f64) -> f64 {
        self.max_y - y
    }

    fn pt(&self, p: &Point) -> (String, String) {
        (fmt(self.tx(p.x)), fmt(self.ty(p.y)))
    }

    fn emit(&self, op: &Op) -> String {
        match op {
            Op::Line {
                p1,
                p2,
                width,
                color,
            } => {
                let (x1, y1) = self.pt(p1);
                let (x2, y2) = self.pt(p2);
                format!(
                    "<line x1=\"{x1}\" y1=\"{y1}\" x2=\"{x2}\" y2=\"{y2}\" stroke=\"{color}\" stroke-width=\"{}\"/>",
                    fmt(width.max(0.9))
                )
            }
            Op::Polyline {
                pts,
                width,
                color,
                closed,
                fill,
                fill_opacity,
            } => {
                if pts.len() < 2 && !closed {
                    return String::new();
                }
                let mut d = String::new();
                for (i, p) in pts.iter().enumerate() {
                    let (x, y) = self.pt(p);
                    d.push_str(if i == 0 { "M" } else { "L" });
                    d.push_str(&format!(" {x} {y}"));
                }
                if *closed {
                    d.push('Z');
                }
                let f = fill.clone().unwrap_or_else(|| "none".to_string());
                let opacity = if *fill_opacity < 1.0 {
                    format!(" fill-opacity=\"{}\"", fmt(*fill_opacity))
                } else {
                    String::new()
                };
                format!(
                    "<path d=\"{d}\" stroke=\"{color}\" stroke-width=\"{}\" fill=\"{f}\"{opacity}/>",
                    fmt(width.max(0.9))
                )
            }
            Op::Ellipse {
                center,
                rx,
                ry,
                stroke,
                fill,
                stroke_width,
            } => {
                let (cx, cy) = self.pt(center);
                let f = fill.clone().unwrap_or_else(|| "none".to_string());
                format!(
                    "<ellipse cx=\"{cx}\" cy=\"{cy}\" rx=\"{}\" ry=\"{}\" fill=\"{f}\" stroke=\"{stroke}\" stroke-width=\"{}\"/>",
                    fmt(rx.max(0.1)),
                    fmt(ry.max(0.1)),
                    fmt(stroke_width.max(0.9))
                )
            }
            Op::Rect {
                center,
                w,
                h,
                rx,
                rotation,
                stroke,
                fill,
            } => {
                let x = self.tx(center.x - w / 2.0);
                let y = self.ty(center.y + h / 2.0);
                let mut attrs = format!(
                    "x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\"",
                    fmt(x),
                    fmt(y),
                    fmt(*w),
                    fmt(*h)
                );
                if *rx > 0.01 {
                    attrs.push_str(&format!(" rx=\"{}\" ry=\"{}\"", fmt(*rx), fmt(*rx)));
                }
                if let Some(f) = fill {
                    attrs.push_str(&format!(" fill=\"{f}\""));
                } else {
                    attrs.push_str(" fill=\"none\"");
                }
                if let Some(s) = stroke {
                    attrs.push_str(&format!(" stroke=\"{s}\" stroke-width=\"10\""));
                }
                if rotation.abs() > 1e-9 {
                    let (cx, cy) = self.pt(center);
                    attrs.push_str(&format!(
                        " transform=\"rotate({} {cx} {cy})\"",
                        fmt(-rotation)
                    ));
                }
                format!("<rect {attrs}/>")
            }
            Op::Text {
                pos,
                size,
                color,
                anchor,
                content,
                rotate,
            } => {
                let (x, y) = self.pt(pos);
                // Standard SVG: `y` is the text baseline.
                let rot = if rotate.abs() > 1e-9 {
                    format!(" transform=\"rotate({} {x} {y})\"", fmt(-rotate))
                } else {
                    String::new()
                };
                format!(
                    "<text x=\"{x}\" y=\"{y}\" font-size=\"{}\" font-family=\"Times New Roman, serif\" fill=\"{color}\" text-anchor=\"{anchor}\"{rot}>{}</text>",
                    fmt(size.max(2.0)),
                    escape_text(content)
                )
            }
        }
    }
}

fn finish(min_x: f64, min_y: f64, max_x: f64, max_y: f64, ops: &[Op], bg: &str) -> String {
    let pad = ((max_x - min_x).max(max_y - min_y) * 0.06).max(4.0);
    let m = Mapper {
        min_x: min_x - pad,
        max_y: max_y + pad,
    };
    let mut body = String::new();
    for op in ops {
        body.push_str(&m.emit(op));
        body.push('\n');
    }
    wrap_svg(
        (max_x - min_x) + pad * 2.0,
        (max_y - min_y) + pad * 2.0,
        &body,
        bg,
    )
}

#[derive(Default)]
struct Bounds {
    min_x: f64,
    min_y: f64,
    max_x: f64,
    max_y: f64,
    valid: bool,
}

impl Bounds {
    fn add(&mut self, x: f64, y: f64) {
        if !self.valid {
            self.min_x = x;
            self.min_y = y;
            self.max_x = x;
            self.max_y = y;
            self.valid = true;
            return;
        }
        self.min_x = self.min_x.min(x);
        self.min_y = self.min_y.min(y);
        self.max_x = self.max_x.max(x);
        self.max_y = self.max_y.max(y);
    }

    fn add_circle(&mut self, x: f64, y: f64, r: f64) {
        self.add(x - r, y - r);
        self.add(x + r, y + r);
    }
}

// ---------------------------------------------------------------------------
// PcbLib rendering
// ---------------------------------------------------------------------------

const PCB_TOP: &str = "#C40000";
const PCB_BOTTOM: &str = "#1464C8";
const PCB_OVERLAY: &str = "#FFCC00";
const PCB_COURTYARD: &str = "#FF66CC";
const PCB_MECH: &str = "#8A8A8A";
const PCB_BODY_FILL: &str = "#3BB143";
const PCB_BODY_STROKE: &str = "#2E8B57";

/// Layers that are meaningful for reviewing a footprint: copper/Signal
/// (Top..Bottom), Overlay (silkscreen) and Courtyard. Everything else
/// (paste, solder mask, mechanical, drill, …) is skipped. `courtyard`
/// holds the per-library courtyard layer ids (which vary between libraries).
fn pcb_render_layer(layer: u8, courtyard: &[u8]) -> bool {
    (pcblib::LAYER_TOP..=pcblib::LAYER_BOTTOM).contains(&layer)
        || layer == pcblib::LAYER_TOP_OVERLAY
        || layer == pcblib::LAYER_BOTTOM_OVERLAY
        || layer == pcblib::LAYER_TOP_COURTYARD
        || layer == pcblib::LAYER_BOTTOM_COURTYARD
        || courtyard.contains(&layer)
}

fn pcb_layer_color(layer: u8, courtyard: &[u8]) -> &'static str {
    match layer {
        pcblib::LAYER_TOP | pcblib::LAYER_MULTI => PCB_TOP,
        pcblib::LAYER_BOTTOM => PCB_BOTTOM,
        pcblib::LAYER_TOP_OVERLAY | pcblib::LAYER_BOTTOM_OVERLAY => PCB_OVERLAY,
        pcblib::LAYER_TOP_COURTYARD | pcblib::LAYER_BOTTOM_COURTYARD => PCB_COURTYARD,
        _ if courtyard.contains(&layer) => PCB_COURTYARD,
        _ => PCB_MECH,
    }
}

pub fn footprint_svg(fp: &pcblib::Footprint, courtyard: &[u8], bg: &str) -> String {
    const U: f64 = pcblib::UNITS_PER_MIL;
    let mut b = Bounds::default();
    let mut ops: Vec<Op> = Vec::new();

    // Draw in layer order: bodies/regions first, then copper artwork, pads on
    // top, texts last — regardless of the order records appear in the stream.
    for pass in 0..4 {
        for p in &fp.prims {
            let emit = match p {
                Prim::Region(_) => pass == 0,
                Prim::Fill(_) | Prim::Track(_) | Prim::Arc(_) => pass == 1,
                Prim::Pad(_) | Prim::Via(_) => pass == 2,
                Prim::Text(_) => pass == 3,
            };
            if !emit {
                continue;
            }
            match p {
                Prim::Track(t) => {
                    if !pcb_render_layer(t.layer, courtyard) {
                        continue;
                    }
                    let (p1, p2, w) = (
                        Point {
                            x: t.x1 as f64 / U,
                            y: t.y1 as f64 / U,
                        },
                        Point {
                            x: t.x2 as f64 / U,
                            y: t.y2 as f64 / U,
                        },
                        t.width as f64 / U,
                    );
                    b.add(p1.x - w, p1.y - w);
                    b.add(p1.x + w, p1.y + w);
                    b.add(p2.x - w, p2.y - w);
                    b.add(p2.x + w, p2.y + w);
                    let color = pcb_layer_color(t.layer, courtyard).to_string();
                    ops.push(Op::Line {
                        p1,
                        p2,
                        width: w,
                        color,
                    });
                }
                Prim::Arc(a) => {
                    if !pcb_render_layer(a.layer, courtyard) {
                        continue;
                    }
                    let c = Point {
                        x: a.cx as f64 / U,
                        y: a.cy as f64 / U,
                    };
                    let r = a.radius as f64 / U;
                    let w = (a.width as f64 / U).max(1.0);
                    b.add_circle(c.x, c.y, r);
                    let color = pcb_layer_color(a.layer, courtyard).to_string();
                    if a.is_full_circle() {
                        ops.push(Op::Ellipse {
                            center: c,
                            rx: r,
                            ry: r,
                            stroke: color,
                            fill: None,
                            stroke_width: w,
                        });
                    } else {
                        let pts = arc_points_mils(c, r, a.start_angle, a.end_angle);
                        ops.push(Op::Polyline {
                            pts,
                            width: w,
                            color,
                            closed: false,
                            fill: None,
                            fill_opacity: 1.0,
                        });
                    }
                }
                Prim::Pad(pad) => draw_pad(&mut b, &mut ops, pad, bg),
                Prim::Via(v) => {
                    let c = Point {
                        x: v.x as f64 / U,
                        y: v.y as f64 / U,
                    };
                    let r = v.diameter as f64 / U / 2.0;
                    let hr = v.hole_size as f64 / U / 2.0;
                    b.add_circle(c.x, c.y, r);
                    ops.push(Op::Ellipse {
                        center: c,
                        rx: r,
                        ry: r,
                        stroke: PCB_TOP.to_string(),
                        fill: Some(PCB_TOP.to_string()),
                        stroke_width: 0.0,
                    });
                    if hr > 0.0 {
                        ops.push(Op::Ellipse {
                            center: c,
                            rx: hr,
                            ry: hr,
                            stroke: bg.to_string(),
                            fill: Some(bg.to_string()),
                            stroke_width: 0.0,
                        });
                    }
                }
                Prim::Fill(f) => {
                    if !pcb_render_layer(f.layer, courtyard) {
                        continue;
                    }
                    let cx = (f.x1 + f.x2) as f64 / U / 2.0;
                    let cy = (f.y1 + f.y2) as f64 / U / 2.0;
                    let w = (f.x1 - f.x2).abs() as f64 / U;
                    let h = (f.y1 - f.y2).abs() as f64 / U;
                    b.add_circle(cx, cy, (w * w + h * h).sqrt() / 2.0);
                    ops.push(Op::Rect {
                        center: Point { x: cx, y: cy },
                        w: w.max(0.01),
                        h: h.max(0.01),
                        rx: 0.0,
                        rotation: f.rotation,
                        stroke: None,
                        fill: Some(pcb_layer_color(f.layer, courtyard).to_string()),
                    });
                }
                Prim::Region(reg) => {
                    if !reg.is_body && !pcb_render_layer(reg.layer, courtyard) {
                        continue;
                    }
                    let color = if reg.is_body {
                        PCB_BODY_STROKE.to_string()
                    } else {
                        pcb_layer_color(reg.layer, courtyard).to_string()
                    };
                    let fill = if reg.is_body {
                        Some(PCB_BODY_FILL.to_string())
                    } else {
                        Some(color.clone())
                    };
                    let opacity = if reg.is_body { 0.25 } else { 1.0 };
                    let pts = region_points(reg, &mut b);
                    if pts.len() >= 3 {
                        ops.push(Op::Polyline {
                            pts,
                            width: 1.0,
                            color,
                            closed: true,
                            fill,
                            fill_opacity: opacity,
                        });
                    } else if pts.len() == 2 {
                        ops.push(Op::Line {
                            p1: pts[0],
                            p2: pts[1],
                            width: 1.0,
                            color,
                        });
                    }
                }
                Prim::Text(t) => {
                    if t.content.trim().is_empty() {
                        continue;
                    }
                    let size = (t.height as f64 / U).max(2.0);
                    let pos = Point {
                        x: t.x as f64 / U,
                        y: t.y as f64 / U,
                    };
                    let est = t.content.len() as f64 * size * 0.6;
                    b.add(pos.x - est, pos.y - size * 2.0);
                    b.add(pos.x + est, pos.y + size * 2.0);
                    let rot = if t.mirrored { t.rotation } else { -t.rotation };
                    ops.push(Op::Text {
                        pos,
                        size,
                        color: pcb_layer_color(t.layer, courtyard).to_string(),
                        anchor: "middle",
                        content: t.content.clone(),
                        rotate: rot,
                    });
                }
            }
        }
    }

    if !b.valid {
        return wrap_svg(10.0, 10.0, "", bg);
    }
    finish(b.min_x, b.min_y, b.max_x, b.max_y, &ops, bg)
}

fn draw_pad(b: &mut Bounds, ops: &mut Vec<Op>, p: &Pad, bg: &str) {
    const U: f64 = pcblib::UNITS_PER_MIL;
    let top = p.is_through_hole() || p.layer != pcblib::LAYER_BOTTOM;
    let (wi, hi) = p.size_on(top);
    let w = wi as f64 / U;
    let h = hi as f64 / U;
    if w <= 0.0 || h <= 0.0 {
        return;
    }
    let center = Point {
        x: p.x as f64 / U,
        y: p.y as f64 / U,
    };
    let r = (w.max(h)) / 2.0;
    b.add_circle(center.x, center.y, r);

    let color = if top { PCB_TOP } else { PCB_BOTTOM }.to_string();

    match p.shape_on(top) {
        pcblib::PAD_SHAPE_CIRCLE => {
            ops.push(Op::Ellipse {
                center,
                rx: w.min(h) / 2.0,
                ry: w.min(h) / 2.0,
                stroke: color.clone(),
                fill: Some(color),
                stroke_width: 0.0,
            });
        }
        pcblib::PAD_SHAPE_OCTAGONAL => {
            let cut = (w.min(h) * 0.28).min(w / 2.0).min(h / 2.0);
            let hw = w / 2.0;
            let hh = h / 2.0;
            let raw = [
                (-hw + cut, -hh),
                (hw - cut, -hh),
                (hw, -hh + cut),
                (hw, hh - cut),
                (hw - cut, hh),
                (-hw + cut, hh),
                (-hw, hh - cut),
                (-hw, -hh + cut),
            ];
            let pts = raw
                .iter()
                .map(|(dx, dy)| rot_pt(*dx, *dy, -p.rotation))
                .map(|(dx, dy)| Point {
                    x: center.x + dx,
                    y: center.y + dy,
                })
                .collect();
            ops.push(Op::Polyline {
                pts,
                width: 1.0,
                color: color.clone(),
                closed: true,
                fill: Some(color),
                fill_opacity: 1.0,
            });
        }
        pcblib::PAD_SHAPE_ROUNDED_RECTANGLE => {
            let rr = (w.min(h) * 0.25).min(w / 2.0).min(h / 2.0);
            ops.push(Op::Rect {
                center,
                w,
                h,
                rx: rr,
                rotation: p.rotation,
                stroke: None,
                fill: Some(color),
            });
        }
        _ => {
            ops.push(Op::Rect {
                center,
                w,
                h,
                rx: 0.0,
                rotation: p.rotation,
                stroke: None,
                fill: Some(color),
            });
        }
    }

    // Hole knockout.
    if p.is_through_hole() {
        let hr = p.hole_size as f64 / U / 2.0;
        if p.has_slot() {
            let sl = p.slot_size as f64 / U;
            let sw = hr * 2.0;
            let half_len = ((sl - sw) / 2.0).max(0.0) + sw / 2.0;
            ops.push(Op::Rect {
                center: Point {
                    x: center.x + half_len.half_offset(p.slot_rotation),
                    y: center.y,
                },
                w: sl.max(sw),
                h: sw,
                rx: hr,
                rotation: p.slot_rotation,
                stroke: None,
                fill: Some(bg.to_string()),
            });
        } else if hr > 0.0 {
            ops.push(Op::Ellipse {
                center,
                rx: hr,
                ry: hr,
                stroke: bg.to_string(),
                fill: Some(bg.to_string()),
                stroke_width: 0.0,
            });
        }
    }

    // Designator, proportional to the pad and centered on it.
    if !p.designator.is_empty() && w > 12.0 && h > 12.0 {
        // Scale with the smaller pad dimension so the numeral matches the pin.
        let size = (w.min(h) * 0.5).clamp(5.0, 60.0);
        // On a dark background white numerals would be stray white patches, so
        // use the background color (reads as a cut-out on the colored pad).
        let text_color = if bg == "#FFFFFF" { "#FFFFFF" } else { bg };
        // SVG text `y` is the baseline, so shift the logical y up by ~0.35*size
        // (Y is up) to vertically center the glyph on the pad center.
        let pos = Point {
            x: center.x,
            y: center.y - size * 0.35,
        };
        ops.push(Op::Text {
            pos,
            size,
            color: text_color.to_string(),
            anchor: "middle",
            content: p.designator.clone(),
            rotate: 0.0,
        });
    }
}

trait HalfOffset {
    fn half_offset(self, rot: f64) -> f64;
}
impl HalfOffset for f64 {
    fn half_offset(self, rot: f64) -> f64 {
        self * rot.to_radians().cos()
    }
}

fn rot_pt(dx: f64, dy: f64, deg: f64) -> (f64, f64) {
    let a = deg.to_radians();
    (dx * a.cos() - dy * a.sin(), dx * a.sin() + dy * a.cos())
}

fn arc_points_mils(c: Point, r: f64, sa: f64, ea: f64) -> Vec<Point> {
    let mut span = ea - sa;
    while span <= 0.0 {
        span += 360.0;
    }
    let steps = ((span / 8.0).ceil() as usize).clamp(2, 90);
    (0..=steps)
        .map(|i| {
            let a = (sa + span * i as f64 / steps as f64).to_radians();
            Point {
                x: c.x + r * a.cos(),
                y: c.y + r * a.sin(),
            }
        })
        .collect()
}

fn region_points(reg: &pcblib::Region, b: &mut Bounds) -> Vec<Point> {
    const U: f64 = pcblib::UNITS_PER_MIL;
    let scale = |v: f64| v / U;
    let mut pts: Vec<Point> = Vec::new();
    for v in &reg.outline {
        b.add(scale(v.x), scale(v.y));
        pts.push(Point {
            x: scale(v.x),
            y: scale(v.y),
        });
    }
    // Flatten round segments between consecutive vertices.
    if reg.outline.iter().any(|v| v.round) {
        let mut flat: Vec<Point> = Vec::with_capacity(pts.len() * 4);
        for (i, v) in reg.outline.iter().enumerate() {
            if i == 0 {
                flat.push(Point {
                    x: scale(v.x),
                    y: scale(v.y),
                });
                continue;
            }
            if v.round && v.radius > 0.0 {
                let prev = reg.outline[i - 1];
                let c = Point {
                    x: scale(v.cx),
                    y: scale(v.cy),
                };
                let r = scale(v.radius);
                b.add_circle(c.x, c.y, r);
                let a0 = deg_at(scale(prev.x), scale(prev.y), c.x, c.y);
                let a1 = deg_at(scale(v.x), scale(v.y), c.x, c.y);
                // Choose direction consistent with stored sweep.
                let mut sweep = v.end_angle - v.start_angle;
                while sweep < 0.0 {
                    sweep += 360.0;
                }
                let forward_ccw = sweep <= 360.0;
                let mut delta = a1 - a0;
                if forward_ccw {
                    while delta < 0.0 {
                        delta += 360.0;
                    }
                } else {
                    while delta > 0.0 {
                        delta -= 360.0;
                    }
                }
                if delta.abs() > 355.0 || delta.abs() < 1e-6 {
                    flat.push(Point {
                        x: scale(v.x),
                        y: scale(v.y),
                    });
                    continue;
                }
                let steps = ((delta.abs() / 10.0).ceil() as usize).clamp(2, 72);
                for k in 1..=steps {
                    let ang = (a0 + delta * k as f64 / steps as f64).to_radians();
                    flat.push(Point {
                        x: c.x + r * ang.cos(),
                        y: c.y + r * ang.sin(),
                    });
                }
            } else {
                flat.push(Point {
                    x: scale(v.x),
                    y: scale(v.y),
                });
            }
        }
        return flat;
    }
    pts
}

fn deg_at(px: f64, py: f64, cx: f64, cy: f64) -> f64 {
    (py - cy).atan2(px - cx).to_degrees()
}

// ---------------------------------------------------------------------------
// SchLib rendering
// ---------------------------------------------------------------------------

pub fn symbol_svg(sym: &sch::Symbol, font_sizes: &[f64], part: Option<i64>, bg: &str) -> String {
    let mut b = Bounds::default();
    let mut ops: Vec<Op> = Vec::new();

    for prim in &sym.prims {
        if !sch::belongs_to_part(prim.owner_part, part) {
            continue;
        }
        draw_sch_prim(&prim.kind, part, &mut b, &mut ops, font_sizes);
    }

    for pin in &sym.pins {
        if pin.hidden || !sch::belongs_to_part(pin.owner_part_id as i64, part) {
            continue;
        }
        draw_pin(pin, &mut b, &mut ops);
    }

    if !b.valid {
        return wrap_svg(10.0, 10.0, "", bg);
    }

    // On a dark background, schematic ink (default black) would be invisible;
    // substitute a light color so the graphics stay legible.
    if bg != "#FFFFFF" {
        let light = "#E8E8E8".to_string();
        for op in &mut ops {
            match op {
                Op::Line { color, .. } if color == "#000000" => *color = light.clone(),
                Op::Polyline { color, fill, .. } => {
                    if color == "#000000" {
                        *color = light.clone();
                    }
                    if fill.as_deref() == Some("#000000") {
                        *fill = Some(light.clone());
                    }
                }
                Op::Ellipse { stroke, fill, .. } => {
                    if stroke == "#000000" {
                        *stroke = light.clone();
                    }
                    if fill.as_deref() == Some("#000000") {
                        *fill = Some(light.clone());
                    }
                }
                Op::Rect { stroke, fill, .. } => {
                    if stroke.as_deref() == Some("#000000") {
                        *stroke = Some(light.clone());
                    }
                    if fill.as_deref() == Some("#000000") {
                        *fill = Some(light.clone());
                    }
                }
                Op::Text { color, .. } if color == "#000000" => *color = light.clone(),
                _ => {}
            }
        }
    }

    finish(b.min_x, b.min_y, b.max_x, b.max_y, &ops, bg)
}

fn label_font_size(font_sizes: &[f64], font_id: i64) -> f64 {
    let idx = (font_id.max(1) - 1) as usize;
    let pt = font_sizes.get(idx).copied().unwrap_or(10.0);
    // ~0.9 schematic units per point; 1 unit = 10 mil.
    pt * 9.0
}

fn draw_sch_prim(
    kind: &SchPrimKind,
    part: Option<i64>,
    b: &mut Bounds,
    ops: &mut Vec<Op>,
    font_sizes: &[f64],
) {
    let _ = part;
    match kind {
        SchPrimKind::Polyline {
            points,
            width,
            color,
        } => {
            mark_points(b, points, 2.0);
            ops.push(Op::Polyline {
                pts: points.clone(),
                width: line_width(*width),
                color: rgb(*color),
                closed: false,
                fill: None,
                fill_opacity: 1.0,
            });
        }
        SchPrimKind::Polygon {
            points,
            color,
            area_color,
            solid,
        } => {
            mark_points(b, points, 2.0);
            ops.push(Op::Polyline {
                pts: points.clone(),
                width: 1.0,
                color: rgb(*color),
                closed: true,
                fill: solid.then(|| rgb(*area_color)),
                fill_opacity: 1.0,
            });
        }
        SchPrimKind::Ellipse {
            center,
            radius_x,
            radius_y,
            color,
            area_color,
            solid,
        } => {
            b.add_circle(center.x, center.y, radius_x.max(*radius_y));
            ops.push(Op::Ellipse {
                center: *center,
                rx: *radius_x,
                ry: *radius_y,
                stroke: rgb(*color),
                fill: solid.then(|| rgb(*area_color)),
                stroke_width: 10.0,
            });
        }
        SchPrimKind::Pie {
            center,
            radius_x,
            radius_y,
            start_angle,
            end_angle,
            color,
            area_color,
            solid,
        } => {
            b.add_circle(center.x, center.y, radius_x.max(*radius_y));
            // Flatten the pie outline.
            let mut pts = vec![*center];
            let steps = (((end_angle - start_angle).abs() / 10.0).ceil() as usize).clamp(2, 72);
            for i in 0..=steps {
                let a = (*start_angle + (*end_angle - *start_angle) * i as f64 / steps as f64)
                    .to_radians();
                pts.push(Point {
                    x: center.x + radius_x * a.cos(),
                    y: center.y + radius_y * a.sin(),
                });
            }
            ops.push(Op::Polyline {
                pts,
                width: 1.0,
                color: rgb(*color),
                closed: true,
                fill: solid.then(|| rgb(*area_color)),
                fill_opacity: 1.0,
            });
        }
        SchPrimKind::RoundRect {
            p1,
            p2,
            corner,
            color,
            area_color,
            solid,
        } => {
            b.add(p1.x, p1.y);
            b.add(p2.x, p2.y);
            let w = (p1.x - p2.x).abs();
            let h = (p1.y - p2.y).abs();
            // Zero-width/height rects are not rendered by SVG; emit a line.
            if w < 0.01 || h < 0.01 {
                ops.push(Op::Line {
                    p1: *p1,
                    p2: *p2,
                    width: 10.0,
                    color: rgb(*color),
                });
                return;
            }
            let center = Point {
                x: (p1.x + p2.x) / 2.0,
                y: (p1.y + p2.y) / 2.0,
            };
            ops.push(Op::Rect {
                center,
                w,
                h,
                rx: corner.x.min(corner.y).min(w / 2.0).min(h / 2.0),
                rotation: 0.0,
                stroke: Some(rgb(*color)),
                fill: solid.then(|| rgb(*area_color)),
            });
        }
        SchPrimKind::Arc {
            center,
            radius,
            start_angle,
            end_angle,
            width,
            color,
        } => {
            b.add_circle(center.x, center.y, *radius);
            // Sweep is always counter-clockwise from start to end; wrap when
            // the end angle is smaller than the start.
            let mut span = *end_angle - *start_angle;
            while span <= 0.0 {
                span += 360.0;
            }
            let steps = ((span / 10.0).ceil() as usize).clamp(2, 72);
            let pts: Vec<Point> = (0..=steps)
                .map(|i| {
                    let a = (*start_angle + span * i as f64 / steps as f64).to_radians();
                    Point {
                        x: center.x + radius * a.cos(),
                        y: center.y + radius * a.sin(),
                    }
                })
                .collect();
            ops.push(Op::Polyline {
                pts,
                width: line_width(*width),
                color: rgb(*color),
                closed: false,
                fill: None,
                fill_opacity: 1.0,
            });
        }
        SchPrimKind::IeeeMarker { pos, color } => {
            // Placeholder glyph: small circle at the marker location.
            let r = PIN_TEXT_SIZE * 0.45;
            b.add_circle(pos.x, pos.y, r);
            ops.push(Op::Ellipse {
                center: *pos,
                rx: r,
                ry: r,
                stroke: rgb(*color),
                fill: None,
                stroke_width: 10.0,
            });
        }
        SchPrimKind::Image { p1, p2 } => {
            b.add(p1.x, p1.y);
            b.add(p2.x, p2.y);
            let center = Point {
                x: (p1.x + p2.x) / 2.0,
                y: (p1.y + p2.y) / 2.0,
            };
            let w = (p1.x - p2.x).abs();
            let h = (p1.y - p2.y).abs();
            ops.push(Op::Rect {
                center,
                w: w.max(1.0),
                h: h.max(1.0),
                rx: 0.0,
                rotation: 0.0,
                stroke: Some("#888888".into()),
                fill: Some("#F0F0F0".into()),
            });
        }
        SchPrimKind::EllipticalArc {
            center,
            radius_x,
            radius_y,
            start_angle,
            end_angle,
            width,
            color,
        } => {
            b.add_circle(center.x, center.y, radius_x.max(*radius_y));
            // Altium sweeps arcs counter-clockwise (increasing angle, wrapping),
            // not along the shortest signed delta.
            let mut span = *end_angle - *start_angle;
            while span <= 0.0 {
                span += 360.0;
            }
            let steps = ((span / 10.0).ceil() as usize).clamp(2, 72);
            let pts: Vec<Point> = (0..=steps)
                .map(|i| {
                    let a = (*start_angle + span * i as f64 / steps as f64).to_radians();
                    Point {
                        x: center.x + radius_x * a.cos(),
                        y: center.y + radius_y * a.sin(),
                    }
                })
                .collect();
            ops.push(Op::Polyline {
                pts,
                width: line_width(*width),
                color: rgb(*color),
                closed: false,
                fill: None,
                fill_opacity: 1.0,
            });
        }
        SchPrimKind::Bezier {
            points,
            width,
            color,
        } => {
            mark_points(b, points, *width as f64);
            // Sample the cubic chain (points come in groups of control nodes).
            let sampled = sample_bezier(points);
            ops.push(Op::Polyline {
                pts: sampled,
                width: line_width(*width),
                color: rgb(*color),
                closed: false,
                fill: None,
                fill_opacity: 1.0,
            });
        }
        SchPrimKind::Label {
            pos,
            text,
            color,
            font_id,
            orientation,
        } => {
            if text.trim().is_empty() {
                return;
            }
            let size = label_font_size(font_sizes, *font_id);
            let est = text.len() as f64 * size * 0.55;
            b.add_circle(pos.x, pos.y, est.max(size * 2.0));
            let rotate = match orientation {
                1 => 90.0,
                3 => 90.0,
                _ => 0.0,
            };
            ops.push(Op::Text {
                pos: *pos,
                size,
                color: rgb(*color),
                anchor: "middle",
                content: text.clone(),
                rotate,
            });
        }
    }
}

fn sample_bezier(points: &[Point]) -> Vec<Point> {
    // Altium bezier records list control points; render each cubic segment.
    let mut out = Vec::new();
    if points.is_empty() {
        return out;
    }
    if points.len() < 4 {
        return points.to_vec();
    }
    let mut idx = 0usize;
    let mut prev = points[0];
    out.push(prev);
    while idx + 3 < points.len() + 1 && idx + 2 < points.len() {
        let p0 = prev;
        let c1 = points[idx];
        let c2 = points
            .get(idx + 1)
            .cloned()
            .unwrap_or_else(|| *points.last().unwrap());
        let p3 = points
            .get(idx + 2)
            .cloned()
            .unwrap_or_else(|| *points.last().unwrap());
        for k in 1..=12 {
            let t = k as f64 / 12.0;
            out.push(bezier_point(&p0, &c1, &c2, &p3, t));
        }
        prev = p3;
        idx += 3;
    }
    out
}

#[allow(clippy::many_single_char_names)]
fn bezier_point(p0: &Point, c1: &Point, c2: &Point, p3: &Point, t: f64) -> Point {
    let u = 1.0 - t;
    Point {
        x: u * u * u * p0.x + 3.0 * u * u * t * c1.x + 3.0 * u * t * t * c2.x + t * t * t * p3.x,
        y: u * u * u * p0.y + 3.0 * u * u * t * c1.y + 3.0 * u * t * t * c2.y + t * t * t * p3.y,
    }
}

fn line_width(w: u8) -> f64 {
    // Schematic line width codes, expressed in mils (1 code unit = 10 mil).
    match w {
        0 => 5.0,
        1 => 10.0,
        2 => 20.0,
        _ => 30.0,
    }
}

fn mark_points(b: &mut Bounds, points: &[Point], m: f64) {
    for p in points {
        b.add(p.x - m, p.y - m);
        b.add(p.x + m, p.y + m);
    }
}

/// Pin text height in mils (9 schematic units; 1 unit = 10 mil).
const PIN_TEXT_SIZE: f64 = 90.0;

fn draw_pin(pin: &sch::Pin, b: &mut Bounds, ops: &mut Vec<Op>) {
    let (dx, dy) = match pin.orientation {
        0 => (1.0, 0.0),
        1 => (0.0, 1.0),
        2 => (-1.0, 0.0),
        _ => (0.0, -1.0),
    };
    let hot = Point {
        x: pin.x + dx * pin.length,
        y: pin.y + dy * pin.length,
    };
    let body = Point { x: pin.x, y: pin.y };

    b.add(body.x - 2.0, body.y - 2.0);
    b.add(body.x + 2.0, body.y + 2.0);
    b.add(hot.x - 2.0, hot.y - 2.0);
    b.add(hot.x + 2.0, hot.y + 2.0);

    ops.push(Op::Line {
        p1: body,
        p2: hot,
        width: 10.0,
        color: "#000000".into(),
    });

    let horizontal = dy == 0.0;

    // Visibility flags from the pin conglomerate byte.
    if pin.show_name && !pin.name.trim().is_empty() {
        let name = pin.name.trim();
        let est = name.len() as f64 * PIN_TEXT_SIZE * 0.55;
        // Name sits just outside the body edge (inside the component
        // outline), baseline slightly below the pin line so the glyph is
        // visually centered on it.
        let (pos, anchor) = if horizontal {
            let anchor: &'static str = if dx > 0.0 { "end" } else { "start" };
            (
                Point {
                    x: body.x - dx * 45.0,
                    y: pin.y - PIN_TEXT_SIZE * 0.4,
                },
                anchor,
            )
        } else {
            // Vertical pins: text to the left of the line, hugging the body.
            (
                Point {
                    x: pin.x - PIN_TEXT_SIZE * 0.65,
                    y: body.y + dy * 25.0,
                },
                "middle",
            )
        };
        b.add(pos.x - est, pos.y - PIN_TEXT_SIZE * 2.0);
        b.add(pos.x + est, pos.y + PIN_TEXT_SIZE * 2.0);
        ops.push(Op::Text {
            pos,
            size: PIN_TEXT_SIZE,
            color: "#000000".into(),
            anchor,
            content: name.to_string(),
            rotate: 0.0,
        });
    }

    if pin.show_designator && !pin.designator.trim().is_empty() {
        let des = pin.designator.trim();
        let est = des.len() as f64 * PIN_TEXT_SIZE * 0.6;
        // Number above the line, next to the body on the pin side; for
        // vertical pins it goes left of the line near the hot-spot end.
        let (pos, anchor) = if horizontal {
            let anchor: &'static str = if dx > 0.0 { "start" } else { "end" };
            (
                Point {
                    x: body.x + dx * 80.0,
                    y: pin.y + PIN_TEXT_SIZE * 0.12,
                },
                anchor,
            )
        } else {
            (
                Point {
                    x: pin.x - PIN_TEXT_SIZE * 0.65,
                    y: hot.y - dy * 25.0,
                },
                "middle",
            )
        };
        b.add(pos.x - est, pos.y - PIN_TEXT_SIZE * 2.0);
        b.add(pos.x + est, pos.y + PIN_TEXT_SIZE * 2.0);
        ops.push(Op::Text {
            pos,
            size: PIN_TEXT_SIZE,
            color: "#000000".into(),
            anchor,
            content: des.to_string(),
            rotate: 0.0,
        });
    }
}
