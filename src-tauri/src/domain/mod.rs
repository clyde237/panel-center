pub mod protection;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum PackageFormat {
    Deb,
    Snap,
    Flatpak,
    Appimage,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplicationInstallation {
    pub id: String,
    pub name: String,
    pub native_id: String,
    pub version: String,
    pub format: PackageFormat,
    pub scope: String,
    pub description: String,
    pub installed_size: Option<u64>,
    pub reported_size: Option<String>,
    pub installation: Option<String>,
    pub reference: Option<String>,
    pub path: Option<String>,
    pub is_executable: Option<bool>,
    pub is_desktop_application: bool,
    pub can_uninstall: bool,
    pub unavailable_reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProviderState {
    Ready,
    Partial,
    Unavailable,
    Planned,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderStatus {
    pub format: PackageFormat,
    pub state: ProviderState,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Inventory {
    pub installations: Vec<ApplicationInstallation>,
    pub providers: Vec<ProviderStatus>,
    pub generated_at: u64,
    pub appimage_locations: Vec<String>,
}

impl ApplicationInstallation {
    pub fn read_only(format: PackageFormat, id: String, native_id: String, name: String) -> Self {
        Self {
            id,
            name,
            native_id,
            format,
            version: String::new(),
            scope: "system".into(),
            description: String::new(),
            installed_size: None,
            reported_size: None,
            installation: None,
            reference: None,
            path: None,
            is_executable: None,
            is_desktop_application: true,
            can_uninstall: false,
            unavailable_reason:
                "Inventaire en lecture seule. La désinstallation sera disponible dans une prochaine étape."
                    .into(),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UninstallPlanOptions {
    pub purge_config: bool,
    pub remove_data: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DebAffectedPackage {
    pub name: String,
    pub version: String,
    pub installed_size: Option<u64>,
    pub description: String,
    pub is_target: bool,
    pub is_protected: bool,
    pub action: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DebDependencyPreview {
    pub target_package: String,
    pub removed_packages: Vec<DebAffectedPackage>,
    pub dependent_packages: Vec<DebAffectedPackage>,
    pub newly_installed_packages: Vec<String>,
    pub autoremove_candidates: Vec<String>,
    pub blocking_dependents: Vec<String>,
    pub total_freed_bytes: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UninstallPlan {
    pub id: String,
    pub target_id: String,
    pub target_name: String,
    pub format: PackageFormat,
    pub scope: String,
    pub native_id: String,
    pub target_path: Option<String>,
    #[serde(default)]
    pub target_installation: Option<String>,
    pub options: UninstallPlanOptions,
    pub affected_items: Vec<String>,
    #[serde(default)]
    pub deb_preview: Option<DebDependencyPreview>,
    pub data_impact: String,
    pub estimated_freed_space: Option<String>,
    pub requires_auth: bool,
    pub is_blocked: bool,
    pub blocked_reason: Option<String>,
    pub created_at: u64,
    pub expires_at: u64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum OperationStatus {
    Pending,
    Running,
    Verifying,
    Completed,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Operation {
    pub id: String,
    pub plan_id: String,
    pub target_id: String,
    pub target_name: String,
    pub format: PackageFormat,
    pub status: OperationStatus,
    pub step: String,
    pub progress: Option<u8>,
    pub message: String,
    pub error: Option<AppError>,
    pub started_at: u64,
    pub finished_at: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryEntry {
    pub id: String,
    pub operation_id: String,
    pub target_name: String,
    pub native_id: String,
    pub format: PackageFormat,
    pub status: OperationStatus,
    pub summary: String,
    pub diagnostic: String,
    pub timestamp: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Preferences {
    pub theme: String,
    pub appimage_folders: Vec<String>,
    pub history_retention_days: u32,
}

impl Default for Preferences {
    fn default() -> Self {
        Self {
            theme: "system".into(),
            appimage_folders: vec![],
            history_retention_days: 30,
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SystemInfo {
    pub os: String,
    pub architecture: String,
    pub app_version: String,
    pub mode: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppError {
    pub code: String,
    pub message: String,
}

impl AppError {
    pub fn new(code: &str, message: &str) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
        }
    }
}
