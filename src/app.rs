use crate::db;
use crate::render;
use calamine::Reader as _;
use eframe::egui;
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
    DefaultSymbol,
    Footprint1,
    Footprint2,
    Footprint3,
    DefaultFootprint,
}

impl BrowseTarget {
    fn is_symbols(self) -> bool {
        matches!(self, BrowseTarget::Symbols | BrowseTarget::DefaultSymbol)
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
    #[serde(default)]
    dsn: String,
    #[serde(default = "default_pg_host")]
    pg_host: String,
    #[serde(default = "default_pg_port")]
    pg_port: String,
    #[serde(default)]
    pg_database: String,
    #[serde(default)]
    pg_user: String,
    #[serde(default)]
    pg_password: String,
    #[serde(default = "default_database_type")]
    database_type: String,
    #[serde(default)]
    symbols_folder: String,
    #[serde(default)]
    footprints_folder: String,
    #[serde(default)]
    default_symbol: String,
    #[serde(default)]
    default_footprint: String,
    #[serde(default)]
    default_symbol_path: String,
    #[serde(default)]
    default_footprint_path: String,
    /// Minutes to wait before reminding about an available update again
    /// after the user dismissed it with "Remind me later".
    #[serde(default = "default_remind_minutes")]
    remind_after_minutes: u64,
}

fn default_database_type() -> String {
    "sqlite".to_string()
}

fn default_pg_host() -> String {
    "localhost".to_string()
}

fn default_pg_port() -> String {
    "5432".to_string()
}

fn default_remind_minutes() -> u64 {
    30
}

#[derive(Clone, Debug)]
struct CategoryInfo {
    name: String,
}

enum DeleteRequest {
    Category(String),
    Component { category: String, id: String },
    Field { category: String, column: String },
}

pub struct AltiumDbApp {
    conn: Option<db::Connection>,
    db_path: PathBuf,
    config_path: PathBuf,

    categories: Vec<CategoryInfo>,
    components: Vec<db::Component>,
    custom_columns: Vec<String>,
    custom_values: Vec<(String, String)>,

    selected_category: Option<String>,
    selected_component_id: Option<String>,
    selected_component_index: Option<usize>,

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
    lcsc_import_open: bool,
    lcsc_code_input: String,

    editing_category: Option<String>,
    editing_component: Option<String>,
    editing_field: Option<String>,
    pending_delete: Option<DeleteRequest>,

    status_msg: String,

    viewer_open: bool,
    viewer_open_at: std::time::Instant,
    viewer_texture: Option<egui::TextureHandle>,
    viewer_svg: Option<String>,
    viewer_raster_size: egui::Vec2,
    viewer_title: String,
    viewer_symbol_parts: u32,
    viewer_symbol_part: u32,
    viewer_library: String,

    browse_open: bool,
    browse_open_at: std::time::Instant,
    browse_target: BrowseTarget,
    browse_path: PathBuf,
    browse_entries: Vec<(String, bool)>,
    browse_selected: Option<String>,
    browse_texture: Option<egui::TextureHandle>,
    browse_svg: Option<String>,
    browse_raster_size: egui::Vec2,
    browse_symbol_parts: u32,
    browse_symbol_part: u32,

    theme: Theme,
    settings_db_path: String,
    settings_pg_host: String,
    settings_pg_port: String,
    settings_pg_database: String,
    settings_pg_user: String,
    settings_pg_password: String,
    settings_database_type: String,
    settings_symbols_folder: String,
    settings_footprints_folder: String,
    settings_default_symbol: String,
    settings_default_footprint: String,
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

