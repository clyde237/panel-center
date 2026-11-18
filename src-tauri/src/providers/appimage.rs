use super::ProviderInventory;
use crate::domain::{AppError, ApplicationInstallation, PackageFormat};
use std::{
    collections::{BTreeMap, HashSet},
    fs::{self, File},
    io::Read,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

const MAX_ENTRIES: usize = 20_000;
const MAX_DEPTH: usize = 4;

pub fn inventory(extra: &[String]) -> Result<ProviderInventory, AppError> {
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .ok_or_else(|| {
            AppError::new(
                "APPIMAGE_HOME",
                "Impossible de déterminer le dossier personnel.",
            )
        })?;
    let config = absolute_env("XDG_CONFIG_HOME").unwrap_or_else(|| home.join(".config"));
    let mut roots = vec![home.join("Applications"), home.join(".local/bin")];
    let download = fs::read_to_string(config.join("user-dirs.dirs"))
        .ok()
        .and_then(|content| download_path(&content, &home))
        .unwrap_or_else(|| home.join("Downloads"));
    roots.push(download);
    let mut scanner = Scanner::new();
    for value in extra {
        match expand_path(value, &home) {
            Ok(path) => scanner.scan_root(&path),
            Err(message) => scanner.warn(message),
        }
    }
    for root in roots.into_iter().filter(|path| path.exists()) {
        scanner.scan_root(&root);
    }
    let data = absolute_env("XDG_DATA_HOME").unwrap_or_else(|| home.join(".local/share"));
    let mut desktop_dirs = vec![data.join("applications")];
    let dirs =
        std::env::var("XDG_DATA_DIRS").unwrap_or_else(|_| "/usr/local/share:/usr/share".into());
    desktop_dirs.extend(
        dirs.split(':')
            .filter(|dir| Path::new(dir).is_absolute())
            .map(|dir| Path::new(dir).join("applications")),
    );
    for directory in desktop_dirs {
        scanner.scan_launchers(&directory);
    }
    Ok(scanner.finish())
}

fn absolute_env(key: &str) -> Option<PathBuf> {
    std::env::var_os(key)
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
}

fn download_path(config: &str, home: &Path) -> Option<PathBuf> {
    let value = config
        .lines()
        .find_map(|line| line.trim().strip_prefix("XDG_DOWNLOAD_DIR="))?;
    let value = value.strip_prefix('"')?.strip_suffix('"')?;
    // Lecture du format XDG, sans interpréteur shell.
    let value = value.replace("\\\"", "\"").replace("\\\\", "\\");
    if let Some(suffix) = value.strip_prefix("$HOME/") {
        Some(home.join(suffix))
    } else if value == "$HOME" {
        Some(home.into())
    } else {
        let path = PathBuf::from(value);
        path.is_absolute().then_some(path)
    }
}

fn expand_path(value: &str, home: &Path) -> Result<PathBuf, String> {
    let value = value.trim();
    if value.is_empty() || value.len() > 4096 || value.contains('\0') {
        return Err("Un emplacement AppImage est vide ou invalide.".into());
    }
    let path = if let Some(suffix) = value.strip_prefix("~/") {
        home.join(suffix)
    } else {
        PathBuf::from(value)
    };
    if !path.is_absolute() {
        return Err("Utilise un chemin absolu ou commençant par ~/ pour les AppImage.".into());
    }
    Ok(path)
}

struct Scanner {
    items: BTreeMap<PathBuf, ApplicationInstallation>,
    visited: HashSet<PathBuf>,
    locations: Vec<String>,
    warnings: Vec<String>,
    entries: usize,
    start: Instant,
    exhausted: bool,
}

impl Scanner {
    fn new() -> Self {
        Self {
            items: BTreeMap::new(),
            visited: HashSet::new(),
            locations: vec![],
            warnings: vec![],
            entries: 0,
            start: Instant::now(),
            exhausted: false,
        }
    }
    fn warn(&mut self, message: String) {
        if self.warnings.len() < 6 && !self.warnings.contains(&message) {
            self.warnings.push(message);
        }
    }
    fn within_budget(&mut self) -> bool {
        if self.exhausted {
            return false;
        }
        if self.entries >= MAX_ENTRIES || self.start.elapsed() >= Duration::from_secs(4) {
            self.exhausted = true;
            self.warn("L’analyse AppImage a atteint sa limite. Ajoute un sous-dossier précis pour compléter les résultats.".into());
            return false;
        }
        self.entries += 1;
        true
    }
    fn scan_root(&mut self, input: &Path) {
        let Ok(path) = input.canonicalize() else {
            self.warn(format!(
                "Emplacement AppImage absent ou inaccessible : {}",
                input.display()
            ));
            return;
        };
        if path == Path::new("/")
            || ["/proc", "/sys", "/dev"]
                .iter()
                .any(|root| path.starts_with(root))
        {
            self.warn("Choisis un dossier d’applications précis pour l’analyse AppImage.".into());
            return;
        }
        if !self
            .locations
            .contains(&path.to_string_lossy().into_owned())
        {
            self.locations.push(path.to_string_lossy().into_owned());
        }
        if path.is_dir() {
            self.walk(&path, 0);
        } else if path.is_file() {
            if !self.add_file(&path, None) {
                self.warn(format!(
                    "Ce fichier ne porte pas une signature AppImage reconnue : {}",
                    path.display()
                ));
            }
        } else {
            self.warn(format!(
                "Cet emplacement n’est pas un fichier ou dossier ordinaire : {}",
                path.display()
            ));
        }
    }
    fn walk(&mut self, path: &Path, depth: usize) {
        if !self.within_budget() || !self.visited.insert(path.to_path_buf()) {
            return;
        }
        let Ok(entries) = fs::read_dir(path) else {
            self.warn(format!("Dossier AppImage illisible : {}", path.display()));
            return;
        };
        for entry in entries {
            if !self.within_budget() {
                break;
            }
            let Ok(entry) = entry else {
                self.warn("Certains fichiers AppImage n’ont pas pu être examinés.".into());
                continue;
            };
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            if kind.is_dir() {
                if ["node_modules", ".git", "target", ".cache"]
                    .contains(&entry.file_name().to_string_lossy().as_ref())
                {
                    continue;
                }
                if depth < MAX_DEPTH {
                    self.walk(&entry.path(), depth + 1);
                } else {
                    self.warn("Certains sous-dossiers dépassent la profondeur d’analyse AppImage (4 niveaux).".into());
                }
            } else if kind.is_file() || kind.is_symlink() {
                self.add_file(&entry.path(), None);
            }
        }
    }
    fn add_file(&mut self, input: &Path, metadata: Option<(&str, &str)>) -> bool {
        let Ok(path) = input.canonicalize() else {
            return false;
        };
        let Ok(info) = fs::metadata(&path) else {
            return false;
        };
        if !info.is_file() {
            return false;
        }
        if let Some(item) = self.items.get_mut(&path) {
            if let Some((name, version)) = metadata {
                item.name = name.into();
                item.version = version.into();
            }
            return true;
        }
        let mut header = [0; 11];
        let Ok(mut file) = File::open(&path) else {
            return false;
        };
        if file.read_exact(&mut header).is_err() || !is_appimage(&header) {
            return false;
        }
        let Some(full_path) = path.to_str() else {
            self.warn("Un chemin AppImage non UTF-8 a été ignoré.".into());
            return false;
        };
        let fallback = path
            .file_stem()
            .and_then(|name| name.to_str())
            .unwrap_or("AppImage");
        let (name, version) = metadata.unwrap_or((fallback, ""));
        let mut item = ApplicationInstallation::read_only(
            PackageFormat::Appimage,
            format!("appimage:{full_path}"),
            full_path.into(),
            name.into(),
        );
        item.version = version.into();
        item.scope = "user".into();
        item.path = Some(full_path.into());
        item.installed_size = Some(info.len());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            item.is_executable = Some(info.permissions().mode() & 0o111 != 0);
        }
        item.description =
            "Fichier AppImage présent sur le disque ; détecté sans exécution.".into();
        item.can_uninstall = true;
        item.unavailable_reason = String::new();
        self.items.insert(path, item);
        true
    }
    fn scan_launchers(&mut self, directory: &Path) {
        if !directory.exists() {
            return;
        }
        let Ok(entries) = fs::read_dir(directory) else {
            self.warn(format!(
                "Raccourcis AppImage inaccessibles : {}",
                directory.display()
            ));
            return;
        };
        for entry in entries.flatten() {
            if !self.within_budget() {
                break;
            }
            if entry.path().extension().is_none_or(|ext| ext != "desktop") {
                continue;
            }
            let Ok(info) = entry.metadata() else {
                continue;
            };
            if !info.is_file() || info.len() > 128 * 1024 {
                continue;
            }
            let Ok(content) = fs::read_to_string(entry.path()) else {
                continue;
            };
            let Some((path, name, version)) = launcher_target(&content) else {
                continue;
            };
            self.add_file(&path, Some((&name, &version)));
        }
    }
    fn finish(self) -> ProviderInventory {
        ProviderInventory {
            installations: self.items.into_values().collect(),
            warnings: self.warnings,
            locations: self.locations,
        }
    }
}

