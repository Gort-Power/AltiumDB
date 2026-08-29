use rusqlite::{params, Connection, Result};
use std::path::Path;

#[derive(Debug, Clone, Default)]
pub struct Component {
    pub id: String,
    pub mpn: String,
    pub manufacturer: String,
    pub description: String,
    pub verified: bool,
    pub library_ref: String,
    pub library_path: String,
    pub footprint_ref: String,
    pub footprint_path: String,
    pub footprint_ref2: String,
    pub footprint_path2: String,
    pub footprint_ref3: String,
    pub footprint_path3: String,
    pub component_link1_description: String,
    pub component_link1_url: String,
    pub component_link2_description: String,
    pub component_link2_url: String,
    pub component_link3_description: String,
    pub component_link3_url: String,
}

pub const BASE_COLUMNS: &[&str] = &[
    "MPN",
    "Manufacturer",
    "Description",
    "Verified",
    "Library Ref",
    "Library Path",
    "Footprint Ref",
    "Footprint Path",
    "Footprint Ref 2",
    "Footprint Path 2",
    "Footprint Ref 3",
    "Footprint Path 3",
    "ComponentLink1Description",
    "ComponentLink1URL",
    "ComponentLink2Description",
    "ComponentLink2URL",
    "ComponentLink3Description",
    "ComponentLink3URL",
];

pub fn open_database(path: &Path) -> Result<Connection> {
    let conn = Connection::open(path)?;
    conn.execute_batch("PRAGMA journal_mode=WAL;")?;
    Ok(conn)
}

pub fn ensure_base_columns(conn: &Connection, table: &str) -> Result<()> {
    let cols = get_columns(conn, table)?;
    for col in [
        "ComponentLink1Description",
        "ComponentLink1URL",
        "ComponentLink2Description",
        "ComponentLink2URL",
        "ComponentLink3Description",
        "ComponentLink3URL",
        "Library Path",
        "Footprint Path",
        "Footprint Ref 2",
        "Footprint Path 2",
        "Footprint Ref 3",
        "Footprint Path 3",
    ] {
        if !cols.iter().any(|c| c == col) {
            let _ = add_column(conn, table, col);
        }
    }
    Ok(())
}

fn rename_column_if_absent(conn: &Connection, table: &str, old: &str, new: &str) -> Result<()> {
    let cols = get_columns(conn, table)?;
    let has_old = cols.iter().any(|c| c == old);
    let has_new = cols.iter().any(|c| c == new);
    if has_old && !has_new {
        let _ = rename_column(conn, table, old, new);
    }
    Ok(())
}

fn drop_column_if_exists(conn: &Connection, table: &str, col: &str) -> Result<()> {
    let cols = get_columns(conn, table)?;
    if cols.iter().any(|c| c == col) {
        let _ = drop_column(conn, table, col);
    }
    Ok(())
}

pub fn migrate(conn: &Connection) -> Result<()> {
    for table in get_tables(conn)? {
        rename_column_if_absent(conn, &table, "Design Item ID", "MPN")?;
        rename_column_if_absent(conn, &table, "Symbols", "Library Ref")?;
        rename_column_if_absent(conn, &table, "Symbol Reference", "Library Ref")?;
        rename_column_if_absent(conn, &table, "Footprints", "Footprint Ref")?;
        rename_column_if_absent(conn, &table, "Footprint Reference", "Footprint Ref")?;
        rename_column_if_absent(conn, &table, "Datasheet", "ComponentLink1URL")?;
        for col in [
            "Comment",
            "Footprint Filters",
            "Keywords",
            "No BOM",
            "Schematic Only",
            "No Sim",
            "Manufacturer Part Number",
        ] {
            drop_column_if_exists(conn, &table, col)?;
        }
        ensure_base_columns(conn, &table)?;
    }
    Ok(())
}

pub fn table_exists(conn: &Connection, name: &str) -> Result<bool> {
    let mut stmt =
        conn.prepare("SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name=?1")?;
    let count: i64 = stmt.query_row(params![name], |row| row.get(0))?;
    Ok(count > 0)
}

