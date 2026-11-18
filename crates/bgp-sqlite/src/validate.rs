use std::path::Path;

use rusqlite::{Connection, OpenFlags};

pub fn validate_sqlite_db(db_path: &Path, indexed: bool) -> Result<(), String> {
    let conn = Connection::open_with_flags(db_path, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(|e| format!("open sqlite: {e}"))?;
    let mut tables = vec!["allocations", "routes", "autnums", "geolocations"];
    if indexed {
        tables.extend(["lookup_prefixes", "range_summaries"]);
    }
    for table in &tables {
        let count: i64 = conn
            .query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r.get(0))
            .map_err(|e| format!("validate {table}: {e}"))?;
        if count == 0 {
            return Err(format!("validate {table}: table is empty"));
        }
        println!("{table}: {count}");
    }
    let mut required = vec!["allocations", "routes", "autnums"];
    if indexed {
        required.push("lookup_prefixes");
    }
    for table in required {
        if !has_column(&conn, table, "abuse_contact")? {
            return Err(format!(
                "validate {table}: missing required column abuse_contact"
            ));
        }
    }
    Ok(())
}

fn has_column(conn: &Connection, table: &str, column: &str) -> Result<bool, String> {
    let mut stmt = conn
        .prepare(&format!("PRAGMA table_info({table})"))
        .map_err(|e| format!("inspect {table}: {e}"))?;
    let mut rows = stmt
        .query([])
        .map_err(|e| format!("inspect {table}: {e}"))?;
    while let Some(row) = rows.next().map_err(|e| format!("inspect {table}: {e}"))? {
        let name: String = row.get(1).map_err(|e| format!("inspect {table}: {e}"))?;
        if name.eq_ignore_ascii_case(column) {
            return Ok(true);
        }
    }
    Ok(false)
}
