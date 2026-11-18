pub mod migrations;

use crate::domain::{AppError, HistoryEntry, OperationStatus, PackageFormat, Preferences};
use rusqlite::{params, Connection, OptionalExtension};
use std::fs;
use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const MAX_HISTORY_ENTRIES: i64 = 500;
static STORAGE_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InFlightOperation {
    pub operation_id: String,
    pub plan_id: String,
    pub target_id: String,
    pub target_name: String,
    pub native_id: String,
    pub format: PackageFormat,
    pub scope: String,
    pub target_path: Option<String>,
    pub target_installation: Option<String>,
    pub started_at: u64,
}

fn config_dir() -> PathBuf {
    if let Ok(path) = std::env::var("PANEL_CENTER_CONFIG_DIR") {
        return PathBuf::from(path);
    }
    if let Ok(xdg) = std::env::var("XDG_CONFIG_HOME") {
        PathBuf::from(xdg).join("panel-center")
    } else if let Ok(home) = std::env::var("HOME") {
        PathBuf::from(home).join(".config").join("panel-center")
    } else {
        PathBuf::from("/tmp/panel-center/config")
    }
}

fn data_dir() -> PathBuf {
    if let Ok(path) = std::env::var("PANEL_CENTER_DATA_DIR") {
        return PathBuf::from(path);
    }
    if let Ok(xdg) = std::env::var("XDG_DATA_HOME") {
        PathBuf::from(xdg).join("panel-center")
    } else if let Ok(home) = std::env::var("HOME") {
        PathBuf::from(home)
            .join(".local")
            .join("share")
            .join("panel-center")
    } else {
        PathBuf::from("/tmp/panel-center/data")
    }
}

pub fn database_path() -> PathBuf {
    if let Ok(path) = std::env::var("PANEL_CENTER_DATA_DIR") {
        return PathBuf::from(path).join("panel_center.db");
    }
    if let Ok(path) = std::env::var("PANEL_CENTER_CONFIG_DIR") {
        return PathBuf::from(path).join("panel_center.db");
    }
    data_dir().join("panel_center.db")
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn format_to_str(format: PackageFormat) -> &'static str {
    match format {
        PackageFormat::Deb => "deb",
        PackageFormat::Snap => "snap",
        PackageFormat::Flatpak => "flatpak",
        PackageFormat::Appimage => "appimage",
        PackageFormat::Local => "local",
    }
}

fn str_to_format(value: &str) -> Option<PackageFormat> {
    match value {
        "deb" => Some(PackageFormat::Deb),
        "snap" => Some(PackageFormat::Snap),
        "flatpak" => Some(PackageFormat::Flatpak),
        "appimage" => Some(PackageFormat::Appimage),
        "local" => Some(PackageFormat::Local),
        _ => None,
    }
}

fn status_to_str(status: OperationStatus) -> &'static str {
    match status {
        OperationStatus::Pending => "pending",
        OperationStatus::Running => "running",
        OperationStatus::Verifying => "verifying",
        OperationStatus::Completed => "completed",
        OperationStatus::Failed => "failed",
        OperationStatus::Cancelled => "cancelled",
    }
}

fn str_to_status(value: &str) -> Option<OperationStatus> {
    match value {
        "pending" => Some(OperationStatus::Pending),
        "running" => Some(OperationStatus::Running),
        "verifying" => Some(OperationStatus::Verifying),
        "completed" => Some(OperationStatus::Completed),
        "failed" => Some(OperationStatus::Failed),
        "cancelled" => Some(OperationStatus::Cancelled),
        _ => None,
    }
}

fn validate_preferences(prefs: &Preferences) -> Result<(), AppError> {
    if !matches!(prefs.theme.as_str(), "system" | "light" | "dark") {
        return Err(AppError::new(
            "INVALID_PREFERENCES",
            "Le thème choisi n’est pas valide (system, light ou dark attendu).",
        ));
    }
    if prefs.history_retention_days > 3650 {
        return Err(AppError::new(
            "INVALID_PREFERENCES",
            "La politique de rétention de l’historique est excessive.",
        ));
    }
    if prefs.appimage_folders.len() > 32
        || prefs
            .appimage_folders
            .iter()
            .any(|f| f.len() > 4096 || f.contains('\0'))
    {
        return Err(AppError::new(
            "INVALID_PREFERENCES",
            "Les emplacements AppImage configurés sont invalides.",
        ));
    }
    Ok(())
}

