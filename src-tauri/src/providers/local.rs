use super::ProviderInventory;
use crate::domain::{AppError, ApplicationInstallation, PackageFormat};
use std::collections::{HashMap, HashSet};
use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

const MAX_DIR_SIZE_ENTRIES: usize = 10_000;
const MAX_DIR_SIZE_DEPTH: usize = 8;

/// Directories directly under `$HOME/.local` or `$HOME/.local/share` that must NEVER
/// be treated as an individual application's dedicated install root.
const RESERVED_LOCAL_SUBDIRS: &[&str] = &[
    "bin", "share", "lib", "lib64", "libexec", "include", "state", "cache", "config", "etc", "var",
    "tmp",
];

const RESERVED_SHARE_SUBDIRS: &[&str] = &[
    "applications",
    "icons",
    "fonts",
    "mime",
    "flatpak",
    "keyrings",
    "dbus-1",
    "Trash",
    "panel-center",
    "themes",
    "pixmaps",
    "man",
    "doc",
    "locale",
    "glib-2.0",
    "gvfs-metadata",
    "nautilus",
    "gnome-shell",
];

const SYSTEM_BIN_PREFIXES: &[&str] = &[
    "/usr/bin",
    "/bin",
    "/sbin",
    "/usr/sbin",
    "/usr/lib",
    "/usr/libexec",
    "/lib",
    "/lib64",
    "/snap",
];

pub fn inventory() -> Result<ProviderInventory, AppError> {
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .ok_or_else(|| {
            AppError::new(
                "LOCAL_HOME",
                "Impossible de déterminer le dossier personnel.",
            )
        })?;

    let data_home = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .unwrap_or_else(|| home.join(".local/share"));

    let scan_dirs = vec![
        (data_home.join("applications"), true),
        (PathBuf::from("/usr/local/share/applications"), false),
    ];

    Ok(scan_local_applications(&scan_dirs, &home))
}

fn scan_local_applications(scan_dirs: &[(PathBuf, bool)], home: &Path) -> ProviderInventory {
    let mut result = ProviderInventory::default();
    let mut seen_execs = HashSet::new();

    for (dir, _is_user_dir) in scan_dirs {
        let Ok(entries) = fs::read_dir(dir) else {
            continue;
        };
        let mut desktop_paths: Vec<PathBuf> = entries
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|ext| ext == "desktop"))
            .collect();
        desktop_paths.sort();

        for desktop_path in desktop_paths {
            let Ok(content) = fs::read_to_string(&desktop_path) else {
                continue;
            };
            let Some(entry) = parse_desktop_entry(&content, home) else {
                continue;
            };

            // Skip AppImages (handled by AppImage provider)
            if is_appimage_path(&entry.exec_path) {
                continue;
            }

            // Skip system binaries, Snap, and Flatpak launchers
            if is_system_or_managed_exec(&entry.exec_path) {
                continue;
            }

            // Skip broken/dangling desktop entries whose executable does not exist
            if !entry.exec_path.exists() {
                continue;
            }

            // Skip anything owned by dpkg (already managed by DEB connector)
            if is_owned_by_dpkg(&entry.exec_path) || is_owned_by_dpkg(&desktop_path) {
                continue;
            }

            let canonical_exec = entry
                .exec_path
                .canonicalize()
                .unwrap_or_else(|_| entry.exec_path.clone());
            if !seen_execs.insert(canonical_exec.clone()) {
                continue;
            }

            let is_user_owned = canonical_exec.starts_with(home);
            let install_root = infer_app_install_root(&canonical_exec, home);
            let size = install_root
                .as_deref()
                .map(bounded_dir_size)
                .or_else(|| fs::metadata(&canonical_exec).ok().map(|m| m.len()));

            let scope = if is_user_owned { "user" } else { "system" };
            let native_id = desktop_path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or(&entry.name)
                .to_string();

            let mut item = ApplicationInstallation::read_only(
                PackageFormat::Local,
                format!("local:{scope}:{}", desktop_path.display()),
                native_id,
                entry.name,
            );
            item.scope = scope.into();
            item.version = entry.version;
            item.description = entry.comment;
            item.installed_size = size;
            item.path = Some(entry.exec_path.to_string_lossy().into());
            item.reference = Some(desktop_path.to_string_lossy().into());
            item.installation = install_root.map(|p| p.to_string_lossy().into());
            item.is_executable = Some(true);
            item.is_desktop_application = true;
            item.can_uninstall = is_user_owned;
            item.unavailable_reason = if is_user_owned {
                String::new()
            } else {
                "Installation manuelle système (/opt ou /usr/local) : retirez-la via son propre désinstalleur ou manuellement par sécurité.".into()
            };

            result.installations.push(item);
        }
    }

    result
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ParsedLocalDesktop {
    pub name: String,
    pub comment: String,
    pub version: String,
    pub exec_path: PathBuf,
}

