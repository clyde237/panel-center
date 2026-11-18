# Panel Center

Application desktop Ubuntu construite avec **Tauri v2, React, TypeScript et Rust**.

Le périmètre produit est décrit dans [CADRAGE_PANEL_CENTER_TAURI_V2.md](./CADRAGE_PANEL_CENTER_TAURI_V2.md).

## État actuel : inventaire unifié

- React + Vite + TypeScript strict à la racine du projet.
- Tailwind CSS v4, Lucide React, configuration shadcn/ui et premières primitives natives adaptées aux tokens du projet.
- Interface française : recherche sans distinction d’accents, filtres par format et portée, paquets techniques et fiche détail.
- Commandes Tauri de lecture : `list_installations` et `get_system_info`.
- Inventaire DEB/APT réel via `dpkg-query`, association aux lanceurs et distinction des architectures.
- Inventaire Snap via l’API locale snapd, avec distinction des applications et composants de base.
- Inventaire Flatpak utilisateur, système et installations nommées ; référence complète conservée.
- Analyse des fichiers AppImage par signature ELF/AppImage, chemins canoniques et lecture des raccourcis, sans exécution.
- Ajout et retrait d’emplacements AppImage pour la session depuis les paramètres.
- Connecteurs indépendants : un gestionnaire indisponible ne masque pas les autres résultats.
- Contrats sérialisés Rust/TypeScript et erreurs lisibles côté interface.
- Capacité Tauri limitée aux deux commandes de lecture, contenu embarqué et CSP explicite.
- Icônes locales et cible de packaging Debian.

L’inventaire reste en lecture seule. Le service Polkit et les transactions de désinstallation constituent le prochain incrément. SQLite sera ajouté avec les fonctions qui nécessitent une persistance. L’interface n’utilise aucune donnée de démonstration comme une application réellement installée.

### Emplacements AppImage

Dans **Paramètres → Emplacements AppImage**, saisir le chemin absolu d’un fichier ou dossier, ou un chemin commençant par `~/`, puis cliquer sur **Analyser**. Le bouton **AppImage** de la liste ouvre cette vue. Les ajouts sont conservés pour la session seulement ; les retirer de la liste ne supprime aucun fichier.

L’analyse inclut par défaut `~/Applications`, `~/.local/bin` et le dossier Téléchargements défini par XDG lorsqu’ils existent. Les raccourcis utilisateur et système sont aussi examinés. Les liens de fichiers sont résolus et dédupliqués ; les liens vers des dossiers ne sont pas parcourus automatiquement.

La recherche est limitée à quatre niveaux de sous-dossiers, 20 000 entrées et un budget indicatif de quatre secondes. Les limites ou accès incomplets sont signalés. Ajouter un emplacement précis permet d’examiner une autre zone ; une détection exhaustive de toutes les AppImage du PC n’est pas garantie.

Flatpak fournit une taille arrondie via sa commande de liste : celle-ci est affichée telle quelle, sans conversion en un nombre d’octets prétendument exact. La portée des AppImage décrit leur présence dans cet inventaire utilisateur, pas un droit de suppression.

## Prérequis

- Ubuntu avec un environnement graphique.
- Node.js 22.12 ou supérieur ; Node 24 utilisé lors de l’initialisation.
- Rust 1.90 ou supérieur, Cargo et les outils système Tauri v2.
- Bibliothèques GTK 3 et WebKitGTK 4.1 de développement.
- `dpkg-query` pour l’inventaire DEB.
- Flatpak et snapd, uniquement pour inventorier leurs formats respectifs. Leur absence reste gérée dans l’interface.

Sur une installation Ubuntu, les prérequis usuels de Tauri sont :

```bash
sudo apt install build-essential curl wget file libssl-dev libwebkit2gtk-4.1-dev libgtk-3-dev librsvg2-dev libayatana-appindicator3-dev patchelf
```

La liste exacte est à vérifier pour la version Ubuntu cible dans la [documentation Tauri](https://v2.tauri.app/start/prerequisites/). Le projet n’installe pas automatiquement de paquets système.

## Lancer l’application desktop

Depuis la racine :

```bash
npm ci
npm run desktop:dev
```

Cette commande démarre Vite et l’application Tauri. La première compilation Rust peut prendre plusieurs minutes.

Ne pas lancer l’application avec `sudo`. L’inventaire utilise uniquement les droits ordinaires de l’utilisateur.

## Aperçu de l’interface dans le navigateur

```bash
npm run dev
```

Ouvrir `http://127.0.0.1:1420`. Un navigateur ordinaire n’a pas accès au moteur Rust : l’écran explique qu’il faut ouvrir la version desktop. Aucun inventaire fictif n’est utilisé en remplacement.

## Vérifier et construire

```bash
npm run build
npm test
npm run format:check
npm run rust:fmt
npm run rust:test
npm run rust:clippy
```

Ou exécuter `npm run check` pour ces contrôles dans l’ordre.

Les résultats obtenus et les limites de la session sont consignés dans [docs/VALIDATION.md](./docs/VALIDATION.md).

Pour produire le paquet Debian du socle en lecture seule :

```bash
npm run desktop:build
```

Les sorties Tauri se trouvent sous `src-tauri/target/release/bundle/deb/`. Tester le paquet sur une machine virtuelle de la version Ubuntu cible avant diffusion. Ce paquet initial ne contient aucun service privilégié.

Pour lire l’inventaire Rust sans ouvrir de fenêtre :

```bash
cargo run --manifest-path src-tauri/Cargo.toml --locked --example inventory
```

## Organisation

| Dossier                      | Responsabilité                                               |
| ---------------------------- | ------------------------------------------------------------ |
| `src/app`                    | Structure, navigation et vues initiales.                     |
| `src/features/applications`  | Chargement de l’inventaire et fiche détail.                  |
| `src/features/uninstall`     | Périmètre réservé aux futures transactions.                  |
| `src/components/ui`          | Primitives visuelles et contrôles communs.                   |
| `src/lib`                    | Contrats, client Tauri et utilitaires.                       |
| `src-tauri/src/commands`     | Commandes exposées au frontend.                              |
| `src-tauri/src/domain`       | Contrats du moteur Rust.                                     |
| `src-tauri/src/providers`    | Connecteurs DEB, Snap, Flatpak et AppImage en lecture seule. |
| `src-tauri/src/services`     | Orchestration de l’inventaire.                               |
| `src-tauri/src/storage`      | Emplacement prévu pour SQLite.                               |
| `crates/panel-center-helper` | Spécification du futur service privilégié.                   |
| `packaging`                  | Notes de livraison et intégration système à venir.           |

## Composants shadcn/ui

`components.json`, l’alias `@/` et les tokens Tailwind sont configurés. Les composants `Button` et `Input` utilisent des éléments natifs et sont adaptés au socle ; les dialogues utilisent actuellement `<dialog>`.

Le générateur shadcn n’est pas nécessaire pour exécuter le projet. Lorsque le registre est accessible, des composants supplémentaires peuvent être ajoutés, par exemple avec `npx shadcn@latest add dialog`. Vérifier les modifications générées avant de remplacer les primitives existantes.

## Prochain incrément

Définir les aperçus de suppression et éprouver le service Polkit dans une machine virtuelle. La validation visuelle native et l’essai réel de snapd restent à compléter sur un poste autorisé. Consulter [docs/DEVELOPPEMENT.md](./docs/DEVELOPPEMENT.md) pour les conventions de cette base.