    update_checker: crate::update::SharedRelease,
    release: Option<crate::update::ReleaseInfo>,
    update_dismissed: bool,
    remind_at: Option<std::time::Instant>,
    remind_timeout: std::time::Duration,
    settings_remind_minutes: u64,
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
    if !folder.is_empty() && full.strip_prefix(folder).is_ok() {
        if let Some(name) = full.file_name() {
            return name.to_string_lossy().to_string();
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

impl AltiumDbApp {
    fn conn(&self) -> &db::Connection {
        self.conn.as_ref().expect("database connection")
    }

    pub fn new(conn: db::Connection, db_path: PathBuf, config_path: PathBuf) -> Self {
        let cfg = Self::load_config_data(&config_path);
        let config_dsn = cfg.dsn;
        let legacy_pg = db::parse_postgres_connection_string(&config_dsn);
        let config_database_type = cfg.database_type;
        let theme = cfg.theme;
        let settings_symbols_folder = cfg.symbols_folder;
        let settings_footprints_folder = cfg.footprints_folder;
        let settings_default_symbol = if cfg.default_symbol_path.is_empty() {
            cfg.default_symbol
        } else {
            resolve_library_path(&settings_symbols_folder, &cfg.default_symbol_path)
        };
        let settings_default_footprint = if cfg.default_footprint_path.is_empty() {
            cfg.default_footprint
        } else {
            resolve_library_path(&settings_footprints_folder, &cfg.default_footprint_path)
        };
        let remind_minutes = cfg.remind_after_minutes.clamp(1, 1440);

        let mut app = Self {
            conn: Some(conn),
            db_path: db_path.clone(),
            config_path: config_path.clone(),
            categories: Vec::new(),
            components: Vec::new(),
            custom_columns: Vec::new(),
            custom_values: Vec::new(),
            selected_category: None,
            selected_component_id: None,
            selected_component_index: None,
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
            lcsc_import_open: false,
            lcsc_code_input: String::new(),
            editing_category: None,
            editing_component: None,
            editing_field: None,
            pending_delete: None,
            status_msg: String::new(),
            viewer_open: false,
            viewer_open_at: std::time::Instant::now(),
            viewer_texture: None,
            viewer_svg: None,
            viewer_raster_size: egui::Vec2::ZERO,
            viewer_title: String::new(),
            viewer_symbol_parts: 1,
            viewer_symbol_part: 1,
            viewer_library: String::new(),
            browse_open: false,
            browse_open_at: std::time::Instant::now(),
            browse_target: BrowseTarget::Symbols,
            browse_path: PathBuf::new(),
            browse_entries: Vec::new(),
            browse_selected: None,
            browse_texture: None,
            browse_svg: None,
            browse_raster_size: egui::Vec2::ZERO,
            browse_symbol_parts: 1,
            browse_symbol_part: 1,
            theme,
            settings_db_path: db_path.display().to_string(),
            settings_pg_host: if cfg.pg_host.is_empty() {
                legacy_pg[0].clone()
            } else {
                cfg.pg_host
            },
            settings_pg_port: if cfg.pg_port.is_empty() {
                legacy_pg[1].clone()
            } else {
                cfg.pg_port
            },
            settings_pg_database: if cfg.pg_database.is_empty() {
                legacy_pg[2].clone()
            } else {
                cfg.pg_database
            },
            settings_pg_user: if cfg.pg_user.is_empty() {
                legacy_pg[3].clone()
            } else {
                cfg.pg_user
            },
            settings_pg_password: if cfg.pg_password.is_empty() {
                legacy_pg[4].clone()
            } else {
                cfg.pg_password
            },
            settings_database_type: config_database_type,
            settings_symbols_folder,
            settings_footprints_folder,
            settings_default_symbol,
            settings_default_footprint,
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
            update_checker: crate::update::spawn_check(),
            release: None,
            update_dismissed: false,
            remind_at: None,
            remind_timeout: std::time::Duration::from_secs(remind_minutes * 60),
            settings_remind_minutes: remind_minutes,
        };
        app.refresh_categories();
        app
    }

    fn refresh_categories(&mut self) {
        self.categories = db::get_tables(self.conn())
            .unwrap_or_default()
            .into_iter()
            .map(|name| CategoryInfo { name })
            .collect();
    }

    fn refresh_components(&mut self) {
        if let Some(ref cat) = self.selected_category {
            if let Err(e) = db::ensure_table(self.conn(), cat) {
                self.components.clear();
                self.custom_columns.clear();
                self.set_status_err(format!("Failed to open category '{}': {}", cat, e));
                return;
            }
            match db::get_components(self.conn(), cat) {
                Ok(components) => {
                    self.components = components;
                    self.selected_component_index = self
                        .selected_component_id
                        .as_ref()
                        .and_then(|id| self.components.iter().position(|item| &item.id == id));
                }
                Err(e) => {
                    self.components.clear();
                    self.set_status_err(format!("Failed to read category '{}': {}", cat, e));
                    return;
                }
            }
            match db::get_columns(self.conn(), cat) {
                Ok(columns) => {
                    self.custom_columns = columns
                        .into_iter()
                        .filter(|c| c != "id" && !db::BASE_COLUMNS.contains(&c.as_str()))
                        .collect();
                }
                Err(e) => {
                    self.custom_columns.clear();
                    self.set_status_err(format!("Failed to read category fields: {}", e));
                }
            }
        } else {
            self.components.clear();
            self.custom_columns.clear();
        }
    }

    fn refresh_custom_values(&mut self) {
        self.custom_values.clear();
        if let (Some(cat), Some(comp_id)) = (
            self.selected_category.clone(),
            self.selected_component_id.clone(),
        ) {
            let mut read_error = None;
            for col in &self.custom_columns {
                match db::get_custom_value(self.conn(), &cat, &comp_id, col) {
                    Ok(val) => self.custom_values.push((col.clone(), val)),
                    Err(e) => {
                        self.custom_values.push((col.clone(), String::new()));
                        read_error = Some(format!("Failed to read field '{}': {}", col, e));
                    }
                }
            }
            if let Some(error) = read_error {
                self.set_status_err(error);
            }
        }
    }

    fn create_or_update_component(&mut self, category: &str, mpn: String) {
        let result = db::ensure_table(self.conn(), category).and_then(|()| {
            if let Some(id) = self.editing_component.clone() {
                db::update_component(
                    self.conn(),
                    category,
                    &db::Component {
                        id,
                        mpn,
                        ..db::Component::default()
                    },
                )
                .map(|_| ())
            } else {
                db::add_component(
                    self.conn(),
                    category,
                    &db::Component {
                        mpn,
                        library_ref: std::path::Path::new(self.settings_default_symbol.trim())
                            .file_stem()
                            .map(|s| s.to_string_lossy().to_string())
                            .unwrap_or_default(),
                        library_path: relative_library_path(
                            &self.settings_symbols_folder,
                            self.settings_default_symbol.trim(),
                        ),
                        footprint_ref: std::path::Path::new(self.settings_default_footprint.trim())
                            .file_stem()
                            .map(|s| s.to_string_lossy().to_string())
                            .unwrap_or_default(),
                        footprint_path: relative_library_path(
                            &self.settings_footprints_folder,
                            self.settings_default_footprint.trim(),
                        ),
                        ..db::Component::default()
                    },
                )
                .map(|_| ())
            }
        });

        match result {
            Ok(()) => {
                self.editing_component = None;
                self.component_input.clear();
                self.refresh_components();
                self.set_status_ok();
            }
            Err(e) => self.set_status_err(format!("Failed to save component: {}", e)),
        }
    }

    /// Switches to the Fill (edit) mode and opens the given component for
    /// editing in its category. Used from parametric search results.
    fn open_component_in_fill(&mut self, cat: &str, comp: &db::Component) {
        self.mode = AppMode::Fill;
        self.selected_category = Some(cat.to_string());
        self.refresh_components();

        let comp_id = comp.id.clone();
        self.selected_component_id = Some(comp_id);
        self.selected_component_index = self.components.iter().position(|item| item.id == comp.id);
        self.editing_component = Some(if comp.id.is_empty() {
            format!("__mpn__{}", comp.mpn)
        } else {
            comp.id.clone()
        });
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
        self.component_link1_description_input = comp.component_link1_description.clone();
        self.component_link1_url_input = comp.component_link1_url.clone();
        self.component_link2_description_input = comp.component_link2_description.clone();
        self.component_link2_url_input = comp.component_link2_url.clone();
        self.component_link3_description_input = comp.component_link3_description.clone();
        self.component_link3_url_input = comp.component_link3_url.clone();
        self.refresh_custom_values();
        self.set_status(format!("Editing {} in '{}'", comp.mpn, cat));
    }

    fn search_param_columns(&self, cat: &str) -> Vec<(String, String)> {
        let excluded = ["id", "MPN", "Verified"];
        let cols = db::get_columns(self.conn(), cat).unwrap_or_default();
        cols.into_iter()
            .filter(|c| !excluded.contains(&c.as_str()))
            .map(|c| {
                let name = c.clone();
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
                db::search_components(self.conn(), cat, &self.selected_search_filters())
                    .unwrap_or_default()
                    .into_iter()
                    .map(|c| (cat.clone(), c))
                    .collect();
        } else if self.search_all {
            let q = self.search_all_query.trim();
            if q.is_empty() {
                self.search_results.clear();
            } else {
                self.search_results = db::search_all_by_mpn(self.conn(), q).unwrap_or_default();
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
                    values: db::get_distinct_values(self.conn(), cat, &column).unwrap_or_default(),
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
            db::get_custom_value(self.conn(), cat, &comp.id, column).unwrap_or_default()
        }
    }

    fn reopen_database(&mut self) {
        let db_path = PathBuf::from(&self.settings_db_path);
        let connection_string = if self.settings_database_type.eq_ignore_ascii_case("sqlite") {
            db_path.display().to_string()
        } else {
            db::postgres_connection_string(
                &self.settings_pg_host,
                &self.settings_pg_port,
                &self.settings_pg_database,
                &self.settings_pg_user,
                &self.settings_pg_password,
            )
        };
        match db::open_database_with_config(&self.settings_database_type, &connection_string) {
            Ok(conn) => {
                if let Err(e) = db::migrate(&conn) {
                    self.set_status_err(format!("Failed to migrate database: {}", e));
                    return;
                }
                self.conn = Some(conn);
                self.db_path = db_path;
                self.selected_category = None;
                self.selected_component_id = None;
                self.selected_component_index = None;
                self.components.clear();
                self.custom_columns.clear();
                self.custom_values.clear();
                self.search_category = None;
                self.search_params.clear();
                self.search_results.clear();
                self.search_selected = None;
                self.refresh_categories();
                self.set_status_ok();
            }
            Err(e) => self.set_status_err(e),
        }
    }

    #[allow(dead_code)]
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
            if category.is_empty() || !db::table_exists(self.conn(), &category).unwrap_or(false) {
                skipped_no_table += 1;
                continue;
            }
            if db::mpn_exists(self.conn(), &category, &item_id).unwrap_or(false) {
                skipped_dup += 1;
                continue;
            }
            values.insert("MPN".to_string(), item_id);
            let default_symbol = self.settings_default_symbol.trim();
            if !default_symbol.is_empty() && !values.contains_key("Library Ref") {
                values.insert(
                    "Library Ref".to_string(),
                    std::path::Path::new(default_symbol)
                        .file_stem()
                        .map(|s| s.to_string_lossy().to_string())
                        .unwrap_or_else(|| default_symbol.to_string()),
                );
            }
            if !default_symbol.is_empty() && !values.contains_key("Library Path") {
                values.insert(
                    "Library Path".to_string(),
                    relative_library_path(&self.settings_symbols_folder, default_symbol),
                );
            }
            let default_footprint = self.settings_default_footprint.trim();
            if !default_footprint.is_empty() && !values.contains_key("Footprint Ref") {
                values.insert(
                    "Footprint Ref".to_string(),
                    std::path::Path::new(default_footprint)
                        .file_stem()
                        .map(|s| s.to_string_lossy().to_string())
                        .unwrap_or_else(|| default_footprint.to_string()),
                );
            }
            if !default_footprint.is_empty() && !values.contains_key("Footprint Path") {
                values.insert(
                    "Footprint Path".to_string(),
                    relative_library_path(&self.settings_footprints_folder, default_footprint),
                );
            }
            match db::insert_component_row(self.conn(), &category, &values) {
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

    fn import_lcsc_component(&mut self) {
        let codes = self
            .lcsc_code_input
            .split(',')
            .map(str::trim)
            .filter(|code| !code.is_empty())
            .map(str::to_uppercase)
            .collect::<Vec<_>>();
        if codes.is_empty() {
            self.set_status("Enter one or more LCSC part numbers separated by commas");
            return;
        }
        let mut imported = 0;
        let mut updated = 0;
        let mut failed = Vec::new();
        for code in codes {
            match self.import_lcsc_component_single(&code) {
                Ok(true) => imported += 1,
                Ok(false) => updated += 1,
                Err(e) => failed.push(format!("{}: {}", code, e)),
            }
        }
        self.lcsc_import_open = false;
        self.lcsc_code_input.clear();
        self.refresh_categories();
        self.refresh_components();
        if failed.is_empty() {
            self.set_status(format!(
                "LCSC import complete: {} added, {} updated",
                imported, updated
            ));
        } else {
            self.set_status_err(format!(
                "LCSC import: {} added, {} updated; failed: {}",
                imported,
                updated,
                failed.join("; ")
            ));
        }
    }

    fn import_lcsc_component_single(&mut self, code: &str) -> Result<bool, String> {
        let url = format!(
            "https://wmsc.lcsc.com/ftps/wm/product/detail?productCode={}",
            code
        );
        let response = match ureq::get(&url)
            .set("Accept", "application/json")
            .set("User-Agent", "Mozilla/5.0")
            .call()
        {
            Ok(response) => response,
            Err(e) => return Err(format!("request failed: {}", e)),
        };
        let payload: serde_json::Value = match response.into_json() {
            Ok(value) => value,
            Err(e) => return Err(format!("invalid response: {}", e)),
        };
        let Some(product) = payload.get("result").filter(|v| !v.is_null()) else {
            return Err("component was not found".to_string());
        };
        let category = product
            .get("catalogName")
            .or_else(|| product.get("catalogNameEn"))
            .or_else(|| product.get("categoryNameEn"))
            .and_then(serde_json::Value::as_str)
            .map(str::trim)
            .filter(|name| !name.is_empty())
            .map(str::to_string)
            .or_else(|| {
                product
                    .get("parentCatalogList")
                    .and_then(serde_json::Value::as_array)
                    .and_then(|items| items.last())
                    .and_then(|item| item.get("catalogNameEn"))
                    .and_then(serde_json::Value::as_str)
                    .map(str::trim)
                    .filter(|name| !name.is_empty())
                    .map(str::to_string)
            })
            .ok_or_else(|| "LCSC response has no category".to_string())?;
        let category_exists = db::table_exists(self.conn(), &category)
            .map_err(|e| format!("cannot inspect category: {}", e))?;
        db::ensure_table(self.conn(), &category)
            .map_err(|e| format!("cannot create category: {}", e))?;

        let text = |name: &str| {
            product
                .get(name)
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default()
                .trim()
                .to_string()
        };
        let mut values = std::collections::HashMap::new();
        let mpn = {
            let model = text("productModel");
            if model.is_empty() {
                code.to_string()
            } else {
                model
            }
        };
        values.insert("Verified".to_string(), "0".to_string());
        values.insert("MPN".to_string(), mpn.clone());
        values.insert("Manufacturer".to_string(), text("brandNameEn"));
        values.insert("Description".to_string(), text("productNameEn"));
        values.insert(
            "ComponentLink1Description".to_string(),
            "Datasheet".to_string(),
        );
        values.insert("ComponentLink1URL".to_string(), text("pdfUrl"));
        let package = text("encapStandard");
        if !package.is_empty() {
            values.insert("Package".to_string(), package);
        }

        let default_symbol = self.settings_default_symbol.trim();
        if !default_symbol.is_empty() {
            values.insert(
                "Library Ref".to_string(),
                std::path::Path::new(default_symbol)
                    .file_stem()
                    .map(|s| s.to_string_lossy().to_string())
                    .unwrap_or_else(|| default_symbol.to_string()),
            );
            values.insert(
                "Library Path".to_string(),
                relative_library_path(&self.settings_symbols_folder, default_symbol),
            );
        }
        let default_footprint = self.settings_default_footprint.trim();
        if !default_footprint.is_empty() {
            values.insert(
                "Footprint Ref".to_string(),
                std::path::Path::new(default_footprint)
                    .file_stem()
                    .map(|s| s.to_string_lossy().to_string())
                    .unwrap_or_else(|| default_footprint.to_string()),
            );
            values.insert(
                "Footprint Path".to_string(),
                relative_library_path(&self.settings_footprints_folder, default_footprint),
            );
        }
        if let Some(params) = product.get("paramVOList").and_then(|v| v.as_array()) {
            for param in params {
                let name = param
                    .get("paramNameEn")
                    .or_else(|| param.get("paramName"))
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or_default()
                    .trim();
                let value = param
                    .get("paramValueEn")
                    .or_else(|| param.get("paramValue"))
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or_default()
                    .trim();
                if !name.is_empty() && !value.is_empty() {
                    values.insert(name.to_string(), value.to_string());
                }
            }
        }
        if !category_exists {
            for column in values.keys() {
                if !db::BASE_COLUMNS.contains(&column.as_str())
                    && column != "id"
                    && !db::get_columns(self.conn(), &category)
                        .map_err(|e| format!("cannot inspect fields: {}", e))?
                        .contains(column)
                {
                    db::add_column(self.conn(), &category, column)
                        .map_err(|e| format!("cannot create field '{}': {}", column, e))?;
                }
            }
        }

        let component = db::Component {
            mpn: values.get("MPN").cloned().unwrap_or_default(),
            manufacturer: values.get("Manufacturer").cloned().unwrap_or_default(),
            description: values.get("Description").cloned().unwrap_or_default(),
            verified: false,
            library_ref: values.get("Library Ref").cloned().unwrap_or_default(),
            library_path: values.get("Library Path").cloned().unwrap_or_default(),
            footprint_ref: values.get("Footprint Ref").cloned().unwrap_or_default(),
            footprint_path: values.get("Footprint Path").cloned().unwrap_or_default(),
            component_link1_description: values
                .get("ComponentLink1Description")
                .cloned()
                .unwrap_or_default(),
            component_link1_url: values.get("ComponentLink1URL").cloned().unwrap_or_default(),
            ..db::Component::default()
        };
        let existing = db::get_components(self.conn(), &category)
            .map_err(|e| format!("cannot read category: {}", e))?
            .into_iter()
            .find(|item| item.mpn.eq_ignore_ascii_case(&mpn));
        if let Some(existing) = existing {
            db::update_component(
                self.conn(),
                &category,
                &db::Component {
                    id: existing.id.clone(),
                    ..component.clone()
                },
            )
            .map_err(|e| format!("update failed: {}", e))?;
            self.save_lcsc_custom_values(&category, &existing.id, &values)?;
            Ok(false)
        } else {
            let id =
                db::add_component_with_custom_values(self.conn(), &category, &component, &values)
                    .map_err(|e| format!("save failed: {}", e))?;
            self.save_lcsc_custom_values(&category, &id.to_string(), &values)?;
            Ok(true)
        }
    }

    fn save_lcsc_custom_values(
        &self,
        category: &str,
        id: &str,
        values: &std::collections::HashMap<String, String>,
    ) -> Result<(), String> {
        if let Ok(columns) = db::get_columns(self.conn(), category) {
            for (column, value) in values {
                if column != "id"
                    && !db::BASE_COLUMNS.contains(&column.as_str())
                    && columns.contains(column)
                {
                    if let Err(e) = db::set_custom_value(self.conn(), category, id, column, value) {
                        return Err(format!("failed to save field '{}': {}", column, e));
                    }
                }
            }
        }
        Ok(())
    }

    #[allow(dead_code)]
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
            dsn: db::postgres_connection_string(
                &self.settings_pg_host,
                &self.settings_pg_port,
                &self.settings_pg_database,
                &self.settings_pg_user,
                &self.settings_pg_password,
            ),
            pg_host: self.settings_pg_host.clone(),
            pg_port: self.settings_pg_port.clone(),
            pg_database: self.settings_pg_database.clone(),
            pg_user: self.settings_pg_user.clone(),
            pg_password: self.settings_pg_password.clone(),
            database_type: self.settings_database_type.clone(),
            symbols_folder: self.settings_symbols_folder.clone(),
            footprints_folder: self.settings_footprints_folder.clone(),
            default_symbol: self.settings_default_symbol.clone(),
            default_footprint: self.settings_default_footprint.clone(),
            default_symbol_path: relative_library_path(
                &self.settings_symbols_folder,
                &self.settings_default_symbol,
            ),
            default_footprint_path: relative_library_path(
                &self.settings_footprints_folder,
                &self.settings_default_footprint,
            ),
            remind_after_minutes: self.settings_remind_minutes,
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
        self.browse_symbol_parts = 1;
        self.browse_symbol_part = 1;
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
            let selected_part = self.browse_symbol_part;
            self.browse_symbol_parts = render::symbol_part_count(&full_str, &stem)
                .unwrap_or(1)
                .max(1);
            self.browse_symbol_part = selected_part.min(self.browse_symbol_parts);
            render::render_symbol(
                &full_str,
                &stem,
                &out,
                bg,
                (self.browse_symbol_parts > 1).then_some(self.browse_symbol_part),
            )
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
            match self.browse_target {
                BrowseTarget::DefaultSymbol => {
                    self.settings_default_symbol = full_str;
                }
                BrowseTarget::Symbols => {
                    self.library_ref_input = stem;
                    self.library_path_input =
                        relative_library_path(&self.settings_symbols_folder, &full_str);
                }
                _ => unreachable!("non-symbol browse target"),
            }
        } else {
            let rel = relative_library_path(&self.settings_footprints_folder, &full_str);
            match self.browse_target {
                BrowseTarget::DefaultFootprint => {
                    self.settings_default_footprint =
                        self.browse_path.join(&name).to_string_lossy().to_string();
                }
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
            self.viewer_symbol_parts = render::symbol_part_count(&lib, name).unwrap_or(1).max(1);
            self.viewer_symbol_part = 1;
            render::render_symbol(
                &lib,
                name,
                &out,
                bg,
                (self.viewer_symbol_parts > 1).then_some(self.viewer_symbol_part),
            )
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
                    self.viewer_library = lib;
                    self.viewer_open_at = std::time::Instant::now();
                    self.viewer_open = true;
                }
                Err(e) => self.set_status_err(format!("Failed to read preview: {}", e)),
            },
            Err(e) => self.set_status_err(format!("Failed to render: {}", e)),
        }
    }

    fn reload_viewer_symbol(&mut self, ctx: &egui::Context) {
        let out = render::temp_preview_path();
        match render::render_symbol(
            &self.viewer_library,
            &self.viewer_title,
            &out,
            self.preview_bg(ctx),
            (self.viewer_symbol_parts > 1).then_some(self.viewer_symbol_part),
        )
        .and_then(|()| {
            std::fs::read_to_string(&out).map_err(|e| format!("Failed to read preview: {e}"))
        }) {
            Ok(svg) => {
                self.viewer_svg = Some(svg);
                self.viewer_texture = None;
                self.viewer_raster_size = egui::Vec2::ZERO;
            }
            Err(e) => self.set_status_err(format!("Failed to render: {e}")),
        }
    }
}

impl eframe::App for AltiumDbApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.apply_theme(ctx);
        self.adapt_viewport(ctx);

        if self.release.is_none() && !self.update_dismissed {
            if let Ok(mut guard) = self.update_checker.lock() {
                if let Some(info) = guard.take() {
                    self.release = Some(info);
                    self.remind_at = None;
                }
            }
        }

        egui::TopBottomPanel::top("menu_bar").show(ctx, |ui| {
            egui::menu::bar(ui, |ui| {
                ui.menu_button("File", |ui| {
                    if ui.button("Import LCSC component...").clicked() {
                        self.lcsc_import_open = true;
                        ui.close_menu();
                    }
                    ui.separator();
                    if ui.button("Quit").clicked() {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
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
                ui.heading("Database");
                egui::ComboBox::from_label("Database type")
                    .selected_text(&self.settings_database_type)
                    .show_ui(ui, |ui| {
                        ui.selectable_value(
                            &mut self.settings_database_type,
                            "sqlite".to_string(),
                            "SQLite",
                        );
                        ui.selectable_value(
                            &mut self.settings_database_type,
                            "postgres".to_string(),
                            "PostgreSQL",
                        );
                    });

                let row = (ui.cursor().min.x, ui.available_width());
                ui.horizontal(|ui| {
                    if self.settings_database_type == "sqlite" {
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
                    }
                });

                if self.settings_database_type != "sqlite" {
                    for (label, value, password) in [
                        ("Host", &mut self.settings_pg_host, false),
                        ("Port", &mut self.settings_pg_port, false),
                        ("Database", &mut self.settings_pg_database, false),
                        ("User", &mut self.settings_pg_user, false),
                        ("Password", &mut self.settings_pg_password, true),
                    ] {
                        ui.horizontal(|ui| {
                            ui.label(label);
                            let mut edit = egui::TextEdit::singleline(value)
                                .desired_width(ui.available_width());
                            if password {
                                edit = edit.password(true);
                            }
                            ui.add(edit);
                        });
                    }
                }

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

                ui.horizontal(|ui| {
                    ui.label("Default Symbol:");
                    if ui.button("Browse").clicked() {
                        self.open_browse(ctx, BrowseTarget::DefaultSymbol);
                    }
                    ui.add(
                        egui::TextEdit::singleline(&mut self.settings_default_symbol)
                            .hint_text("Full path to the default .SchLib file")
                            .desired_width(ui.available_width()),
                    );
                });

                ui.horizontal(|ui| {
                    ui.label("Default Footprint:");
                    if ui.button("Browse").clicked() {
                        self.open_browse(ctx, BrowseTarget::DefaultFootprint);
                    }
                    ui.add(
                        egui::TextEdit::singleline(&mut self.settings_default_footprint)
                            .hint_text("Full path to the default .PcbLib file")
                            .desired_width(ui.available_width()),
                    );
                });

                ui.separator();
                ui.heading("Updates");
                ui.horizontal(|ui| {
                    ui.label("Remind me about updates after (minutes):");
                    ui.add(
                        egui::DragValue::new(&mut self.settings_remind_minutes)
                            .range(1..=1440)
                            .speed(1.0),
                    );
                });

                ui.separator();
                if ui.button("Save").clicked() {
                    save_clicked = true;
                    self.remind_timeout = std::time::Duration::from_secs(
                        self.settings_remind_minutes.clamp(1, 1440) * 60,
                    );
                    self.reopen_database();
                    self.save_config_data();
                    self.set_status("Settings saved");
                }
            });

            if save_clicked || modal.should_close() {
                self.settings_open = false;
            }
        }

        if self.lcsc_import_open {
            let mut import_clicked = false;
            let modal = egui::Modal::new(egui::Id::new("lcsc_import_modal")).show(ctx, |ui| {
                ui.set_min_width(self.modal_size(ctx, egui::vec2(460.0, 220.0)).x);
                ui.heading("Import LCSC component");
                ui.label("LCSC part number:");
                let code_response = ui.add(
                    egui::TextEdit::singleline(&mut self.lcsc_code_input)
                        .hint_text("For example, C2856764")
                        .desired_width(ui.available_width()),
                );
                ui.label("Category is detected automatically from LCSC.");
                ui.horizontal(|ui| {
                    if ui.button("Import").clicked()
                        || (code_response.lost_focus()
                            && ui.input(|i| i.key_pressed(egui::Key::Enter)))
                    {
                        import_clicked = true;
                    }
                    if ui.button("Cancel").clicked() {
                        self.lcsc_import_open = false;
                    }
                });
            });
            if import_clicked {
                self.import_lcsc_component();
            } else if modal.should_close() {
                self.lcsc_import_open = false;
            }
        }

        if self.about_open {
            let modal = egui::Modal::new(egui::Id::new("about_modal")).show(ctx, |ui| {
                ui.set_min_width(self.modal_size(ctx, egui::vec2(400.0, 240.0)).x);
                ui.heading("About AltiumDB");
                ui.label("AltiumDB вЂ” Altium Designer Database Library manager");
                ui.label("Manage component database, browse symbols,");
                ui.label("footprints and edit addition fields.");
                ui.separator();
                ui.label(format!("Version: {}", crate::update::current_version()));
                if ui.button("Check for updates").clicked() {
                    self.update_checker = crate::update::spawn_check();
                    self.update_dismissed = false;
                    self.set_status("Checking for updates...");
                }
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

        let show_update = self.release.is_some()
            && !self.update_dismissed
            && self
                .remind_at
                .is_none_or(|t| std::time::Instant::now() >= t);
        if show_update {
            let release = self.release.clone().unwrap();
            let mut open = true;
            egui::Window::new("Update available")
                .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
                .collapsible(false)
                .resizable(false)
                .open(&mut open)
                .show(ctx, |ui| {
                    ui.label(format!(
                        "A new version {} is available (you are running {}).",
                        release.version,
                        crate::update::current_version()
                    ));
                    if !release.name.is_empty() && release.name != release.version {
                        ui.label(format!("Release: {}", release.name));
                    }
                    ui.separator();
                    ui.horizontal_wrapped(|ui| {
                        if ui.button("Open release page").clicked() {
                            ui.ctx().open_url(egui::OpenUrl {
                                url: release.url.clone(),
                                new_tab: true,
                            });
                            self.update_dismissed = true;
                            self.release = None;
                        }
                        if ui.button("Remind me later").clicked() {
                            self.remind_at = Some(std::time::Instant::now() + self.remind_timeout);
                        }
                        if ui.button("Skip this version").clicked() {
                            self.update_dismissed = true;
                            self.release = None;
                        }
                    });
                });
            // Window closed via its [X] button: remind again after the timeout.
            if !open {
                self.remind_at = Some(std::time::Instant::now() + self.remind_timeout);
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
                                        db::rename_table(self.conn(), edit_name, &name).ok();
                                    }
                                    self.editing_category = None;
                                } else {
                                    if !db::table_exists(self.conn(), &name).unwrap_or(false) {
                                        db::ensure_table(self.conn(), &name).ok();
                                    }
                                }
                                self.category_input.clear();
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
                                        db::rename_table(self.conn(), edit_name, &name).ok();
                                    }
                                    self.editing_category = None;
                                } else {
                                    if !db::table_exists(self.conn(), &name).unwrap_or(false) {
                                        db::ensure_table(self.conn(), &name).ok();
                                    }
                                }
                                self.category_input.clear();
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
                    self.selected_component_index = None;
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
                    let exists = |n: &str| db::table_exists(self.conn(), n).unwrap_or(false);
                    let new_name = unique_name(&name, exists);
                    db::clone_table(self.conn(), &name, &new_name).ok();
                    self.refresh_categories();
                    self.set_status(format!("Category cloned as '{}'", new_name));
                }
                if let Some(name) = to_delete {
                    self.pending_delete = Some(DeleteRequest::Category(name));
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
                                    if let Some(cat) = self.selected_category.clone() {
                                        self.create_or_update_component(&cat, item_id);
                                    }
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
                                    if let Some(cat) = self.selected_category.clone() {
                                        self.create_or_update_component(&cat, item_id);
                                    }
                                }
                            }
                        });
                    } else {
                        ui.label("Select a category first");
                    }

                    ui.separator();

                    let selected_index = self.selected_component_index;
                    let mut to_select = None;
                    let mut to_select_index = None;
                    let mut to_delete = None;
                    let mut to_edit = None;
                    let mut to_clone_comp = None;
                    let mut hovered_comp: Option<String> = None;

                    egui::ScrollArea::vertical().show(ui, |ui| {
                        for (index, comp) in self.components.iter().enumerate() {
                            ui.push_id(index, |ui| {
                                let is_selected = selected_index == Some(index);
                                let response = ui.selectable_label(is_selected, &comp.mpn);
                                if response.clicked() {
                                    to_select = Some(comp.id.clone());
                                    to_select_index = Some(index);
                                }
                                if response.hovered() {
                                    hovered_comp = Some(if comp.id.is_empty() {
                                        comp.mpn.clone()
                                    } else {
                                        comp.id.clone()
                                    });
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
                                        to_delete = Some(if comp.id.is_empty() {
                                            comp.mpn.clone()
                                        } else {
                                            comp.id.clone()
                                        });
                                        ui.close_menu();
                                    }
                                });
                            });
                        }
                    });

