# Validation du projet

Date : 18 novembre 2026.

## Étape 3 & 4 : désinstallations, transactions, persistance et préférences

- 37 tests Rust réussis :
  - Détection et blocage formel des paquets indispensables Ubuntu (`apt`, `dpkg`, `systemd`, `ubuntu-desktop`, `panel-center`, `grub-common`, `grub-pc`, `grub-efi-amd64`, `linux-base`, `linux-image-generic`, noyaux actifs `linux-image-*`, modules noyau `linux-modules-*`, `polkitd`, etc.) avec vérification défensive systématique dans `execute_deb_removal`.
  - Transaction unifiée : cycle préparation → aperçu → validation → consommation atomique du plan (protection anti-double-clic).
  - Unicité stricte des identifiants de plans et d'opérations via compteurs atomiques relaxés et horodatages.
  - Validation de sécurité des chemins utilisateurs stricte par composants (`Path::starts_with`), éliminant tout risque de confusion de préfixes (`/tmpx`, `/home/user_evil`), interdisant `/bin`, `/sbin`, `/usr`, `/etc`, `/lib`, `/lib64`, `/sys`, `/proc`, `/dev`, `/boot`, `/root`, `/var`, `/opt`, `/srv` et les liens symboliques pointant vers ces répertoires ou hors des dossiers utilisateur autorisés.
  - Rejet systématique des identifiants malveillants et injections de drapeaux CLI (`--all`, options avec tirets de tête, espaces, caractères de contrôle ou octets nuls) avec utilisation du séparateur d'arguments `--` pour Flatpak.
  - Désinstallation sécurisée AppImage : identification des lanceurs de bureau associés avant suppression du binaire pour préserver la résolution canonique des liens symboliques, suppression du fichier exécutable, blocage proactif dès l'aperçu si fichier introuvable/volume déconnecté, nettoyage ciblé sans faux positif des lanceurs `.desktop` associés (support `XDG_DATA_HOME`, `~/Desktop`, `~/Bureau` via `user-dirs.dirs`, expansion `~/`, support de `/bin/env` et drapeaux `-u VAR`, déduplication canonique évitant les doublons lors de liens entre dossiers bureau et rejet strict des sous-chaînes ou commentaires) et vérification post-opération par métadonnées de liens.
  - Désinstallation Flatpak utilisateur avec mode `--noninteractive`, ciblage par référence exacte complète (`app/org.../arch/branch`) pour éviter les conflits multibranches, et vérification post-opération (`flatpak info --user`).
  - Détection non invasive des verrous APT/dpkg via inspection `/proc/[pid]/comm` (`MANAGER_LOCKED`) couvrant `apt`, `apt-get`, `dpkg`, `dpkg-deb`, `unattended-upgr`, `aptitude`, `synaptic`, `packagekitd`, `apt-fast` et blocage proactif dès l'aperçu (`prepare_uninstall`).
  - Accès concurrent sans blocage au gestionnaire de transaction (`std::sync::OnceLock`).
  - Persistance XDG atomique (`preferences.json` et `history.json`) protégée par exclusion mutuelle interne (`STORAGE_LOCK`), validation stricte des dossiers configurés, validation des thèmes autorisés (`system`, `light`, `dark`) et plafonnement de rétention, rotation selon la politique de rétention, plafonnement automatique (500 entrées) et purge manuelle avec isolation stricte des tests.
- 17 tests TypeScript réussis :
  - Recherche insensible aux accents et identifiants natifs.
  - Filtres par format et portée préservant les installations distinctes.
  - Paquets techniques optionnels et fichiers portables visibles.
  - Affichage précis des tailles Flatpak déclarées sans invention de bytes.
  - Formatage unifié des erreurs (`errorMessage`).
  - Gestion explicite du prérequis desktop (`DesktopRequiredError`).
  - Validation des contrats `UninstallPlan` (état bloqué, conséquences, `targetPath` avec support référence Flatpak et blocage AppImage manquant).
  - Validation du rejet des identifiants malveillants ou drapeaux de commande.
  - Validation des limites et caractères autorisés pour les dossiers AppImage.
  - Validation stricte des thèmes autorisés et des limites de rétention de l'historique.
  - Validation contractuelle de la liste des paquets indispensables protégés (noyaux Ubuntu, grub, polkit).
  - Application du thème dynamique (`applyTheme`) avec bascule effective de la classe `.dark` sur `documentElement` et gestion gracieuse en environnement headless.
  - Déduplication et conservation des dossiers AppImage dans les préférences locales.
- TypeScript strict, build Vite, Prettier, rustfmt et Clippy sur toutes les cibles : 100 % conformes sans avertissement.

## Étape 2 : inventaire unifié