fn open_connection() -> Result<Connection, AppError> {
    let path = database_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| {
            AppError::new(
                "STORAGE_DIR",
                &format!("Impossible de créer le dossier de stockage : {e}"),
            )
        })?;
    }

    let mut conn = Connection::open(&path).map_err(|e| {
        AppError::new(
            "STORAGE_OPEN",
            &format!("Impossible d’ouvrir la base SQLite locale : {e}"),
        )
    })?;

    let _ = conn.busy_timeout(Duration::from_secs(5));
    let _ = conn.execute_batch("PRAGMA journal_mode = WAL; PRAGMA foreign_keys = ON;");

    migrations::run_migrations(&mut conn).map_err(|e| {
        AppError::new(
            "STORAGE_MIGRATION",
            &format!("Échec lors de la migration de la base SQLite : {e}"),
        )
    })?;

    import_legacy_json_if_empty(&mut conn);
    Ok(conn)
}

/// Imports legacy `preferences.json` and `history.json` once when migrating
/// from the earlier JSON persistence to SQLite.
fn import_legacy_json_if_empty(conn: &mut Connection) {
    let pref_count: i64 = conn
        .query_row("SELECT COUNT(*) FROM preferences", [], |row| row.get(0))
        .unwrap_or(1);
    if pref_count == 0 {
        let legacy_pref_path = config_dir().join("preferences.json");
        if let Ok(content) = fs::read_to_string(&legacy_pref_path) {
            if let Ok(prefs) = serde_json::from_str::<Preferences>(&content) {
                if validate_preferences(&prefs).is_ok() {
                    let _ = upsert_preferences(conn, &prefs);
                }
            }
        }
    }

    let hist_count: i64 = conn
        .query_row("SELECT COUNT(*) FROM operation_history", [], |row| {
            row.get(0)
        })
        .unwrap_or(1);
    if hist_count == 0 {
        let legacy_hist_path = data_dir().join("history.json");
        if let Ok(content) = fs::read_to_string(&legacy_hist_path) {
            if let Ok(entries) = serde_json::from_str::<Vec<HistoryEntry>>(&content) {
                if let Ok(tx) = conn.transaction() {
                    for entry in entries.into_iter().take(MAX_HISTORY_ENTRIES as usize) {
                        let _ = tx.execute(
                            "INSERT OR IGNORE INTO operation_history
                             (id, operation_id, target_name, native_id, format, status, summary, diagnostic, timestamp)
                             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                            params![
                                entry.id,
                                entry.operation_id,
                                entry.target_name,
                                entry.native_id,
                                format_to_str(entry.format),
                                status_to_str(entry.status),
                                entry.summary,
                                entry.diagnostic,
                                entry.timestamp as i64,
                            ],
                        );
                    }
                    let _ = tx.commit();
                }
            }
        }
    }
}

fn upsert_preferences(conn: &Connection, prefs: &Preferences) -> Result<(), AppError> {
    let folders_json = serde_json::to_string(&prefs.appimage_folders).map_err(|e| {
        AppError::new(
            "PREFERENCES_SERIALIZE",
            &format!("Erreur lors de la sérialisation des dossiers AppImage : {e}"),
        )
    })?;
    conn.execute(
        "INSERT INTO preferences (id, theme, appimage_folders_json, history_retention_days, updated_at)
         VALUES (1, ?1, ?2, ?3, ?4)
         ON CONFLICT(id) DO UPDATE SET
            theme = excluded.theme,
            appimage_folders_json = excluded.appimage_folders_json,
            history_retention_days = excluded.history_retention_days,
            updated_at = excluded.updated_at",
        params![
            prefs.theme,
            folders_json,
            prefs.history_retention_days as i64,
            now_secs() as i64,
        ],
    )
    .map_err(|e| {
        AppError::new(
            "PREFERENCES_WRITE",
            &format!("Impossible d’enregistrer les préférences dans SQLite : {e}"),
        )
    })?;
    Ok(())
}

