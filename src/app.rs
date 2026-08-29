use crate::altium_dbl;
use crate::db;
use crate::render;
use calamine::Reader as _;
use eframe::egui;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Clone, Copy, PartialEq, Serialize, Deserialize, Default)]
enum Theme {
    #[default]
    System,
    Light,
    Dark,
}

/// Background color used for rendered previews when the dark theme is active.
const DARK_BG: &str = "#1E1E1E";
/// Background color used for rendered previews when the light theme is active.
const LIGHT_BG: &str = "#FFFFFF";
/// Sentinel category entry that triggers a database-wide MPN search.
const ALL_CATEGORIES: &str = "All categories";

#[derive(Clone, Copy, PartialEq, Eq, Default)]
enum AppMode {
    #[default]
    Fill,
    Search,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum BrowseTarget {
    Symbols,
    Footprint1,
    Footprint2,
    Footprint3,
}

impl BrowseTarget {
    fn is_symbols(self) -> bool {
        self == BrowseTarget::Symbols
    }
}

#[derive(Clone)]
struct SearchParam {
    column: String,
    name: String,
    values: Vec<String>,
    selected: Vec<String>,
}

#[derive(Serialize, Deserialize, Default)]
struct ConfigData {
    theme: Theme,
    db_path: String,
    dbl_path: String,
    #[serde(default)]
    dsn: String,
    #[serde(default)]
    symbols_folder: String,
    #[serde(default)]
    footprints_folder: String,
}

#[derive(Clone, Debug)]
struct CategoryInfo {
    name: String,
    #[allow(dead_code)]
    fields: Vec<altium_dbl::Field>,
}

pub struct AltiumDbApp {
    conn: Connection,
    db_path: PathBuf,
    dbl_path: PathBuf,
    config_path: PathBuf,
    dbl: altium_dbl::AltiumDbl,

    categories: Vec<CategoryInfo>,
    components: Vec<db::Component>,
    custom_columns: Vec<String>,
    custom_values: Vec<(String, String)>,

    selected_category: Option<String>,
    selected_component_id: Option<String>,

    category_input: String,
    component_input: String,
    mpn_input: String,
    manufacturer_input: String,
    verified_input: bool,
    library_ref_input: String,
    footprint_ref_input: String,
    library_path_input: String,
    footprint_path_input: String,
    footprint_ref2_input: String,
    footprint_path2_input: String,
    footprint_ref3_input: String,
    footprint_path3_input: String,
    description_input: String,
    component_link1_description_input: String,
    component_link1_url_input: String,
    component_link2_description_input: String,
    component_link2_url_input: String,
    component_link3_description_input: String,
    component_link3_url_input: String,
    field_col_input: String,

    editing_category: Option<String>,
    editing_component: Option<String>,
    editing_field: Option<String>,
    fields_editor_open: bool,

    status_msg: String,

    viewer_open: bool,
    viewer_open_at: std::time::Instant,
    viewer_texture: Option<egui::TextureHandle>,
    viewer_svg: Option<String>,
    viewer_raster_size: egui::Vec2,
    viewer_title: String,

    browse_open: bool,
    browse_open_at: std::time::Instant,
    browse_target: BrowseTarget,
    browse_path: PathBuf,
    browse_entries: Vec<(String, bool)>,
    browse_selected: Option<String>,
    browse_texture: Option<egui::TextureHandle>,
    browse_svg: Option<String>,
    browse_raster_size: egui::Vec2,

    theme: Theme,
    settings_db_path: String,
    settings_dbl_path: String,
    settings_dsn: String,
    settings_symbols_folder: String,
    settings_footprints_folder: String,
    settings_open: bool,
    about_open: bool,
    viewport_adapted: bool,

    mode: AppMode,
    search_category: Option<String>,
    search_all: bool,
    search_all_query: String,
    search_params: Vec<SearchParam>,
    search_results: Vec<(String, db::Component)>,
    search_selected: Option<String>,
}

fn unique_name(base: &str, exists: impl Fn(&str) -> bool) -> String {
    if !exists(base) {
        return base.to_string();
    }
    let mut i = 1;
    loop {
        let candidate = format!("{}_{}", base, i);
        if !exists(&candidate) {
            return candidate;
        }
        i += 1;
    }
}

fn button_width(ui: &egui::Ui, text: &str) -> f32 {
    let pad = ui.spacing().button_padding.x * 2.0;
    let font = egui::FontId::proportional(ui.text_style_height(&egui::TextStyle::Button));
    let text_w: f32 = text
        .chars()
        .map(|c| ui.fonts(|f| f.glyph_width(&font, c)))
        .sum();
    text_w + pad + 2.0
}

fn stretch_width(ui: &egui::Ui, row: (f32, f32), extra: f32) -> f32 {
    (row.0 + row.1 - ui.cursor().min.x - extra).max(80.0)
}

fn text_width(ctx: &egui::Context, text: &str) -> f32 {
    let font = egui::TextStyle::Body.resolve(&ctx.style());
    ctx.fonts(|f| text.chars().map(|c| f.glyph_width(&font, c)).sum::<f32>())
}

fn panel_extra_width(ctx: &egui::Context) -> f32 {
    let s = &ctx.style().spacing;
    s.item_spacing.x + s.scroll.bar_width + 20.0
}

fn relative_library_path(folder: &str, file: &str) -> String {
    let folder = folder.trim();
    let file = file.trim();
    let full = std::path::Path::new(file);
    if !folder.is_empty() {
        if full.strip_prefix(folder).is_ok() {
            if let Some(name) = full.file_name() {
                return name.to_string_lossy().to_string();
            }
        }
    }
    file.to_string()
}

fn find_file_in_dir(folder: &str, name: &str) -> Option<String> {
    let folder = folder.trim();
    if folder.is_empty() {
        return None;
    }
    let mut stack = vec![std::path::PathBuf::from(folder)];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let p = entry.path();
            if p.is_dir() {
                stack.push(p);
            } else if p.file_name().map(|n| n == name).unwrap_or(false) {
                return Some(p.to_string_lossy().to_string());
            }
        }
    }
    None
}

fn resolve_library_path(folder: &str, relative: &str) -> String {
    let folder = folder.trim();
    let relative = relative.trim();
    if relative.is_empty() {
        return String::new();
    }
    let rel_path = std::path::Path::new(relative);
    if rel_path.is_absolute() {
        return relative.to_string();
    }
    // Backward-compatible: values that still carry a subdirectory component are
    // resolved by a simple join.
    if relative.contains('\\') || relative.contains('/') {
        let trimmed = relative.trim_start_matches(['\\', '/']);
        if folder.is_empty() {
            return trimmed.to_string();
        }
        return std::path::Path::new(folder)
            .join(trimmed)
            .to_string_lossy()
            .to_string();
    }
    // Bare library file name: search the configured folder recursively.
    if let Some(found) = find_file_in_dir(folder, relative) {
        return found;
    }
    if folder.is_empty() {
        return relative.to_string();
    }
    std::path::Path::new(folder)
        .join(relative)
        .to_string_lossy()
        .to_string()
}

fn ensure_svg_texture(
    ctx: &egui::Context,
    svg: &str,
    rect: egui::Rect,
    slot: &mut Option<egui::TextureHandle>,
    last_size: &mut egui::Vec2,
    name: &str,
) -> Result<(), String> {
    let ppp = ctx.pixels_per_point();
    let phys = egui::vec2((rect.width() * ppp).round(), (rect.height() * ppp).round());
    if phys.x >= 1.0 && phys.y >= 1.0 && (*last_size != phys || slot.is_none()) {
        let img = render::rasterize_svg(svg, phys.x as u32, phys.y as u32)?;
        *slot = Some(ctx.load_texture(name, img, egui::TextureOptions::LINEAR));
        *last_size = phys;
    }
    Ok(())
}

/// Canvas background matching the active theme, so the area around a
/// fitted preview image blends with the app instead of showing white.
fn preview_bg_color32(ctx: &egui::Context) -> egui::Color32 {
    if ctx.theme() == egui::Theme::Dark {
        egui::Color32::from_hex(DARK_BG).unwrap_or(egui::Color32::from_rgb(30, 30, 30))
    } else {
        egui::Color32::WHITE
    }
}

fn draw_texture_fitted(ui: &egui::Ui, canvas_rect: egui::Rect, tex: &egui::TextureHandle) {
    let painter = ui.painter().with_clip_rect(canvas_rect);
    painter.rect_filled(canvas_rect, 0.0, preview_bg_color32(ui.ctx()));
    let disp = tex.size_vec2() / ui.ctx().pixels_per_point();
    let min = egui::pos2(
        canvas_rect.center().x - disp.x / 2.0,
        canvas_rect.center().y - disp.y / 2.0,
    );
    painter.image(
        tex.id(),
        egui::Rect::from_min_size(min, disp),
        egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
        egui::Color32::WHITE,
    );
}

const UPDATE_MODES: [(u8, &str); 3] = [(0, "Default"), (1, "Do not update"), (2, "Update")];
const ADD_MODES: [(u8, &str); 4] = [
    (0, "Default"),
    (1, "Do not add"),
    (2, "Add"),
    (3, "Add only if not blank in database"),
];
const REMOVE_MODES: [(u8, &str); 3] = [
    (0, "Default"),
    (1, "Do not remove"),
    (2, "Remove only if blank in database"),
];

