use postgres::{Client, NoTls, Row};
use rusqlite::Connection as SqliteConnection;
use std::cell::{RefCell, RefMut};

pub enum DatabaseConnection {
    Sqlite(RefCell<SqliteConnection>),
    Postgres(RefCell<Client>),
}
pub type Connection = DatabaseConnection;

#[derive(Debug)]
pub enum Error {
    Sqlite(rusqlite::Error),
    Postgres(postgres::Error),
    Message(String),
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Sqlite(e) => e.fmt(f),
            Self::Postgres(e) => {
                if let Some(db_error) = e.as_db_error() {
                    write!(f, "{}", db_error.message())?;
                    if let Some(detail) = db_error.detail() {
                        write!(f, "; detail: {}", detail)?;
                    }
                    if let Some(constraint) = db_error.constraint() {
                        write!(f, "; constraint: {}", constraint)?;
                    }
                    Ok(())
                } else {
                    e.fmt(f)
                }
            }
            Self::Message(e) => f.write_str(e),
        }
    }
}
impl std::error::Error for Error {}
impl From<rusqlite::Error> for Error {
    fn from(e: rusqlite::Error) -> Self {
        Self::Sqlite(e)
    }
}
impl From<postgres::Error> for Error {
    fn from(e: postgres::Error) -> Self {
        Self::Postgres(e)
    }
}
pub type Result<T> = std::result::Result<T, Error>;

impl DatabaseConnection {
    fn is_pg(&self) -> bool {
        matches!(self, Self::Postgres(_))
    }
}

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

pub fn open_database_with_config(kind: &str, value: &str) -> Result<Connection> {
    if kind.eq_ignore_ascii_case("postgres") || kind.eq_ignore_ascii_case("postgresql") {
        Ok(Connection::Postgres(RefCell::new(Client::connect(
            value, NoTls,
        )?)))
    } else {
        let c = SqliteConnection::open(value)?;
        c.execute_batch("PRAGMA journal_mode=WAL;")?;
        Ok(Connection::Sqlite(RefCell::new(c)))
    }
}

pub fn postgres_connection_string(
    host: &str,
    port: &str,
    database: &str,
    user: &str,
    password: &str,
) -> String {
    [
        ("host", host),
        ("port", port),
        ("dbname", database),
        ("user", user),
        ("password", password),
    ]
    .into_iter()
    .filter(|(_, value)| !value.trim().is_empty())
    .map(|(key, value)| format!("{}={}", key, value.trim()))
    .collect::<Vec<_>>()
    .join(" ")
}

pub fn parse_postgres_connection_string(value: &str) -> [String; 5] {
    let mut fields = [
        String::new(),
        String::new(),
        String::new(),
        String::new(),
        String::new(),
    ];
    for part in value.split_whitespace() {
        let Some((key, val)) = part.split_once('=') else {
            continue;
        };
        let val = val.trim_matches(['\'', '"']);
        match key.to_ascii_lowercase().as_str() {
            "host" => fields[0] = val.to_string(),
            "port" => fields[1] = val.to_string(),
            "dbname" | "database" => fields[2] = val.to_string(),
            "user" | "username" => fields[3] = val.to_string(),
            "password" | "pass" => fields[4] = val.to_string(),
            _ => {}
        }
    }
    fields
}
pub fn checkpoint(c: &Connection) {
    if let Connection::Sqlite(c) = c {
        let _ = c.borrow().execute_batch("PRAGMA wal_checkpoint(TRUNCATE);");
    }
}

fn qi(s: &str) -> String {
    format!("\"{}\"", s.replace('"', "\"\""))
}
fn sql_param(pg: bool, n: usize) -> String {
    if pg {
        format!("${n}")
    } else {
        "?".into()
    }
}
fn exec(c: &Connection, sql: &str, vals: &[String]) -> Result<u64> {
    match c {
        Connection::Sqlite(c) => {
            let c = c.borrow();
            let mut st = c.prepare(sql)?;
            let p: Vec<&dyn rusqlite::ToSql> =
                vals.iter().map(|v| v as &dyn rusqlite::ToSql).collect();
            Ok(st.execute(rusqlite::params_from_iter(p))? as u64)
        }

        Connection::Postgres(_) => {
            let p: Vec<&(dyn postgres::types::ToSql + Sync)> = vals
                .iter()
                .map(|v| v as &(dyn postgres::types::ToSql + Sync))
                .collect();
            Ok(c.as_pg()?.execute(sql, &p)?)
        }
    }
}

#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PgValueKind {
    Text,
    Integer,
    Boolean,
    Float,
}

#[cfg(test)]
fn pg_value_kind(data_type: &str, udt_name: &str) -> PgValueKind {
    match (data_type, udt_name) {
        ("smallint", _)
        | ("integer", _)
        | ("bigint", _)
        | ("USER-DEFINED", "serial")
        | ("USER-DEFINED", "bigserial") => PgValueKind::Integer,
        ("boolean", _) => PgValueKind::Boolean,
        ("real", _) | ("double precision", _) | ("numeric", _) | ("decimal", _) => {
            PgValueKind::Float
        }
        _ => PgValueKind::Text,
    }
}