                    if ui.ctx().input(|i| i.key_pressed(egui::Key::Delete)) {
                        to_delete = hovered_comp.or_else(|| {
                            selected_index.and_then(|index| {
                                self.components.get(index).map(|comp| {
                                    if comp.id.is_empty() {
                                        comp.mpn.clone()
                                    } else {
                                        comp.id.clone()
                                    }
                                })
                            })
                        });
                    }

                    if let Some(index) = to_select_index {
                        if let Some(id) = to_select {
                            self.selected_component_id = Some(id);
                            self.selected_component_index = Some(index);
                            self.refresh_custom_values();
                        }
                        if let Some(comp) = self.components.get(index).cloned() {
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
                        self.selected_component_index =
                            self.components.iter().position(|item| item.id == comp.id);
                        self.editing_component = Some(if comp_id.is_empty() {
                            format!("__mpn__{}", comp.mpn)
                        } else {
                            comp_id
                        });
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
                        let mut cloned = false;
                        let mut cloned_mpn = None;
                        if let Some(ref cat) = self.selected_category {
                            let exists =
                                |id: &str| db::mpn_exists(self.conn(), cat, id).unwrap_or(false);
                            let new_id = unique_name(&comp.mpn, exists);
                            let source_id = if comp.id.is_empty() {
                                comp.mpn.clone()
                            } else {
                                comp.id.clone()
                            };
                            match db::clone_component(self.conn(), cat, &source_id, &new_id) {
                                Ok(()) => {
                                    let source_key = if comp.id.is_empty() {
                                        format!("__mpn__{}", comp.mpn)
                                    } else {
                                        comp.id.clone()
                                    };
                                    let target_key = format!("__mpn__{}", new_id);
                                    for column in self.custom_columns.clone() {
                                        match db::get_custom_value(
                                            self.conn(),
                                            cat,
                                            &source_key,
                                            &column,
                                        )
                                        .and_then(
                                            |value| {
                                                db::set_custom_value(
                                                    self.conn(),
                                                    cat,
                                                    &target_key,
                                                    &column,
                                                    &value,
                                                )
                                            },
                                        ) {
                                            Ok(()) => {}
                                            Err(e) => {
                                                self.set_status_err(format!(
                                                    "Failed to clone field '{}': {}",
                                                    column, e
                                                ));
                                                break;
                                            }
                                        }
                                    }
                                    cloned = true;
                                    cloned_mpn = Some(new_id.clone());
                                    self.set_status(format!("Component cloned as '{}'", new_id));
                                }
                                Err(e) => {
                                    self.set_status_err(format!(
                                        "Failed to clone component: {}",
                                        e
                                    ));
                                }
                            }
                        }
                        if cloned {
                            self.refresh_components();
                            let new_id = cloned_mpn.unwrap_or_default();
                            if let Some(index) = self
                                .components
                                .iter()
                                .position(|component| component.mpn == new_id)
                            {
                                let component = self.components[index].clone();
                                self.selected_component_index = Some(index);
                                self.selected_component_id = Some(if component.id.is_empty() {
                                    format!("__mpn__{}", component.mpn)
                                } else {
                                    component.id.clone()
                                });
                                self.refresh_custom_values();
                            }
                        }
                    }
                    if let Some(id) = to_delete {
                        if let Some(category) = self.selected_category.clone() {
                            self.pending_delete = Some(DeleteRequest::Component { category, id });
                        }
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
                                    for (cat, comp) in &self.search_results {
                                        let select_key = format!("{}|{}", cat, comp.id);
                                        let is_selected = selected == Some(select_key.clone());
                                        let response = ui.selectable_label(
                                            is_selected,
                                            egui::RichText::new(format!("{}\n{}", comp.mpn, cat)),
                                        );
                                        if response.clicked() {
                                            to_select = Some(select_key);
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
                        .find(|(cat, comp)| {
                            self.search_selected.as_ref() == Some(&format!("{}|{}", cat, comp.id))
                        })
                        .cloned();
                    if let Some((cat, comp)) = found {
                        ui.horizontal(|ui| {
                            ui.heading(&comp.mpn);
                            if ui.button("Copy ID").clicked() {
                                ui.ctx().copy_text(comp.mpn.clone());
                                self.set_status(format!("MPN copied: {}", comp.mpn));
                            }
                            if ui.button("Edit").clicked() {
                                self.open_component_in_fill(&cat, &comp);
                            }
                        });
                        ui.label(egui::RichText::new(format!("Category: {}", cat)).weak());
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
                                        if let Some(ref old_col) = self.editing_field.clone() {
                                            if old_col != &field_name {
                                                db::rename_column(
                                                    self.conn(),
                                                    cat,
                                                    old_col,
                                                    &field_name,
                                                )
                                                .ok();
                                            }
                                        } else {
                                            db::add_column(self.conn(), cat, &field_name).ok();
                                        }
                                        self.refresh_components();
                                        self.refresh_custom_values();
                                        self.field_col_input.clear();
                                        self.editing_field = None;
                                        self.set_status_ok();
                                    }
                                }
                            });

                            ui.separator();

                            let mut values = self.custom_values.clone();
                            let mut changed = None;
                            let mut save_custom_values = false;
                            let mut to_delete_field = None;
                            let mut to_edit_field = None;
                            let mut hovered_field: Option<String> = None;

                            for (i, (col, val)) in values.iter_mut().enumerate() {
                                let display = col.clone();

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
                                            changed = Some((i, buf.clone(), r.lost_focus()));
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
                                    save_custom_values = true;
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
                                let save_result =
                                    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                                        db::update_component(
                                            self.conn(),
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
                                    }));
                                match save_result {
                                    Ok(Ok(())) => {
                                        if let Some(index) = self.selected_component_index {
                                            if let Some(component) = self.components.get_mut(index)
                                            {
                                                component.mpn = self.mpn_input.clone();
                                                component.manufacturer =
                                                    self.manufacturer_input.clone();
                                                component.verified = self.verified_input;
                                                component.library_ref =
                                                    self.library_ref_input.clone();
                                                component.library_path =
                                                    self.library_path_input.clone();
                                                component.footprint_ref =
                                                    self.footprint_ref_input.clone();
                                                component.footprint_path =
                                                    self.footprint_path_input.clone();
                                                component.footprint_ref2 =
                                                    self.footprint_ref2_input.clone();
                                                component.footprint_path2 =
                                                    self.footprint_path2_input.clone();
                                                component.footprint_ref3 =
                                                    self.footprint_ref3_input.clone();
                                                component.footprint_path3 =
                                                    self.footprint_path3_input.clone();
                                                component.description =
                                                    self.description_input.clone();
                                                component.component_link1_description =
                                                    self.component_link1_description_input.clone();
                                                component.component_link1_url =
                                                    self.component_link1_url_input.clone();
                                                component.component_link2_description =
                                                    self.component_link2_description_input.clone();
                                                component.component_link2_url =
                                                    self.component_link2_url_input.clone();
                                                component.component_link3_description =
                                                    self.component_link3_description_input.clone();
                                                component.component_link3_url =
                                                    self.component_link3_url_input.clone();
                                            }
                                        }
                                        self.set_status_ok();
                                    }
                                    Ok(Err(e)) => {
                                        self.set_status_err(format!(
                                            "Failed to save component: {}",
                                            e
                                        ));
                                    }
                                    Err(_) => {
                                        self.set_status_err(
                                            "Failed to save component: database operation panicked",
                                        );
                                    }
                                }
                            }