fn mode_label(modes: &[(u8, &'static str)], v: u8) -> &'static str {
    modes
        .iter()
        .find(|(m, _)| *m == v)
        .map(|(_, l)| *l)
        .unwrap_or("Default")
}

impl AltiumDbApp {
    pub fn new(
        conn: Connection,
        db_path: PathBuf,
        dbl_path: PathBuf,
        config_path: PathBuf,
    ) -> Self {
        let cfg = Self::load_config_data(&config_path);
        let config_dsn = cfg.dsn;
        let theme = cfg.theme;
        let settings_symbols_folder = cfg.symbols_folder;
        let settings_footprints_folder = cfg.footprints_folder;

        let mut dbl = if dbl_path.exists() {
            altium_dbl::AltiumDbl::load(&dbl_path, &db_path, &config_dsn)
                .unwrap_or_else(|_| altium_dbl::AltiumDbl::new(&db_path))
        } else {
            let mut d = altium_dbl::AltiumDbl::new(&db_path);
            if !config_dsn.trim().is_empty() {
                d.set_dsn(config_dsn.trim());
            }
            d
        };
        dbl.ensure_base_fields();

        let mut app = Self {
            conn,
            db_path: db_path.clone(),
            dbl_path: dbl_path.clone(),
            config_path: config_path.clone(),
            dbl,
            categories: Vec::new(),
            components: Vec::new(),
            custom_columns: Vec::new(),
            custom_values: Vec::new(),
            selected_category: None,
            selected_component_id: None,
            category_input: String::new(),
            component_input: String::new(),
            mpn_input: String::new(),
            manufacturer_input: String::new(),
            verified_input: false,
            library_ref_input: String::new(),
            footprint_ref_input: String::new(),
            library_path_input: String::new(),
            footprint_path_input: String::new(),
            footprint_ref2_input: String::new(),
            footprint_path2_input: String::new(),
            footprint_ref3_input: String::new(),
            footprint_path3_input: String::new(),
            description_input: String::new(),
            component_link1_description_input: String::new(),
            component_link1_url_input: String::new(),
            component_link2_description_input: String::new(),
            component_link2_url_input: String::new(),
            component_link3_description_input: String::new(),
            component_link3_url_input: String::new(),
            field_col_input: String::new(),
            editing_category: None,
            editing_component: None,
            editing_field: None,
            fields_editor_open: false,
            status_msg: String::new(),
            viewer_open: false,
            viewer_open_at: std::time::Instant::now(),
            viewer_texture: None,
            viewer_svg: None,
            viewer_raster_size: egui::Vec2::ZERO,
            viewer_title: String::new(),
            browse_open: false,
            browse_open_at: std::time::Instant::now(),
            browse_target: BrowseTarget::Symbols,
            browse_path: PathBuf::new(),
            browse_entries: Vec::new(),
            browse_selected: None,
            browse_texture: None,
            browse_svg: None,
            browse_raster_size: egui::Vec2::ZERO,
            theme,
            settings_db_path: db_path.display().to_string(),
            settings_dbl_path: dbl_path.display().to_string(),
            settings_dsn: config_dsn,
            settings_symbols_folder,
            settings_footprints_folder,
            settings_open: false,
            about_open: false,
            viewport_adapted: false,
            mode: AppMode::default(),
            search_category: None,
            search_all: false,
            search_all_query: String::new(),
            search_params: Vec::new(),
            search_results: Vec::new(),
            search_selected: None,
        };
        app.sync_dbl_fields_with_db();
        app.sync_libraries_with_db();
        app.refresh_categories();
        app.save_dbl();
        app
    }

    fn refresh_categories(&mut self) {
        self.categories = self
            .dbl
            .libraries
            .iter()
            .map(|lib| CategoryInfo {
                name: lib.name.clone(),
                fields: lib.fields.clone(),
            })
            .collect();
    }

    fn refresh_components(&mut self) {
        if let Some(ref cat) = self.selected_category {
            db::ensure_table(&self.conn, cat).ok();
            self.components = db::get_components(&self.conn, cat).unwrap_or_default();
            self.custom_columns = db::get_columns(&self.conn, cat)
                .unwrap_or_default()
                .into_iter()
                .filter(|c| c != "id" && !db::BASE_COLUMNS.contains(&c.as_str()))
                .collect();
        } else {
            self.components.clear();
            self.custom_columns.clear();
        }
    }

    fn refresh_custom_values(&mut self) {
        self.custom_values.clear();
        if let (Some(ref cat), Some(comp_id)) =
            (&self.selected_category, self.selected_component_id.clone())
        {
            for col in &self.custom_columns {
                let val = db::get_custom_value(&self.conn, cat, &comp_id, col).unwrap_or_default();
                self.custom_values.push((col.clone(), val));
            }
        }
    }

    fn search_param_columns(&self, cat: &str) -> Vec<(String, String)> {
        let excluded = ["id", "MPN", "Verified"];
        let cols = db::get_columns(&self.conn, cat).unwrap_or_default();
        cols.into_iter()
            .filter(|c| !excluded.contains(&c.as_str()))
            .map(|c| {
                let name = self
                    .dbl
                    .find_library(cat)
                    .and_then(|l| l.fields.iter().find(|f| f.column == c))
                    .map(|f| f.column.clone())
                    .unwrap_or_else(|| c.clone());
                (c, name)
            })
            .collect()
    }

    fn selected_search_filters(&self) -> Vec<(String, Vec<String>)> {
        self.search_params
            .iter()
            .filter(|p| !p.selected.is_empty())
            .map(|p| (p.column.clone(), p.selected.clone()))
            .collect()
    }

    fn refresh_search(&mut self) {
        if let Some(ref cat) = self.search_category {
            self.search_results =
                db::search_components(&self.conn, cat, &self.selected_search_filters())
                    .unwrap_or_default()
                    .into_iter()
                    .map(|c| (cat.clone(), c))
                    .collect();
        } else if self.search_all {
            let q = self.search_all_query.trim();
            if q.is_empty() {
                self.search_results.clear();
            } else {
                self.search_results = db::search_all_by_mpn(&self.conn, q).unwrap_or_default();
            }
        } else {
            self.search_results.clear();
        }
        self.search_selected = None;
    }

    fn init_search(&mut self) {
        if self.search_all {
            self.search_params.clear();
            self.refresh_search();
            return;
        }
        if let Some(ref cat) = self.search_category {
            self.search_params = self
                .search_param_columns(cat)
                .into_iter()
                .map(|(column, name)| SearchParam {
                    values: db::get_distinct_values(&self.conn, cat, &column).unwrap_or_default(),
                    column,
                    name,
                    selected: Vec::new(),
                })
                .collect();
        } else {
            self.search_params.clear();
        }
        self.refresh_search();
    }

    fn search_detail_value(&self, cat: &str, comp: &db::Component, column: &str) -> String {
        let base = [
            "Manufacturer",
            "Library Ref",
            "Library Path",
            "Footprint Ref",
            "Footprint Path",
            "Footprint Ref 2",
            "Footprint Path 2",
            "Footprint Ref 3",
            "Footprint Path 3",
            "Description",
            "ComponentLink1Description",
            "ComponentLink1URL",
            "ComponentLink2Description",
            "ComponentLink2URL",
            "ComponentLink3Description",
            "ComponentLink3URL",
        ];
        if base.contains(&column) {
            match column {
                "Manufacturer" => comp.manufacturer.clone(),
                "Library Ref" => comp.library_ref.clone(),
                "Library Path" => comp.library_path.clone(),
                "Footprint Ref" => comp.footprint_ref.clone(),
                "Footprint Path" => comp.footprint_path.clone(),
                "Footprint Ref 2" => comp.footprint_ref2.clone(),
                "Footprint Path 2" => comp.footprint_path2.clone(),
                "Footprint Ref 3" => comp.footprint_ref3.clone(),
                "Footprint Path 3" => comp.footprint_path3.clone(),
                "Description" => comp.description.clone(),
                "ComponentLink1Description" => comp.component_link1_description.clone(),
                "ComponentLink1URL" => comp.component_link1_url.clone(),
                "ComponentLink2Description" => comp.component_link2_description.clone(),
                "ComponentLink2URL" => comp.component_link2_url.clone(),
                "ComponentLink3Description" => comp.component_link3_description.clone(),
                "ComponentLink3URL" => comp.component_link3_url.clone(),
                _ => String::new(),
            }
        } else {
            db::get_custom_value(&self.conn, cat, &comp.id, column).unwrap_or_default()
        }
    }

    fn sync_dbl_fields_with_db(&mut self) {
        for lib in &mut self.dbl.libraries {
            let Ok(cols) = db::get_columns(&self.conn, &lib.table) else {
                continue;
            };
            lib.fields.retain(|f| cols.contains(&f.column));
            for col in cols {
                if col == "id" || db::BASE_COLUMNS.contains(&col.as_str()) {
                    continue;
                }
                if !lib.fields.iter().any(|f| f.column == col) {
                    lib.fields.push(altium_dbl::Field {
                        column: col.clone(),
                        parameter: col,
                        is_key: false,
                        visible_on_add: true,
                        add_mode: 0,
                        remove_mode: 0,
                        update_mode: 0,
                    });
                }
            }
        }
    }

    fn sync_libraries_with_db(&mut self) {
        if let Ok(tables) = db::get_tables(&self.conn) {
            for table in tables {
                if self.dbl.find_library(&table).is_none() {
                    self.dbl.add_library(altium_dbl::create_library(&table));
                }
            }
        }
    }

    fn reopen_database(&mut self) {
        let db_path = PathBuf::from(&self.settings_db_path);
        let dbl_path = PathBuf::from(&self.settings_dbl_path);
        let dsn = self.settings_dsn.trim().to_string();
        match db::open_database(&db_path) {
            Ok(conn) => {
                db::migrate(&conn).ok();
                let mut dbl = if dbl_path.exists() {
                    altium_dbl::AltiumDbl::load(&dbl_path, &db_path, &dsn)
                        .unwrap_or_else(|_| altium_dbl::AltiumDbl::new(&db_path))
                } else {
                    altium_dbl::AltiumDbl::new(&db_path)
                };
                if !dsn.is_empty() {
                    dbl.set_dsn(&dsn);
                }
                dbl.ensure_base_fields();
                self.conn = conn;
                self.db_path = db_path;
                self.dbl_path = dbl_path;
                self.dbl = dbl;
                self.selected_category = None;
                self.selected_component_id = None;
                self.components.clear();
                self.custom_columns.clear();
                self.custom_values.clear();
                self.search_category = None;
                self.search_params.clear();
                self.search_results.clear();
                self.search_selected = None;
                self.sync_dbl_fields_with_db();
                self.sync_libraries_with_db();
                self.refresh_categories();
                self.save_dbl();
                self.set_status_ok();
            }
            Err(e) => self.set_status_err(e),
        }
    }

    fn import_lcsc_xls(&mut self, path: &std::path::Path) {
        let mut wb = match calamine::open_workbook_auto(path) {
            Ok(wb) => wb,
            Err(e) => {
                self.set_status_err(format!("Failed to open XLS: {}", e));
                return;
            }
        };
        let range = match wb.worksheet_range_at(0) {
            Some(Ok(r)) => r,
            Some(Err(e)) => {
                self.set_status_err(format!("Failed to read sheet: {}", e));
                return;
            }
            None => {
                self.set_status_err("Workbook has no sheets");
                return;
            }
        };

        let mut rows = range.rows();
        let headers: Vec<String> = rows
            .next()
            .map(|row| row.iter().map(Self::cell_to_string).collect())
            .unwrap_or_default();

        let mut imported = 0usize;
        let mut skipped_no_table = 0usize;
        let mut skipped_dup = 0usize;
        let mut skipped_no_id = 0usize;

        for row in rows {
            let mut values: std::collections::HashMap<String, String> =
                std::collections::HashMap::new();
            for (i, h) in headers.iter().enumerate() {
                let header = h.trim().to_string();
                if header.is_empty() {
                    continue;
                }
                if let Some(cell) = row.get(i) {
                    let val = Self::cell_to_string(cell);
                    if !val.is_empty() {
                        values.insert(header, val);
                    }
                }
            }

            let item_id = values
                .get("MPN")
                .map(|v| v.trim().to_string())
                .unwrap_or_default();
            if item_id.is_empty() {
                skipped_no_id += 1;
                continue;
            }
            let category = values
                .get("Category")
                .map(|v| v.trim().to_string())
                .unwrap_or_default();
            if category.is_empty() || !db::table_exists(&self.conn, &category).unwrap_or(false) {
                skipped_no_table += 1;
                continue;
            }
            if db::mpn_exists(&self.conn, &category, &item_id).unwrap_or(false) {
                skipped_dup += 1;
                continue;
            }
            values.insert("MPN".to_string(), item_id);
            match db::insert_component_row(&self.conn, &category, &values) {
                Ok(()) => imported += 1,
                Err(e) => {
                    self.set_status_err(format!("Failed to insert component: {}", e));
                    return;
                }
            }
        }

        if self.selected_category.is_some() {
            self.refresh_components();
        }
        self.set_status(format!(
            "Imported: {}, skipped (no matching category table): {}, skipped (duplicate MPN): {}, skipped (no MPN): {}",
            imported, skipped_no_table, skipped_dup, skipped_no_id
        ));
    }

    fn cell_to_string(cell: &calamine::Data) -> String {
        match cell {
            calamine::Data::String(s) => s.clone(),
            calamine::Data::Float(f) => {
                if f.fract() == 0.0 && f.abs() < 1e15 {
                    format!("{}", *f as i64)
                } else {
                    f.to_string()
                }
            }
            calamine::Data::Int(i) => i.to_string(),
            calamine::Data::Bool(b) => b.to_string(),
            calamine::Data::DateTime(d) => d.to_string(),
            calamine::Data::DateTimeIso(s) => s.clone(),
            calamine::Data::DurationIso(s) => s.clone(),
            calamine::Data::Error(_) | calamine::Data::Empty => String::new(),
        }
    }

    fn save_dbl(&mut self) {
        let search_path = [
            self.settings_symbols_folder.trim(),
            self.settings_footprints_folder.trim(),
        ]
        .iter()
        .filter(|s| !s.is_empty())
        .cloned()
        .collect::<Vec<_>>()
        .join(";");
        self.dbl
            .set_database_link("LibrarySearchPath", &search_path);
        if let Err(e) = self.dbl.save(&self.dbl_path) {
            self.set_status_err(e);
        }
    }

    fn set_status(&mut self, msg: impl Into<String>) {
        self.status_msg = msg.into();
    }

    fn set_status_ok(&mut self) {
        self.status_msg = "OK".into();
    }

    fn set_status_err(&mut self, e: impl std::fmt::Display) {
        self.status_msg = format!("Error: {}", e);
    }

    fn apply_theme(&self, ctx: &egui::Context) {
        match self.theme {
            Theme::Light => ctx.set_theme(egui::Theme::Light),
            Theme::Dark => ctx.set_theme(egui::Theme::Dark),
            Theme::System => ctx.set_theme(egui::ThemePreference::System),
        }
    }

    /// Whether the active (resolved) theme is dark. For `Theme::System` this
    /// consults egui's resolved theme.
    fn is_dark(&self, ctx: &egui::Context) -> bool {
        match self.theme {
            Theme::Dark => true,
            Theme::Light => false,
            Theme::System => ctx.theme() == egui::Theme::Dark,
        }
    }

    /// Preview background color matching the current theme.
    fn preview_bg(&self, ctx: &egui::Context) -> &'static str {
        if self.is_dark(ctx) {
            DARK_BG
        } else {
            LIGHT_BG
        }
    }

    fn adapt_viewport(&mut self, ctx: &egui::Context) {
        if self.viewport_adapted {
            return;
        }
        self.viewport_adapted = true;
        if let Some(monitor) = ctx.input(|i| i.viewport().monitor_size) {
            let min_w = monitor.x.min(700.0);
            let min_h = monitor.y.min(520.0);
            ctx.send_viewport_cmd(egui::ViewportCommand::MinInnerSize(egui::vec2(
                min_w, min_h,
            )));
            let w = 1200.0f32.min(monitor.x * 0.9);
            let h = 700.0f32.min(monitor.y * 0.9);
            if w < 1200.0 || h < 700.0 {
                let ppp = ctx.pixels_per_point();
                let pos = egui::pos2((monitor.x - w) / 2.0 * ppp, (monitor.y - h) / 2.0 * ppp);
                ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::vec2(w, h)));
                ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(pos));
            }
        }
    }

    fn modal_size(&self, ctx: &egui::Context, ideal: egui::Vec2) -> egui::Vec2 {
        let screen = ctx.screen_rect();
        egui::vec2(
            ideal.x.min(screen.width() * 0.92).max(screen.width() * 0.4),
            ideal
                .y
                .min(screen.height() * 0.85)
                .max(screen.height() * 0.4),
        )
    }

    fn load_config_data(config_path: &PathBuf) -> ConfigData {
        if let Ok(data) = std::fs::read_to_string(config_path) {
            if let Ok(cfg) = serde_json::from_str::<ConfigData>(&data) {
                return cfg;
            }
        }
        ConfigData::default()
    }

    fn save_config_data(&self) {
        let cfg = ConfigData {
            theme: self.theme,
            db_path: self.db_path.display().to_string(),
            dbl_path: self.dbl_path.display().to_string(),
            dsn: self.settings_dsn.clone(),
            symbols_folder: self.settings_symbols_folder.clone(),
            footprints_folder: self.settings_footprints_folder.clone(),
        };
        if let Ok(data) = serde_json::to_string_pretty(&cfg) {
            let _ = std::fs::write(&self.config_path, data);
        }
    }

    fn pick_symbol_lib(&mut self, ctx: &egui::Context) {
        self.open_browse(ctx, BrowseTarget::Symbols);
    }

    fn open_browse(&mut self, _ctx: &egui::Context, target: BrowseTarget) {
        let folder = if target.is_symbols() {
            self.settings_symbols_folder.trim().to_string()
        } else {
            self.settings_footprints_folder.trim().to_string()
        };
        if folder.is_empty() {
            self.set_status(if target.is_symbols() {
                "Set Symbols folder in Settings first"
            } else {
                "Set Footprints folder in Settings first"
            });
            return;
        }
        self.browse_target = target;
        self.browse_path = PathBuf::from(&folder);
        self.browse_selected = None;
        self.browse_texture = None;
        self.browse_svg = None;
        self.browse_raster_size = egui::Vec2::ZERO;
        self.refresh_browse_entries();
        self.browse_open_at = std::time::Instant::now();
        self.browse_open = true;
    }

    fn refresh_browse_entries(&mut self) {
        self.browse_entries.clear();
        let ext = if self.browse_target.is_symbols() {
            "schlib"
        } else {
            "pcblib"
        };
        if let Ok(entries) = std::fs::read_dir(&self.browse_path) {
            for entry in entries.flatten() {
                let is_dir = entry.file_type().map(|t| t.is_dir()).unwrap_or(false);
                let ext_match = entry
                    .path()
                    .extension()
                    .map(|x| x.to_string_lossy().eq_ignore_ascii_case(ext))
                    .unwrap_or(false);
                if is_dir || ext_match {
                    self.browse_entries
                        .push((entry.file_name().to_string_lossy().to_string(), is_dir));
                }
            }
            self.browse_entries
                .sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        }
    }

    fn browse_file_stem(name: &str) -> String {
        std::path::Path::new(name)
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_default()
    }

    fn load_browse_preview(&mut self, ctx: &egui::Context, name: &str) {
        let full = self.browse_path.join(name);
        let full_str = full.to_string_lossy().to_string();
        let stem = Self::browse_file_stem(name);
        let out = render::temp_preview_path();
        let bg = self.preview_bg(ctx);
        let result = if self.browse_target.is_symbols() {
            render::render_symbol(&full_str, &stem, &out, bg)
        } else {
            render::render_footprint(&full_str, &stem, &out, bg)
        };
        match result {
            Ok(()) => match std::fs::read_to_string(&out) {
                Ok(svg) => {
                    self.browse_svg = Some(svg);
                    self.browse_texture = None;
                    self.browse_raster_size = egui::Vec2::ZERO;
                }
                Err(e) => {
                    self.set_status_err(format!("Failed to read preview: {}", e));
                    self.browse_svg = None;
                    self.browse_texture = None;
                }
            },
            Err(e) => {
                self.set_status_err(format!("Failed to render: {}", e));
                self.browse_svg = None;
                self.browse_texture = None;
            }
        }
    }

    fn apply_browse_selection(&mut self) {
        let Some(name) = self.browse_selected.clone() else {
            self.set_status("Select a library file first");
            return;
        };
        let full_str = self.browse_path.join(&name).to_string_lossy().to_string();
        let stem = Self::browse_file_stem(&name);
        if self.browse_target.is_symbols() {
            self.library_ref_input = stem;
            self.library_path_input =
                relative_library_path(&self.settings_symbols_folder, &full_str);
        } else {
            let rel = relative_library_path(&self.settings_footprints_folder, &full_str);
            match self.browse_target {
                BrowseTarget::Footprint1 => {
                    self.footprint_ref_input = stem;
                    self.footprint_path_input = rel;
                }
                BrowseTarget::Footprint2 => {
                    self.footprint_ref2_input = stem;
                    self.footprint_path2_input = rel;
                }
                _ => {
                    self.footprint_ref3_input = stem;
                    self.footprint_path3_input = rel;
                }
            }
        }
        self.browse_open = false;
        self.set_status_ok();
    }

    fn render_and_show(
        &mut self,
        ctx: &egui::Context,
        folder: String,
        rel: String,
        is_symbol: bool,
        name: &str,
    ) {
        let lib = resolve_library_path(folder.trim(), rel.trim());
        if lib.is_empty() {
            self.set_status("Library Path is empty. Browse for a library first");
            return;
        }
        let out = render::temp_preview_path();
        let bg = self.preview_bg(ctx);
        let result = if is_symbol {
            render::render_symbol(&lib, name, &out, bg)
        } else {
            render::render_footprint(&lib, name, &out, bg)
        };
        match result {
            Ok(()) => match std::fs::read_to_string(&out) {
                Ok(svg) => {
                    self.viewer_svg = Some(svg);
                    self.viewer_texture = None;
                    self.viewer_raster_size = egui::Vec2::ZERO;
                    self.viewer_title = name.to_string();
                    self.viewer_open_at = std::time::Instant::now();
                    self.viewer_open = true;
                }
                Err(e) => self.set_status_err(format!("Failed to read preview: {}", e)),
            },
            Err(e) => self.set_status_err(format!("Failed to render: {}", e)),
        }
    }
}

