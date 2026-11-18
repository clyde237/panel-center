# Développement du socle

## Environnement observé lors de l’initialisation

Le 18 novembre 2026 : Ubuntu 26.04.1 LTS, x86_64, Node 24.20.0, npm 11.19.0, Rust/Cargo 1.99.1, WebKitGTK 2.52.6 et GTK 3.24.52.

Cette observation décrit le poste de construction ; elle ne valide pas une compatibilité avec toutes les éditions Ubuntu.

## Frontière React/Rust

Le client de `src/lib/desktop.ts` est l’unique point d’accès IPC utilisé par l’interface. Les champs sont sérialisés en camelCase. Les dates d’inventaire sont des secondes Unix.

La clé DEB inclut le format, la portée et l’identifiant dpkg, avec l’architecture lorsqu’elle est présente. L’inventaire expose aussi les paquets techniques pour permettre une consultation avancée, tout en filtrant la vue initiale sur les lanceurs graphiques identifiables.

Les états des connecteurs ne doivent pas laisser croire qu’un format est pris en charge lorsqu’il est seulement prévu. L’absence de lanceur ne signifie pas qu’un paquet n’est pas installé ou qu’il est inutile.

Les nouvelles commandes doivent être enregistrées dans le handler Rust et dans le manifeste de permissions, puis autorisées explicitement dans la capacité de la fenêtre concernée. Aucun accès shell générique n’est accordé au frontend.

Les types TypeScript et Rust doivent évoluer ensemble. Les tests Rust couvrent le parsing des états dpkg et la sélection des métadonnées de bureau ; compléter par des tests de contrat et d’intégration à mesure que les fonctionnalités évoluent.

## Inventaire unifié

`list_installations` accepte une liste optionnelle `appimagePaths` (au maximum 16 chemins de 4 096 octets). L’ancienne invocation sans argument reste valide. Les champs `installation`, `reference`, `path`, `isExecutable` et `reportedSize` sont explicitement nullables.

L’inventaire renvoie `appimageLocations` et un état par connecteur : prêt, partiel, indisponible ou en erreur. Les lectures sont exécutées indépendamment sur des fils de travail ; les résultats disponibles sont réunis puis triés. Les réponses devenues obsolètes sont ignorées côté React.

- Snap : requête GET à `/v2/snaps` sur `/run/snapd.socket`, sans secret ni privilège administrateur. Les types base, core, kernel, gadget et snapd sont classés comme techniques. La taille est celle déclarée par snapd, hors données utilisateur.
- Flatpak : colonnes explicitement choisies, locale C et absence d’ellipses ; identifiant composé de la portée, de l’installation et de la référence `app/identifiant/architecture/branche`. `system` est normalisé vers l’installation `default`.
- AppImage : signature ELF et octets `AI` de type 1 ou 2. Aucun fichier découvert ni champ Exec n’est exécuté. Les chemins explicites sont prioritaires, les cycles de liens de dossiers sont évités et une limite de parcours produit un état partiel.

Les tailles formatées Flatpak sont conservées dans `reportedSize`. Elles ne sont pas converties artificiellement en octets. Les emplacements AppImage supplémentaires résident uniquement dans l’état de la session ; aucune dépendance SQLite n’a été ajoutée.

Les tests TypeScript utilisent le runner Node avec isolation désactivée pour fonctionner dans l’environnement restreint de développement. Les fixtures Snap servent aux tests du contrat et ne remplacent pas une connexion réelle à snapd.

## Architecture future

La consultation de l’inventaire n’appelle aucun service administrateur. Ajouter le service D-Bus/Polkit comme un composant séparé au moment de développer les suppressions. Valider les aperçus, identifiants, droits, refus et verrous dans une machine virtuelle avec des logiciels de test.

Ne pas ajouter de politique Polkit permissive simplement pour rendre un bouton fonctionnel. Ne pas tester de suppression sur les logiciels personnels du poste de développement.

L’historique et les préférences seront placés dans SQLite avec migrations versionnées. Tant que cette persistance n’est pas implémentée, ne pas annoncer une sauvegarde de préférences ou d’opérations.

## Direction visuelle

Thème clair, surfaces gris/vert sobres, texte système Ubuntu/Noto Sans, accent vert foncé. Les états de format sont textuels ; les icônes Lucide servent les actions et ne remplacent pas les noms accessibles.

Le composant de détail utilise le dialogue natif : fermeture par Échap, maintien du focus dans le dialogue et retour au contrôle déclencheur. Respecter la réduction des animations et tester le zoom ainsi que les dimensions minimales de fenêtre.

Le scanner visuel peut signaler l’absence de mentions légales, de politique publique ou d’image Open Graph. Ces éléments de site public ne sont pas des exigences de cette application desktop locale ; ne pas créer de pages factices pour faire disparaître ces signaux.

## Dépendances et reproductibilité

Conserver `package-lock.json` et `src-tauri/Cargo.lock`. Utiliser `npm ci` et les options Cargo `--locked` après l’initialisation.

L’accès au registre npm était indisponible pendant l’initialisation. Les dépendances ont été résolues depuis les caches locaux disponibles. Aucun chemin de cache de cette session ne doit être ajouté à la configuration versionnée du projet.

## Sources techniques

Consultées le 18 novembre 2026 :

- [Tauri v2 : création du projet](https://v2.tauri.app/start/create-project/)
- [Tauri v2 : configuration Vite](https://v2.tauri.app/start/frontend/vite/)
- [Tauri v2 : commandes Rust](https://v2.tauri.app/develop/calling-rust/)
- [Tauri v2 : capacités](https://v2.tauri.app/security/capabilities/)
- [shadcn/ui : installation Vite](https://ui.shadcn.com/docs/installation/vite)
- [Snap : utilisation de l’API locale](https://snapcraft.io/docs/how-to-guides/snap-development/use-the-rest-api/)
- [Snap : référence API](https://staging-api.snapcraft.io/docs/snapd-api)
- [Flatpak : liste et références des installations](https://docs.flatpak.org/en/latest/flatpak-command-reference.html)
- [AppImage : intégration aux raccourcis](https://docs.appimage.org/reference/desktop-integration.html)
- [AppImage : spécification du format](https://github.com/AppImage/AppImageSpec/blob/master/draft.md)
