//! Native Altium library parsing and preview rendering.
//!
//! Replaces the external Python helper: reads `.SchLib` / `.PcbLib` OLE
//! containers directly and renders symbols/footprints to SVG.

pub mod pcblib;
pub mod records;
pub mod sch;
pub mod svg;

use std::path::Path;

/// Render a symbol from a `.SchLib` file to an SVG string.
///
/// `name` selects the symbol; when not found, the first symbol is used.
/// Multi-part symbols render part 1 (matching Altium's library display).
pub fn symbol_svg(path: &Path, name: &str, bg: &str) -> Result<String, String> {
    let lib = sch::open(path)?;
    let sym = lib
        .symbols
        .iter()
        .find(|s| s.name == name || s.lib_reference == name)
        .or_else(|| lib.symbols.first())
        .ok_or_else(|| "library has no symbols".to_string())?;
    // Multi-part symbols show their first part only.
    let part = if sym.part_count > 1 { Some(1) } else { None };
    Ok(svg::symbol_svg(sym, &lib.font_sizes, part, bg))
}

/// Render a footprint from a `.PcbLib` file to an SVG string.
///
/// `name` selects the footprint; when not found, the first footprint is used.
/// `bg` is the SVG background color (e.g. "#FFFFFF" for light theme, a dark
/// color for dark theme).
pub fn footprint_svg(path: &Path, name: &str, bg: &str) -> Result<String, String> {
    let lib = pcblib::open(path)?;
    let fp = lib
        .footprints
        .iter()
        .find(|f| f.name == name)
        .or_else(|| lib.footprints.first())
        .ok_or_else(|| "library has no footprints".to_string())?;
    Ok(svg::footprint_svg(fp, &lib.courtyard_layers, bg))
}

/// List symbol names in a `.SchLib`.
#[allow(dead_code)]
pub fn list_symbols(path: &Path) -> Result<Vec<String>, String> {
    let lib = sch::open(path)?;
    Ok(lib.symbols.iter().map(|s| s.name.clone()).collect())
}