pub fn get_tables(conn: &Connection) -> Result<Vec<String>> {
    let mut stmt = conn.prepare(
        "SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%' ORDER BY name"
    )?;
    let rows = stmt.query_map([], |row| row.get(0))?;
    rows.collect()
}

pub fn ensure_table(conn: &Connection, table_name: &str) -> Result<()> {
    let safe = sanitize_table_name(table_name);
    if !table_exists(conn, &safe)? {
        conn.execute_batch(&format!(
            "CREATE TABLE [{}] (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                MPN TEXT NOT NULL UNIQUE,
                Manufacturer TEXT NOT NULL DEFAULT '',
                Description TEXT NOT NULL DEFAULT '',
                Verified INTEGER NOT NULL DEFAULT 0,
                [Library Ref] TEXT NOT NULL DEFAULT '',
                [Library Path] TEXT NOT NULL DEFAULT '',
                [Footprint Ref] TEXT NOT NULL DEFAULT '',
                [Footprint Path] TEXT NOT NULL DEFAULT '',
                [Footprint Ref 2] TEXT NOT NULL DEFAULT '',
                [Footprint Path 2] TEXT NOT NULL DEFAULT '',
                [Footprint Ref 3] TEXT NOT NULL DEFAULT '',
                [Footprint Path 3] TEXT NOT NULL DEFAULT '',
                [ComponentLink1Description] TEXT NOT NULL DEFAULT '',
                [ComponentLink1URL] TEXT NOT NULL DEFAULT '',
                [ComponentLink2Description] TEXT NOT NULL DEFAULT '',
                [ComponentLink2URL] TEXT NOT NULL DEFAULT '',
                [ComponentLink3Description] TEXT NOT NULL DEFAULT '',
                [ComponentLink3URL] TEXT NOT NULL DEFAULT ''
            );",
            safe
        ))?;
    } else {
        ensure_base_columns(conn, &safe)?;
    }
    Ok(())
}

pub fn get_columns(conn: &Connection, table_name: &str) -> Result<Vec<String>> {
    let safe = sanitize_table_name(table_name);
    let mut stmt = conn.prepare(&format!("PRAGMA table_info([{}])", safe))?;
    let rows = stmt.query_map([], |row| {
        let name: String = row.get(1)?;
        Ok(name)
    })?;
    rows.collect()
}

#[allow(dead_code)]
pub fn add_column(conn: &Connection, table_name: &str, column_name: &str) -> Result<()> {
    let safe = sanitize_table_name(table_name);
    let col = sanitize_table_name(column_name);
    conn.execute_batch(&format!(
        "ALTER TABLE [{}] ADD COLUMN [{}] TEXT NOT NULL DEFAULT '';",
        safe, col
    ))?;
    Ok(())
}

pub fn rename_column(
    conn: &Connection,
    table_name: &str,
    old_name: &str,
    new_name: &str,
) -> Result<()> {
    let safe = sanitize_table_name(table_name);
    let old = sanitize_table_name(old_name);
    let new = sanitize_table_name(new_name);
    conn.execute_batch(&format!(
        "ALTER TABLE [{}] RENAME COLUMN [{}] TO [{}];",
        safe, old, new
    ))?;
    Ok(())
}

pub fn drop_column(conn: &Connection, table_name: &str, column_name: &str) -> Result<()> {
    let safe = sanitize_table_name(table_name);
    let col = sanitize_table_name(column_name);
    conn.execute_batch(&format!("ALTER TABLE [{}] DROP COLUMN [{}];", safe, col))?;
    Ok(())
}

pub fn drop_table(conn: &Connection, table_name: &str) -> Result<()> {
    let safe = sanitize_table_name(table_name);
    conn.execute_batch(&format!("DROP TABLE IF EXISTS [{}];", safe))?;
    Ok(())
}