fn is_appimage(header: &[u8]) -> bool {
    header.len() >= 11
        && &header[..4] == b"\x7fELF"
        && &header[8..10] == b"AI"
        && matches!(header[10], 1 | 2)
}

fn launcher_target(content: &str) -> Option<(PathBuf, String, String)> {
    let mut in_entry = false;
    let mut fields = BTreeMap::new();
    for line in content.lines().map(str::trim) {
        let line = line.trim_start_matches('\u{feff}');
        if line.starts_with('[') {
            in_entry = line == "[Desktop Entry]";
        } else if in_entry {
            if let Some((key, value)) = line.split_once('=') {
                fields.insert(key, value);
            }
        }
    }
    if fields.get("Type") != Some(&"Application") || fields.get("Hidden") == Some(&"true") {
        return None;
    }
    let tokens = exec_tokens(fields.get("Exec")?)?;
    let mut tokens = tokens.iter();
    let mut program = tokens.next()?;
    if program == "env" || program.ends_with("/env") {
        let mut target = None;
        let mut skip_next = false;
        for token in tokens {
            if skip_next {
                skip_next = false;
                continue;
            }
            if token == "-u"
                || token == "--unset"
                || token == "-C"
                || token == "--chdir"
                || token == "-S"
                || token == "--split-string"
            {
                skip_next = true;
                continue;
            }
            if token.starts_with('-') || token.contains('=') {
                continue;
            }
            target = Some(token);
            break;
        }
        program = target?;
    }
    let path = if let Some(suffix) = program.strip_prefix("~/") {
        if let Some(home) = std::env::var_os("HOME").map(PathBuf::from) {
            home.join(suffix)
        } else {
            PathBuf::from(program)
        }
    } else {
        PathBuf::from(program)
    };
    if !path.is_absolute() {
        return None;
    }
    let name = fields.get("Name[fr]").or_else(|| fields.get("Name"))?;
    if name.is_empty() {
        return None;
    }
    Some((
        path,
        name.to_string(),
        fields.get("X-AppImage-Version").unwrap_or(&"").to_string(),
    ))
}