/// List footprint names in a `.PcbLib`.
#[allow(dead_code)]
pub fn list_footprints(path: &Path) -> Result<Vec<String>, String> {
    let lib = pcblib::open(path)?;
    Ok(lib.footprints.iter().map(|f| f.name.clone()).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::altium::pcblib::Prim;

    #[test]
    fn parses_sot23_3_reference() {
        let p = Path::new("footprints/SOT23-3.PcbLib");
        if !p.exists() {
            return;
        }
        let lib = pcblib::open(p).unwrap();
        assert_eq!(lib.footprints.len(), 1);
        let fp = &lib.footprints[0];
        let pads = fp.prims.iter().filter(|p| matches!(p, Prim::Pad(_))).count();
        let tracks = fp.prims.iter().filter(|p| matches!(p, Prim::Track(_))).count();
        let arcs = fp.prims.iter().filter(|p| matches!(p, Prim::Arc(_))).count();
        let bodies = fp.prims.iter().filter(|p| matches!(p, Prim::Region(r) if r.is_body)).count();
        assert_eq!((pads, tracks, arcs, bodies), (3, 20, 2, 1));
        // First pad position: (-41.73, 37.4) mil.
        match fp.prims.iter().find(|p| matches!(p, Prim::Pad(_))) {
            Some(Prim::Pad(pad)) => {
                assert!(((pad.x as f64 / pcblib::UNITS_PER_MIL) - -41.73).abs() < 0.05);
                assert!(((pad.y as f64 / pcblib::UNITS_PER_MIL) - 37.40).abs() < 0.05);
            }
            _ => unreachable!(),
        }
    }

    #[test]
    fn matches_python_reference_counts() {
        let Ok(text) = std::fs::read_to_string("target/ref_counts.json") else {
            return; // Reference not generated; skip.
        };
        let Ok(map) = serde_json::from_str::<serde_json::Map<String, serde_json::Value>>(&text) else {
            return;
        };
        let mut failures = Vec::new();
        for (name, expected) in &map {
            let dir = if name.ends_with(".PcbLib") { "footprints" } else { "symbols" };
            let path = Path::new(dir).join(name);

            if name.ends_with(".PcbLib") {
                let want_all: i64 = ["pads", "tracks", "arcs", "texts", "fills", "regions", "bodies", "vias"]
                    .iter()
                    .map(|k| expected[*k].as_i64().unwrap_or(0))
                    .sum();
                if want_all == 0 {
                    continue; // Python failed to parse this file at all.
                }
                let Ok(lib) = pcblib::open(&path) else {
                    failures.push(format!("{name}: open failed"));
                    continue;
                };
                let Some(fp) = lib.footprints.first() else {
                    failures.push(format!("{name}: no footprints"));
                    continue;
                };
                let c = |pred: fn(&Prim) -> bool| {
                    fp.prims.iter().filter(|p| pred(p)).count() as i64
                };
                let got = [
                    ("pads", c(|p| matches!(p, Prim::Pad(_)))),
                    ("tracks", c(|p| matches!(p, Prim::Track(_)))),
                    ("arcs", c(|p| matches!(p, Prim::Arc(_)))),
                    ("texts", c(|p| matches!(p, Prim::Text(_)))),
                    ("fills", c(|p| matches!(p, Prim::Fill(_)))),
                    ("regions", c(|p| matches!(p, Prim::Region(r) if !r.is_body))),
                    ("bodies", c(|p| matches!(p, Prim::Region(r) if r.is_body))),
                    ("vias", c(|p| matches!(p, Prim::Via(_)))),
                ];
                for (key, got_n) in got {
                    let want = expected[key].as_i64().unwrap_or(0);
                    if got_n != want {
                        failures.push(format!("{name}: {key} got {got_n}, want {want}"));
                    }
                }
            } else {
                let Ok(lib) = sch::open(&path) else {
                    failures.push(format!("{name}: open failed"));
                    continue;
                };
                let Some(sym) = lib.symbols.first() else {
                    failures.push(format!("{name}: no symbols"));
                    continue;
                };
                let want_pins = expected["pins"].as_i64().unwrap_or(0);
                if (sym.pins.len() as i64) != want_pins {
                    failures.push(format!("{name}: pins got {}, want {want_pins}", sym.pins.len()));
                }
                let want_gfx = expected["graphics"].as_i64().unwrap_or(0);
                if (sym.prims.len() as i64) < want_gfx {
                    failures.push(format!("{name}: graphics got {}, want >= {want_gfx}", sym.prims.len()));
                }
            }
        }
        assert!(failures.is_empty(), "{} mismatches:\n{}", failures.len(), failures.join("\n"));
    }

    #[test]
    fn renders_sample_previews() {
        // Renders sample previews to target/preview for manual inspection.
        let out_dir = std::path::Path::new("target/preview");
        let _ = std::fs::create_dir_all(out_dir);
        let samples: &[&str] = &[
            "symbols/0402 RES_1.Schlib",
            "symbols/LM5023.Schlib",
            "symbols/3224W1103E.Schlib",
            "symbols/6TCE470MI.Schlib",
            "symbols/XAL8080.Schlib",
            "symbols/DA2025.Schlib",
            "symbols/XFCN T44001.Schlib",
            "symbols/MOSFET-N BSS138.Schlib",
            "symbols/LM5039.Schlib",
            "symbols/BAT54A.Schlib",
            "symbols/FODM8801A.Schlib",
            "footprints/SOT23-3.PcbLib",
            "footprints/SOIC127P599X175-8N.PcbLib",
            "footprints/QFN50P500X500X80-29N.PcbLib",
            "footprints/Planar ER23.PcbLib",
            "footprints/TO-277.PcbLib",
        ];
        for file in samples {
            let path = std::path::Path::new(file);
            if !path.exists() {
                continue;
            }
            let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else { continue };
            let svg = if file.ends_with("Schlib") {
                symbol_svg(path, "", "#FFFFFF")
            } else {
                footprint_svg(path, "", "#FFFFFF")
            };
            let Ok(svg) = svg else { continue };
            let _ = std::fs::write(out_dir.join(format!("{stem}.svg")), &svg);
            let img = crate::render::rasterize_svg(&svg, 640, 480).unwrap();
            let mut rgba = img.clone();
            for px in rgba.pixels.iter_mut() {
                *px = eframe::egui::Color32::from_rgba_unmultiplied(px.r(), px.g(), px.b(), 255);
            }
            let png_path = out_dir.join(format!("{stem}.png"));
            let raw: Vec<u8> = rgba
                .pixels
                .iter()
                .flat_map(|p| [p.r(), p.g(), p.b(), p.a()])
                .collect();
            image::save_buffer(
                &png_path,
                &raw,
                rgba.size[0] as u32,
                rgba.size[1] as u32,
                image::ColorType::Rgba8,
            )
            .unwrap();
        }

        // Rasterize Python-reference SVGs (target/preview_py) if present.
        if let Ok(entries) = std::fs::read_dir("target/preview_py") {
            for e in entries.flatten() {
                let p = e.path();
                if p.extension().and_then(|x| x.to_str()) != Some("svg") {
                    continue;
                }
                let Ok(svg) = std::fs::read_to_string(&p) else { continue };
                let Ok(img) = crate::render::rasterize_svg(&svg, 640, 480) else { continue };
                let mut rgba = img.clone();
                for px in rgba.pixels.iter_mut() {
                    *px = eframe::egui::Color32::from_rgba_unmultiplied(px.r(), px.g(), px.b(), 255);
                }
                let stem = p.file_stem().unwrap().to_string_lossy().to_string();
                let png_path = out_dir.join(format!("{stem}_py.png"));
                let raw: Vec<u8> = rgba
                    .pixels
                    .iter()
                    .flat_map(|p| [p.r(), p.g(), p.b(), p.a()])
                    .collect();
                let _ = image::save_buffer(
                    &png_path,
                    &raw,
                    rgba.size[0] as u32,
                    rgba.size[1] as u32,
                    image::ColorType::Rgba8,
                );
            }
        }
    }

    #[test]
    fn dark_theme_uses_dark_background_and_light_ink() {
        let sym = std::path::Path::new("symbols/0402 RES_1.Schlib");
        if sym.exists() {
            let svg = symbol_svg(sym, "", "#1E1E1E").unwrap();
            assert!(svg.contains("fill=\"#1E1E1E\""), "dark background expected");
            assert!(svg.contains("#E8E8E8"), "black ink should become light on dark bg");
            assert!(!svg.contains("#000000"), "no black ink should remain on dark bg");
        }
        let fp = std::path::Path::new("footprints/Planar ER23.PcbLib");
        if fp.exists() {
            let svg = footprint_svg(fp, "", "#1E1E1E").unwrap();
            assert!(svg.contains("fill=\"#1E1E1E\""), "dark background expected");
        }
    }

    #[test]
    fn light_theme_uses_white_background() {
        let sym = std::path::Path::new("symbols/0402 RES_1.Schlib");
        if sym.exists() {
            let svg = symbol_svg(sym, "", "#FFFFFF").unwrap();
            assert!(svg.contains("fill=\"#FFFFFF\""), "white background expected");
        }
    }

    #[test]
    fn renders_all_test_libraries() {
        for dir in ["symbols", "footprints"] {
            let Ok(entries) = std::fs::read_dir(dir) else { continue };
            for e in entries.flatten() {
                let path = e.path();
                let ext = path.extension().and_then(|x| x.to_str()).unwrap_or("");
                if !ext.eq_ignore_ascii_case("schlib") && !ext.eq_ignore_ascii_case("pcblib") {
                    continue;
                }
                if ext.eq_ignore_ascii_case("schlib") {
                    let svg = symbol_svg(&path, "", "#FFFFFF");
                    assert!(svg.is_ok(), "symbol {}: {svg:?}", path.display());
                } else {
                    let svg = footprint_svg(&path, "", "#FFFFFF");
                    assert!(svg.is_ok(), "footprint {}: {svg:?}", path.display());
                }
            }
        }
    }

    #[test]
    fn probe_dark_footprint_holes() {
        for name in ["TO-277", "Planar ER23", "ONSC-WDFN-8-511AB_V"] {
            let path_str = format!("footprints/{name}.PcbLib");
            let p = std::path::Path::new(&path_str);
            if !p.exists() { continue; }
            let svg = footprint_svg(p, "", "#1E1E1E").unwrap();
            std::fs::write(format!("target/probe_{name}_dark.svg"), &svg).unwrap();
            // White is allowed only for small pad designator <text>; filled
            // shapes (holes) must follow the dark background.
            let bad = svg.contains("<rect fill=\"#FFFFFF\"")
                || svg.contains("<ellipse fill=\"#FFFFFF\"")
                || svg.contains("<path fill=\"#FFFFFF\"");
            assert!(!bad, "{name} has white filled shape on dark bg");
        }
    }
}
