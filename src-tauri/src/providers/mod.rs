pub mod appimage;
mod command;
pub mod deb;
pub mod flatpak;
pub mod local;
pub mod snap;

use crate::domain::ApplicationInstallation;

#[derive(Default)]
pub struct ProviderInventory {
    pub installations: Vec<ApplicationInstallation>,
    pub warnings: Vec<String>,
    pub locations: Vec<String>,
}