#[cfg(test)]
fn convert_pg_value(
    kind: PgValueKind,
    value: &str,
) -> Result<Box<dyn postgres::types::ToSql + Sync>> {
    match kind {
        PgValueKind::Text => Ok(Box::new(value.to_owned())),
        PgValueKind::Integer => value
            .parse::<i64>()
            .map(|v| Box::new(v) as Box<dyn postgres::types::ToSql + Sync>)
            .map_err(|_| Error::Message(format!("invalid PostgreSQL integer value: {value}"))),
        PgValueKind::Boolean => {
            let v = match value.trim().to_ascii_lowercase().as_str() {
                "1" | "true" | "t" | "yes" | "y" => true,
                "0" | "false" | "f" | "no" | "n" => false,
                _ => {
                    return Err(Error::Message(format!(
                        "invalid PostgreSQL boolean value: {value}"
                    )))
                }
            };
            Ok(Box::new(v))
        }
        PgValueKind::Float => value
            .parse::<f64>()
            .map(|v| Box::new(v) as Box<dyn postgres::types::ToSql + Sync>)
            .map_err(|_| Error::Message(format!("invalid PostgreSQL numeric value: {value}"))),
    }
}

fn pg_column_cast(c: &Connection, table: &str, column: &str, n: usize) -> Result<String> {
    let row = c.as_pg()?.query_opt(
        "SELECT format_type(a.atttypid, a.atttypmod),
                COALESCE(base.typcategory, typ.typcategory)::text
         FROM pg_catalog.pg_attribute a
         JOIN pg_catalog.pg_class r ON r.oid = a.attrelid
         JOIN pg_catalog.pg_namespace nsp ON nsp.oid = r.relnamespace
         JOIN pg_catalog.pg_type typ ON typ.oid = a.atttypid
         LEFT JOIN pg_catalog.pg_type base ON base.oid = NULLIF(typ.typbasetype, 0)
         WHERE nsp.nspname = current_schema()
           AND r.relname = $1 AND a.attname = $2 AND a.attnum > 0
           AND NOT a.attisdropped",
        &[&table, &column],
    )?;
    let ty = match row {
        Some(r) => (
            r.try_get::<_, String>(0).map_err(|e| {
                Error::Message(format!("failed to read PostgreSQL column type: {e}"))
            })?,
            r.try_get::<_, String>(1).map_err(|e| {
                Error::Message(format!("failed to read PostgreSQL type category: {e}"))
            })?,
        ),
        None => {
            return Err(Error::Message(format!(
                "PostgreSQL column '{}' was not found in table '{}'",
                column, table
            )))
        }
    };
    let (ty, category) = ty;
    Ok(pg_cast_expression(n, &ty, &category))
}

fn pg_cast_expression(n: usize, ty: &str, category: &str) -> String {
    let value = if matches!(category, "N" | "B") {
        format!("NULLIF(${n}::text, '')")
    } else {
        format!("${n}::text")
    };
    // Keep the wire parameter as text.  This avoids postgres' ToSql rejecting
    // domains and other user-defined types; PostgreSQL performs the final cast.
    format!("CAST({value} AS {ty})")
}

fn pg_param(c: &Connection, table: &str, column: &str, n: usize) -> Result<String> {
    pg_column_cast(c, table, column, n)
}

fn pg_id_has_generation(c: &Connection, table: &str) -> Result<bool> {
    let row = c.as_pg()?.query_opt(
        "SELECT column_default IS NOT NULL OR is_identity = 'YES'
         FROM information_schema.columns
         WHERE table_schema = current_schema()
           AND table_name = $1 AND column_name = 'id'",
        &[&table],
    )?;
    row.map(|r| {
        r.try_get::<_, bool>(0)
            .map_err(|e| Error::Message(format!("failed to read PostgreSQL id metadata: {e}")))
    })
    .unwrap_or_else(|| {
        Err(Error::Message(format!(
            "PostgreSQL id column was not found in table '{}'",
            table
        )))
    })
}

fn pg_params(
    _c: &Connection,
    _table: &str,
    _columns: &[&str],
    values: &[String],
) -> Result<Vec<Box<dyn postgres::types::ToSql + Sync>>> {
    Ok(values
        .iter()
        .map(|value| Box::new(value.clone()) as Box<dyn postgres::types::ToSql + Sync>)
        .collect())
}

fn exec_pg_params(
    c: &Connection,
    sql: &str,
    params: &[Box<dyn postgres::types::ToSql + Sync>],
) -> Result<u64> {
    let refs = params
        .iter()
        .map(|p| p.as_ref() as &(dyn postgres::types::ToSql + Sync))
        .collect::<Vec<_>>();
    Ok(c.as_pg()?.execute(sql, &refs)?)
}