impl eframe::App for AltiumDbApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.apply_theme(ctx);
        self.adapt_viewport(ctx);

        egui::TopBottomPanel::top("menu_bar").show(ctx, |ui| {
            egui::menu::bar(ui, |ui| {
                ui.menu_button("File", |ui| {
                    if ui.button("Import LCSC XLS...").clicked() {
                        if let Some(path) = rfd::FileDialog::new()
                            .add_filter("Excel workbook", &["xls", "xlsx"])
                            .pick_file()
                        {
                            self.import_lcsc_xls(&path);
                        }
                        ui.close_menu();
                    }
                    if ui.button("Export .DbLib...").clicked() {
                        if let Some(path) = rfd::FileDialog::new()
                            .add_filter("Altium Database Library", &["DbLib"])
                            .save_file()
                        {
                            match self.dbl.save(&path) {
                                Ok(()) => {
                                    self.set_status(format!("Exported to {}", path.display()))
                                }
                                Err(e) => self.set_status_err(e),
                            }
                        }
                        ui.close_menu();
                    }
                    ui.separator();
                    if ui.button("Quit").clicked() {
                        std::process::exit(0);
                    }
                });
                if ui.button("Settings").clicked() {
                    self.settings_open = true;
                }
                if ui.button("About").clicked() {
                    self.about_open = true;
                }
                ui.separator();
                ui.selectable_value(&mut self.mode, AppMode::Fill, "Fill DB");
                ui.selectable_value(&mut self.mode, AppMode::Search, "Search");
            });
        });

        egui::TopBottomPanel::bottom("status_bar").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label("Status:");
                ui.label(&self.status_msg);
            });
        });

        if self.settings_open {
            let mut save_clicked = false;

            let modal = egui::Modal::new(egui::Id::new("settings_modal")).show(ctx, |ui| {
                ui.set_min_width(self.modal_size(ctx, egui::vec2(460.0, 300.0)).x);
                ui.heading("Settings");
                ui.horizontal(|ui| {
                    ui.radio_value(&mut self.theme, Theme::System, "System");
                    ui.radio_value(&mut self.theme, Theme::Light, "Light");
                    ui.radio_value(&mut self.theme, Theme::Dark, "Dark");
                });

                ui.separator();
                ui.heading("Paths");

                let row = (ui.cursor().min.x, ui.available_width());
                ui.horizontal(|ui| {
                    ui.label("Database (.sqlite):");
                    if ui.button("Browse").clicked() {
                        if let Some(path) = rfd::FileDialog::new()
                            .add_filter("SQLite Database", &["sqlite"])
                            .pick_file()
                        {
                            self.settings_db_path = path.display().to_string();
                        }
                    }
                    let w = stretch_width(ui, row, 0.0);
                    ui.add(
                        egui::TextEdit::singleline(&mut self.settings_db_path)
                            .hint_text("Path to .sqlite file")
                            .desired_width(w),
                    );
                });

                let row = (ui.cursor().min.x, ui.available_width());
                ui.horizontal(|ui| {
                    ui.label(".DbLib:");
                    if ui.button("Browse").clicked() {
                        if let Some(path) = rfd::FileDialog::new()
                            .add_filter("Altium Database Library", &["DbLib"])
                            .pick_file()
                        {
                            self.settings_dbl_path = path.display().to_string();
                        }
                    }
                    let w = stretch_width(ui, row, 0.0);
                    ui.add(
                        egui::TextEdit::singleline(&mut self.settings_dbl_path)
                            .hint_text("Path to .DbLib file")
                            .desired_width(w),
                    );
                });

                let row = (ui.cursor().min.x, ui.available_width());
                ui.horizontal(|ui| {
                    ui.label("ODBC Data Source:");
                    let w = stretch_width(ui, row, 0.0);
                    ui.add(
                        egui::TextEdit::singleline(&mut self.settings_dsn)
                            .hint_text("ODBC DSN name (e.g. gortpower)")
                            .desired_width(w),
                    );
                });

                let row = (ui.cursor().min.x, ui.available_width());
                ui.horizontal(|ui| {
                    ui.label("Symbols folder:");
                    if ui.button("Browse").clicked() {
                        if let Some(path) = rfd::FileDialog::new().pick_folder() {
                            self.settings_symbols_folder = path.display().to_string();
                        }
                    }
                    let w = stretch_width(ui, row, 0.0);
                    ui.add(
                        egui::TextEdit::singleline(&mut self.settings_symbols_folder)
                            .hint_text("Base folder for symbols (.SchLib)")
                            .desired_width(w),
                    );
                });

                let row = (ui.cursor().min.x, ui.available_width());
                ui.horizontal(|ui| {
                    ui.label("Footprints folder:");
                    if ui.button("Browse").clicked() {
                        if let Some(path) = rfd::FileDialog::new().pick_folder() {
                            self.settings_footprints_folder = path.display().to_string();
                        }
                    }
                    let w = stretch_width(ui, row, 0.0);
                    ui.add(
                        egui::TextEdit::singleline(&mut self.settings_footprints_folder)
                            .hint_text("Base folder for footprints (.PcbLib)")
                            .desired_width(w),
                    );
                });

                ui.separator();
                if ui.button("Save").clicked() {
                    save_clicked = true;
                    self.reopen_database();
                    self.save_config_data();
                    self.set_status("Settings saved");
                }
            });

            if save_clicked || modal.should_close() {
                self.settings_open = false;
            }
        }

        if self.about_open {
            let modal = egui::Modal::new(egui::Id::new("about_modal")).show(ctx, |ui| {
                ui.set_min_width(self.modal_size(ctx, egui::vec2(400.0, 240.0)).x);
                ui.heading("About AltiumDB");
                ui.label("AltiumDB — Altium Designer Database Library manager");
                ui.label("Manage component database, browse symbols,");
                ui.label("footprints and edit addition fields.");
                ui.separator();
                ui.label(format!("Author: {}", "Selyutin Anton aka YOUASSBEE"));
                ui.label(format!("E-mail: {}", "selutin.anton@yandex.ru"));
                ui.label(format!("Telegram: {}", "@YOUASSBEE"));
                ui.separator();
                ui.horizontal(|ui| {
                    ui.add_space(ui.available_width() - 80.0);
                    if ui.button("OK").clicked() {
                        self.about_open = false;
                    }
                });
            });
            if modal.should_close() {
                self.about_open = false;
            }
        }

        // --- Categories panel ---
        let cat_extra = panel_extra_width(ctx);
        let cat_content = self
            .categories
            .iter()
            .map(|c| text_width(ctx, &c.name))
            .fold(0.0f32, f32::max);
        let cat_w = (cat_content + cat_extra).clamp(150.0, 300.0);
        let cat_max = (ctx.screen_rect().width() * 0.4).max(150.0);
        egui::SidePanel::left("categories_panel")
            .default_width(cat_w)
            .width_range(150.0..=cat_max)
            .show(ctx, |ui| {
                ui.heading("Categories");
                ui.separator();

                if self.mode == AppMode::Fill {
                    let row = (ui.cursor().min.x, ui.available_width());
                    ui.horizontal(|ui| {
                        let btn_text = if self.editing_category.is_some() {
                            "Save"
                        } else {
                            "+"
                        };
                        if ui.small_button(btn_text).clicked() {
                            let name = self.category_input.trim().to_string();
                            if !name.is_empty() {
                                if let Some(ref edit_name) = self.editing_category.clone() {
                                    if edit_name != &name {
                                        db::rename_table(&self.conn, edit_name, &name).ok();
                                        self.dbl.remove_library(edit_name);
                                        let mut lib = altium_dbl::create_library(&name);
                                        lib.name = name.clone();
                                        lib.table = name.clone();
                                        self.dbl.add_library(lib);
                                    }
                                    self.editing_category = None;
                                } else {
                                    if !db::table_exists(&self.conn, &name).unwrap_or(false) {
                                        db::ensure_table(&self.conn, &name).ok();
                                    }
                                    if self.dbl.find_library(&name).is_none() {
                                        self.dbl.add_library(altium_dbl::create_library(&name));
                                    }
                                }
                                self.category_input.clear();
                                self.save_dbl();
                                self.refresh_categories();
                                self.set_status_ok();
                            }
                        }
                        let w = stretch_width(ui, row, 0.0);
                        let response = ui.add(
                            egui::TextEdit::singleline(&mut self.category_input).desired_width(w),
                        );
                        if response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                            let name = self.category_input.trim().to_string();
                            if !name.is_empty() {
                                if let Some(ref edit_name) = self.editing_category.clone() {
                                    if edit_name != &name {
                                        db::rename_table(&self.conn, edit_name, &name).ok();
                                        self.dbl.remove_library(edit_name);
                                        let mut lib = altium_dbl::create_library(&name);
                                        lib.name = name.clone();
                                        lib.table = name.clone();
                                        self.dbl.add_library(lib);
                                    }
                                    self.editing_category = None;
                                } else {
                                    if !db::table_exists(&self.conn, &name).unwrap_or(false) {
                                        db::ensure_table(&self.conn, &name).ok();
                                    }
                                    if self.dbl.find_library(&name).is_none() {
                                        self.dbl.add_library(altium_dbl::create_library(&name));
                                    }
                                }
                                self.category_input.clear();
                                self.save_dbl();
                                self.refresh_categories();
                                self.set_status_ok();
                            }
                        }
                    });

                    ui.separator();
                }

                let selected = self.selected_category.clone();
                let mut to_select = None;
                let mut to_delete = None;
                let mut to_edit = None;
                let mut to_clone_cat = None;
                let mut hovered_cat: Option<String> = None;
                let mut to_search_cat = None;

                egui::ScrollArea::vertical().show(ui, |ui| {
                    if self.mode == AppMode::Search {
                        let all_selected = self.search_all && self.search_category.is_none();
                        if ui.selectable_label(all_selected, ALL_CATEGORIES).clicked() {
                            to_search_cat = Some(ALL_CATEGORIES.to_string());
                        }
                        for cat in &self.categories {
                            let is_selected = self.search_category.as_deref() == Some(&cat.name);
                            if ui.selectable_label(is_selected, &cat.name).clicked() {
                                to_search_cat = Some(cat.name.clone());
                            }
                        }
                    } else {
                        for cat in &self.categories {
                            let is_selected = selected.as_deref() == Some(&cat.name);
                            let response = ui.selectable_label(is_selected, &cat.name);
                            if response.clicked() {
                                to_select = Some(cat.name.clone());
                            }
                            if response.hovered() {
                                hovered_cat = Some(cat.name.clone());
                            }
                            response.context_menu(|ui| {
                                if ui.button("Edit").clicked() {
                                    to_edit = Some(cat.name.clone());
                                    ui.close_menu();
                                }
                                if ui.button("Clone").clicked() {
                                    to_clone_cat = Some(cat.name.clone());
                                    ui.close_menu();
                                }
                                if ui.button("Delete").clicked() {
                                    to_delete = Some(cat.name.clone());
                                    ui.close_menu();
                                }
                            });
                        }
                    }
                });

                if hovered_cat.is_some() && ui.ctx().input(|i| i.key_pressed(egui::Key::Delete)) {
                    to_delete = hovered_cat;
                }

                if let Some(name) = to_select {
                    self.selected_category = Some(name);
                    self.selected_component_id = None;
                    self.refresh_components();
                    self.custom_values.clear();
                }
                if let Some(name) = to_search_cat {
                    let is_all = name == ALL_CATEGORIES;
                    let switching = if is_all {
                        !self.search_all || self.search_category.is_some()
                    } else {
                        self.search_category.as_deref() != Some(&name) || self.search_all
                    };
                    if switching {
                        if is_all {
                            self.search_all = true;
                            self.search_category = None;
                        } else {
                            self.search_all = false;
                            self.search_category = Some(name);
                        }
                        self.init_search();
                    }
                }
                if let Some(name) = to_edit {
                    self.editing_category = Some(name.clone());
                    self.category_input = name;
                }
                if let Some(name) = to_clone_cat {
                    let exists = |n: &str| {
                        db::table_exists(&self.conn, n).unwrap_or(false)
                            || self.dbl.find_library(n).is_some()
                    };
                    let new_name = unique_name(&name, exists);
                    db::clone_table(&self.conn, &name, &new_name).ok();
                    let lib = self.dbl.find_library(&name).cloned();
                    if let Some(mut lib) = lib {
                        lib.name = new_name.clone();
                        lib.table = new_name.clone();
                        self.dbl.add_library(lib);
                    } else {
                        self.dbl.add_library(altium_dbl::create_library(&new_name));
                    }
                    self.save_dbl();
                    self.refresh_categories();
                    self.set_status(format!("Category cloned as '{}'", new_name));
                }
                if let Some(name) = to_delete {
                    db::drop_table(&self.conn, &name).ok();
                    self.dbl.remove_library(&name);
                    self.save_dbl();
                    self.refresh_categories();
                    self.selected_category = None;
                    self.components.clear();
                    self.selected_component_id = None;
                    self.custom_values.clear();
                    self.set_status_ok();
                }
            });

        // --- Components panel ---
        let comp_extra = panel_extra_width(ctx);
        let comp_content = self
            .components
            .iter()
            .map(|c| text_width(ctx, &c.mpn))
            .fold(0.0f32, f32::max);
        let comp_w = (comp_content + comp_extra).clamp(180.0, 320.0);
        let comp_max = (ctx.screen_rect().width() * 0.5).max(180.0);
        egui::SidePanel::left("components_panel")
            .default_width(comp_w)
            .width_range(180.0..=comp_max)
            .show(ctx, |ui| {
                if self.mode == AppMode::Search {
                    ui.heading("Parameters");
                    ui.separator();

                    if self.search_all {
                        ui.label("Search the entire database by MPN:");
                        ui.separator();
                        let mut changed = false;
                        let r = ui.text_edit_singleline(&mut self.search_all_query);
                        if r.changed() {
                            changed = true;
                        }
                        if changed {
                            self.refresh_search();
                        }
                    } else if self.search_category.is_some() {
                        if ui.button("Reset filters").clicked() {
                            for p in &mut self.search_params {
                                p.selected.clear();
                            }
                            self.refresh_search();
                            self.set_status_ok();
                        }
                        ui.separator();

                        let mut changed = false;
                        egui::ScrollArea::vertical().show(ui, |ui| {
                            for p in &mut self.search_params {
                                if p.values.is_empty() {
                                    continue;
                                }
                                let current = if p.selected.is_empty() {
                                    "Any".to_string()
                                } else if p.selected.len() == p.values.len() {
                                    "(All)".to_string()
                                } else {
                                    p.selected.join(", ")
                                };
                                let active = !p.selected.is_empty();
                                let highlight = if ui.visuals().dark_mode {
                                    egui::Color32::from_rgba_unmultiplied(255, 196, 0, 55)
                                } else {
                                    egui::Color32::from_rgba_unmultiplied(255, 196, 0, 95)
                                };
                                let frame = if active {
                                    egui::Frame::NONE
                                        .fill(highlight)
                                        .inner_margin(egui::Margin::symmetric(6, 4))
                                        .corner_radius(egui::CornerRadius::from(4.0))
                                } else {
                                    egui::Frame::NONE
                                };
                                frame.show(ui, |ui| {
                                    let mut sel = p.selected.clone();
                                    if active {
                                        ui.label(egui::RichText::new(&p.name).strong());
                                    } else {
                                        ui.label(&p.name);
                                    }
                                    egui::ComboBox::from_id_salt(("search_param", &p.column))
                                        .width(ui.available_width())
                                        .selected_text(&current)
                                        .show_ui(ui, |ui| {
                                            if ui.selectable_label(sel.is_empty(), "Any").clicked()
                                            {
                                                sel.clear();
                                            }
                                            ui.separator();
                                            for v in &p.values {
                                                let mut checked = sel.contains(v);
                                                if ui.checkbox(&mut checked, v.as_str()).changed() {
                                                    if checked {
                                                        sel.push(v.clone());
                                                    } else {
                                                        sel.retain(|x| x != v);
                                                    }
                                                }
                                            }
                                        });
                                    sel.retain(|v| p.values.contains(v));
                                    if sel != p.selected {
                                        p.selected = sel;
                                        changed = true;
                                    }
                                });
                            }
                        });
                        if changed {
                            self.refresh_search();
                        }
                    } else {
                        ui.label("Select a category first");
                    }
                } else {
                    ui.heading("Components");
                    ui.separator();

                    if self.selected_category.is_some() {
                        let row = (ui.cursor().min.x, ui.available_width());
                        ui.horizontal(|ui| {
                            let btn_text = if self.editing_component.is_some() {
                                "Save"
                            } else {
                                "+"
                            };
                            if ui.small_button(btn_text).clicked() {
                                let item_id = self.component_input.trim().to_string();
                                if !item_id.is_empty() {
                                    if let Some(ref cat) = self.selected_category {
                                        db::ensure_table(&self.conn, cat).ok();
                                        if let Some(edit_id) = self.editing_component.clone() {
                                            db::update_component(
                                                &self.conn,
                                                cat,
                                                &db::Component {
                                                    id: edit_id,
                                                    mpn: item_id.clone(),
                                                    ..db::Component::default()
                                                },
                                            )
                                            .ok();
                                            self.editing_component = None;
                                        } else {
                                            db::add_component(
                                                &self.conn,
                                                cat,
                                                &db::Component {
                                                    mpn: item_id.clone(),
                                                    ..db::Component::default()
                                                },
                                            )
                                            .ok();
                                        }
                                    }
                                    self.component_input.clear();
                                    self.refresh_components();
                                    self.set_status_ok();
                                }
                            }
                            let w = stretch_width(ui, row, 0.0);
                            let response = ui.add(
                                egui::TextEdit::singleline(&mut self.component_input)
                                    .desired_width(w),
                            );
                            if response.lost_focus()
                                && ui.input(|i| i.key_pressed(egui::Key::Enter))
                            {
                                let item_id = self.component_input.trim().to_string();
                                if !item_id.is_empty() {
                                    if let Some(ref cat) = self.selected_category {
                                        db::ensure_table(&self.conn, cat).ok();
                                        if let Some(edit_id) = self.editing_component.clone() {
                                            db::update_component(
                                                &self.conn,
                                                cat,
                                                &db::Component {
                                                    id: edit_id,
                                                    mpn: item_id.clone(),
                                                    ..db::Component::default()
                                                },
                                            )
                                            .ok();
                                            self.editing_component = None;
                                        } else {
                                            db::add_component(
                                                &self.conn,
                                                cat,
                                                &db::Component {
                                                    mpn: item_id.clone(),
                                                    ..db::Component::default()
                                                },
                                            )
                                            .ok();
                                        }
                                    }
                                    self.component_input.clear();
                                    self.refresh_components();
                                    self.set_status_ok();
                                }
                            }
                        });
                    } else {
                        ui.label("Select a category first");
                    }

                    ui.separator();

                    let selected = self.selected_component_id.clone();
                    let mut to_select = None;
                    let mut to_delete = None;
                    let mut to_edit = None;
                    let mut to_clone_comp = None;
                    let mut hovered_comp: Option<String> = None;

                    egui::ScrollArea::vertical().show(ui, |ui| {
                        for comp in &self.components {
                            let is_selected = selected == Some(comp.id.clone());
                            let response = ui.selectable_label(is_selected, &comp.mpn);
                            if response.clicked() {
                                to_select = Some(comp.id.clone());
                            }
                            if response.hovered() {
                                hovered_comp = Some(comp.id.clone());
                            }
                            response.context_menu(|ui| {
                                if ui.button("Edit").clicked() {
                                    to_edit = Some(comp.clone());
                                    ui.close_menu();
                                }
                                if ui.button("Clone").clicked() {
                                    to_clone_comp = Some(comp.clone());
                                    ui.close_menu();
                                }
                                if ui.button("Delete").clicked() {
                                    to_delete = Some(comp.id.clone());
                                    ui.close_menu();
                                }
                            });
                        }
                    });

                    if hovered_comp.is_some()
                        && ui.ctx().input(|i| i.key_pressed(egui::Key::Delete))
                    {
                        to_delete = hovered_comp;
                    }

                    if let Some(id) = to_select {
                        self.selected_component_id = Some(id.clone());
                        self.refresh_custom_values();
                        if let Some(comp) = self.components.iter().find(|c| c.id == id) {
                            self.mpn_input = comp.mpn.clone();
                            self.manufacturer_input = comp.manufacturer.clone();
                            self.verified_input = comp.verified;
                            self.library_ref_input = comp.library_ref.clone();
                            self.footprint_ref_input = comp.footprint_ref.clone();
                            self.library_path_input = comp.library_path.clone();
                            self.footprint_path_input = comp.footprint_path.clone();
                            self.footprint_ref2_input = comp.footprint_ref2.clone();
                            self.footprint_path2_input = comp.footprint_path2.clone();
                            self.footprint_ref3_input = comp.footprint_ref3.clone();
                            self.footprint_path3_input = comp.footprint_path3.clone();
                            self.description_input = comp.description.clone();
                            self.component_link1_description_input =
                                comp.component_link1_description.clone();
                            self.component_link1_url_input = comp.component_link1_url.clone();
                            self.component_link2_description_input =
                                comp.component_link2_description.clone();
                            self.component_link2_url_input = comp.component_link2_url.clone();
                            self.component_link3_description_input =
                                comp.component_link3_description.clone();
                            self.component_link3_url_input = comp.component_link3_url.clone();
                        }
                    }
                    if let Some(comp) = to_edit {
                        let comp_id = comp.id.clone();
                        self.selected_component_id = Some(comp_id.clone());
                        self.editing_component = Some(comp_id);
                        self.component_input = comp.mpn.clone();
                        self.mpn_input = comp.mpn.clone();
                        self.manufacturer_input = comp.manufacturer.clone();
                        self.verified_input = comp.verified;
                        self.library_ref_input = comp.library_ref.clone();
                        self.footprint_ref_input = comp.footprint_ref.clone();
                        self.library_path_input = comp.library_path.clone();
                        self.footprint_path_input = comp.footprint_path.clone();
                        self.footprint_ref2_input = comp.footprint_ref2.clone();
                        self.footprint_path2_input = comp.footprint_path2.clone();
                        self.footprint_ref3_input = comp.footprint_ref3.clone();
                        self.footprint_path3_input = comp.footprint_path3.clone();
                        self.description_input = comp.description.clone();
                        self.component_link1_description_input =
                            comp.component_link1_description.clone();
                        self.component_link1_url_input = comp.component_link1_url.clone();
                        self.component_link2_description_input =
                            comp.component_link2_description.clone();
                        self.component_link2_url_input = comp.component_link2_url.clone();
                        self.component_link3_description_input =
                            comp.component_link3_description.clone();
                        self.component_link3_url_input = comp.component_link3_url.clone();
                        self.refresh_custom_values();
                    }
                    if let Some(comp) = to_clone_comp {
                        if let Some(ref cat) = self.selected_category {
                            let exists =
                                |id: &str| db::mpn_exists(&self.conn, cat, id).unwrap_or(false);
                            let new_id = unique_name(&comp.mpn, exists);
                            db::clone_component(&self.conn, cat, &comp.id, &new_id).ok();
                            self.set_status(format!("Component cloned as '{}'", new_id));
                        }
                        self.refresh_components();
                    }
                    if let Some(id) = to_delete {
                        if let Some(ref cat) = self.selected_category {
                            db::delete_component(&self.conn, cat, &id).ok();
                        }
                        self.refresh_components();
                        self.selected_component_id = None;
                        self.custom_values.clear();
                        self.set_status_ok();
                    }
                }
            });

        // --- Properties panel ---
        egui::CentralPanel::default().show(ctx, |ui| {
            if self.mode == AppMode::Search {
                let has_search = self.search_category.is_some() || self.search_all;
                if has_search {
                    let avail_w = ui.available_width();
                    egui::SidePanel::left("search_results_panel")
                        .resizable(true)
                        .default_width((avail_w / 2.0).clamp(220.0, 500.0))
                        .width_range(180.0..=(avail_w * 0.8).max(300.0))
                        .show_inside(ui, |ui| {
                            ui.heading(format!("Components ({})", self.search_results.len()));
                            ui.separator();

                            let selected = self.search_selected.clone();
                            let mut to_select = None;
                            let mut to_copy = None;

                            egui::ScrollArea::vertical()
                                .id_salt("search_results_scroll")
                                .show(ui, |ui| {
                                    for (_cat, comp) in &self.search_results {
                                        let is_selected = selected == Some(comp.id.clone());
                                        let response =
                                            ui.selectable_label(is_selected, comp.mpn.clone());
                                        if response.clicked() {
                                            to_select = Some(comp.id.clone());
                                        }
                                        if response.double_clicked() {
                                            to_copy = Some(comp.mpn.clone());
                                        }
                                    }
                                });

                            if let Some(id) = to_select {
                                self.search_selected = Some(id);
                            }
                            if let Some(id) = to_copy {
                                ui.ctx().copy_text(id.clone());
                                self.set_status(format!("MPN copied: {}", id));
                            }
                        });

                    let found = self
                        .search_results
                        .iter()
                        .find(|(_, comp)| self.search_selected.as_ref() == Some(&comp.id))
                        .cloned();
                    if let Some((cat, comp)) = found {
                        ui.horizontal(|ui| {
                            ui.heading(&comp.mpn);
                            if ui.button("Copy ID").clicked() {
                                ui.ctx().copy_text(comp.mpn.clone());
                                self.set_status(format!("MPN copied: {}", comp.mpn));
                            }
                        });
                        ui.separator();

                        let mut detail_rows: Vec<(String, String)> = Vec::new();
                        for (column, _) in self.search_param_columns(&cat) {
                            let value = self.search_detail_value(&cat, &comp, &column);
                            if !value.is_empty() {
                                detail_rows.push((column, value));
                            }
                        }

                        egui::ScrollArea::vertical()
                            .id_salt("search_details_scroll")
                            .auto_shrink(false)
                            .show(ui, |ui| {
                                egui::Grid::new("search_details_grid")
                                    .num_columns(2)
                                    .striped(true)
                                    .show(ui, |ui| {
                                        for (name, value) in &detail_rows {
                                            ui.label(format!("{}:", name));
                                            ui.add(egui::Label::new(value).wrap());
                                            ui.end_row();
                                        }
                                    });
                            });
                    } else {
                        ui.centered_and_justified(|ui| {
                            ui.label("Select a component to see details");
                        });
                    }
                } else {
                    ui.centered_and_justified(|ui| {
                        ui.label("Select a category or 'All categories' to start searching");
                    });
                }
            } else {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    if let Some(ref cat) = self.selected_category.clone() {
                        if let Some(comp_id) = self.selected_component_id.clone() {
                            ui.horizontal(|ui| {
                                ui.heading("Base Fields");
                                if ui.button("Fields...").clicked() {
                                    self.fields_editor_open = true;
                                }
                            });
                            ui.separator();

                            let mut save_component = false;

                            let row = (ui.cursor().min.x, ui.available_width());
                            ui.horizontal(|ui| {
                                ui.label("MPN:");
                                let w = stretch_width(ui, row, 0.0);
                                let r = ui.add(
                                    egui::TextEdit::singleline(&mut self.mpn_input)
                                        .desired_width(w),
                                );
                                if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                                    save_component = true;
                                }
                            });
                            let row = (ui.cursor().min.x, ui.available_width());
                            ui.horizontal(|ui| {
                                ui.label("Manufacturer:");
                                let w = stretch_width(ui, row, 0.0);
                                let r = ui.add(
                                    egui::TextEdit::singleline(&mut self.manufacturer_input)
                                        .desired_width(w),
                                );
                                if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                                    save_component = true;
                                }
                            });
                            let row = (ui.cursor().min.x, ui.available_width());
                            ui.horizontal(|ui| {
                                ui.label("Library Ref:");
                                let extra = button_width(ui, "Browse")
                                    + button_width(ui, "View")
                                    + 2.0 * ui.spacing().item_spacing.x;
                                let w = stretch_width(ui, row, extra);
                                let r = ui.add(
                                    egui::TextEdit::singleline(&mut self.library_ref_input)
                                        .desired_width(w),
                                );
                                if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                                    save_component = true;
                                }
                                if ui.button("Browse").clicked() {
                                    self.pick_symbol_lib(ctx);
                                }
                                if ui.button("View").clicked() && !self.library_ref_input.is_empty()
                                {
                                    let name = self.library_ref_input.trim().to_string();
                                    let folder = self.settings_symbols_folder.clone();
                                    let rel = self.library_path_input.trim().to_string();
                                    self.render_and_show(ctx, folder, rel, true, &name);
                                }
                            });
                            let row = (ui.cursor().min.x, ui.available_width());
                            ui.horizontal(|ui| {
                                ui.label("Library Path:");
                                let w = stretch_width(ui, row, 0.0);
                                let r = ui.add(
                                    egui::TextEdit::singleline(&mut self.library_path_input)
                                        .desired_width(w),
                                );
                                if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                                    save_component = true;
                                }
                            });
                            let row = (ui.cursor().min.x, ui.available_width());
                            ui.horizontal(|ui| {
                                ui.label("Footprint Ref:");
                                let extra = button_width(ui, "Browse")
                                    + button_width(ui, "View")
                                    + 2.0 * ui.spacing().item_spacing.x;
                                let w = stretch_width(ui, row, extra);
                                let r = ui.add(
                                    egui::TextEdit::singleline(&mut self.footprint_ref_input)
                                        .desired_width(w),
                                );
                                if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                                    save_component = true;
                                }
                                if ui.button("Browse").clicked() {
                                    self.open_browse(ctx, BrowseTarget::Footprint1);
                                }
                                if ui.button("View").clicked()
                                    && !self.footprint_ref_input.is_empty()
                                {
                                    let name = self.footprint_ref_input.trim().to_string();
                                    let folder = self.settings_footprints_folder.clone();
                                    let rel = self.footprint_path_input.trim().to_string();
                                    self.render_and_show(ctx, folder, rel, false, &name);
                                }
                            });
                            let row = (ui.cursor().min.x, ui.available_width());
                            ui.horizontal(|ui| {
                                ui.label("Footprint Path:");
                                let w = stretch_width(ui, row, 0.0);
                                let r = ui.add(
                                    egui::TextEdit::singleline(&mut self.footprint_path_input)
                                        .desired_width(w),
                                );
                                if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                                    save_component = true;
                                }
                            });
                            let row = (ui.cursor().min.x, ui.available_width());
                            ui.horizontal(|ui| {
                                ui.label("Footprint Ref 2:");
                                let extra = button_width(ui, "Browse")
                                    + button_width(ui, "View")
                                    + 2.0 * ui.spacing().item_spacing.x;
                                let w = stretch_width(ui, row, extra);
                                let r = ui.add(
                                    egui::TextEdit::singleline(&mut self.footprint_ref2_input)
                                        .desired_width(w),
                                );
                                if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                                    save_component = true;
                                }
                                if ui.button("Browse").clicked() {
                                    self.open_browse(ctx, BrowseTarget::Footprint2);
                                }
                                if ui.button("View").clicked()
                                    && !self.footprint_ref2_input.is_empty()
                                {
                                    let name = self.footprint_ref2_input.trim().to_string();
                                    let folder = self.settings_footprints_folder.clone();
                                    let rel = self.footprint_path2_input.trim().to_string();
                                    self.render_and_show(ctx, folder, rel, false, &name);
                                }
                            });
                            let row = (ui.cursor().min.x, ui.available_width());
                            ui.horizontal(|ui| {
                                ui.label("Footprint Path 2:");
                                let w = stretch_width(ui, row, 0.0);
                                let r = ui.add(
                                    egui::TextEdit::singleline(&mut self.footprint_path2_input)
                                        .desired_width(w),
                                );
                                if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                                    save_component = true;
                                }
                            });
                            let row = (ui.cursor().min.x, ui.available_width());
                            ui.horizontal(|ui| {
                                ui.label("Footprint Ref 3:");
                                let extra = button_width(ui, "Browse")
                                    + button_width(ui, "View")
                                    + 2.0 * ui.spacing().item_spacing.x;
                                let w = stretch_width(ui, row, extra);
                                let r = ui.add(
                                    egui::TextEdit::singleline(&mut self.footprint_ref3_input)
                                        .desired_width(w),
                                );
                                if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                                    save_component = true;
                                }
                                if ui.button("Browse").clicked() {
                                    self.open_browse(ctx, BrowseTarget::Footprint3);
                                }
                                if ui.button("View").clicked()
                                    && !self.footprint_ref3_input.is_empty()
                                {
                                    let name = self.footprint_ref3_input.trim().to_string();
                                    let folder = self.settings_footprints_folder.clone();
                                    let rel = self.footprint_path3_input.trim().to_string();
                                    self.render_and_show(ctx, folder, rel, false, &name);
                                }
                            });
                            let row = (ui.cursor().min.x, ui.available_width());
                            ui.horizontal(|ui| {
                                ui.label("Footprint Path 3:");
                                let w = stretch_width(ui, row, 0.0);
                                let r = ui.add(
                                    egui::TextEdit::singleline(&mut self.footprint_path3_input)
                                        .desired_width(w),
                                );
                                if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                                    save_component = true;
                                }
                            });
                            let row = (ui.cursor().min.x, ui.available_width());
                            ui.horizontal(|ui| {
                                ui.label("ComponentLink1Description:");
                                let w = stretch_width(ui, row, 0.0);
                                let r = ui.add(
                                    egui::TextEdit::singleline(
                                        &mut self.component_link1_description_input,
                                    )
                                    .desired_width(w),
                                );
                                if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                                    save_component = true;
                                }
                            });
                            let row = (ui.cursor().min.x, ui.available_width());
                            ui.horizontal(|ui| {
                                ui.label("ComponentLink1URL:");
                                let w = stretch_width(ui, row, 0.0);
                                let r = ui.add(
                                    egui::TextEdit::singleline(&mut self.component_link1_url_input)
                                        .desired_width(w),
                                );
                                if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                                    save_component = true;
                                }
                            });
                            let row = (ui.cursor().min.x, ui.available_width());
                            ui.horizontal(|ui| {
                                ui.label("ComponentLink2Description:");
                                let w = stretch_width(ui, row, 0.0);
                                let r = ui.add(
                                    egui::TextEdit::singleline(
                                        &mut self.component_link2_description_input,
                                    )
                                    .desired_width(w),
                                );
                                if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                                    save_component = true;
                                }
                            });
                            let row = (ui.cursor().min.x, ui.available_width());
                            ui.horizontal(|ui| {
                                ui.label("ComponentLink2URL:");
                                let w = stretch_width(ui, row, 0.0);
                                let r = ui.add(
                                    egui::TextEdit::singleline(&mut self.component_link2_url_input)
                                        .desired_width(w),
                                );
                                if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                                    save_component = true;
                                }
                            });
                            let row = (ui.cursor().min.x, ui.available_width());
                            ui.horizontal(|ui| {
                                ui.label("ComponentLink3Description:");
                                let w = stretch_width(ui, row, 0.0);
                                let r = ui.add(
                                    egui::TextEdit::singleline(
                                        &mut self.component_link3_description_input,
                                    )
                                    .desired_width(w),
                                );
                                if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                                    save_component = true;
                                }
                            });
                            let row = (ui.cursor().min.x, ui.available_width());
                            ui.horizontal(|ui| {
                                ui.label("ComponentLink3URL:");
                                let w = stretch_width(ui, row, 0.0);
                                let r = ui.add(
                                    egui::TextEdit::singleline(&mut self.component_link3_url_input)
                                        .desired_width(w),
                                );
                                if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                                    save_component = true;
                                }
                            });
                            ui.horizontal(|ui| {
                                ui.label("Description:");
                                let w = stretch_width(ui, row, 0.0);
                                let r = ui.add(
                                    egui::TextEdit::singleline(&mut self.description_input)
                                        .desired_width(w),
                                );
                                if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                                    save_component = true;
                                }
                            });
                            ui.horizontal(|ui| {
                                ui.checkbox(&mut self.verified_input, "Verified");
                            });

                            ui.separator();
                            ui.heading("Custom Fields");
                            ui.separator();

                            let row = (ui.cursor().min.x, ui.available_width());
                            ui.horizontal(|ui| {
                                ui.label("Field name:");
                                let btn_text = if self.editing_field.is_some() {
                                    "Save"
                                } else {
                                    "+"
                                };
                                let w = stretch_width(
                                    ui,
                                    row,
                                    button_width(ui, btn_text) + ui.spacing().item_spacing.x,
                                );
                                let r = ui.add(
                                    egui::TextEdit::singleline(&mut self.field_col_input)
                                        .desired_width(w),
                                );
                                let mut do_action = ui.button(btn_text).clicked();
                                if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                                    do_action = true;
                                }
                                if do_action {
                                    let field_name = self.field_col_input.trim().to_string();
                                    if !field_name.is_empty() {
                                        if let Some(lib) = self.dbl.find_library_mut(cat) {
                                            if let Some(ref old_col) = self.editing_field.clone() {
                                                if old_col != &field_name {
                                                    if let Some(f) = lib
                                                        .fields
                                                        .iter_mut()
                                                        .find(|f| f.column == *old_col)
                                                    {
                                                        f.column = field_name.clone();
                                                        f.parameter = field_name.clone();
                                                    }
                                                    db::rename_column(
                                                        &self.conn,
                                                        cat,
                                                        old_col,
                                                        &field_name,
                                                    )
                                                    .ok();
                                                }
                                                self.editing_field = None;
                                            } else {
                                                if !lib
                                                    .fields
                                                    .iter()
                                                    .any(|f| f.column == field_name)
                                                {
                                                    lib.fields.push(altium_dbl::Field {
                                                        column: field_name.clone(),
                                                        parameter: field_name.clone(),
                                                        is_key: false,
                                                        visible_on_add: true,
                                                        add_mode: 0,
                                                        remove_mode: 0,
                                                        update_mode: 0,
                                                    });
                                                    db::add_column(&self.conn, cat, &field_name)
                                                        .ok();
                                                }
                                            }
                                            self.save_dbl();
                                            self.refresh_components();
                                            self.refresh_custom_values();
                                            self.field_col_input.clear();
                                            self.editing_field = None;
                                            self.set_status_ok();
                                        }
                                    }
                                }
                            });

                            ui.separator();

                            let mut values = self.custom_values.clone();
                            let mut changed = None;
                            let mut to_delete_field = None;
                            let mut to_edit_field = None;
                            let mut hovered_field: Option<String> = None;

                            for (i, (col, val)) in values.iter_mut().enumerate() {
                                let display = self
                                    .dbl
                                    .find_library(cat)
                                    .and_then(|l| l.fields.iter().find(|f| &f.column == col))
                                    .map(|f| f.column.clone())
                                    .unwrap_or_else(|| col.clone());

                                let mut buf = val.clone();
                                let row = (ui.cursor().min.x, ui.available_width());
                                let text_response = ui
                                    .horizontal(|ui| {
                                        ui.label(format!("{}:", display));
                                        let w = stretch_width(ui, row, 0.0);
                                        let r = ui.add(
                                            egui::TextEdit::singleline(&mut buf).desired_width(w),
                                        );
                                        if r.changed() {
                                            changed = Some((i, buf.clone()));
                                        }
                                        r
                                    })
                                    .inner;

                                if text_response.hovered() {
                                    hovered_field = Some(col.clone());
                                }

                                text_response.context_menu(|ui| {
                                    if ui.button("Edit").clicked() {
                                        to_edit_field = Some((col.clone(), display.clone()));
                                        ui.close_menu();
                                    }
                                    if ui.button("Delete").clicked() {
                                        to_delete_field = Some(col.clone());
                                        ui.close_menu();
                                    }
                                });
                            }

                            ui.horizontal(|ui| {
                                if ui.button("Save").clicked()
                                    || ui.input(|i| i.modifiers.ctrl && i.key_pressed(egui::Key::S))
                                {
                                    save_component = true;
                                }
                                if ui.small_button("Clear").clicked() {
                                    self.mpn_input.clear();
                                    self.manufacturer_input.clear();
                                    self.verified_input = false;
                                    self.library_ref_input.clear();
                                    self.footprint_ref_input.clear();
                                    self.library_path_input.clear();
                                    self.footprint_path_input.clear();
                                    self.footprint_ref2_input.clear();
                                    self.footprint_path2_input.clear();
                                    self.footprint_ref3_input.clear();
                                    self.footprint_path3_input.clear();
                                    self.description_input.clear();
                                    self.component_link1_description_input.clear();
                                    self.component_link1_url_input.clear();
                                    self.component_link2_description_input.clear();
                                    self.component_link2_url_input.clear();
                                    self.component_link3_description_input.clear();
                                    self.component_link3_url_input.clear();
                                    for (_, val) in &mut self.custom_values {
                                        val.clear();
                                    }
                                }
                            });

                            if save_component {
                                let mpn = self.mpn_input.trim().to_string();
                                let manufacturer = self.manufacturer_input.trim().to_string();
                                let verified = self.verified_input;
                                let library_ref = self.library_ref_input.trim().to_string();
                                let footprint_ref = self.footprint_ref_input.trim().to_string();
                                let library_path = self.library_path_input.trim().to_string();
                                let footprint_path = self.footprint_path_input.trim().to_string();
                                let footprint_ref2 = self.footprint_ref2_input.trim().to_string();
                                let footprint_path2 = self.footprint_path2_input.trim().to_string();
                                let footprint_ref3 = self.footprint_ref3_input.trim().to_string();
                                let footprint_path3 = self.footprint_path3_input.trim().to_string();
                                let description = self.description_input.trim().to_string();
                                let component_link1_description =
                                    self.component_link1_description_input.trim().to_string();
                                let component_link1_url =
                                    self.component_link1_url_input.trim().to_string();
                                let component_link2_description =
                                    self.component_link2_description_input.trim().to_string();
                                let component_link2_url =
                                    self.component_link2_url_input.trim().to_string();
                                let component_link3_description =
                                    self.component_link3_description_input.trim().to_string();
                                let component_link3_url =
                                    self.component_link3_url_input.trim().to_string();
                                db::update_component(
                                    &self.conn,
                                    cat,
                                    &db::Component {
                                        id: comp_id.clone(),
                                        mpn,
                                        manufacturer,
                                        verified,
                                        library_ref,
                                        footprint_ref,
                                        description,
                                        component_link1_description,
                                        component_link1_url,
                                        component_link2_description,
                                        component_link2_url,
                                        component_link3_description,
                                        component_link3_url,
                                        library_path,
                                        footprint_path,
                                        footprint_ref2,
                                        footprint_path2,
                                        footprint_ref3,
                                        footprint_path3,
                                    },
                                )
                                .ok();
                                self.refresh_components();
                                self.set_status_ok();
                            }

                            if hovered_field.is_some()
                                && ui.ctx().input(|i| i.key_pressed(egui::Key::Delete))
                            {
                                to_delete_field = hovered_field;
                            }

                            if let Some((i, new_val)) = changed {
                                if let Some((col, _)) = values.get(i) {
                                    let col_clone = col.clone();
                                    self.custom_values[i].1 = new_val.clone();
                                    db::set_custom_value(
                                        &self.conn, cat, &comp_id, &col_clone, &new_val,
                                    )
                                    .ok();
                                }
                            }

                            if let Some((col, _display)) = to_edit_field {
                                self.field_col_input = col.clone();
                                self.editing_field = Some(col);
                            }

                            if let Some(col) = to_delete_field {
                                db::drop_column(&self.conn, cat, &col).ok();
                                if let Some(lib) = self.dbl.find_library_mut(cat) {
                                    lib.fields.retain(|f| f.column != col);
                                }
                                self.save_dbl();
                                self.refresh_components();
                                self.refresh_custom_values();
                                self.set_status_ok();
                            }
                        } else {
                            ui.centered_and_justified(|ui| {
                                ui.label("Select a component to edit its fields");
                            });
                        }
                    } else {
                        ui.centered_and_justified(|ui| {
                            ui.label("Select a category to get started");
                        });
                    }
                });
            }
        });

        // Viewer modal
        if self.viewer_open {
            let svg = self.viewer_svg.clone();
            let modal = egui::Modal::new(egui::Id::new("viewer_modal")).show(ctx, |ui| {
                ui.set_min_size(self.modal_size(ctx, egui::vec2(600.0, 450.0)));
                ui.heading(format!("Preview: {}", self.viewer_title));
                let avail = ui.available_size();
                let canvas_size = egui::vec2(avail.x.max(100.0), avail.y.max(100.0));
                let (cid, canvas_rect) = ui.allocate_space(canvas_size);
                ui.interact(canvas_rect, cid, egui::Sense::hover());

                if let Some(svg) = &svg {
                    if let Err(e) = ensure_svg_texture(
                        ui.ctx(),
                        svg,
                        canvas_rect,
                        &mut self.viewer_texture,
                        &mut self.viewer_raster_size,
                        "altiumdb_preview",
                    ) {
                        self.set_status_err(e);
                    }
                    match &self.viewer_texture {
                        Some(tex) => draw_texture_fitted(ui, canvas_rect, tex),
                        None => {
                            let painter = ui.painter().with_clip_rect(canvas_rect);
                            painter.rect_filled(canvas_rect, 0.0, preview_bg_color32(ui.ctx()));
                        }
                    }
                } else {
                    ui.label("No preview loaded");
                }
            });
            if modal.should_close()
                && self.viewer_open_at.elapsed() >= std::time::Duration::from_millis(150)
            {
                self.viewer_open = false;
                self.viewer_svg = None;
                self.viewer_texture = None;
            }
        }

        // Browse modal
        if self.browse_open {
            let mut nav_to: Option<PathBuf> = None;
            let mut picked: Option<String> = None;
            let mut apply_now = false;
            let mut apply_clicked = false;
            let mut cancel_clicked = false;

            let modal = egui::Modal::new(egui::Id::new("browse_modal")).show(ctx, |ui| {
                ui.set_min_size(self.modal_size(ctx, egui::vec2(880.0, 560.0)));
                ui.heading(if self.browse_target.is_symbols() {
                    "Browse Symbols"
                } else {
                    "Browse Footprints"
                });

                ui.horizontal(|ui| {
                    if ui.button("Back").clicked() {
                        let mut parent = self.browse_path.clone();
                        if parent.pop() {
                            nav_to = Some(parent);
                        }
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.add(
                            egui::Label::new(
                                egui::RichText::new(self.browse_path.to_string_lossy().as_ref())
                                    .weak(),
                            )
                            .truncate(),
                        );
                    });
                });
                ui.separator();

                egui::TopBottomPanel::bottom("browse_actions_panel").show_inside(ui, |ui| {
                    ui.add_space(4.0);
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button("Cancel").clicked() {
                            cancel_clicked = true;
                        }
                        if ui.button("Select").clicked() {
                            apply_clicked = true;
                        }
                    });
                });

                let entries = self.browse_entries.clone();
                let sel = self.browse_selected.clone();
                let current_dir = self.browse_path.clone();

                egui::SidePanel::left("browse_list_panel")
                    .resizable(true)
                    .default_width(300.0)
                    .width_range(160.0..=460.0)
                    .show_inside(ui, |ui| {
                        egui::ScrollArea::vertical()
                            .id_salt("browse_entries_scroll")
                            .show(ui, |ui| {
                                for (name, is_dir) in &entries {
                                    let display = if *is_dir {
                                        format!("[{}]", name)
                                    } else {
                                        name.clone()
                                    };
                                    let is_sel = sel.as_deref() == Some(name.as_str()) && !is_dir;
                                    let resp = ui.selectable_label(is_sel, display);
                                    if resp.clicked() {
                                        if *is_dir {
                                            nav_to = Some(current_dir.join(name));
                                        } else {
                                            picked = Some(name.clone());
                                        }
                                    }
                                    if !*is_dir && resp.double_clicked() {
                                        picked = Some(name.clone());
                                        apply_now = true;
                                    }
                                }
                            });
                    });

                egui::CentralPanel::default().show_inside(ui, |ui| match &self.browse_svg {
                    Some(svg) => {
                        let avail = ui.available_size();
                        let canvas_size = egui::vec2(avail.x.max(50.0), avail.y.max(50.0));
                        let (cid, canvas_rect) = ui.allocate_space(canvas_size);
                        ui.interact(canvas_rect, cid, egui::Sense::hover());
                        if let Err(e) = ensure_svg_texture(
                            ui.ctx(),
                            svg,
                            canvas_rect,
                            &mut self.browse_texture,
                            &mut self.browse_raster_size,
                            "altiumdb_browse_preview",
                        ) {
                            self.set_status_err(e);
                        }
                        match &self.browse_texture {
                            Some(tex) => draw_texture_fitted(ui, canvas_rect, tex),
                            None => {
                                let painter = ui.painter().with_clip_rect(canvas_rect);
                                painter.rect_filled(canvas_rect, 0.0, preview_bg_color32(ui.ctx()));
                            }
                        }
                    }
                    None => {
                        ui.centered_and_justified(|ui| {
                            ui.label("Select a file to preview");
                        });
                    }
                });
            });

            if modal.should_close()
                && self.browse_open_at.elapsed() >= std::time::Duration::from_millis(150)
            {
                cancel_clicked = true;
            }
            if cancel_clicked {
                self.browse_open = false;
                self.browse_svg = None;
                self.browse_texture = None;
            }
            if let Some(path) = nav_to {
                self.browse_path = path;
                self.browse_selected = None;
                self.browse_svg = None;
                self.browse_texture = None;
                self.refresh_browse_entries();
            }
            if let Some(name) = picked {
                self.browse_selected = Some(name.clone());
                if apply_now {
                    self.apply_browse_selection();
                } else {
                    self.load_browse_preview(ctx, &name);
                }
            }
            if apply_clicked && self.browse_open {
                self.apply_browse_selection();
            }
        }

        // Fields editor window
        if self.fields_editor_open {
            let modal = egui::Modal::new(egui::Id::new("fields_editor_modal")).show(ctx, |ui| {
                ui.set_min_size(self.modal_size(ctx, egui::vec2(950.0, 460.0)));
                ui.heading("Field Properties");
                ui.separator();
                if let Some(ref cat) = self.selected_category.clone() {
                    let db_cols: Vec<String> = db::get_columns(&self.conn, cat)
                        .unwrap_or_default()
                        .into_iter()
                        .filter(|c| c != "id")
                        .collect();
                    if self.dbl.find_library(cat).is_none() {
                        self.dbl.add_library(altium_dbl::create_library(cat));
                    }
                    let mut rows: Vec<altium_dbl::Field> = Vec::new();
                    for col in &db_cols {
                        match self
                            .dbl
                            .find_library(cat)
                            .and_then(|l| l.fields.iter().find(|f| &f.column == col).cloned())
                        {
                            Some(f) => rows.push(f),
                            None => rows.push(altium_dbl::Field {
                                column: col.clone(),
                                parameter: col.clone(),
                                is_key: false,
                                visible_on_add: true,
                                add_mode: 0,
                                remove_mode: 0,
                                update_mode: 0,
                            }),
                        }
                    }

                    let mut changed = false;
                    egui::ScrollArea::both()
                        .id_salt("fields_editor_scroll")
                        .show(ui, |ui| {
                            egui::Grid::new("fields_editor_grid")
                                .striped(true)
                                .show(ui, |ui| {
                                    ui.label("Field");
                                    ui.label("Design Parameter");
                                    ui.label("Key");
                                    ui.label("Visible on add");
                                    ui.label("Update Values");
                                    ui.label("Add To Design");
                                    ui.label("Remove From Design");
                                    ui.end_row();
                                    for field in &rows {
                                        let mut f = field.clone();
                                        ui.label(&f.column);
                                        let mut param = f.parameter.clone();
                                        ui.add(
                                            egui::TextEdit::singleline(&mut param)
                                                .desired_width(170.0),
                                        );
                                        if param != f.parameter {
                                            f.parameter = param;
                                            changed = true;
                                        }
                                        ui.checkbox(&mut f.is_key, "");
                                        ui.checkbox(&mut f.visible_on_add, "");
                                        let mut upd = f.update_mode;
                                        egui::ComboBox::from_id_salt(("fld_update", &f.column))
                                            .selected_text(mode_label(&UPDATE_MODES, upd))
                                            .width(130.0)
                                            .show_ui(ui, |ui| {
                                                for (val, label) in UPDATE_MODES {
                                                    if ui
                                                        .selectable_label(upd == val, label)
                                                        .clicked()
                                                    {
                                                        upd = val;
                                                    }
                                                }
                                            });
                                        let mut add = f.add_mode;
                                        egui::ComboBox::from_id_salt(("fld_add", &f.column))
                                            .selected_text(mode_label(&ADD_MODES, add))
                                            .width(210.0)
                                            .show_ui(ui, |ui| {
                                                for (val, label) in ADD_MODES {
                                                    if ui
                                                        .selectable_label(add == val, label)
                                                        .clicked()
                                                    {
                                                        add = val;
                                                    }
                                                }
                                            });
                                        let mut rem = f.remove_mode;
                                        egui::ComboBox::from_id_salt(("fld_remove", &f.column))
                                            .selected_text(mode_label(&REMOVE_MODES, rem))
                                            .width(200.0)
                                            .show_ui(ui, |ui| {
                                                for (val, label) in REMOVE_MODES {
                                                    if ui
                                                        .selectable_label(rem == val, label)
                                                        .clicked()
                                                    {
                                                        rem = val;
                                                    }
                                                }
                                            });
                                        if upd != f.update_mode
                                            || add != f.add_mode
                                            || rem != f.remove_mode
                                        {
                                            f.update_mode = upd;
                                            f.add_mode = add;
                                            f.remove_mode = rem;
                                            changed = true;
                                        }
                                        if f != *field {
                                            if let Some(lib) = self.dbl.find_library_mut(cat) {
                                                if let Some(existing) = lib
                                                    .fields
                                                    .iter_mut()
                                                    .find(|x| x.column == f.column)
                                                {
                                                    *existing = f.clone();
                                                } else {
                                                    lib.fields.push(f.clone());
                                                }
                                            }
                                            changed = true;
                                        }
                                        ui.end_row();
                                    }
                                });
                        });
                    if changed {
                        self.save_dbl();
                    }
                } else {
                    ui.label("Select a category first");
                }
            });
            if modal.should_close() {
                self.fields_editor_open = false;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn library_path_keeps_only_file_name() {
        let folder = "C:\\gortpowerlib\\footprints";
        let full = "C:\\gortpowerlib\\footprints\\Capacitor - MLCC\\CAP 0603_1608.PcbLib";
        assert_eq!(relative_library_path(folder, full), "CAP 0603_1608.PcbLib");
    }

    #[test]
    fn resolve_finds_bare_file_name_recursively() {
        let dir = std::env::temp_dir().join("altiumdb_resolve_test");
        let _ = std::fs::remove_dir_all(&dir);
        let sub = dir.join("Capacitor - MLCC");
        std::fs::create_dir_all(&sub).unwrap();
        let target = sub.join("CAP 0603_1608.PcbLib");
        std::fs::write(&target, b"").unwrap();

        let resolved = resolve_library_path(dir.to_str().unwrap(), "CAP 0603_1608.PcbLib");
        assert_eq!(resolved, target.to_string_lossy().to_string());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn resolve_keeps_legacy_subdirectory_paths() {
        let folder = "C:\\gortpowerlib\\footprints";
        let rel = "\\Capacitor - MLCC\\CAP 0603_1608.PcbLib";
        assert_eq!(
            resolve_library_path(folder, rel),
            "C:\\gortpowerlib\\footprints\\Capacitor - MLCC\\CAP 0603_1608.PcbLib"
        );
    }
}