fn load_preferences_from_conn(conn: &Connection) -> Preferences {
    let row = conn
        .query_row(
            "SELECT theme, appimage_folders_json, history_retention_days FROM preferences WHERE id = 1",
            [],
            |row| {
                let theme: String = row.get(0)?;
                let folders_json: String = row.get(1)?;
                let retention: i64 = row.get(2)?;
                Ok((theme, folders_json, retention))
            },
        )
        .optional();

    if let Ok(Some((theme, folders_json, retention))) = row {
        let appimage_folders =
            serde_json::from_str::<Vec<String>>(&folders_json).unwrap_or_default();
        return Preferences {
            theme,
            appimage_folders,
            history_retention_days: u32::try_from(retention).unwrap_or(30),
        };
    }

    Preferences::default()
}

pub fn load_preferences() -> Preferences {
    let _guard = STORAGE_LOCK.lock().unwrap();
    match open_connection() {
        Ok(conn) => load_preferences_from_conn(&conn),
        Err(_) => Preferences::default(),
    }
}

pub fn save_preferences(prefs: &Preferences) -> Result<(), AppError> {
    let _guard = STORAGE_LOCK.lock().unwrap();
    validate_preferences(prefs)?;
    let conn = open_connection()?;
    upsert_preferences(&conn, prefs)
}

pub fn load_history() -> Vec<HistoryEntry> {
    let _guard = STORAGE_LOCK.lock().unwrap();
    let Ok(conn) = open_connection() else {
        return vec![];
    };
    let Ok(mut stmt) = conn.prepare(
        "SELECT id, operation_id, target_name, native_id, format, status, summary, diagnostic, timestamp
         FROM operation_history
         ORDER BY timestamp DESC, rowid DESC
         LIMIT ?1",
    ) else {
        return vec![];
    };

    let rows = stmt.query_map(params![MAX_HISTORY_ENTRIES], |row| {
        let format_str: String = row.get(4)?;
        let status_str: String = row.get(5)?;
        let ts: i64 = row.get(8)?;
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, String>(3)?,
            format_str,
            status_str,
            row.get::<_, String>(6)?,
            row.get::<_, String>(7)?,
            ts,
        ))
    });

    let Ok(mapped) = rows else {
        return vec![];
    };

    mapped
        .filter_map(Result::ok)
        .filter_map(
            |(id, operation_id, target_name, native_id, fmt, st, summary, diagnostic, ts)| {
                Some(HistoryEntry {
                    id,
                    operation_id,
                    target_name,
                    native_id,
                    format: str_to_format(&fmt)?,
                    status: str_to_status(&st)?,
                    summary,
                    diagnostic,
                    timestamp: u64::try_from(ts).unwrap_or(0),
                })
            },
        )
        .collect()
}

pub fn append_history(entry: HistoryEntry) -> Result<(), AppError> {
    let _guard = STORAGE_LOCK.lock().unwrap();
    let mut conn = open_connection()?;
    let prefs = load_preferences_from_conn(&conn);
    let now = now_secs();

    let tx = conn.transaction().map_err(|e| {
        AppError::new(
            "HISTORY_WRITE",
            &format!("Impossible d’ouvrir la transaction SQLite : {e}"),
        )
    })?;

    if prefs.history_retention_days > 0 {
        let max_age_secs = u64::from(prefs.history_retention_days) * 86_400;
        let cutoff = now.saturating_sub(max_age_secs) as i64;
        tx.execute(
            "DELETE FROM operation_history WHERE timestamp < ?1",
            params![cutoff],
        )
        .map_err(|e| {
            AppError::new(
                "HISTORY_WRITE",
                &format!("Impossible d’appliquer la rétention de l’historique : {e}"),
            )
        })?;
    }

    tx.execute(
        "INSERT OR REPLACE INTO operation_history
         (id, operation_id, target_name, native_id, format, status, summary, diagnostic, timestamp)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params![
            entry.id,
            entry.operation_id,
            entry.target_name,
            entry.native_id,
            format_to_str(entry.format),
            status_to_str(entry.status),
            entry.summary,
            entry.diagnostic,
            entry.timestamp as i64,
        ],
    )
    .map_err(|e| {
        AppError::new(
            "HISTORY_WRITE",
            &format!("Impossible d’enregistrer l’historique dans SQLite : {e}"),
        )
    })?;

    // Cap stored rows at MAX_HISTORY_ENTRIES, retaining the newest entries.
    tx.execute(
        "DELETE FROM operation_history
         WHERE id NOT IN (
             SELECT id FROM operation_history
             ORDER BY timestamp DESC, rowid DESC
             LIMIT ?1
         )",
        params![MAX_HISTORY_ENTRIES],
    )
    .map_err(|e| {
        AppError::new(
            "HISTORY_WRITE",
            &format!("Impossible de plafonner l’historique SQLite : {e}"),
        )
    })?;

    tx.commit().map_err(|e| {
        AppError::new(
            "HISTORY_WRITE",
            &format!("Impossible de valider l’enregistrement de l’historique : {e}"),
        )
    })?;

    Ok(())
}

