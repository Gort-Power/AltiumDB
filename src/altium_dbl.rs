use std::path::{Path, PathBuf};

#[derive(Clone, Debug)]
pub struct AltiumDbl {
    pub connection_string: String,
    pub database_links: Vec<(String, String)>,
    pub libraries: Vec<Library>,
}

#[derive(Clone, Debug)]
pub struct Library {
    pub name: String,
    pub table: String,
    pub enabled: bool,
    pub user_where: bool,
    pub user_where_text: String,
    pub fields: Vec<Field>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Field {
    pub column: String,
    pub parameter: String,
    pub is_key: bool,
    pub visible_on_add: bool,
    pub add_mode: u8,
    pub remove_mode: u8,
    pub update_mode: u8,
}

impl Field {
    fn new(column: &str, parameter: &str, is_key: bool, visible_on_add: bool) -> Self {
        Field {
            column: column.to_string(),
            parameter: parameter.to_string(),
            is_key,
            visible_on_add,
            add_mode: 0,
            remove_mode: 0,
            update_mode: 0,
        }
    }
}

fn base_fields() -> Vec<Field> {
    vec![
        Field::new("MPN", "MPN", true, true),
        Field::new("Manufacturer", "Manufacturer", false, false),
        Field::new("Library Ref", "[Library Ref]", false, false),
        Field::new("Library Path", "[Library Path]", false, false),
        Field::new("Footprint Ref", "[Footprint Ref]", false, false),
        Field::new("Footprint Path", "[Footprint Path]", false, false),
        Field::new("Footprint Ref 2", "[Footprint Ref 2]", false, false),
        Field::new("Footprint Path 2", "[Footprint Path 2]", false, false),
        Field::new("Footprint Ref 3", "[Footprint Ref 3]", false, false),
        Field::new("Footprint Path 3", "[Footprint Path 3]", false, false),
        Field::new(
            "ComponentLink1Description",
            "ComponentLink1Description",
            false,
            false,
        ),
        Field::new("ComponentLink1URL", "ComponentLink1URL", false, false),
        Field::new(
            "ComponentLink2Description",
            "ComponentLink2Description",
            false,
            false,
        ),
        Field::new("ComponentLink2URL", "ComponentLink2URL", false, false),
        Field::new(
            "ComponentLink3Description",
            "ComponentLink3Description",
            false,
            false,
        ),
        Field::new("ComponentLink3URL", "ComponentLink3URL", false, false),
        Field::new("Description", "[Description]", false, false),
        Field::new("Verified", "Verified", false, false),
    ]
}

fn connection_string_for(dsn: &str) -> String {
    format!(
        "Provider=MSDASQL.1;Persist Security Info=False;Data Source={}",
        dsn
    )
}

fn default_dsn(db_path: &Path) -> String {
    db_path
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default()
}

fn default_database_links(connection_string: &str) -> Vec<(String, String)> {
    vec![
        (
            "ConnectionString".to_string(),
            connection_string.to_string(),
        ),
        ("AddMode".to_string(), "3".to_string()),
        ("RemoveMode".to_string(), "1".to_string()),
        ("UpdateMode".to_string(), "2".to_string()),
        ("ViewMode".to_string(), "0".to_string()),
        ("LeftQuote".to_string(), "[".to_string()),
        ("RightQuote".to_string(), "]".to_string()),
        ("QuoteTableNames".to_string(), "1".to_string()),
        ("UseTableSchemaName".to_string(), "0".to_string()),
        ("DefaultColumnType".to_string(), "VARCHAR(255)".to_string()),
        ("LibraryDatabaseType".to_string(), String::new()),
        ("LibraryDatabasePath".to_string(), String::new()),
        ("DatabasePathRelative".to_string(), "0".to_string()),
        ("TopPanelCollapsed".to_string(), "0".to_string()),
        ("LibrarySearchPath".to_string(), String::new()),
        ("OrcadMultiValueDelimiter".to_string(), ",".to_string()),
        ("SearchSubDirectories".to_string(), "0".to_string()),
        ("SchemaName".to_string(), String::new()),
        ("LastFocusedTable".to_string(), String::new()),
    ]
}

fn is_internal_table(name: &str) -> bool {
    name.to_ascii_lowercase().starts_with("sqlite_")
}

impl AltiumDbl {
    pub fn new(db_path: &Path) -> Self {
        let connection_string = connection_string_for(&default_dsn(db_path));
        AltiumDbl {
            database_links: default_database_links(&connection_string),
            connection_string,
            libraries: Vec::new(),
        }
    }

