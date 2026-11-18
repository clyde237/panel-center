use crate::domain::{AppError, HistoryEntry, Preferences};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

static WRITE_COUNTER: AtomicU64 = AtomicU64::new(1);
static STORAGE_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

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

fn atomic_write(target: &Path, content: &str) -> Result<(), std::io::Error> {
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent)?;
    }
    let counter = WRITE_COUNTER.fetch_add(1, Ordering::Relaxed);
    let temp_name = format!(
        "{}.tmp.{}.{}",
        target
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("file"),
        std::process::id(),
        counter
    );
    let temp_path = target.with_file_name(temp_name);
    fs::write(&temp_path, content)?;
    fs::rename(temp_path, target)?;
    Ok(())
}

pub fn load_preferences() -> Preferences {
    let _guard = STORAGE_LOCK.lock().unwrap();
    load_preferences_unlocked()
}

fn load_preferences_unlocked() -> Preferences {
    let path = config_dir().join("preferences.json");
    if let Ok(content) = fs::read_to_string(&path) {
        if let Ok(prefs) = serde_json::from_str::<Preferences>(&content) {
            return prefs;
        }
    }
    Preferences::default()
}

pub fn save_preferences(prefs: &Preferences) -> Result<(), AppError> {
    let _guard = STORAGE_LOCK.lock().unwrap();
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

    let path = config_dir().join("preferences.json");
    let content = serde_json::to_string_pretty(prefs).map_err(|e| {
        AppError::new(
            "PREFERENCES_SERIALIZE",
            &format!("Erreur lors de la sérialisation des préférences : {e}"),
        )
    })?;
    atomic_write(&path, &content).map_err(|e| {
        AppError::new(
            "PREFERENCES_WRITE",
            &format!("Impossible d’enregistrer les préférences : {e}"),
        )
    })
}

pub fn load_history() -> Vec<HistoryEntry> {
    let _guard = STORAGE_LOCK.lock().unwrap();
    load_history_unlocked()
}

fn load_history_unlocked() -> Vec<HistoryEntry> {
    let path = data_dir().join("history.json");
    if let Ok(content) = fs::read_to_string(&path) {
        if let Ok(mut entries) = serde_json::from_str::<Vec<HistoryEntry>>(&content) {
            entries.sort_by_key(|e| std::cmp::Reverse(e.timestamp));
            return entries;
        }
    }
    vec![]
}

pub fn append_history(entry: HistoryEntry) -> Result<(), AppError> {
    let _guard = STORAGE_LOCK.lock().unwrap();
    let mut entries = load_history_unlocked();
    let prefs = load_preferences_unlocked();

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    if prefs.history_retention_days > 0 {
        let max_age_secs = (prefs.history_retention_days as u64) * 86400;
        entries.retain(|e| now.saturating_sub(e.timestamp) <= max_age_secs);
    }

    entries.insert(0, entry);
    if entries.len() > 500 {
        entries.truncate(500);
    }

    let path = data_dir().join("history.json");
    let content = serde_json::to_string_pretty(&entries).map_err(|e| {
        AppError::new(
            "HISTORY_SERIALIZE",
            &format!("Erreur lors de la sérialisation de l’historique : {e}"),
        )
    })?;
    atomic_write(&path, &content).map_err(|e| {
        AppError::new(
            "HISTORY_WRITE",
            &format!("Impossible d’enregistrer l’historique : {e}"),
        )
    })
}

pub fn clear_history() -> Result<(), AppError> {
    let _guard = STORAGE_LOCK.lock().unwrap();
    let path = data_dir().join("history.json");
    if path.exists() {
        let _ = fs::remove_file(path);
    }
    Ok(())
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
        std::env::set_var("PANEL_CENTER_CONFIG_DIR", &temp_dir);

        let prefs = Preferences {
            theme: "dark".into(),
            history_retention_days: 90,
            appimage_folders: vec!["/opt/apps".into()],
        };

        save_preferences(&prefs).expect("save should succeed");
        let loaded = load_preferences();
        assert_eq!(loaded.theme, "dark");
        assert_eq!(loaded.history_retention_days, 90);
        assert_eq!(loaded.appimage_folders, vec!["/opt/apps"]);

        let _ = fs::remove_dir_all(&temp_dir);
        std::env::remove_var("PANEL_CENTER_CONFIG_DIR");
    }

    #[test]
    fn history_retention_and_clear() {
        let _lock = TEST_STORAGE_LOCK.lock().unwrap();
        let temp_dir =
            std::env::temp_dir().join(format!("panel_center_test_hist_{}", std::process::id()));
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

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

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
        // The old entry (> 1 day) should have been pruned!
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].id, "hist_2");

        clear_history().expect("clear");
        assert_eq!(load_history().len(), 0);

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