fn query_strings(c: &Connection, sql: &str, vals: &[String]) -> Result<Vec<String>> {
    match c {
        Connection::Sqlite(c) => {
            let c = c.borrow();
            let mut st = c.prepare(sql)?;
            let p: Vec<&dyn rusqlite::ToSql> =
                vals.iter().map(|v| v as &dyn rusqlite::ToSql).collect();
            let result = st
                .query_map(rusqlite::params_from_iter(p), |r| r.get(0))?
                .collect::<std::result::Result<_, _>>()?;
            Ok(result)
        }
        Connection::Postgres(_) => {
            let p: Vec<&(dyn postgres::types::ToSql + Sync)> = vals
                .iter()
                .map(|v| v as &(dyn postgres::types::ToSql + Sync))
                .collect();
            Ok(c.as_pg()?
                .query(sql, &p)?
                .into_iter()
                .map(|r| r.get(0))
                .collect())
        }
    }
}
fn component_sql(c: &Connection, t: &str) -> String {
    let id_column = if c.is_pg() {
        format!("CAST({} AS varchar)", qi("id"))
    } else {
        qi("id")
    };
    format!(
        "SELECT {}, {} FROM {} ORDER BY {}",
        id_column,
        BASE_COLUMNS
            .iter()
            .map(|c| qi(c))
            .collect::<Vec<_>>()
            .join(", "),
        qi(t),
        qi("MPN")
    )
}
fn component_row(r: &Row) -> Component {
    let text = |index| {
        r.try_get::<_, Option<String>>(index)
            .ok()
            .flatten()
            .unwrap_or_default()
    };
    let id = r
        .try_get::<_, Option<String>>(0)
        .ok()
        .flatten()
        .or_else(|| r.try_get::<_, i64>(0).ok().map(|value| value.to_string()))
        .or_else(|| r.try_get::<_, i32>(0).ok().map(|value| value.to_string()))
        .or_else(|| r.try_get::<_, i16>(0).ok().map(|value| value.to_string()))
        .unwrap_or_default();
    let verified = r.try_get::<_, i64>(4).unwrap_or_default() != 0;
    Component {
        id,
        mpn: text(1),
        manufacturer: text(2),
        description: text(3),
        verified,
        library_ref: text(5),
        library_path: text(6),
        footprint_ref: text(7),
        footprint_path: text(8),
        footprint_ref2: text(9),
        footprint_path2: text(10),
        footprint_ref3: text(11),
        footprint_path3: text(12),
        component_link1_description: text(13),
        component_link1_url: text(14),
        component_link2_description: text(15),
        component_link2_url: text(16),
        component_link3_description: text(17),
        component_link3_url: text(18),
    }
}
fn sqlite_component(r: &rusqlite::Row<'_>) -> rusqlite::Result<Component> {
    Ok(Component {
        id: r.get::<_, i64>(0)?.to_string(),
        mpn: r.get(1)?,
        manufacturer: r.get(2)?,
        description: r.get(3)?,
        verified: r.get::<_, i64>(4)? != 0,
        library_ref: r.get(5)?,
        library_path: r.get(6)?,
        footprint_ref: r.get(7)?,
        footprint_path: r.get(8)?,
        footprint_ref2: r.get(9)?,
        footprint_path2: r.get(10)?,
        footprint_ref3: r.get(11)?,
        footprint_path3: r.get(12)?,
        component_link1_description: r.get(13)?,
        component_link1_url: r.get(14)?,
        component_link2_description: r.get(15)?,
        component_link2_url: r.get(16)?,
        component_link3_description: r.get(17)?,
        component_link3_url: r.get(18)?,
    })
}