    pub fn load(path: &Path, db_path: &Path, dsn: &str) -> Result<Self, String> {
        let data = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
        let mut dbl = Self::from_ini(&data)?;
        let effective = if dsn.trim().is_empty() {
            default_dsn(db_path)
        } else {
            dsn.trim().to_string()
        };
        dbl.set_connection_string(connection_string_for(&effective));
        Ok(dbl)
    }

    pub fn set_dsn(&mut self, dsn: &str) {
        self.set_connection_string(connection_string_for(dsn.trim()));
    }

    pub fn save(&self, path: &Path) -> Result<(), String> {
        let content = self.to_ini();
        if std::fs::read_to_string(path)
            .map(|existing| existing == content)
            .unwrap_or(false)
        {
            return Ok(());
        }
        std::fs::write(path, content).map_err(|e| e.to_string())
    }

    pub fn set_connection_string(&mut self, cs: String) {
        self.connection_string = cs.clone();
        self.set_database_link("ConnectionString", &cs);
    }

    pub fn set_database_link(&mut self, key: &str, value: &str) {
        if let Some(entry) = self.database_links.iter_mut().find(|(k, _)| k == key) {
            entry.1 = value.to_string();
        } else {
            self.database_links
                .push((key.to_string(), value.to_string()));
        }
    }

    pub fn find_library(&self, table_name: &str) -> Option<&Library> {
        self.libraries.iter().find(|l| l.table == table_name)
    }

    pub fn find_library_mut(&mut self, table_name: &str) -> Option<&mut Library> {
        self.libraries.iter_mut().find(|l| l.table == table_name)
    }

    pub fn add_library(&mut self, lib: Library) {
        self.libraries.push(lib);
    }

    pub fn remove_library(&mut self, table_name: &str) {
        self.libraries.retain(|l| l.table != table_name);
    }

    pub fn ensure_base_fields(&mut self) {
        for lib in &mut self.libraries {
            let mut seen = std::collections::HashSet::new();
            lib.fields.retain(|f| seen.insert(f.column.clone()));
            let mut existing = std::mem::take(&mut lib.fields);
            let mut ordered: Vec<Field> = Vec::new();
            for bf in base_fields() {
                if let Some(pos) = existing.iter().position(|f| f.column == bf.column) {
                    ordered.push(existing.remove(pos));
                } else {
                    ordered.push(bf);
                }
            }
            ordered.append(&mut existing);
            lib.fields = ordered;
        }
    }

    pub fn to_ini(&self) -> String {
        let mut s = String::new();
        s.push_str("[OutputDatabaseLinkFile]\n");
        s.push_str("Version=1.1\n");
        s.push_str("[DatabaseLinks]\n");
        for (key, value) in &self.database_links {
            let value = if key == "ConnectionString" {
                &self.connection_string
            } else {
                value
            };
            s.push_str(&format!("{}={}\n", key, value));
        }

        for (i, lib) in (1usize..).zip(self.libraries.iter()) {
            s.push_str(&format!("[Table{}]\n", i));
            s.push_str("SchemaName=\n");
            s.push_str(&format!("TableName={}\n", lib.table));
            s.push_str(&format!(
                "Enabled={}\n",
                if lib.enabled { "True" } else { "False" }
            ));
            s.push_str(&format!(
                "UserWhere={}\n",
                if lib.user_where { "1" } else { "0" }
            ));
            s.push_str(&format!("UserWhereText={}\n", lib.user_where_text));
        }

        let mut fm = 1usize;
        for lib in &self.libraries {
            for f in &lib.fields {
                let field_type = if f.is_key { 0 } else { 1 };
                let visible = if f.visible_on_add { "True" } else { "False" };
                s.push_str(&format!("[FieldMap{}]\n", fm));
                s.push_str(&format!(
                    "Options=FieldName={}.{}|TableNameOnly={}|FieldNameOnly={}|FieldType={}|ParameterName={}|VisibleOnAdd={}|AddMode={}|RemoveMode={}|UpdateMode={}\n",
                    lib.table, f.column, lib.table, f.column, field_type, f.parameter, visible,
                    f.add_mode, f.remove_mode, f.update_mode
                ));
                fm += 1;
            }
        }
        s
    }