pub fn rename_table(conn: &Connection, old_name: &str, new_name: &str) -> Result<()> {
    let old = sanitize_table_name(old_name);
    let new = sanitize_table_name(new_name);
    conn.execute_batch(&format!("ALTER TABLE [{}] RENAME TO [{}];", old, new))?;
    Ok(())
}

// --- Components ---

fn component_from_row(row: &rusqlite::Row) -> Result<Component> {
    Ok(Component {
        id: row.get::<_, i64>(0)?.to_string(),
        mpn: row.get(1)?,
        manufacturer: row.get(2)?,
        description: row.get(3)?,
        verified: row.get::<_, i64>(4)? != 0,
        library_ref: row.get(5)?,
        library_path: row.get(6)?,
        footprint_ref: row.get(7)?,
        footprint_path: row.get(8)?,
        footprint_ref2: row.get(9)?,
        footprint_path2: row.get(10)?,
        footprint_ref3: row.get(11)?,
        footprint_path3: row.get(12)?,
        component_link1_description: row.get(13)?,
        component_link1_url: row.get(14)?,
        component_link2_description: row.get(15)?,
        component_link2_url: row.get(16)?,
        component_link3_description: row.get(17)?,
        component_link3_url: row.get(18)?,
    })
}

const COMPONENT_SELECT: &str =
    "SELECT id, MPN, Manufacturer, Description, Verified, [Library Ref], [Library Path], \
    [Footprint Ref], [Footprint Path], [Footprint Ref 2], [Footprint Path 2], \
    [Footprint Ref 3], [Footprint Path 3], \
    [ComponentLink1Description], [ComponentLink1URL], \
    [ComponentLink2Description], [ComponentLink2URL], \
    [ComponentLink3Description], [ComponentLink3URL] FROM [";

pub fn get_components(conn: &Connection, table_name: &str) -> Result<Vec<Component>> {
    let safe = sanitize_table_name(table_name);
    let mut stmt = conn.prepare(&format!("{}{}] ORDER BY MPN", COMPONENT_SELECT, safe))?;
    let rows = stmt.query_map([], component_from_row)?;
    rows.collect()
}

pub fn get_distinct_values(
    conn: &Connection,
    table_name: &str,
    column: &str,
) -> Result<Vec<String>> {
    let safe = sanitize_table_name(table_name);
    let col = sanitize_table_name(column);
    let mut stmt = conn.prepare(&format!(
        "SELECT DISTINCT [{}] FROM [{}] WHERE [{}] IS NOT NULL AND [{}] <> '' ORDER BY [{}] COLLATE NOCASE",
        col, safe, col, col, col
    ))?;
    let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
    rows.collect()
}

pub fn search_components(
    conn: &Connection,
    table_name: &str,
    filters: &[(String, Vec<String>)],
) -> Result<Vec<Component>> {
    let safe = sanitize_table_name(table_name);
    let mut sql = String::from(COMPONENT_SELECT);
    sql.push_str(&safe);
    sql.push_str("] WHERE 1=1");
    let mut params: Vec<String> = Vec::new();
    for (col, vals) in filters {
        if vals.is_empty() {
            continue;
        }
        let col = sanitize_table_name(col);
        let placeholders: Vec<String> = vals.iter().map(|_| "?".to_string()).collect();
        sql.push_str(&format!(" AND [{}] IN ({})", col, placeholders.join(", ")));
        for v in vals {
            params.push(v.clone());
        }
    }
    sql.push_str(" ORDER BY MPN");
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(
        rusqlite::params_from_iter(params.iter()),
        component_from_row,
    )?;
    rows.collect()
}