pub(crate) fn exec_tokens(value: &str) -> Option<Vec<String>> {
    let mut tokens = vec![];
    let mut token = String::new();
    let (mut quoted, mut escaped) = (false, false);
    for c in value.chars() {
        if escaped {
            token.push(c);
            escaped = false;
        } else if c == '\\' {
            escaped = true;
        } else if c == '"' {
            quoted = !quoted;
        } else if c.is_whitespace() && !quoted {
            if !token.is_empty() {
                tokens.push(std::mem::take(&mut token));
            }
        } else {
            token.push(c);
        }
    }
    if quoted || escaped {
        return None;
    }
    if !token.is_empty() {
        tokens.push(token);
    }
    Some(tokens)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "panel-center-scan-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&path).unwrap();
            Self(path)
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    #[test]
    fn detects_renamed_files_rejects_fake_extensions_and_deduplicates_links() {
        let temp = Fixture::new();
        let mut bytes = [0u8; 16];
        bytes[..4].copy_from_slice(b"\x7fELF");
        bytes[8..11].copy_from_slice(b"AI\x02");
        let real = temp.0.join("renamed-file");
        fs::write(&real, bytes).unwrap();
        fs::write(temp.0.join("fake.AppImage"), b"not an AppImage").unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(&real, temp.0.join("alias.AppImage")).unwrap();
        let mut scan = Scanner::new();
        scan.scan_root(&temp.0);
        scan.scan_root(&real);
        let found = scan.finish();
        assert_eq!(found.installations.len(), 1);
        assert_eq!(found.installations[0].path.as_deref(), real.to_str());
        assert_eq!(found.installations[0].installed_size, Some(16));
        assert!(found.installations[0].is_executable == Some(false));
    }
    #[test]
    fn does_not_follow_directory_symlink_cycles() {
        let temp = Fixture::new();
        #[cfg(unix)]
        std::os::unix::fs::symlink(&temp.0, temp.0.join("loop")).unwrap();
        let mut scan = Scanner::new();
        scan.scan_root(&temp.0);
        assert!(scan.entries < 5);
    }
    #[test]
    fn parses_xdg_without_evaluating_shell_and_handles_missing_sources() {
        assert_eq!(
            download_path(
                "XDG_DOWNLOAD_DIR=\"$HOME/Téléchargements\"",
                Path::new("/home/test")
            ),
            Some(PathBuf::from("/home/test/Téléchargements"))
        );
        assert!(download_path(
            "XDG_DOWNLOAD_DIR=\"$(touch /tmp/no)\"",
            Path::new("/home/test")
        )
        .is_none());
        assert!(expand_path("relative/file", Path::new("/home/test")).is_err());
        let temp = Fixture::new();
        let mut scan = Scanner::new();
        scan.scan_root(&temp.0.join("missing"));
        assert_eq!(scan.finish().warnings.len(), 1);
    }
    #[test]
    fn reads_quoted_launcher_paths_without_executing_them() {
        let text = "[Desktop Entry]\nType=Application\nName=Editor\nName[fr]=Éditeur\nExec=\"/home/test/My Editor\" --no-sandbox %U\nX-AppImage-Version=1.2\n[Desktop Action Test]\nName=Other";
        let (path, name, version) = launcher_target(text).unwrap();
        assert_eq!(path, PathBuf::from("/home/test/My Editor"));
        assert_eq!(name, "Éditeur");
        assert_eq!(version, "1.2");
        assert!(launcher_target(
            "[Desktop Entry]\nType=Application\nName=Bad\nExec=sh -c anything"
        )
        .is_none());
    }

    #[test]
    fn launcher_target_supports_tilde_expansion_and_bom() {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
        let text = "\u{feff}[Desktop Entry]\nType=Application\nName=PortableApp\nExec=env -u GDK_BACKEND ~/Applications/PortableApp.AppImage --run\nX-AppImage-Version=3.0\n";
        let (path, name, version) = launcher_target(text).unwrap();
        assert_eq!(
            path,
            PathBuf::from(home).join("Applications/PortableApp.AppImage")
        );
        assert_eq!(name, "PortableApp");
        assert_eq!(version, "3.0");
    }
}