                            if hovered_field.is_some()
                                && ui.ctx().input(|i| i.key_pressed(egui::Key::Delete))
                            {
                                to_delete_field = hovered_field;
                            }

                            if let Some((i, new_val, should_save)) = changed {
                                self.custom_values[i].1 = new_val.clone();
                                if let Some((_, value)) = values.get_mut(i) {
                                    *value = new_val.clone();
                                }
                                if should_save {
                                    if let Some((col, _)) = values.get(i) {
                                        let col_clone = col.clone();
                                        let save_result = db::set_custom_value(
                                            self.conn(),
                                            cat,
                                            &comp_id,
                                            &col_clone,
                                            &new_val,
                                        );
                                        match save_result {
                                            Ok(()) => {
                                                self.set_status_ok();
                                            }
                                            Err(e) => {
                                                self.set_status_err(format!(
                                                    "Failed to save field '{}': {}",
                                                    col_clone, e
                                                ));
                                            }
                                        }
                                    }
                                }
                            }

                            if save_custom_values {
                                let custom_values = self.custom_values.clone();
                                for (col, value) in custom_values {
                                    match db::set_custom_value(
                                        self.conn(),
                                        cat,
                                        &comp_id,
                                        &col,
                                        &value,
                                    ) {
                                        Ok(()) => {}
                                        Err(e) => {
                                            self.set_status_err(format!(
                                                "Failed to save field '{}': {}",
                                                col, e
                                            ));
                                            break;
                                        }
                                    }
                                }
                            }