    pub fn from_ini(content: &str) -> Result<Self, String> {
        let mut dbl = AltiumDbl {
            connection_string: String::new(),
            database_links: Vec::new(),
            libraries: Vec::new(),
        };
        let mut section = String::new();
        let mut last_table: Option<usize> = None;
        for raw in content.lines() {
            let line = raw.trim();
            if line.is_empty() {
                continue;
            }
            if line.starts_with('[') && line.ends_with(']') {
                section = line[1..line.len() - 1].to_string();
                last_table = None;
                continue;
            }
            let (key, val) = match line.split_once('=') {
                Some((k, v)) => (k.trim(), v.trim()),
                None => continue,
            };
            if section == "DatabaseLinks" {
                if key == "ConnectionString" {
                    dbl.connection_string = val.to_string();
                }
                dbl.database_links.push((key.to_string(), val.to_string()));
            } else if section.starts_with("Table") {
                match key {
                    "TableName" => {
                        if is_internal_table(val) {
                            last_table = None;
                        } else {
                            dbl.libraries.push(Library {
                                name: val.to_string(),
                                table: val.to_string(),
                                enabled: true,
                                user_where: false,
                                user_where_text: String::new(),
                                fields: Vec::new(),
                            });
                            last_table = Some(dbl.libraries.len() - 1);
                        }
                    }
                    "Enabled" => {
                        if let Some(idx) = last_table {
                            dbl.libraries[idx].enabled = val.eq_ignore_ascii_case("true");
                        }
                    }
                    "UserWhere" => {
                        if let Some(idx) = last_table {
                            dbl.libraries[idx].user_where = val == "1";
                        }
                    }
                    "UserWhereText" => {
                        if let Some(idx) = last_table {
                            dbl.libraries[idx].user_where_text = val.to_string();
                        }
                    }
                    _ => {}
                }
            } else if section.starts_with("FieldMap") && key == "Options" {
                let mut column = String::new();
                let mut table_only = String::new();
                let mut parameter = String::new();
                let mut field_type = 1u8;
                let mut visible = false;
                let mut add_mode = 0u8;
                let mut remove_mode = 0u8;
                let mut update_mode = 0u8;
                for part in val.split('|') {
                    if let Some((k, v)) = part.split_once('=') {
                        match k {
                            "FieldNameOnly" => column = v.to_string(),
                            "TableNameOnly" => table_only = v.to_string(),
                            "ParameterName" => parameter = v.to_string(),
                            "FieldType" => field_type = v.parse().unwrap_or(1),
                            "VisibleOnAdd" => visible = v.eq_ignore_ascii_case("true"),
                            "AddMode" => add_mode = v.parse().unwrap_or(0),
                            "RemoveMode" => remove_mode = v.parse().unwrap_or(0),
                            "UpdateMode" => update_mode = v.parse().unwrap_or(0),
                            _ => {}
                        }
                    }
                }
                if !column.is_empty() && !is_internal_table(&table_only) {
                    if let Some(lib) = dbl.libraries.iter_mut().find(|l| l.table == table_only) {
                        lib.fields.push(Field {
                            column,
                            parameter,
                            is_key: field_type == 0,
                            visible_on_add: visible,
                            add_mode,
                            remove_mode,
                            update_mode,
                        });
                    }
                }
            }
        }
        Ok(dbl)
    }
}

pub fn create_library(name: &str) -> Library {
    Library {
        name: name.to_string(),
        table: name.to_string(),
        enabled: true,
        user_where: false,
        user_where_text: String::new(),
        fields: base_fields(),
    }
}

pub fn dbl_path_for_db(db_path: &Path) -> PathBuf {
    let mut dbl = db_path.to_path_buf();
    dbl.set_extension("DbLib");
    dbl
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_ini() {
        let mut dbl = AltiumDbl::new(Path::new("test.sqlite"));
        dbl.add_library(create_library("Resistors"));
        dbl.ensure_base_fields();
        let ini = dbl.to_ini();
        let parsed = AltiumDbl::from_ini(&ini).unwrap();
        assert_eq!(parsed.libraries.len(), 1);
        assert_eq!(parsed.libraries[0].table, "Resistors");
        assert_eq!(
            parsed.libraries[0].fields.len(),
            dbl.libraries[0].fields.len()
        );
        assert!(parsed.libraries[0]
            .fields
            .iter()
            .any(|f| f.is_key && f.column == "MPN"));
        assert!(parsed.libraries[0]
            .fields
            .iter()
            .any(|f| f.column == "Library Path" && f.parameter == "[Library Path]"));
        assert!(parsed.libraries[0]
            .fields
            .iter()
            .any(|f| f.column == "Footprint Path" && f.parameter == "[Footprint Path]"));
        assert!(parsed.libraries[0].enabled);
        assert!(parsed
            .database_links
            .iter()
            .any(|(k, v)| k == "ConnectionString" && v.contains("Data Source=test")));
        assert!(parsed
            .database_links
            .iter()
            .any(|(k, v)| k == "AddMode" && v == "3"));
    }

    #[test]
    fn skips_internal_tables_and_preserves_state() {
        let content = concat!(
            "[OutputDatabaseLinkFile]\n",
            "Version=1.1\n",
            "[DatabaseLinks]\n",
            "ConnectionString=Provider=MSDASQL.1;Persist Security Info=False;Data Source=gortpower\n",
            "AddMode=3\n",
            "RemoveMode=1\n",
            "UpdateMode=2\n",
            "ViewMode=0\n",
            "LeftQuote=[\n",
            "RightQuote=]\n",
            "QuoteTableNames=1\n",
            "UseTableSchemaName=0\n",
            "DefaultColumnType=VARCHAR(255)\n",
            "LibraryDatabaseType=\n",
            "LibraryDatabasePath=\n",
            "DatabasePathRelative=0\n",
            "TopPanelCollapsed=0\n",
            "LibrarySearchPath=C:\\libs\\symbols;C:\\libs\\footprints\n",
            "OrcadMultiValueDelimiter=,\n",
            "SearchSubDirectories=0\n",
            "SchemaName=\n",
            "LastFocusedTable=\n",
            "[Table1]\n",
            "SchemaName=\n",
            "TableName=Resistors\n",
            "Enabled=False\n",
            "UserWhere=1\n",
            "UserWhereText=[MPN] = 'R1'\n",
            "[Table2]\n",
            "SchemaName=\n",
            "TableName=sqlite_sequence\n",
            "Enabled=True\n",
            "UserWhere=0\n",
            "UserWhereText=\n"
        );
        let parsed = AltiumDbl::from_ini(content).unwrap();
        assert_eq!(parsed.libraries.len(), 1);
        assert_eq!(parsed.libraries[0].table, "Resistors");
        assert!(!parsed.libraries[0].enabled);
        assert!(parsed.libraries[0].user_where);
        assert_eq!(parsed.libraries[0].user_where_text, "[MPN] = 'R1'");
        assert!(parsed
            .database_links
            .iter()
            .any(|(k, v)| k == "LibrarySearchPath" && v.contains("symbols")));

        let mut dbl = parsed;
        dbl.set_dsn("gortpower");
        assert_eq!(
            dbl.connection_string,
            "Provider=MSDASQL.1;Persist Security Info=False;Data Source=gortpower"
        );
        let out = dbl.to_ini();
        assert!(!out.contains("sqlite_sequence"));
        assert!(out.contains("Enabled=False"));
        assert!(out.contains("LibrarySearchPath=C:\\libs\\symbols;C:\\libs\\footprints"));
    }

    #[test]
    fn save_does_not_rewrite_unchanged_file() {
        let dir = std::env::temp_dir().join("altiumdb_dbl_test");
        std::fs::create_dir_all(&dir).ok();
        let path = dir.join("altiumdb.DbLib");

        let mut dbl = AltiumDbl::new(Path::new("test.sqlite"));
        dbl.add_library(create_library("Resistors"));
        dbl.ensure_base_fields();
        dbl.save(&path).unwrap();

        let modified_after_first = std::fs::metadata(&path).and_then(|m| m.modified()).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(20));

        dbl.save(&path).unwrap();
        let modified_after_second = std::fs::metadata(&path).and_then(|m| m.modified()).unwrap();
        assert_eq!(modified_after_first, modified_after_second);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn canonical_ini_roundtrips_unchanged() {
        // A file already produced by this tool must round-trip byte-for-byte, so
        // repeated saves do not touch the file.
        let mut dbl = AltiumDbl::new(Path::new("test.sqlite"));
        dbl.add_library(create_library("Resistors"));
        dbl.set_dsn("gortpower");
        dbl.ensure_base_fields();
        let canonical = dbl.to_ini();
        let reparsed = AltiumDbl::from_ini(&canonical).unwrap();
        assert_eq!(canonical, reparsed.to_ini());
    }
}
