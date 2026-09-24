// Hide the console window in release builds; keep it in debug for logging.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use eframe::egui;
use image::GenericImageView;
use serde::Deserialize;
use std::path::{Path, PathBuf};

fn configure_unicode_fonts(ctx: &egui::Context) {
    let candidates = [
        r"C:\Windows\Fonts\segoeui.ttf",
        r"C:\Windows\Fonts\arial.ttf",
        r"C:\Windows\Fonts\seguisym.ttf",
    ];
    let mut fonts = egui::FontDefinitions::default();
    let mut loaded = Vec::new();
    for (index, path) in candidates.iter().enumerate() {
        if let Ok(data) = std::fs::read(path) {
            let name = format!("windows_unicode_{index}");
            fonts
                .font_data
                .insert(name.clone(), egui::FontData::from_owned(data).into());
            loaded.push(name);
        }
    }
    if loaded.is_empty() {
        return;
    }
    for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
        if let Some(family_fonts) = fonts.families.get_mut(&family) {
            for name in loaded.iter().rev() {
                family_fonts.insert(0, name.clone());
            }
        }
    }
    ctx.set_fonts(fonts);
}

mod app;
mod db;
mod render;
mod update;

#[derive(Deserialize)]
struct ConfigData {
    #[serde(default)]
    db_path: String,
    #[serde(default)]
    database_type: String,
    #[serde(default)]
    dsn: String,
    #[serde(default)]
    pg_host: String,
    #[serde(default)]
    pg_port: String,
    #[serde(default)]
    pg_database: String,
    #[serde(default)]
    pg_user: String,
    #[serde(default)]
    pg_password: String,
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

fn resolve_paths() -> (PathBuf, PathBuf, String, String) {
    let default_db = PathBuf::from("altiumdb.sqlite");
    let cwd = std::env::current_dir().unwrap_or_default();
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|p| p.to_path_buf()))
        .unwrap_or_default();

    if let Some(arg) = std::env::args().nth(1) {
        let db = PathBuf::from(arg);
        let config = db.with_extension("config.json");
        return (db, config, "sqlite".to_string(), String::new());
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
                return (
                    db,
                    config,
                    if cfg.database_type.is_empty() {
                        "sqlite".to_string()
                    } else {
                        cfg.database_type
                    },
                    if cfg.pg_host.is_empty()
                        && cfg.pg_port.is_empty()
                        && cfg.pg_database.is_empty()
                        && cfg.pg_user.is_empty()
                        && cfg.pg_password.is_empty()
                    {
                        cfg.dsn
                    } else {
                        db::postgres_connection_string(
                            if cfg.pg_host.is_empty() {
                                "localhost"
                            } else {
                                &cfg.pg_host
                            },
                            if cfg.pg_port.is_empty() {
                                "5432"
                            } else {
                                &cfg.pg_port
                            },
                            &cfg.pg_database,
                            &cfg.pg_user,
                            &cfg.pg_password,
                        )
                    },
                );
            }
        }
    }

    let db = find_file_up(&cwd, "altiumdb.sqlite", 6)
        .or_else(|| find_file_up(&exe_dir, "altiumdb.sqlite", 6))
        .or_else(|| find_sqlite_up(&cwd, 6))
        .or_else(|| find_sqlite_up(&exe_dir, 6))
        .unwrap_or(default_db.clone());
    let config = PathBuf::from("altiumdb.config.json");
    (db, config, "sqlite".to_string(), String::new())
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
            configure_unicode_fonts(&_cc.egui_ctx);
            let (db_path, config_path, database_type, dsn) = resolve_paths();
            let connection_string = if database_type.eq_ignore_ascii_case("sqlite") {
                db_path.display().to_string()
            } else {
                dsn
            };
            let conn = db::open_database_with_config(&database_type, &connection_string)
                .expect("Failed to open database");
            db::migrate(&conn).expect("Failed to migrate database");
            Ok(Box::new(app::AltiumDbApp::new(conn, db_path, config_path)))
        }),
    )
}
