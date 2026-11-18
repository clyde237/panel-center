# Livraison (paquet `.deb`)

La commande `npm run desktop:build` génère le paquet Debian dans `src-tauri/target/release/bundle/deb/`.

## Contenu et dépendances

- Binaire desktop natif `/usr/bin/panel-center` (Tauri v2 + React + Rust).
- Entrée de menu freedesktop (`/usr/share/applications/Panel Center.desktop`) et jeu d’icônes hicolor générés depuis `logo.png`.
- Dépendances déclarées dans `src-tauri/tauri.conf.json` :
  - `libwebkit2gtk-4.1-0`, `libgtk-3-0` (interface WebView GTK)
  - `libsqlite3-0` (stockage local SQLite versionné)
  - `dpkg`, `apt`, `pkexec | policykit-1` (inventaire, simulation APT et autorisation Polkit)

## Vérification avant diffusion

1. Contrôler les métadonnées et fichiers du paquet avec `dpkg-deb --info` et `dpkg-deb --contents`.
2. Tester l’installation (`sudo apt install ./panel-center_*.deb`), le lancement, l’utilisation et la désinstallation (`sudo apt remove panel-center`) sur une machine virtuelle Ubuntu propre correspondant à la cible.
