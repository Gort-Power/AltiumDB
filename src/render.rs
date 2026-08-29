use eframe::egui;
use std::path::Path;
use std::sync::{Arc, OnceLock};

use crate::altium;

/// Render a symbol from `lib_path` to `out_svg` using the native parser.
/// `bg` is the SVG background color (light theme → "#FFFFFF", dark theme → a dark color).
pub fn render_symbol(lib_path: &str, name: &str, out_svg: &str, bg: &str) -> Result<(), String> {
    let svg = altium::symbol_svg(Path::new(lib_path), name, bg)?;
    write_svg(out_svg, &svg)
}

/// Render a footprint from `lib_path` to `out_svg` using the native parser.
/// `bg` is the SVG background color (light theme → "#FFFFFF", dark theme → a dark color).
pub fn render_footprint(lib_path: &str, name: &str, out_svg: &str, bg: &str) -> Result<(), String> {
    let svg = altium::footprint_svg(Path::new(lib_path), name, bg)?;
    write_svg(out_svg, &svg)
}

fn write_svg(out_svg: &str, svg: &str) -> Result<(), String> {
    std::fs::write(out_svg, svg).map_err(|e| format!("Failed to write preview: {}", e))
}

pub fn temp_preview_path() -> String {
    std::env::temp_dir()
        .join("altiumdb_preview.svg")
        .to_string_lossy()
        .to_string()
}

fn loaded_fontdb() -> Arc<usvg::fontdb::Database> {
    static FONTDB: OnceLock<Arc<usvg::fontdb::Database>> = OnceLock::new();
    FONTDB
        .get_or_init(|| {
            let mut db = usvg::fontdb::Database::new();
            db.load_system_fonts();
            Arc::new(db)
        })
        .clone()
}

pub fn rasterize_svg(svg: &str, max_w: u32, max_h: u32) -> Result<egui::ColorImage, String> {
    let mut opt = usvg::Options::default();
    opt.fontdb = loaded_fontdb();
    let tree = usvg::Tree::from_str(svg, &opt).map_err(|e| e.to_string())?;
    let size = tree.size();
    let sw = size.width();
    let sh = size.height();
    if sw <= 0.0 || sh <= 0.0 || max_w == 0 || max_h == 0 {
        return Err("empty SVG".to_string());
    }
    let scale = (max_w as f32 / sw).min(max_h as f32 / sh);
    let w = (sw * scale).round().max(1.0) as u32;
    let h = (sh * scale).round().max(1.0) as u32;
    let mut pixmap =
        tiny_skia::Pixmap::new(w, h).ok_or_else(|| "Failed to allocate pixmap".to_string())?;
    let transform = tiny_skia::Transform::from_scale(scale, scale);
    resvg::render(&tree, transform, &mut pixmap.as_mut());
    Ok(egui::ColorImage::from_rgba_premultiplied(
        [w as usize, h as usize],
        pixmap.data(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rasterize_simple_svg() {
        let svg = r#"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="80"><rect x="10" y="10" width="50" height="30" fill="red"/></svg>"#;
        let img = rasterize_svg(svg, 100, 80).unwrap();
        assert_eq!(img.size, [100, 80]);
    }
}