pub fn table_exists(c: &Connection, name: &str) -> Result<bool> {
    let sql = if c.is_pg() {
        "SELECT EXISTS (SELECT 1 FROM information_schema.tables WHERE table_schema=current_schema() AND table_name=$1)"
    } else {
        "SELECT COUNT(*) > 0 FROM sqlite_master WHERE type='table' AND name=?"
    };
    match c {
        Connection::Sqlite(c) => {
            Ok(c.borrow().query_row(sql, [&name], |r| r.get::<_, i64>(0))? != 0)
        }
        Connection::Postgres(_) => Ok(c.as_pg()?.query_one(sql, &[&name])?.get(0)),
    }
}
pub fn get_tables(c: &Connection) -> Result<Vec<String>> {
    let sql = if c.is_pg() {
        "SELECT table_name FROM information_schema.tables WHERE table_schema=current_schema() AND table_type='BASE TABLE' ORDER BY table_name"
    } else {
        "SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%' ORDER BY name"
    };
    Ok(query_strings(c, sql, &[])?)
}
pub fn get_columns(c: &Connection, t: &str) -> Result<Vec<String>> {
    if c.is_pg() {
        Ok(c.as_pg()?.query("SELECT column_name FROM information_schema.columns WHERE table_schema=current_schema() AND table_name=$1 ORDER BY ordinal_position", &[&t])?.into_iter().map(|r| r.get(0)).collect())
    } else {
        Ok(c.as_sqlite()?
            .prepare(&format!("PRAGMA table_info({})", qi(t)))?
            .query_map([], |r| r.get(1))?
            .collect::<std::result::Result<_, _>>()?)
    }
}
trait ConnExt {
    fn as_pg(&self) -> Result<RefMut<'_, Client>>;
    fn as_sqlite(&self) -> Result<RefMut<'_, SqliteConnection>>;
}
impl ConnExt for Connection {
    fn as_pg(&self) -> Result<RefMut<'_, Client>> {
        match self {
            Connection::Postgres(c) => c
                .try_borrow_mut()
                .map_err(|_| Error::Message("database connection is busy".into())),
            _ => Err(Error::Message("operation requires PostgreSQL".into())),
        }
    }
    fn as_sqlite(&self) -> Result<RefMut<'_, SqliteConnection>> {
        match self {
            Connection::Sqlite(c) => c
                .try_borrow_mut()
                .map_err(|_| Error::Message("database connection is busy".into())),
            _ => Err(Error::Message("operation requires SQLite".into())),
        }
    }
}
pub fn add_column(c: &Connection, t: &str, col: &str) -> Result<()> {
    exec(
        c,
        &format!(
            "ALTER TABLE {} ADD COLUMN {} TEXT NOT NULL DEFAULT ''",
            qi(t),
            qi(col)
        ),
        &[],
    )?;
    Ok(())
}
pub fn rename_column(c: &Connection, t: &str, old: &str, new: &str) -> Result<()> {
    exec(
        c,
        &format!(
            "ALTER TABLE {} RENAME COLUMN {} TO {}",
            qi(t),
            qi(old),
            qi(new)
        ),
        &[],
    )?;
    Ok(())
}
pub fn drop_column(c: &Connection, t: &str, col: &str) -> Result<()> {
    exec(
        c,
        &format!("ALTER TABLE {} DROP COLUMN {}", qi(t), qi(col)),
        &[],
    )?;
    Ok(())
}
pub fn drop_table(c: &Connection, t: &str) -> Result<()> {
    exec(c, &format!("DROP TABLE IF EXISTS {}", qi(t)), &[])?;
    Ok(())
}
pub fn rename_table(c: &Connection, old: &str, new: &str) -> Result<()> {
    exec(
        c,
        &format!("ALTER TABLE {} RENAME TO {}", qi(old), qi(new)),
        &[],
    )?;
    Ok(())
}
pub fn ensure_base_columns(c: &Connection, t: &str) -> Result<()> {
    let cols = get_columns(c, t)?;
    for x in [
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
        if !cols.iter().any(|v| v == x) {
            add_column(c, t, x)?;
        }
    }
    Ok(())
}
pub fn ensure_table(c: &Connection, t: &str) -> Result<()> {
    if !table_exists(c, t)? {
        let id = if c.is_pg() {
            "BIGSERIAL"
        } else {
            "INTEGER PRIMARY KEY AUTOINCREMENT"
        };
        let pk = if c.is_pg() { "PRIMARY KEY" } else { "" };
        let cols = BASE_COLUMNS
            .iter()
            .map(|x| {
                format!(
                    "{} {} NOT NULL DEFAULT {}",
                    qi(x),
                    if *x == "Verified" { "BIGINT" } else { "TEXT" },
                    if *x == "Verified" { "0" } else { "''" }
                )
            })
            .collect::<Vec<_>>();
        exec(
            c,
            &format!(
                "CREATE TABLE {} ({} {} {}, {})",
                qi(t),
                qi("id"),
                id,
                pk,
                cols.join(", ")
            ),
            &[],
        )?;
    } else {
        ensure_base_columns(c, t)?;
    }
    Ok(())
}
fn rename_if(c: &Connection, t: &str, a: &str, b: &str) -> Result<()> {
    let x = get_columns(c, t)?;
    if x.iter().any(|v| v == a) && !x.iter().any(|v| v == b) {
        rename_column(c, t, a, b)?;
    }
    Ok(())
}
pub fn migrate(c: &Connection) -> Result<()> {
    for t in get_tables(c)? {
        for (a, b) in [
            ("Design Item ID", "MPN"),
            ("Symbols", "Library Ref"),
            ("Symbol Reference", "Library Ref"),
            ("Footprints", "Footprint Ref"),
            ("Footprint Reference", "Footprint Ref"),
            ("Datasheet", "ComponentLink1URL"),
        ] {
            rename_if(c, &t, a, b)?;
        }
        for x in [
            "Comment",
            "Footprint Filters",
            "Keywords",
            "No BOM",
            "Schematic Only",
            "No Sim",
            "Manufacturer Part Number",
        ] {
            if get_columns(c, &t)?.iter().any(|v| v == x) {
                drop_column(c, &t, x)?;
            }
        }
        ensure_base_columns(c, &t)?;
    }
    Ok(())
}