pub fn clear_history() -> Result<(), AppError> {
    let _guard = STORAGE_LOCK.lock().unwrap();
    let conn = open_connection()?;
    conn.execute("DELETE FROM operation_history", [])
        .map_err(|e| {
            AppError::new(
                "HISTORY_CLEAR",
                &format!("Impossible d’effacer l’historique : {e}"),
            )
        })?;
    Ok(())
}

/// Persists an operation as in-flight before privileged/user execution begins,
/// so a crash or forced exit can be reconciled on next launch.
pub fn record_in_flight_operation(op: &InFlightOperation) -> Result<(), AppError> {
    let _guard = STORAGE_LOCK.lock().unwrap();
    let conn = open_connection()?;
    conn.execute(
        "INSERT OR REPLACE INTO in_flight_operations
         (operation_id, plan_id, target_id, target_name, native_id, format, scope, target_path, target_installation, started_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        params![
            op.operation_id,
            op.plan_id,
            op.target_id,
            op.target_name,
            op.native_id,
            format_to_str(op.format),
            op.scope,
            op.target_path,
            op.target_installation,
            op.started_at as i64,
        ],
    )
    .map_err(|e| {
        AppError::new(
            "STORAGE_WRITE",
            &format!("Impossible d’enregistrer l’opération en cours : {e}"),
        )
    })?;
    Ok(())
}

/// Removes an operation from the in-flight table once it has cleanly finished.
pub fn remove_in_flight_operation(operation_id: &str) -> Result<(), AppError> {
    let _guard = STORAGE_LOCK.lock().unwrap();
    let conn = open_connection()?;
    conn.execute(
        "DELETE FROM in_flight_operations WHERE operation_id = ?1",
        params![operation_id],
    )
    .map_err(|e| {
        AppError::new(
            "STORAGE_WRITE",
            &format!("Impossible de clore l’opération en cours : {e}"),
        )
    })?;
    Ok(())
}

/// Loads all unfinished operations persisted before an unexpected shutdown.
pub fn list_in_flight_operations() -> Vec<InFlightOperation> {
    let _guard = STORAGE_LOCK.lock().unwrap();
    let Ok(conn) = open_connection() else {
        return vec![];
    };
    let Ok(mut stmt) = conn.prepare(
        "SELECT operation_id, plan_id, target_id, target_name, native_id, format, scope, target_path, target_installation, started_at
         FROM in_flight_operations
         ORDER BY started_at ASC",
    ) else {
        return vec![];
    };

    let Ok(rows) = stmt.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, String>(3)?,
            row.get::<_, String>(4)?,
            row.get::<_, String>(5)?,
            row.get::<_, String>(6)?,
            row.get::<_, Option<String>>(7)?,
            row.get::<_, Option<String>>(8)?,
            row.get::<_, i64>(9)?,
        ))
    }) else {
        return vec![];
    };

    rows.filter_map(Result::ok)
        .filter_map(
            |(
                operation_id,
                plan_id,
                target_id,
                target_name,
                native_id,
                fmt,
                scope,
                target_path,
                target_installation,
                started_at,
            )| {
                Some(InFlightOperation {
                    operation_id,
                    plan_id,
                    target_id,
                    target_name,
                    native_id,
                    format: str_to_format(&fmt)?,
                    scope,
                    target_path,
                    target_installation,
                    started_at: u64::try_from(started_at).unwrap_or(0),
                })
            },
        )
        .collect()
}

