use rusqlite::{params, Connection};
use std::time::{SystemTime, UNIX_EPOCH};

pub struct Migration {
    pub version: i64,
    pub name: &'static str,
    pub sql: &'static str,
}

pub const MIGRATIONS: &[Migration] = &[
    Migration {
        version: 1,
        name: "initial_preferences_and_history",
        sql: r#"
            CREATE TABLE IF NOT EXISTS preferences (
                id INTEGER PRIMARY KEY CHECK (id = 1),
                theme TEXT NOT NULL CHECK (theme IN ('system', 'light', 'dark')),
                appimage_folders_json TEXT NOT NULL,
                history_retention_days INTEGER NOT NULL CHECK (history_retention_days >= 0 AND history_retention_days <= 3650),
                updated_at INTEGER NOT NULL
            );

            CREATE TABLE IF NOT EXISTS operation_history (
                id TEXT PRIMARY KEY,
                operation_id TEXT NOT NULL,
                target_name TEXT NOT NULL,
                native_id TEXT NOT NULL,
                format TEXT NOT NULL CHECK (format IN ('deb', 'snap', 'flatpak', 'appimage')),
                status TEXT NOT NULL CHECK (status IN ('pending', 'running', 'verifying', 'completed', 'failed', 'cancelled')),
                summary TEXT NOT NULL,
                diagnostic TEXT NOT NULL,
                timestamp INTEGER NOT NULL
            );

            CREATE INDEX IF NOT EXISTS idx_operation_history_timestamp
                ON operation_history (timestamp DESC);
        "#,
    },
    Migration {
        version: 2,
        name: "in_flight_operations",
        sql: r#"
            CREATE TABLE IF NOT EXISTS in_flight_operations (
                operation_id TEXT PRIMARY KEY,
                plan_id TEXT NOT NULL,
                target_id TEXT NOT NULL,
                target_name TEXT NOT NULL,
                native_id TEXT NOT NULL,
                format TEXT NOT NULL CHECK (format IN ('deb', 'snap', 'flatpak', 'appimage')),
                scope TEXT NOT NULL,
                target_path TEXT,
                target_installation TEXT,
                started_at INTEGER NOT NULL
            );
        "#,
    },
    Migration {
        version: 3,
        name: "support_local_package_format",
        sql: r#"
            CREATE TABLE operation_history_v3 (
                id TEXT PRIMARY KEY,
                operation_id TEXT NOT NULL,
                target_name TEXT NOT NULL,
                native_id TEXT NOT NULL,
                format TEXT NOT NULL CHECK (format IN ('deb', 'snap', 'flatpak', 'appimage', 'local')),
                status TEXT NOT NULL CHECK (status IN ('pending', 'running', 'verifying', 'completed', 'failed', 'cancelled')),
                summary TEXT NOT NULL,
                diagnostic TEXT NOT NULL,
                timestamp INTEGER NOT NULL
            );
            INSERT INTO operation_history_v3
                SELECT id, operation_id, target_name, native_id, format, status, summary, diagnostic, timestamp
                FROM operation_history;
            DROP TABLE operation_history;
            ALTER TABLE operation_history_v3 RENAME TO operation_history;
            CREATE INDEX IF NOT EXISTS idx_operation_history_timestamp
                ON operation_history (timestamp DESC);

            CREATE TABLE in_flight_operations_v3 (
                operation_id TEXT PRIMARY KEY,
                plan_id TEXT NOT NULL,
                target_id TEXT NOT NULL,
                target_name TEXT NOT NULL,
                native_id TEXT NOT NULL,
                format TEXT NOT NULL CHECK (format IN ('deb', 'snap', 'flatpak', 'appimage', 'local')),
                scope TEXT NOT NULL,
                target_path TEXT,
                target_installation TEXT,
                started_at INTEGER NOT NULL
            );
            INSERT INTO in_flight_operations_v3
                SELECT operation_id, plan_id, target_id, target_name, native_id, format, scope, target_path, target_installation, started_at
                FROM in_flight_operations;
            DROP TABLE in_flight_operations;
            ALTER TABLE in_flight_operations_v3 RENAME TO in_flight_operations;
        "#,
    },
];

pub const LATEST_SCHEMA_VERSION: i64 = 3;

fn now_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

/// Applies all pending versioned SQLite migrations atomically.
pub fn run_migrations(conn: &mut Connection) -> Result<(), rusqlite::Error> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS schema_migrations (
            version INTEGER PRIMARY KEY,
            name TEXT NOT NULL,
            applied_at INTEGER NOT NULL
        );",
    )?;

    let current_version: i64 = conn.query_row(
        "SELECT COALESCE(MAX(version), 0) FROM schema_migrations",
        [],
        |row| row.get(0),
    )?;

    for migration in MIGRATIONS {
        if migration.version <= current_version {
            continue;
        }
        let tx = conn.transaction()?;
        tx.execute_batch(migration.sql)?;
        tx.execute(
            "INSERT INTO schema_migrations (version, name, applied_at) VALUES (?1, ?2, ?3)",
            params![migration.version, migration.name, now_secs()],
        )?;
        tx.commit()?;
    }

    Ok(())
}

/// Returns the latest applied migration version.
pub fn current_schema_version(conn: &Connection) -> Result<i64, rusqlite::Error> {
    conn.query_row(
        "SELECT COALESCE(MAX(version), 0) FROM schema_migrations",
        [],
        |row| row.get(0),
    )
}