pub fn get_components(c: &Connection, t: &str) -> Result<Vec<Component>> {
    let sql = component_sql(c, t);
    match c {
        Connection::Sqlite(c) => Ok(c
            .borrow()
            .prepare(&sql)?
            .query_map([], sqlite_component)?
            .collect::<std::result::Result<_, _>>()?),
        Connection::Postgres(_) => Ok(c
            .as_pg()?
            .query(&sql, &[])?
            .iter()
            .map(component_row)
            .collect()),
    }
}
pub fn get_distinct_values(c: &Connection, t: &str, col: &str) -> Result<Vec<String>> {
    let sql = format!(
        "SELECT DISTINCT {} FROM {} WHERE {} IS NOT NULL AND {} <> '' ORDER BY {}",
        qi(col),
        qi(t),
        qi(col),
        qi(col),
        qi(col)
    );
    Ok(query_strings(c, &sql, &[])?)
}
pub fn search_components(
    c: &Connection,
    t: &str,
    filters: &[(String, Vec<String>)],
) -> Result<Vec<Component>> {
    let mut sql = component_sql(c, t).replace(&format!(" ORDER BY {}", qi("MPN")), " WHERE 1=1");
    let mut vals = Vec::new();
    for (col, vs) in filters {
        if !vs.is_empty() {
            let p = (1..=vs.len())
                .map(|i| sql_param(c.is_pg(), i + vals.len()))
                .collect::<Vec<_>>();
            sql += &format!(" AND {} IN ({})", qi(col), p.join(","));
            vals.extend(vs.iter().cloned());
        }
    }
    sql += &format!(" ORDER BY {}", qi("MPN"));
    match c {
        Connection::Sqlite(c) => {
            let c = c.borrow();
            let mut st = c.prepare(&sql)?;
            let p: Vec<&dyn rusqlite::ToSql> = vals.iter().map(|v| v as _).collect();
            let result = st
                .query_map(rusqlite::params_from_iter(p), sqlite_component)?
                .collect::<std::result::Result<_, _>>()?;
            Ok(result)
        }
        Connection::Postgres(_) => Ok(c
            .as_pg()?
            .query(
                &sql,
                &vals
                    .iter()
                    .map(|v| v as &(dyn postgres::types::ToSql + Sync))
                    .collect::<Vec<_>>(),
            )?
            .iter()
            .map(component_row)
            .collect()),
    }
}
pub fn search_all_by_mpn(c: &Connection, q: &str) -> Result<Vec<(String, Component)>> {
    let mut out = Vec::new();
    if q.trim().is_empty() {
        return Ok(out);
    }
    let pat = format!("%{}%", q);
    for t in get_tables(c)? {
        let sql = format!(
            "{} WHERE {} LIKE {} ORDER BY {}",
            component_sql(c, &t).replace(&format!(" ORDER BY {}", qi("MPN")), ""),
            qi("MPN"),
            sql_param(c.is_pg(), 1),
            qi("MPN")
        );
        let rows = get_components_like(c, &sql, &pat)?;
        out.extend(rows.into_iter().map(|x| (t.clone(), x)));
    }
    Ok(out)
}
fn get_components_like(c: &Connection, sql: &str, v: &str) -> Result<Vec<Component>> {
    match c {
        Connection::Sqlite(c) => Ok(c
            .borrow()
            .prepare(sql)?
            .query_map([v], sqlite_component)?
            .collect::<std::result::Result<_, _>>()?),
        Connection::Postgres(_) => Ok(c
            .as_pg()?
            .query(sql, &[&v])?
            .iter()
            .map(component_row)
            .collect()),
    }
}
fn component_values(c: &Component) -> Vec<String> {
    vec![
        c.mpn.clone(),
        c.manufacturer.clone(),
        c.description.clone(),
        (c.verified as i64).to_string(),
        c.library_ref.clone(),
        c.library_path.clone(),
        c.footprint_ref.clone(),
        c.footprint_path.clone(),
        c.footprint_ref2.clone(),
        c.footprint_path2.clone(),
        c.footprint_ref3.clone(),
        c.footprint_path3.clone(),
        c.component_link1_description.clone(),
        c.component_link1_url.clone(),
        c.component_link2_description.clone(),
        c.component_link2_url.clone(),
        c.component_link3_description.clone(),
        c.component_link3_url.clone(),
    ]
}
pub fn add_component(c: &Connection, t: &str, x: &Component) -> Result<i64> {
    add_component_with_custom_values(c, t, x, &std::collections::HashMap::new())
}