pub fn schema_version() -> Result<i64, AppError> {
    let _guard = STORAGE_LOCK.lock().unwrap();
    let conn = open_connection()?;
    migrations::current_schema_version(&conn).map_err(|e| {
        AppError::new(
            "STORAGE_QUERY",
            &format!("Impossible de lire la version du schéma SQLite : {e}"),
        )
    })
}

#[cfg(test)]
pub static TEST_STORAGE_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{OperationStatus, PackageFormat};

    #[test]
    fn preferences_round_trip() {
        let _lock = TEST_STORAGE_LOCK.lock().unwrap();
        let temp_dir =
            std::env::temp_dir().join(format!("panel_center_test_pref_{}", std::process::id()));
        let _ = fs::remove_dir_all(&temp_dir);
        std::env::set_var("PANEL_CENTER_DATA_DIR", &temp_dir);
        std::env::set_var("PANEL_CENTER_CONFIG_DIR", &temp_dir);

        let prefs = Preferences {
            theme: "dark".into(),
            history_retention_days: 90,
            appimage_folders: vec!["/opt/apps".into()],
        };

        save_preferences(&prefs).expect("save should succeed");
        assert!(temp_dir.join("panel_center.db").exists());
        assert_eq!(schema_version().unwrap(), migrations::LATEST_SCHEMA_VERSION);

        let loaded = load_preferences();
        assert_eq!(loaded.theme, "dark");
        assert_eq!(loaded.history_retention_days, 90);
        assert_eq!(loaded.appimage_folders, vec!["/opt/apps"]);

        let _ = fs::remove_dir_all(&temp_dir);
        std::env::remove_var("PANEL_CENTER_DATA_DIR");
        std::env::remove_var("PANEL_CENTER_CONFIG_DIR");
    }

    #[test]
    fn history_retention_and_clear() {
        let _lock = TEST_STORAGE_LOCK.lock().unwrap();
        let temp_dir =
            std::env::temp_dir().join(format!("panel_center_test_hist_{}", std::process::id()));
        let _ = fs::remove_dir_all(&temp_dir);
        std::env::set_var("PANEL_CENTER_DATA_DIR", &temp_dir);
        std::env::set_var("PANEL_CENTER_CONFIG_DIR", &temp_dir);

        let prefs = Preferences {
            theme: "system".into(),
            appimage_folders: vec![],
            history_retention_days: 1, // 1 day
        };
        save_preferences(&prefs).expect("save pref");

        let old_entry = HistoryEntry {
            id: "hist_1".into(),
            operation_id: "op_1".into(),
            target_name: "Old App".into(),
            native_id: "old-app".into(),
            format: PackageFormat::Deb,
            status: OperationStatus::Completed,
            summary: "Suppression terminée".into(),
            diagnostic: "".into(),
            timestamp: 100, // very old
        };

        let now = now_secs();
        let new_entry = HistoryEntry {
            id: "hist_2".into(),
            operation_id: "op_2".into(),
            target_name: "Recent App".into(),
            native_id: "recent-app".into(),
            format: PackageFormat::Flatpak,
            status: OperationStatus::Completed,
            summary: "Suppression terminée".into(),
            diagnostic: "".into(),
            timestamp: now,
        };

        append_history(old_entry).expect("append 1");
        append_history(new_entry).expect("append 2");

        let loaded = load_history();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].id, "hist_2");

        clear_history().expect("clear");
        assert_eq!(load_history().len(), 0);

        let _ = fs::remove_dir_all(&temp_dir);
        std::env::remove_var("PANEL_CENTER_DATA_DIR");
        std::env::remove_var("PANEL_CENTER_CONFIG_DIR");
    }

    #[test]
    fn imports_legacy_json_preferences_and_history_into_sqlite() {
        let _lock = TEST_STORAGE_LOCK.lock().unwrap();
        let temp_dir =
            std::env::temp_dir().join(format!("panel_center_test_legacy_{}", std::process::id()));
        let _ = fs::remove_dir_all(&temp_dir);
        fs::create_dir_all(&temp_dir).unwrap();
        std::env::set_var("PANEL_CENTER_DATA_DIR", &temp_dir);
        std::env::set_var("PANEL_CENTER_CONFIG_DIR", &temp_dir);

        let legacy_prefs = Preferences {
            theme: "light".into(),
            appimage_folders: vec!["/home/user/Apps".into()],
            history_retention_days: 45,
        };
        fs::write(
            temp_dir.join("preferences.json"),
            serde_json::to_string(&legacy_prefs).unwrap(),
        )
        .unwrap();

        let legacy_history = vec![HistoryEntry {
            id: "hist_legacy".into(),
            operation_id: "op_legacy".into(),
            target_name: "LegacyApp".into(),
            native_id: "legacy-app".into(),
            format: PackageFormat::Appimage,
            status: OperationStatus::Completed,
            summary: "Retiré".into(),
            diagnostic: "".into(),
            timestamp: now_secs(),
        }];
        fs::write(
            temp_dir.join("history.json"),
            serde_json::to_string(&legacy_history).unwrap(),
        )
        .unwrap();

        let loaded_prefs = load_preferences();
        assert_eq!(loaded_prefs.theme, "light");
        assert_eq!(loaded_prefs.history_retention_days, 45);
        assert_eq!(loaded_prefs.appimage_folders, vec!["/home/user/Apps"]);

        let loaded_history = load_history();
        assert_eq!(loaded_history.len(), 1);
        assert_eq!(loaded_history[0].id, "hist_legacy");

        let _ = fs::remove_dir_all(&temp_dir);
        std::env::remove_var("PANEL_CENTER_DATA_DIR");
        std::env::remove_var("PANEL_CENTER_CONFIG_DIR");
    }

    #[test]
    fn records_and_clears_in_flight_operations() {
        let _lock = TEST_STORAGE_LOCK.lock().unwrap();
        let temp_dir =
            std::env::temp_dir().join(format!("panel_center_test_inflight_{}", std::process::id()));
        let _ = fs::remove_dir_all(&temp_dir);
        std::env::set_var("PANEL_CENTER_DATA_DIR", &temp_dir);
        std::env::set_var("PANEL_CENTER_CONFIG_DIR", &temp_dir);

        let op = InFlightOperation {
            operation_id: "op_crash_1".into(),
            plan_id: "plan_1".into(),
            target_id: "appimage:/tmp/crash.AppImage".into(),
            target_name: "CrashApp".into(),
            native_id: "crash-app".into(),
            format: PackageFormat::Appimage,
            scope: "user".into(),
            target_path: Some("/tmp/crash.AppImage".into()),
            target_installation: None,
            started_at: now_secs(),
        };

        record_in_flight_operation(&op).expect("record");
        let list = list_in_flight_operations();
        assert_eq!(list, vec![op.clone()]);

        remove_in_flight_operation(&op.operation_id).expect("remove");
        assert!(list_in_flight_operations().is_empty());

        let _ = fs::remove_dir_all(&temp_dir);
        std::env::remove_var("PANEL_CENTER_DATA_DIR");
        std::env::remove_var("PANEL_CENTER_CONFIG_DIR");
    }

    #[test]
    fn preferences_validation_rejects_invalid_folders() {
        let invalid_prefs = Preferences {
            theme: "system".into(),
            history_retention_days: 30,
            appimage_folders: vec!["/path/with/\0null".into()],
        };
        let result = save_preferences(&invalid_prefs);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().code, "INVALID_PREFERENCES");
    }

    #[test]
    fn preferences_validation_rejects_invalid_theme_and_retention() {
        let invalid_theme = Preferences {
            theme: "neon-blue".into(),
            history_retention_days: 30,
            appimage_folders: vec![],
        };
        let result = save_preferences(&invalid_theme);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().code, "INVALID_PREFERENCES");

        let excessive_retention = Preferences {
            theme: "dark".into(),
            history_retention_days: 99999,
            appimage_folders: vec![],
        };
        let result2 = save_preferences(&excessive_retention);
        assert!(result2.is_err());
        assert_eq!(result2.unwrap_err().code, "INVALID_PREFERENCES");
    }
}
