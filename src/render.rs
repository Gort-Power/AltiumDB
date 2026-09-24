use eframe::egui;
use std::process::Command;
use std::sync::{Arc, OnceLock};

const MONKEY_SCRIPT: &str = include_str!("../tools/altium_monkey_render.py");

pub fn render_symbol(
    lib_path: &str,
    name: &str,
    out_svg: &str,
    _bg: &str,
    part_id: Option<u32>,
) -> Result<(), String> {
    render_with_monkey("symbol", lib_path, name, out_svg, part_id)
}

pub fn render_footprint(
    lib_path: &str,
    name: &str,
    out_svg: &str,
    _bg: &str,
) -> Result<(), String> {
    render_with_monkey("footprint", lib_path, name, out_svg, None)
}

pub fn symbol_part_count(lib_path: &str, name: &str) -> Result<u32, String> {
    let output = run_monkey("symbol_parts", lib_path, name, None, None)?;
    output
        .trim()
        .parse()
        .map_err(|e| format!("Invalid symbol section count: {e}"))
}

fn render_with_monkey(
    kind: &str,
    library: &str,
    name: &str,
    output: &str,
    part_id: Option<u32>,
) -> Result<(), String> {
    run_monkey(kind, library, name, part_id, Some(output)).map(|_| ())
}

fn run_monkey(
    kind: &str,
    library: &str,
    name: &str,
    part_id: Option<u32>,
    output: Option<&str>,
) -> Result<String, String> {
    let script = std::env::temp_dir().join("altiumdb_altium_monkey_render.py");
    std::fs::write(&script, MONKEY_SCRIPT)
        .map_err(|e| format!("Failed to prepare Altium Monkey renderer: {e}"))?;
    let mut command = Command::new("python");
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;

        command.creation_flags(0x08000000);
    }
    command.arg(&script).arg(kind).arg(library).arg(name);
    if let Some(part_id) = part_id {
        command.arg(part_id.to_string());
    }
    if let Some(output) = output {
        command.arg(output);
    }
    let result = command
        .output()
        .map_err(|e| format!("Failed to start Altium Monkey (python): {e}"))?;
    if result.status.success() {
        Ok(String::from_utf8_lossy(&result.stdout).to_string())
    } else {
        let error = String::from_utf8_lossy(&result.stderr);
        Err(format!("Altium Monkey rendering failed: {}", error.trim()))
    }
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
    let opt = usvg::Options::<'_> {
        fontdb: loaded_fontdb(),
        ..Default::default()
    };
    let tree = usvg::Tree::from_str(svg, &opt).map_err(|e| e.to_string())?;
    let size = tree.size();
    let scale = (max_w as f32 / size.width()).min(max_h as f32 / size.height());
    let w = (size.width() * scale).round().max(1.0) as u32;
    let h = (size.height() * scale).round().max(1.0) as u32;
    let mut pixmap = tiny_skia::Pixmap::new(w, h).ok_or("Failed to allocate pixmap")?;
    resvg::render(
        &tree,
        tiny_skia::Transform::from_scale(scale, scale),
        &mut pixmap.as_mut(),
    );
    Ok(egui::ColorImage::from_rgba_premultiplied(
        [w as usize, h as usize],
        pixmap.data(),
    ))
}