pub fn add_component_with_custom_values(
    c: &Connection,
    t: &str,
    x: &Component,
    custom_values: &std::collections::HashMap<String, String>,
) -> Result<i64> {
    let table_columns = get_columns(c, t)?;
    let custom_columns = table_columns
        .iter()
        .filter(|column| *column != "id" && !BASE_COLUMNS.contains(&column.as_str()))
        .cloned()
        .collect::<Vec<_>>();
    let mut columns = BASE_COLUMNS
        .iter()
        .map(|column| (*column).to_owned())
        .collect::<Vec<_>>();
    columns.extend(custom_columns.clone());
    let mut vals = component_values(x);
    vals.extend(
        custom_columns
            .iter()
            .map(|column| custom_values.get(column).cloned().unwrap_or_default()),
    );
    let quoted_columns = columns.iter().map(|column| qi(column)).collect::<Vec<_>>();
    let ps = if c.is_pg() {
        columns
            .iter()
            .enumerate()
            .map(|(i, col)| pg_param(c, t, col, i + 1))
            .collect::<Result<Vec<_>>>()?
    } else {
        (1..=vals.len()).map(|i| sql_param(false, i)).collect()
    };
    let sql = format!(
        "INSERT INTO {} ({}) VALUES ({})",
        qi(t),
        quoted_columns.join(","),
        ps.join(",")
    );
    if c.is_pg() {
        let has_generated_id = pg_id_has_generation(c, t)?;
        let column_refs = columns.iter().map(String::as_str).collect::<Vec<_>>();
        let params = pg_params(c, t, &column_refs, &vals)?;
        let refs = params
            .iter()
            .map(|v| v.as_ref() as &(dyn postgres::types::ToSql + Sync))
            .collect::<Vec<_>>();
        let sql = if has_generated_id {
            sql
        } else {
            format!(
                "INSERT INTO {} ({}, {}) SELECT COALESCE(MAX({}), 0) + 1, {} FROM {}",
                qi(t),
                qi("id"),
                quoted_columns.join(","),
                qi("id"),
                ps.join(","),
                qi(t)
            )
        };
        let r = c
            .as_pg()?
            .query_one(&format!("{} RETURNING {}", sql, qi("id")), &refs)?;
        r.try_get::<_, i64>(0)
            .or_else(|_| r.try_get::<_, i32>(0).map(i64::from))
            .map_err(|e| Error::Message(format!("failed to read new component id: {e}")))
    } else {
        exec(c, &sql, &vals)?;
        Ok(c.as_sqlite()?.last_insert_rowid())
    }
}
pub fn mpn_exists(c: &Connection, t: &str, m: &str) -> Result<bool> {
    let mpn_param = if c.is_pg() {
        pg_param(c, t, "MPN", 1)?
    } else {
        sql_param(false, 1)
    };
    let sql = format!(
        "SELECT COUNT(*) FROM {} WHERE {}={}",
        qi(t),
        qi("MPN"),
        mpn_param
    );
    if c.is_pg() {
        let params = pg_params(c, t, &["MPN"], &[m.to_owned()])?;
        let refs = params
            .iter()
            .map(|v| v.as_ref() as &(dyn postgres::types::ToSql + Sync))
            .collect::<Vec<_>>();
        Ok(c.as_pg()?.query_one(&sql, &refs)?.get::<_, i64>(0) > 0)
    } else {
        Ok(c.as_sqlite()?
            .query_row(&sql, [m], |r| r.get::<_, i64>(0))?
            > 0)
    }
}
pub fn update_component(c: &Connection, t: &str, x: &Component) -> Result<()> {
    let mpn_key = x.id.strip_prefix("__mpn__");
    let vals = component_values(x);
    let set = if c.is_pg() {
        BASE_COLUMNS
            .iter()
            .enumerate()
            .map(|(i, v)| Ok(format!("{}={}", qi(v), pg_param(c, t, v, i + 1)?)))
            .collect::<Result<Vec<_>>>()?
    } else {
        BASE_COLUMNS
            .iter()
            .enumerate()
            .map(|(i, v)| format!("{}={}", qi(v), sql_param(false, i + 1)))
            .collect()
    };
    let id_param = sql_param(c.is_pg(), 19);
    let where_clause = if c.is_pg() && mpn_key.is_some() {
        format!("{}=$19::text", qi("MPN"))
    } else if c.is_pg() {
        format!("CAST({} AS text)=$19::text", qi("id"))
    } else {
        format!("{}={}", qi("id"), id_param)
    };
    let sql = format!(
        "UPDATE {} SET {} WHERE {}",
        qi(t),
        set.join(","),
        where_clause
    );
    let mut v = vals;
    v.push(if let Some(old_mpn) = mpn_key {
        old_mpn.to_owned()
    } else if x.id.is_empty() {
        x.mpn.clone()
    } else {
        x.id.clone()
    });
    if c.is_pg() {
        let mut columns = BASE_COLUMNS.to_vec();
        columns.push("id");
        let params = pg_params(c, t, &columns, &v)?;
        if exec_pg_params(c, &sql, &params)? == 0 {
            return Err(Error::Message(format!(
                "component '{}' was not found in table '{}'",
                x.id, t
            )));
        }
    } else {
        exec(c, &sql, &v)?;
    }
    Ok(())
}
pub fn delete_component(c: &Connection, t: &str, id: &str) -> Result<()> {
    let id_param = sql_param(c.is_pg(), 1);
    let id_column = if c.is_pg() {
        "CAST(\"id\" AS text)"
    } else {
        "\"id\""
    };
    let sql = if c.is_pg() {
        format!(
            "DELETE FROM {} WHERE {}={} OR {}={}",
            qi(t),
            id_column,
            id_param,
            qi("MPN"),
            id_param
        )
    } else {
        format!("DELETE FROM {} WHERE {}={}", qi(t), id_column, id_param)
    };
    if c.is_pg() {
        if exec_pg_params(c, &sql, &pg_params(c, t, &["id"], &[id.into()])?)? == 0 {
            return Err(Error::Message(format!(
                "component '{}' was not found in table '{}'",
                id, t
            )));
        }
    } else {
        exec(c, &sql, &[id.into()])?;
    }
    Ok(())
}
pub fn clone_component(c: &Connection, t: &str, id: &str, new_mpn: &str) -> Result<()> {
    let cols = get_columns(c, t)?;
    let cols: Vec<_> = cols
        .into_iter()
        .filter(|x| x != "id" && x != "MPN")
        .collect();
    let list = cols.iter().map(|x| qi(x)).collect::<Vec<_>>().join(",");
    let (mpn_param, id_param) = if c.is_pg() {
        (pg_param(c, t, "MPN", 1)?, "$2::text".to_owned())
    } else {
        (sql_param(false, 1), sql_param(false, 2))
    };
    let id_column = if c.is_pg() {
        "CAST(\"id\" AS text)"
    } else {
        "\"id\""
    };
    let sql = format!(
        "INSERT INTO {} ({},{}) SELECT {},{} FROM {} WHERE {}={}",
        qi(t),
        qi("MPN"),
        list,
        mpn_param,
        list,
        qi(t),
        id_column,
        id_param
    );
    let sql = if c.is_pg() {
        format!("{} OR {}={}", sql, qi("MPN"), id_param)
    } else {
        sql
    };
    if c.is_pg() {
        let values = vec![new_mpn.to_owned(), id.to_owned()];
        let params = pg_params(c, t, &["MPN", "id"], &values)?;
        if exec_pg_params(c, &sql, &params)? == 0 {
            return Err(Error::Message(format!(
                "component '{}' was not found in table '{}'",
                id, t
            )));
        }
    } else {
        exec(c, &sql, &[new_mpn.into(), id.into()])?;
    }
    Ok(())
}
pub fn clone_table(c: &Connection, old: &str, new: &str) -> Result<()> {
    ensure_table(c, new)?;
    for col in get_columns(c, old)? {
        if col != "id" && !get_columns(c, new)?.contains(&col) {
            add_column(c, new, &col)?;
        }
    }
    let cols = get_columns(c, old)?
        .into_iter()
        .filter(|x| x != "id")
        .map(|x| qi(&x))
        .collect::<Vec<_>>()
        .join(",");
    if !cols.is_empty() {
        exec(
            c,
            &format!(
                "INSERT INTO {} ({}) SELECT {} FROM {}",
                qi(new),
                cols,
                cols,
                qi(old)
            ),
            &[],
        )?;
    }
    Ok(())
}
pub fn get_custom_value(c: &Connection, t: &str, id: &str, col: &str) -> Result<String> {
    if c.is_pg() {
        let (where_clause, parameter_column) = if id.starts_with("__mpn__") {
            (format!("{}=$1::text", qi("MPN")), "MPN")
        } else {
            (format!("CAST({} AS text)=$1::text", qi("id")), "id")
        };
        let sql = format!(
            "SELECT CAST({} AS text) FROM {} WHERE {}",
            qi(col),
            qi(t),
            where_clause
        );
        let parameter = id.strip_prefix("__mpn__").unwrap_or(id).to_owned();
        let params = pg_params(c, t, &[parameter_column], &[parameter])?;
        let refs = params
            .iter()
            .map(|value| value.as_ref() as &(dyn postgres::types::ToSql + Sync))
            .collect::<Vec<_>>();
        let row = c.as_pg()?.query_opt(&sql, &refs)?;
        Ok(row
            .and_then(|row| row.try_get::<_, Option<String>>(0).ok().flatten())
            .unwrap_or_default())
    } else {
        let (where_column, parameter) = id
            .strip_prefix("__mpn__")
            .map(|mpn| ("MPN", mpn))
            .unwrap_or(("id", id));
        let sql = format!(
            "SELECT {} FROM {} WHERE {}=?",
            qi(col),
            qi(t),
            qi(where_column)
        );
        Ok(c.as_sqlite()?
            .query_row(&sql, [parameter], |r| r.get::<_, Option<String>>(0))?
            .unwrap_or_default())
    }
}
pub fn set_custom_value(c: &Connection, t: &str, id: &str, col: &str, v: &str) -> Result<()> {
    let (value_param, id_param) = if c.is_pg() {
        (pg_param(c, t, col, 1)?, "$2::text".to_owned())
    } else {
        (sql_param(false, 1), sql_param(false, 2))
    };
    let where_clause = if c.is_pg() && id.starts_with("__mpn__") {
        format!("{}={}", qi("MPN"), id_param)
    } else if c.is_pg() {
        format!("CAST({} AS text)={}", qi("id"), id_param)
    } else {
        format!("{}={}", qi("id"), id_param)
    };
    let sql = format!(
        "UPDATE {} SET {}={} WHERE {}",
        qi(t),
        qi(col),
        value_param,
        where_clause
    );
    if c.is_pg() {
        let values = vec![
            v.to_owned(),
            id.strip_prefix("__mpn__").unwrap_or(id).to_owned(),
        ];
        let params = pg_params(c, t, &[col, "id"], &values)?;
        let sql = sql.replace(
            &format!("{}={}", qi("id"), id_param),
            &format!("CAST({} AS text)={}", qi("id"), id_param),
        );
        if exec_pg_params(c, &sql, &params)? == 0 {
            return Err(Error::Message(format!(
                "component '{}' was not found in table '{}'",
                id, t
            )));
        }
    } else {
        exec(c, &sql, &[v.into(), id.into()])?;
    }
    Ok(())
}
pub fn insert_component_row(
    c: &Connection,
    t: &str,
    values: &std::collections::HashMap<String, String>,
) -> Result<()> {
    let cols = get_columns(c, t)?;
    let mut x: Vec<_> = values
        .iter()
        .filter(|(k, v)| cols.contains(k) && *k != "id" && !v.is_empty())
        .collect();
    x.sort_by_key(|(k, _)| *k);
    if x.is_empty() {
        return Ok(());
    }
    let names = x.iter().map(|(k, _)| qi(k)).collect::<Vec<_>>().join(",");
    let ps = if c.is_pg() {
        x.iter()
            .enumerate()
            .map(|(i, (col, _))| pg_param(c, t, col, i + 1))
            .collect::<Result<Vec<_>>>()?
            .join(",")
    } else {
        (1..=x.len())
            .map(|i| sql_param(false, i))
            .collect::<Vec<_>>()
            .join(",")
    };
    let vals = x.iter().map(|(_, v)| (*v).clone()).collect::<Vec<_>>();
    let sql = format!("INSERT INTO {} ({}) VALUES ({})", qi(t), names, ps);
    if c.is_pg() {
        let columns = x.iter().map(|(k, _)| k.as_str()).collect::<Vec<_>>();
        let params = pg_params(c, t, &columns, &vals)?;
        exec_pg_params(c, &sql, &params)?;
    } else {
        exec(c, &sql, &vals)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn postgres_parameter_conversion_uses_column_types() {
        assert_eq!(pg_value_kind("bigint", "int8"), PgValueKind::Integer);
        assert_eq!(pg_value_kind("boolean", "bool"), PgValueKind::Boolean);
        assert_eq!(
            pg_value_kind("double precision", "float8"),
            PgValueKind::Float
        );
        assert_eq!(pg_value_kind("numeric", "numeric"), PgValueKind::Float);
        assert_eq!(pg_value_kind("text", "text"), PgValueKind::Text);

        assert!(convert_pg_value(PgValueKind::Integer, "42").is_ok());
        assert!(convert_pg_value(PgValueKind::Boolean, "1").is_ok());
        assert!(convert_pg_value(PgValueKind::Float, "1.25").is_ok());
        assert!(convert_pg_value(PgValueKind::Integer, "not-a-number").is_err());
        assert!(convert_pg_value(PgValueKind::Boolean, "maybe").is_err());
        assert_eq!(
            pg_cast_expression(3, "my_schema.my_domain", "S"),
            "CAST($3::text AS my_schema.my_domain)"
        );
        assert_eq!(
            pg_cast_expression(4, "integer", "N"),
            "CAST(NULLIF($4::text, '') AS integer)"
        );
    }

    #[test]
    fn sqlite_unified_crud_and_custom_fields() {
        let db = Connection::Sqlite(RefCell::new(SqliteConnection::open_in_memory().unwrap()));
        ensure_table(&db, "Parts \"quoted\"").unwrap();
        add_column(&db, "Parts \"quoted\"", "Custom Field").unwrap();
        let id = add_component(
            &db,
            "Parts \"quoted\"",
            &Component {
                mpn: "R-1".into(),
                manufacturer: "Acme".into(),
                ..Default::default()
            },
        )
        .unwrap();
        assert!(mpn_exists(&db, "Parts \"quoted\"", "R-1").unwrap());
        set_custom_value(
            &db,
            "Parts \"quoted\"",
            &id.to_string(),
            "Custom Field",
            "x",
        )
        .unwrap();
        assert_eq!(
            get_custom_value(&db, "Parts \"quoted\"", &id.to_string(), "Custom Field").unwrap(),
            "x"
        );
        update_component(
            &db,
            "Parts \"quoted\"",
            &Component {
                id: id.to_string(),
                mpn: "R-2".into(),
                ..Default::default()
            },
        )
        .unwrap();
        assert!(mpn_exists(&db, "Parts \"quoted\"", "R-2").unwrap());
        delete_component(&db, "Parts \"quoted\"", &id.to_string()).unwrap();
        assert!(!mpn_exists(&db, "Parts \"quoted\"", "R-2").unwrap());
    }
}