pub(crate) fn parse_desktop_entry(content: &str, home: &Path) -> Option<ParsedLocalDesktop> {
    let mut in_entry = false;
    let mut fields = HashMap::new();
    for line in content.lines() {
        let trimmed = line.trim().trim_start_matches('\u{feff}');
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        if trimmed.starts_with('[') {
            in_entry = trimmed == "[Desktop Entry]";
        } else if in_entry {
            if let Some((key, value)) = trimmed.split_once('=') {
                fields.insert(key.trim(), value.trim());
            }
        }
    }

    if fields.get("Type") != Some(&"Application")
        || fields.get("Hidden") == Some(&"true")
        || fields.get("NoDisplay") == Some(&"true")
    {
        return None;
    }

    let name = fields
        .get("Name[fr]")
        .or_else(|| fields.get("Name"))
        .filter(|n| !n.is_empty())?
        .to_string();

    let comment = fields
        .get("Comment[fr]")
        .or_else(|| fields.get("Comment"))
        .copied()
        .unwrap_or_default()
        .to_string();

    let exec_raw = fields.get("Exec").copied()?;
    let exec_path = extract_exec_path(exec_raw, home)?;

    // Desktop Entry `Version` is normally the freedesktop spec version (1.0, 1.1, 1.4, 1.5),
    // not the app version, so prefer explicit `X-App-Version` if provided.
    let version = fields
        .get("X-App-Version")
        .or_else(|| fields.get("X-AppImage-Version"))
        .copied()
        .unwrap_or_default()
        .to_string();

    Some(ParsedLocalDesktop {
        name,
        comment,
        version,
        exec_path,
    })
}

/// Extracts the absolute executable path from a Desktop Entry `Exec=` line,
/// handling quotes, `~/`, and `env`/`/usr/bin/env` wrappers.
pub(crate) fn extract_exec_path(exec: &str, home: &Path) -> Option<PathBuf> {
    let mut rest = exec.trim();
    let mut tokens = Vec::new();
    while !rest.is_empty() {
        rest = rest.trim_start();
        if rest.is_empty() {
            break;
        }
        let (tok, next) = if let Some(inner) = rest.strip_prefix('"') {
            let mut val = String::new();
            let mut chars = inner.char_indices();
            let mut end_idx = None;
            while let Some((idx, ch)) = chars.next() {
                if ch == '\\' {
                    if let Some((_, esc)) = chars.next() {
                        val.push(esc);
                    }
                } else if ch == '"' {
                    end_idx = Some(idx + 1);
                    break;
                } else {
                    val.push(ch);
                }
            }
            let remaining = end_idx.map(|i| &inner[i..]).unwrap_or("");
            (val, remaining)
        } else if let Some(inner) = rest.strip_prefix('\'') {
            if let Some((val, remaining)) = inner.split_once('\'') {
                (val.to_string(), remaining)
            } else {
                (inner.to_string(), "")
            }
        } else {
            let mut end = rest.len();
            for (idx, ch) in rest.char_indices() {
                if ch.is_whitespace() {
                    end = idx;
                    break;
                }
            }
            (rest[..end].to_string(), &rest[end..])
        };
        if !tok.is_empty() {
            tokens.push(tok);
        }
        rest = next;
    }

    let mut idx = 0;
    while idx < tokens.len() {
        let tok = &tokens[idx];
        if matches!(tok.as_str(), "env" | "/usr/bin/env" | "/bin/env") {
            idx += 1;
            continue;
        }
        if tok == "-u" && idx + 1 < tokens.len() {
            idx += 2;
            continue;
        }
        if tok.starts_with('-') || tok.contains('=') {
            idx += 1;
            continue;
        }
        let expanded = if let Some(suffix) = tok.strip_prefix("~/") {
            home.join(suffix)
        } else {
            PathBuf::from(tok)
        };
        return expanded.is_absolute().then_some(expanded);
    }
    None
}