- 13 tests Rust réussis : DEB, contrats Snap et Flatpak, signatures AppImage, chemins XDG, liens symboliques et maintien des résultats lorsqu’un autre connecteur échoue.
- 4 tests TypeScript réussis : recherche sans accents, filtres par format et portée, paquets techniques et affichage des tailles déclarées.
- TypeScript strict, build Vite, Prettier, rustfmt et Clippy sur toutes les cibles : réussis.
- Construction Tauri debug avec les quatre connecteurs : réussie, sans production d’un paquet DEB.
- Inventaire réel : 1 920 paquets DEB, 6 applications Flatpak (1 système et 5 utilisateur) et 2 fichiers AppImage.
- Concordance du nombre d’applications avec la commande Flatpak vérifiée ; signatures et tailles des fichiers AppImage contrôlées indépendamment.
- Ajout explicite d’une AppImage déjà détectée : aucune duplication. Ajout d’un chemin inexistant : analyse AppImage marquée partielle, sans perdre les résultats Flatpak et DEB.

Le connecteur Snap est implémenté et son parsing est testé sur des fixtures. La connexion réelle à `/run/snapd.socket` est inaccessible dans cette session ; l’interface doit donc afficher Snap indisponible. Aucun nombre d’applications Snap n’est inventé.

Les nouveaux emplacements AppImage sont conservés pendant la session uniquement. Aucune persistance SQLite, permission administrateur ou opération de suppression n’a été ajoutée.

### Limites de vérification de l’interface

Une nouvelle tentative de lancement de Vite échoue sur `127.0.0.1:1420` avec `EPERM`. Un aperçu statique de test, préparé hors du produit avec IPC simulé, n’a pas pu être ouvert non plus : la politique du navigateur refuse les URL de fichiers locaux. Aucune capture ou validation visuelle n’a été obtenue ; aucune autre voie de navigation n’a été utilisée après ce refus.

Le parcours natif React → Tauri → Rust et la navigation clavier sont à exercer dans une session graphique autorisée avec `npm run desktop:dev`. Les tests de filtrage frontend ne remplacent pas cette vérification.

## Historique de l’initialisation

Les sections suivantes décrivent le socle avant l’ajout des trois connecteurs.

## Contrôles exécutés

| Contrôle                                                   | Résultat                                                                                                |
| ---------------------------------------------------------- | ------------------------------------------------------------------------------------------------------- |
| Installation npm depuis les dépendances en cache           | Réussie ; `package-lock.json` conservé.                                                                 |
| TypeScript strict et build Vite                            | Réussis.                                                                                                |
| Formatage Prettier et rustfmt                              | Conforme.                                                                                               |
| Tests unitaires Rust                                       | 4 réussis, aucun échec.                                                                                 |
| Clippy sur toutes les cibles avec refus des avertissements | Réussi.                                                                                                 |
| Construction Tauri en mode debug, sans packaging           | Réussie ; exécutable dans `src-tauri/target/debug/panel-center`.                                        |
| Exécution réelle de l’exemple d’inventaire Rust            | Réussie.                                                                                                |
| Comparaison de l’inventaire avec l’état dpkg               | 1 920 paquets installés des deux côtés, dont 32 applications graphiques identifiées par leurs lanceurs. |
| Unicité des identifiants d’installation                    | Vérifiée sur l’inventaire réel.                                                                         |
| État des connecteurs                                       | DEB disponible ; Snap, Flatpak et AppImage explicitement marqués comme prévus.                          |

Les nombres reflètent le poste lors de ce contrôle et ne sont pas codés dans l’interface.

## Comportements couverts par les tests

- Exclusion des paquets supprimés dont il reste des configurations ; conservation des identifiants d’architecture.
- Inclusion des paquets réellement installés, y compris ceux marqués comme maintenus à leur version actuelle (`hold`).
- Exclusion des lanceurs masqués et distinction des sections Desktop Entry et Desktop Action.
- Traitement des tailles absentes ou dépassant la capacité de représentation.

## Vérifications restant à effectuer sur le poste utilisateur

Le serveur Vite ne peut pas écouter sur `127.0.0.1:1420` dans l’environnement d’exécution de cette session : la création du socket échoue avec `EPERM`. La tentative d’aperçu navigateur n’a donc pas pu aboutir.

La compilation ne constitue pas une vérification visuelle. L’affichage de la fenêtre native, le parcours React → IPC Tauri → Rust dans cette fenêtre, les dimensions de fenêtre et le comportement clavier restent à exercer dans une session graphique autorisée avec `npm run desktop:dev`.

Le paquet DEB n’a pas été produit ni installé. Le service privilégié, Polkit, les transactions de désinstallation et SQLite ne font pas partie du code de ce socle ; leurs vérifications restent à réaliser avec les prochains incréments.

Le scanner de composition a été exécuté sur les sources frontend. Les seuls signaux automatiques concernent des pages légales et une image de partage destinées aux sites publics, non applicables à ce socle desktop local. Cela ne remplace pas la revue visuelle encore à effectuer.