/// Search every category table for components whose `MPN` contains `query`
/// (case-insensitive substring match). Returns matches paired with their
/// source table name so the caller can tell which category each came from.
pub fn search_all_by_mpn(conn: &Connection, query: &str) -> Result<Vec<(String, Component)>> {
    let mut out: Vec<(String, Component)> = Vec::new();
    let query = query.trim();
    if query.is_empty() {
        return Ok(out);
    }
    let tables = get_tables(conn)?;
    // Escape SQLite LIKE wildcards so a literal `%`/`_` in the query is matched
    // literally, then wrap as a substring pattern.
    let escaped: String = query
        .chars()
        .flat_map(|c| match c {
            '\\' | '%' | '_' => vec!['\\', c],
            c => vec![c],
        })
        .collect();
    let pattern = format!("%{}%", escaped);
    for table in tables {
        let safe = sanitize_table_name(&table);
        let sql = format!("{COMPONENT_SELECT}{safe}] WHERE MPN LIKE ?1 ESCAPE '\\' ORDER BY MPN");
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(params![pattern], component_from_row)?;
        for c in rows.flatten() {
            out.push((table.clone(), c));
        }
    }
    Ok(out)
}

pub fn add_component(conn: &Connection, table_name: &str, c: &Component) -> Result<i64> {
    let safe = sanitize_table_name(table_name);
    conn.execute(
        &format!(
            "INSERT INTO [{}] (MPN, Manufacturer, Description, Verified, [Library Ref], [Library Path], \
                    [Footprint Ref], [Footprint Path], [Footprint Ref 2], [Footprint Path 2], \
                    [Footprint Ref 3], [Footprint Path 3], \
                    [ComponentLink1Description], [ComponentLink1URL], \
                    [ComponentLink2Description], [ComponentLink2URL], \
                    [ComponentLink3Description], [ComponentLink3URL]) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18)",
            safe
        ),
        params![
            c.mpn,
            c.manufacturer,
            c.description,
            c.verified as i64,
            c.library_ref,
            c.library_path,
            c.footprint_ref,
            c.footprint_path,
            c.footprint_ref2,
            c.footprint_path2,
            c.footprint_ref3,
            c.footprint_path3,
            c.component_link1_description,
            c.component_link1_url,
            c.component_link2_description,
            c.component_link2_url,
            c.component_link3_description,
            c.component_link3_url,
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn mpn_exists(conn: &Connection, table_name: &str, mpn: &str) -> Result<bool> {
    let safe = sanitize_table_name(table_name);
    let mut stmt = conn.prepare(&format!("SELECT COUNT(*) FROM [{}] WHERE MPN = ?1", safe))?;
    let count: i64 = stmt.query_row(params![mpn], |row| row.get(0))?;
    Ok(count > 0)
}

pub fn insert_component_row(
    conn: &Connection,
    table_name: &str,
    values: &std::collections::HashMap<String, String>,
) -> Result<()> {
    let safe = sanitize_table_name(table_name);
    let cols = get_columns(conn, &safe)?;
    let mut filtered: Vec<(String, String)> = values
        .iter()
        .filter(|(k, v)| cols.contains(k) && !k.eq_ignore_ascii_case("id") && !v.is_empty())
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    if filtered.is_empty() {
        return Ok(());
    }
    filtered.sort_by(|a, b| a.0.cmp(&b.0));
    let col_list: Vec<String> = filtered.iter().map(|(k, _)| format!("[{}]", k)).collect();
    let placeholders: Vec<String> = vec!["?".to_string(); filtered.len()];
    let sql = format!(
        "INSERT INTO [{}] ({}) VALUES ({})",
        safe,
        col_list.join(", "),
        placeholders.join(", ")
    );
    let sql_values: Vec<rusqlite::types::Value> = filtered
        .iter()
        .map(|(_, v)| rusqlite::types::Value::Text(v.clone()))
        .collect();
    conn.execute(&sql, rusqlite::params_from_iter(sql_values.iter()))?;
    Ok(())
}

pub fn update_component(conn: &Connection, table_name: &str, c: &Component) -> Result<()> {
    let safe = sanitize_table_name(table_name);
    conn.execute(
        &format!(
            "UPDATE [{}] SET MPN=?1, Manufacturer=?2, Description=?3, Verified=?4, [Library Ref]=?5, [Library Path]=?6, \
                    [Footprint Ref]=?7, [Footprint Path]=?8, [Footprint Ref 2]=?9, [Footprint Path 2]=?10, \
                    [Footprint Ref 3]=?11, [Footprint Path 3]=?12, \
                    [ComponentLink1Description]=?13, [ComponentLink1URL]=?14, \
                    [ComponentLink2Description]=?15, [ComponentLink2URL]=?16, \
                    [ComponentLink3Description]=?17, [ComponentLink3URL]=?18 WHERE id=?19",
            safe
        ),
        params![
            c.mpn,
            c.manufacturer,
            c.description,
            c.verified as i64,
            c.library_ref,
            c.library_path,
            c.footprint_ref,
            c.footprint_path,
            c.footprint_ref2,
            c.footprint_path2,
            c.footprint_ref3,
            c.footprint_path3,
            c.component_link1_description,
            c.component_link1_url,
            c.component_link2_description,
            c.component_link2_url,
            c.component_link3_description,
            c.component_link3_url,
            c.id,
        ],
    )?;
    Ok(())
}

pub fn delete_component(conn: &Connection, table_name: &str, id: &str) -> Result<()> {
    let safe = sanitize_table_name(table_name);
    conn.execute(
        &format!("DELETE FROM [{}] WHERE id = ?1", safe),
        params![id.parse::<i64>().unwrap_or(0)],
    )?;
    Ok(())
}

pub fn clone_component(conn: &Connection, table_name: &str, id: &str, new_mpn: &str) -> Result<()> {
    let safe = sanitize_table_name(table_name);
    let cols = get_columns(conn, &safe)?;
    let other: Vec<String> = cols
        .iter()
        .filter(|c| *c != "id" && *c != "MPN")
        .map(|c| format!("[{}]", c))
        .collect();
    if other.is_empty() {
        return Ok(());
    }
    let sql = format!(
        "INSERT INTO [{}] (MPN, {}) SELECT ?1, {} FROM [{}] WHERE id = ?2",
        safe,
        other.join(", "),
        other.join(", "),
        safe
    );
    conn.execute(&sql, params![new_mpn, id.parse::<i64>().unwrap_or(0)])?;
    Ok(())
}

pub fn clone_table(conn: &Connection, old_name: &str, new_name: &str) -> Result<()> {
    let old = sanitize_table_name(old_name);
    let new = sanitize_table_name(new_name);
    ensure_table(conn, &new)?;
    let old_cols = get_columns(conn, &old)?;
    let new_cols = get_columns(conn, &new)?;
    for col in &old_cols {
        if col != "id" && !new_cols.contains(col) {
            let _ = add_column(conn, &new, col);
        }
    }
    let shared: Vec<String> = old_cols
        .iter()
        .filter(|c| *c != "id")
        .map(|c| format!("[{}]", c))
        .collect();
    if !shared.is_empty() {
        let sql = format!(
            "INSERT INTO [{}] ({}) SELECT {} FROM [{}]",
            new,
            shared.join(", "),
            shared.join(", "),
            old
        );
        conn.execute(&sql, [])?;
    }
    Ok(())
}

// --- Custom field values ---

pub fn get_custom_value(
    conn: &Connection,
    table_name: &str,
    component_id: &str,
    column: &str,
) -> Result<String> {
    let safe = sanitize_table_name(table_name);
    let col = sanitize_table_name(column);
    let mut stmt = conn.prepare(&format!("SELECT [{}] FROM [{}] WHERE id = ?1", col, safe))?;
    stmt.query_row(params![component_id.parse::<i64>().unwrap_or(0)], |row| {
        row.get(0)
    })
}

pub fn set_custom_value(
    conn: &Connection,
    table_name: &str,
    component_id: &str,
    column: &str,
    value: &str,
) -> Result<()> {
    let safe = sanitize_table_name(table_name);
    let col = sanitize_table_name(column);
    conn.execute(
        &format!("UPDATE [{}] SET [{}] = ?1 WHERE id = ?2", safe, col),
        params![value, component_id.parse::<i64>().unwrap_or(0)],
    )?;
    Ok(())
}

fn sanitize_table_name(name: &str) -> String {
    name.chars()
        .map(|c| {
            if c == ';' || c == '\'' || c == '"' {
                '_'
            } else {
                c
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migrate_renames_and_drops_columns() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE [Test] (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                [Design Item ID] TEXT NOT NULL UNIQUE,
                Manufacturer TEXT NOT NULL DEFAULT '',
                Datasheet TEXT NOT NULL DEFAULT '',
                Verified INTEGER NOT NULL DEFAULT 0,
                [Symbol Reference] TEXT NOT NULL DEFAULT '',
                [Footprint Reference] TEXT NOT NULL DEFAULT '',
                Description TEXT NOT NULL DEFAULT '',
                [Footprint Filters] TEXT NOT NULL DEFAULT '',
                Keywords TEXT NOT NULL DEFAULT '',
                [No BOM] TEXT NOT NULL DEFAULT '0',
                [Schematic Only] TEXT NOT NULL DEFAULT '0',
                [No Sim] TEXT NOT NULL DEFAULT '0',
                [Manufacturer Part Number] TEXT NOT NULL DEFAULT ''
            );",
        )
        .unwrap();
        conn.execute(
            "INSERT INTO [Test] ([Design Item ID], Manufacturer, Datasheet) VALUES ('ABC', 'Acme', 'http://x')",
            [],
        ).unwrap();
        migrate(&conn).unwrap();
        let cols = get_columns(&conn, "Test").unwrap();
        assert!(cols.iter().any(|c| c == "MPN"));
        assert!(cols.iter().any(|c| c == "Library Ref"));
        assert!(cols.iter().any(|c| c == "Footprint Ref"));
        assert!(cols.iter().any(|c| c == "Library Path"));
        assert!(cols.iter().any(|c| c == "Footprint Path"));
        assert!(cols.iter().any(|c| c == "ComponentLink1Description"));
        assert!(cols.iter().any(|c| c == "ComponentLink1URL"));
        assert!(!cols.iter().any(|c| c == "Datasheet"));
        assert!(!cols.iter().any(|c| c == "Footprint Filters"));
        assert!(!cols.iter().any(|c| c == "Keywords"));
        assert!(!cols.iter().any(|c| c == "No BOM"));
        assert!(!cols.iter().any(|c| c == "Schematic Only"));
        assert!(!cols.iter().any(|c| c == "No Sim"));
        assert!(!cols.iter().any(|c| c == "Manufacturer Part Number"));
        assert!(!cols.iter().any(|c| c == "Comment"));
        let comps = get_components(&conn, "Test").unwrap();
        assert_eq!(comps.len(), 1);
        assert_eq!(comps[0].mpn, "ABC");
        assert_eq!(comps[0].component_link1_url, "http://x");
    }

    #[test]
    fn search_all_by_mpn_spans_tables() {
        let conn = Connection::open_in_memory().unwrap();
        for t in ["Resistors", "Capacitors"] {
            ensure_table(&conn, t).unwrap();
        }
        add_component(
            &conn,
            "Resistors",
            &Component {
                mpn: "R100".into(),
                ..Default::default()
            },
        )
        .unwrap();
        add_component(
            &conn,
            "Capacitors",
            &Component {
                mpn: "C100".into(),
                ..Default::default()
            },
        )
        .unwrap();
        add_component(
            &conn,
            "Capacitors",
            &Component {
                mpn: "C200".into(),
                ..Default::default()
            },
        )
        .unwrap();

        let all = search_all_by_mpn(&conn, "100").unwrap();
        let mpns: Vec<&str> = all.iter().map(|(_, c)| c.mpn.as_str()).collect();
        assert!(mpns.contains(&"R100"));
        assert!(mpns.contains(&"C100"));
        assert!(!mpns.contains(&"C200"));

        // Empty query returns nothing.
        assert!(search_all_by_mpn(&conn, "").unwrap().is_empty());
    }
}