fn is_system_or_managed_exec(path: &Path) -> bool {
    for prefix in SYSTEM_BIN_PREFIXES {
        if path.starts_with(Path::new(prefix)) {
            return true;
        }
    }
    false
}

fn is_appimage_path(path: &Path) -> bool {
    if path
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("appimage"))
    {
        return true;
    }
    let Ok(mut file) = File::open(path) else {
        return false;
    };
    let mut header = [0u8; 12];
    if file.read_exact(&mut header).is_err() {
        return false;
    }
    header.starts_with(b"\x7fELF") && (&header[8..11] == b"AI\x02" || &header[8..11] == b"AI\x01")
}

fn is_owned_by_dpkg(path: &Path) -> bool {
    if !Path::new("/usr/bin/dpkg-query").exists() {
        return false;
    }
    Command::new("/usr/bin/dpkg-query")
        .args(["--search", "--", &path.to_string_lossy()])
        .env("LC_ALL", "C")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

/// Infers a dedicated per-application installation directory for a local executable,
/// returning `None` if the executable lives directly in a shared folder like `~/.local/bin`.
pub(crate) fn infer_app_install_root(exec_path: &Path, home: &Path) -> Option<PathBuf> {
    // Pattern 1: $HOME/.local/<AppDir>/... (e.g. ~/.local/ABDownloadManager/bin/ABDownloadManager)
    let dot_local = home.join(".local");
    if let Ok(rel) = exec_path.strip_prefix(&dot_local) {
        let mut comps = rel.components();
        let first = comps.next()?.as_os_str().to_str()?;
        if comps.next().is_some() {
            if !RESERVED_LOCAL_SUBDIRS.contains(&first) && !first.starts_with('.') {
                return Some(dot_local.join(first));
            }
            // Pattern 2: $HOME/.local/share/<AppDir>/...
            if first == "share" {
                let mut share_comps = rel.components().skip(1);
                let app_dir = share_comps.next()?.as_os_str().to_str()?;
                if share_comps.next().is_some()
                    && !RESERVED_SHARE_SUBDIRS.contains(&app_dir)
                    && !app_dir.starts_with('.')
                {
                    return Some(dot_local.join("share").join(app_dir));
                }
            }
        }
        return None;
    }

    // Pattern 3: $HOME/Applications/<AppDir>/... | $HOME/Apps/<AppDir>/... | $HOME/opt/<AppDir>/...
    for base_name in ["Applications", "Apps", "opt"] {
        let base = home.join(base_name);
        if let Ok(rel) = exec_path.strip_prefix(&base) {
            let mut comps = rel.components();
            let first = comps.next()?.as_os_str().to_str()?;
            if comps.next().is_some() && !first.starts_with('.') {
                return Some(base.join(first));
            }
        }
    }

    // Pattern 4: /opt/<AppDir>/...
    let opt = Path::new("/opt");
    if let Ok(rel) = exec_path.strip_prefix(opt) {
        let mut comps = rel.components();
        let first = comps.next()?.as_os_str().to_str()?;
        if comps.next().is_some() && !first.starts_with('.') {
            return Some(opt.join(first));
        }
    }

    None
}

fn bounded_dir_size(root: &Path) -> u64 {
    let mut total: u64 = 0;
    let mut entries_seen: usize = 0;
    let mut stack = vec![(root.to_path_buf(), 0usize)];

    while let Some((dir, depth)) = stack.pop() {
        if depth > MAX_DIR_SIZE_DEPTH || entries_seen >= MAX_DIR_SIZE_ENTRIES {
            break;
        }
        let Ok(read_dir) = fs::read_dir(&dir) else {
            continue;
        };
        for entry in read_dir.flatten() {
            entries_seen += 1;
            if entries_seen >= MAX_DIR_SIZE_ENTRIES {
                break;
            }
            let Ok(meta) = entry.metadata() else {
                continue;
            };
            if meta.file_type().is_symlink() {
                continue;
            }
            if meta.is_file() {
                total = total.saturating_add(meta.len());
            } else if meta.is_dir() {
                stack.push((entry.path(), depth + 1));
            }
        }
    }
    total
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_ab_download_manager_desktop_entry_and_install_root() {
        let home = Path::new("/home/tester");
        let desktop = "\
[Desktop Entry]
Name=AB Download Manager
Comment=Manage and organize your download files better than before
Exec=\"/home/tester/.local/ABDownloadManager/bin/ABDownloadManager\"
Terminal=false
Type=Application
";
        let parsed = parse_desktop_entry(desktop, home).expect("should parse");
        assert_eq!(parsed.name, "AB Download Manager");
        assert_eq!(
            parsed.exec_path,
            PathBuf::from("/home/tester/.local/ABDownloadManager/bin/ABDownloadManager")
        );
        assert_eq!(
            infer_app_install_root(&parsed.exec_path, home),
            Some(PathBuf::from("/home/tester/.local/ABDownloadManager"))
        );
    }

    #[test]
    fn never_infers_shared_directories_as_app_install_root() {
        let home = Path::new("/home/tester");
        assert_eq!(
            infer_app_install_root(Path::new("/home/tester/.local/bin/mytool"), home),
            None
        );
        assert_eq!(
            infer_app_install_root(
                Path::new("/home/tester/.local/share/applications/foo.desktop"),
                home
            ),
            None
        );
        assert_eq!(
            infer_app_install_root(Path::new("/home/tester/.local/share/MyApp/run"), home),
            Some(PathBuf::from("/home/tester/.local/share/MyApp"))
        );
        assert_eq!(
            infer_app_install_root(Path::new("/opt/Antigravity/antigravity"), home),
            Some(PathBuf::from("/opt/Antigravity"))
        );
    }

    #[test]
    fn scans_local_user_app_and_ignores_hidden_and_system_entries() {
        let temp_home =
            std::env::temp_dir().join(format!("panel_center_local_test_{}", std::process::id()));
        let _ = fs::remove_dir_all(&temp_home);
        let apps_dir = temp_home.join(".local/share/applications");
        let app_bin_dir = temp_home.join(".local/ABDownloadManager/bin");
        fs::create_dir_all(&apps_dir).unwrap();
        fs::create_dir_all(&app_bin_dir).unwrap();

        let bin_path = app_bin_dir.join("ABDownloadManager");
        fs::write(&bin_path, b"#!/bin/sh\necho ok\n").unwrap();

        let ab_desktop = format!(
            "[Desktop Entry]\nType=Application\nName=AB Download Manager\nComment=Downloader\nExec=\"{}\"\n",
            bin_path.display()
        );
        fs::write(apps_dir.join("com.abdownloadmanager.desktop"), ab_desktop).unwrap();

        // Hidden handler must be skipped
        fs::write(
            apps_dir.join("hidden.desktop"),
            format!(
                "[Desktop Entry]\nType=Application\nNoDisplay=true\nName=Hidden\nExec=\"{}\"\n",
                bin_path.display()
            ),
        )
        .unwrap();

        // System /usr/bin launcher must be skipped
        fs::write(
            apps_dir.join("sys.desktop"),
            "[Desktop Entry]\nType=Application\nName=SysApp\nExec=/usr/bin/env\n",
        )
        .unwrap();

        let result = scan_local_applications(&[(apps_dir, true)], &temp_home);
        assert_eq!(result.installations.len(), 1);
        let app = &result.installations[0];
        assert_eq!(app.format, PackageFormat::Local);
        assert_eq!(app.name, "AB Download Manager");
        assert_eq!(app.scope, "user");
        assert!(app.can_uninstall);
        assert_eq!(
            app.installation.as_deref(),
            Some(
                temp_home
                    .join(".local/ABDownloadManager")
                    .to_string_lossy()
                    .as_ref()
            )
        );

        let _ = fs::remove_dir_all(&temp_home);
    }
}
