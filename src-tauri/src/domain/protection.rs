pub const PROTECTED_PACKAGES: &[&str] = &[
    "apt",
    "dpkg",
    "base-files",
    "base-passwd",
    "coreutils",
    "libc6",
    "libgcc-s1",
    "libstdc++6",
    "systemd",
    "systemd-sysv",
    "init",
    "login",
    "passwd",
    "sudo",
    "ubuntu-minimal",
    "ubuntu-standard",
    "ubuntu-desktop",
    "ubuntu-desktop-minimal",
    "gnome-shell",
    "gdm3",
    "xorg",
    "xserver-xorg",
    "grub-common",
    "grub2-common",
    "grub-pc",
    "grub-efi-amd64",
    "linux-base",
    "linux-image-generic",
    "polkitd",
    "policykit-1",
    "panel-center",
    "dbus",
];

pub const PROTECTED_SNAPS: &[&str] = &[
    "snapd",
    "bare",
    "core",
    "core18",
    "core20",
    "core22",
    "core24",
    "core26",
    "pc",
    "pc-kernel",
    "snapd-desktop-integration",
];

pub fn is_protected_deb(native_id: &str) -> bool {
    let normalized = native_id.split(':').next().unwrap_or(native_id).trim();
    PROTECTED_PACKAGES.contains(&normalized)
        || normalized.starts_with("linux-image-")
        || normalized.starts_with("linux-modules-")
}

pub fn is_protected_snap(name: &str) -> bool {
    let normalized = name.trim();
    let is_core_base = normalized
        .strip_prefix("core")
        .is_some_and(|rest| !rest.is_empty() && rest.chars().all(|c| c.is_ascii_digit()));
    PROTECTED_SNAPS.contains(&normalized) || is_core_base
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identifies_vital_ubuntu_packages_as_protected() {
        assert!(is_protected_deb("apt"));
        assert!(is_protected_deb("apt:amd64"));
        assert!(is_protected_deb("dpkg"));
        assert!(is_protected_deb("systemd"));
        assert!(is_protected_deb("ubuntu-desktop"));
        assert!(is_protected_deb("libc6"));
        assert!(is_protected_deb("panel-center"));
        assert!(is_protected_deb("grub-common"));
        assert!(is_protected_deb("grub2-common"));
        assert!(is_protected_deb("linux-image-generic"));
        assert!(is_protected_deb("linux-image-6.8.0-45-generic"));
        assert!(is_protected_deb("linux-modules-6.8.0-45-generic"));
        assert!(is_protected_deb("polkitd"));
        assert!(is_protected_deb("dbus"));
    }

    #[test]
    fn allows_regular_applications_to_be_uninstalled() {
        assert!(!is_protected_deb("vlc"));
        assert!(!is_protected_deb("gimp"));
        assert!(!is_protected_deb("firefox"));
    }

    #[test]
    fn identifies_protected_system_snaps() {
        assert!(is_protected_snap("snapd"));
        assert!(is_protected_snap("core24"));
        assert!(is_protected_snap("core28"));
        assert!(is_protected_snap("bare"));
        assert!(is_protected_snap("snapd-desktop-integration"));
        assert!(!is_protected_snap("firefox"));
        assert!(!is_protected_snap("vlc"));
        assert!(!is_protected_snap("coreutils-demo"));
    }
}
