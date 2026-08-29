// Hide the console window in release builds; keep it in debug for logging.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use eframe::egui;
use image::GenericImageView;
use serde::Deserialize;
use std::path::{Path, PathBuf};

mod altium;
mod altium_dbl;
mod app;
mod db;
mod render;

#[derive(Deserialize)]
struct ConfigData {
    #[serde(default)]
    db_path: String,
    #[serde(default)]
    dbl_path: String,
}

fn load_icon() -> Option<egui::IconData> {
    let png = include_bytes!("../icon.png");
    let img = image::load_from_memory_with_format(png, image::ImageFormat::Png).ok()?;
    let (width, height) = img.dimensions();
    Some(egui::IconData {
        rgba: img.to_rgba8().into_raw(),
        width,
        height,
    })
}

fn find_file_up(start: &Path, filename: &str, max_depth: usize) -> Option<PathBuf> {
    let mut cur = Some(start.to_path_buf());
    for _ in 0..max_depth {
        let dir = cur?;
        let candidate = dir.join(filename);
        if candidate.is_file() {
            return Some(candidate);
        }
        cur = dir.parent().map(|p| p.to_path_buf());
    }
    None
}

fn find_config_up(start: &Path, max_depth: usize) -> Option<PathBuf> {
    let mut cur = Some(start.to_path_buf());
    for _ in 0..max_depth {
        let dir = cur?;
        if let Ok(entries) = std::fs::read_dir(&dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_file() {
                    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
                    if name.ends_with(".config.json") {
                        return Some(path);
                    }
                }
            }
        }
        cur = dir.parent().map(|p| p.to_path_buf());
    }
    None
}

fn find_sqlite_up(start: &Path, max_depth: usize) -> Option<PathBuf> {
    let mut cur = Some(start.to_path_buf());
    for _ in 0..max_depth {
        let dir = cur?;
        if let Ok(entries) = std::fs::read_dir(&dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_file() && path.extension().map(|e| e == "sqlite").unwrap_or(false) {
                    return Some(path);
                }
            }
        }
        cur = dir.parent().map(|p| p.to_path_buf());
    }
    None
}

fn config_dbl_path(config: &Path) -> Option<PathBuf> {
    let data = std::fs::read_to_string(config).ok()?;
    let cfg = serde_json::from_str::<ConfigData>(&data).ok()?;
    if cfg.dbl_path.is_empty() {
        None
    } else {
        Some(PathBuf::from(cfg.dbl_path))
    }
}

fn resolve_paths() -> (PathBuf, PathBuf, PathBuf) {
    let default_db = PathBuf::from("altiumdb.sqlite");
    let cwd = std::env::current_dir().unwrap_or_default();
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|p| p.to_path_buf()))
        .unwrap_or_default();

    if let Some(arg) = std::env::args().nth(1) {
        let db = PathBuf::from(arg);
        let config = db.with_extension("config.json");
        let dbl = config_dbl_path(&config).unwrap_or_else(|| altium_dbl::dbl_path_for_db(&db));
        return (db, dbl, config);
    }

    if let Some(config) = find_file_up(&cwd, "altiumdb.config.json", 6)
        .or_else(|| find_file_up(&exe_dir, "altiumdb.config.json", 6))
        .or_else(|| find_config_up(&cwd, 6))
        .or_else(|| find_config_up(&exe_dir, 6))
    {
        if let Ok(data) = std::fs::read_to_string(&config) {
            if let Ok(cfg) = serde_json::from_str::<ConfigData>(&data) {
                let base = config.parent().map(|p| p.to_path_buf()).unwrap_or_default();
                let db = if cfg.db_path.is_empty() {
                    let stem = config
                        .file_stem()
                        .map(|s| s.to_string_lossy().to_string())
                        .unwrap_or_default();
                    let stem = stem.strip_suffix(".config").unwrap_or(&stem);
                    base.join(format!("{}.sqlite", stem))
                } else {
                    PathBuf::from(cfg.db_path)
                };
                let dbl = if cfg.dbl_path.is_empty() {
                    altium_dbl::dbl_path_for_db(&db)
                } else {
                    PathBuf::from(cfg.dbl_path)
                };
                return (db, dbl, config);
            }
        }
    }

    let db = find_file_up(&cwd, "altiumdb.sqlite", 6)
        .or_else(|| find_file_up(&exe_dir, "altiumdb.sqlite", 6))
        .or_else(|| find_sqlite_up(&cwd, 6))
        .or_else(|| find_sqlite_up(&exe_dir, 6))
        .unwrap_or(default_db.clone());
    let config = PathBuf::from("altiumdb.config.json");
    let dbl = config_dbl_path(&config).unwrap_or_else(|| altium_dbl::dbl_path_for_db(&db));
    (db, dbl, config)
}

fn main() -> eframe::Result<()> {
    let mut viewport = egui::ViewportBuilder::default()
        .with_inner_size([1200.0, 700.0])
        .with_min_inner_size([700.0, 520.0])
        .with_title("AltiumDB");
    if let Some(icon) = load_icon() {
        viewport = viewport.with_icon(std::sync::Arc::new(icon));
    }
    let options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };

    eframe::run_native(
        "AltiumDB",
        options,
        Box::new(|_cc| {
            let (db_path, dbl_path, config_path) = resolve_paths();
            let conn = db::open_database(&db_path).expect("Failed to open database");
            db::migrate(&conn).expect("Failed to migrate database");
            Ok(Box::new(app::AltiumDbApp::new(
                conn,
                db_path,
                dbl_path,
                config_path,
            )))
        }),
    )
}