                            if let Some((col, _display)) = to_edit_field {
                                self.field_col_input = col.clone();
                                self.editing_field = Some(col);
                            }

                            if let Some(col) = to_delete_field {
                                self.pending_delete = Some(DeleteRequest::Field {
                                    category: cat.clone(),
                                    column: col,
                                });
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

        if let Some(request) = self.pending_delete.take() {
            let title = match &request {
                DeleteRequest::Category(name) => format!("Delete category '{}'?", name),
                DeleteRequest::Component { id, .. } => {
                    format!("Delete component '{}'?", id)
                }
                DeleteRequest::Field { column, .. } => format!("Delete field '{}'?", column),
            };
            let mut confirmed = false;
            let mut cancelled = false;
            let modal = egui::Modal::new(egui::Id::new("confirm_deletion_modal")).show(ctx, |ui| {
                ui.set_min_width(360.0);
                ui.heading("Confirm deletion");
                ui.label(title);
                ui.horizontal(|ui| {
                    if ui.button("Delete").clicked() {
                        confirmed = true;
                    }
                    if ui.button("Cancel").clicked() {
                        cancelled = true;
                    }
                });
            });
            if confirmed {
                match request {
                    DeleteRequest::Category(name) => match db::drop_table(self.conn(), &name) {
                        Ok(()) => {
                            self.refresh_categories();
                            self.selected_category = None;
                            self.components.clear();
                            self.selected_component_id = None;
                            self.custom_values.clear();
                            self.set_status_ok();
                        }
                        Err(e) => self.set_status_err(format!("Failed to delete category: {}", e)),
                    },
                    DeleteRequest::Component { category, id } => {
                        match db::delete_component(self.conn(), &category, &id) {
                            Ok(()) => {
                                self.refresh_components();
                                self.selected_component_id = None;
                                self.selected_component_index = None;
                                self.custom_values.clear();
                                self.set_status_ok();
                            }
                            Err(e) => {
                                self.set_status_err(format!("Failed to delete component: {}", e))
                            }
                        }
                    }
                    DeleteRequest::Field { category, column } => {
                        match db::drop_column(self.conn(), &category, &column) {
                            Ok(()) => {
                                self.refresh_components();
                                self.refresh_custom_values();
                                self.set_status_ok();
                            }
                            Err(e) => self.set_status_err(format!("Failed to delete field: {}", e)),
                        }
                    }
                }
            } else if !cancelled && !modal.should_close() {
                self.pending_delete = Some(request);
            }
        }

        // Viewer modal
        if self.viewer_open {
            let svg = self.viewer_svg.clone();
            let mut viewer_part_changed = false;
            let modal = egui::Modal::new(egui::Id::new("viewer_modal")).show(ctx, |ui| {
                ui.set_min_size(self.modal_size(ctx, egui::vec2(600.0, 450.0)));
                ui.horizontal(|ui| {
                    ui.heading(format!("Preview: {}", self.viewer_title));
                    if self.viewer_symbol_parts > 1 {
                        viewer_part_changed = egui::ComboBox::from_id_salt("viewer_symbol_part")
                            .selected_text(format!("Section {}", self.viewer_symbol_part))
                            .show_ui(ui, |ui| {
                                (1..=self.viewer_symbol_parts)
                                    .map(|part| {
                                        ui.selectable_value(
                                            &mut self.viewer_symbol_part,
                                            part,
                                            format!("Section {}", part),
                                        )
                                    })
                                    .any(|response| response.changed())
                            })
                            .inner
                            .unwrap_or(false);
                    }
                });
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
            if viewer_part_changed {
                self.reload_viewer_symbol(ctx);
            }
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
            let mut browse_part_changed = false;

            let modal = egui::Modal::new(egui::Id::new("browse_modal")).show(ctx, |ui| {
                ui.set_min_size(self.modal_size(ctx, egui::vec2(880.0, 560.0)));
                ui.heading(if self.browse_target.is_symbols() {
                    "Browse Symbols"
                } else {
                    "Browse Footprints"
                });
                if self.browse_target.is_symbols() && self.browse_symbol_parts > 1 {
                    browse_part_changed = egui::ComboBox::from_id_salt("browse_symbol_part")
                        .selected_text(format!("Section {}", self.browse_symbol_part))
                        .show_ui(ui, |ui| {
                            (1..=self.browse_symbol_parts)
                                .map(|part| {
                                    ui.selectable_value(
                                        &mut self.browse_symbol_part,
                                        part,
                                        format!("Section {}", part),
                                    )
                                })
                                .any(|response| response.changed())
                        })
                        .inner
                        .unwrap_or(false);
                }

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
                if browse_part_changed {
                    if let Some(name) = self.browse_selected.clone() {
                        self.load_browse_preview(ctx, &name);
                    }
                }
            }
            if apply_clicked && self.browse_open {
                self.apply_browse_selection();
            }
        }
    }
}

impl Drop for AltiumDbApp {
    fn drop(&mut self) {
        if let Some(conn) = self.conn.take() {
            db::checkpoint(&conn);
            drop(conn);
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
