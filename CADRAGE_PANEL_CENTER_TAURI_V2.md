# Panel Center — Cadrage produit et technique

Version : 1.0  
Date : 18 novembre 2026  
Statut : cadrage initial pour réalisation  
Cible : application desktop Ubuntu  
Stack de référence : Tauri v2, React, TypeScript et Rust

## 1. Objectif

Créer une application graphique qui rassemble les logiciels présents sur un PC Ubuntu et permet de les désinstaller depuis une interface commune, quel que soit leur mode d’installation : paquet DEB, APT, Snap, Flatpak ou fichier AppImage.

L’application doit prendre en charge les logiciels installés depuis le Centre d’applications, le terminal ou un installateur téléchargé. Leur présence dans un catalogue en ligne n’est pas une condition pour les afficher.

Exemple de parcours : l’utilisateur recherche VLC, voit ses éventuelles installations DEB et Flatpak, choisit celle à retirer, consulte les conséquences, confirme et suit la progression jusqu’au résultat.

Le nom de travail est **Panel Center** ; l’identifiant technique du projet est `panel_center`.

## 2. Public et principes

- Utilisateur principal : propriétaire d’un PC Ubuntu souhaitant gérer ses logiciels sans utiliser le terminal.
- Fonctionnement local ; aucun compte, hébergement ou abonnement nécessaire.
- Interface en français pour la première version.
- Les droits ordinaires suffisent à consulter l’inventaire.
- Chaque désinstallation vise une installation précise et présente les conséquences avant confirmation.
- Le résultat affiché repose sur une nouvelle vérification de l’état local après l’opération.

## 3. Périmètre de la première version

| Fonction | Comportement attendu |
| --- | --- |
| Inventaire | Réunir les applications DEB/APT, Snap, Flatpak et les AppImage détectées. |
| Recherche | Rechercher par nom et identifiant technique. |
| Filtres | Filtrer par format et, lorsque pertinent, par portée utilisateur ou système. |
| Détails | Afficher nom, icône, version, format, identifiant, portée et taille lorsqu’ils sont disponibles. |
| Aperçu | Présenter l’installation ciblée, les éléments concernés et les options applicables. |
| Désinstallation | Utiliser le mécanisme approprié au format, avec autorisation administrateur si nécessaire. |
| Progression | Afficher les étapes connues et les messages utiles pendant l’opération. |
| Historique | Conserver localement les opérations, leur résultat et leur diagnostic. |
| AppImage | Rechercher dans les emplacements configurés et permettre l’ajout manuel d’un fichier. |
| Préférences | Gérer les dossiers AppImage, l’apparence et la conservation de l’historique. |

Le périmètre initial exclut l’installation de logiciels, les mises à jour, le catalogue du store, la suppression en masse, l’administration distante et le nettoyage automatique des fichiers personnels. Les logiciels compilés manuellement, les scripts d’installation et les gestionnaires comme pip, npm ou Homebrew demanderaient des connecteurs supplémentaires.

## 4. Définir correctement une installation

L’unité de gestion est **l’installation**, pas seulement le nom commercial d’un logiciel.

- Deux installations de VLC, l’une en DEB et l’autre en Flatpak, restent distinguables.
- Un Flatpak utilisateur et un Flatpak système sont des cibles différentes.
- Un paquet peut fournir plusieurs lanceurs ; plusieurs lanceurs ne doivent pas créer plusieurs opérations de suppression identiques.
- Une application peut dépendre de plusieurs paquets ; les dépendances ne sont pas présentées comme autant d’applications utilisateur.
- Un fichier `.deb` téléchargé mais jamais installé est un installateur, pas une application installée.
- Une AppImage est un fichier exécutable autonome : son affichage indique sa présence, sans prétendre qu’elle a déjà été utilisée.

Les métadonnées de bureau et AppStream servent à enrichir l’affichage. L’état d’installation est établi auprès du gestionnaire concerné.

## 5. Traitement par format

| Format | Inventaire | Retrait | Particularités |
| --- | --- | --- | --- |
| DEB / APT | Base dpkg, requêtes structurées et métadonnées de bureau. | Transaction APT avec aperçu des paquets affectés. | Comprend les DEB installés manuellement ; traiter les dépendances et paquets protégés. |
| Snap | Inventaire snapd ou commande Snap documentée. | Transaction de suppression Snap. | Distinguer applications, bases et composants techniques ; expliquer les sauvegardes automatiques éventuelles. |
| Flatpak | Inventaire des applications par installation. | Transaction Flatpak avec portée explicite. | Distinguer utilisateur, système, installation nommée et référence exacte ; séparer applications et runtimes. |
| AppImage | Dossiers configurés, raccourcis existants et ajout manuel. | Suppression du fichier identifié et de l’intégration attribuable à ce fichier. | Aucun registre central obligatoire ; la détection exhaustive ne peut être garantie. |

### DEB / APT

La vue principale privilégie les applications identifiables ; une vue avancée peut exposer davantage de paquets avec leurs limites de gestion. L’absence de lanceur ne signifie pas qu’un paquet est inutile.

L’aperçu montre tous les paquets qu’APT propose de retirer. La suppression d’un paquet essentiel, protégé ou d’un composant indispensable au bureau doit être bloquée selon une politique documentée et vérifiée sur les versions Ubuntu prises en charge. Ne pas activer automatiquement `autoremove`.

Une purge des configurations gérées par le paquet, si proposée, doit être distinguée de la suppression des fichiers personnels : elle ne garantit pas leur effacement.

### Snap et Flatpak

Respecter les choix et la sémantique des gestionnaires. L’interface explique les données retirées, conservées ou sauvegardées. Une option unique « tout effacer » ne doit pas masquer des comportements différents selon le format.

Pour Flatpak, l’identifiant d’application seul est insuffisant : conserver la référence et l’installation visées. Les runtimes partagés ne sont pas supprimés automatiquement avec une application.

### AppImage

Proposer initialement les dossiers usuels existants, tels que `~/Applications` et le dossier Téléchargements déterminé via XDG. L’utilisateur peut ajouter d’autres emplacements et relancer l’analyse.

La recherche ne doit pas dépendre exclusivement de l’extension du fichier. Vérifier le format autant que possible sans exécuter le fichier trouvé. Les liens symboliques, emplacements externes et fichiers renommés demandent un traitement explicite.

L’aperçu indique le chemin exact et les raccourcis associés. Ne retirer que les éléments dont l’association est établie ; les caches, préférences et documents restent hors suppression automatique. Un fichier manquant ou un volume déconnecté produit un état explicite.

## 6. Stack retenue pour le cadrage

| Couche | Choix | Responsabilité |
| --- | --- | --- |
| Conteneur desktop | Tauri v2 | Fenêtre, cycle de vie et communication entre interface et moteur. |
| Interface | React + TypeScript | Présentation, navigation et interactions. |
| Construction frontend | Vite | Développement et génération des ressources embarquées. |
| Styles | Tailwind CSS | Espacements, couleurs, états et adaptation aux dimensions de fenêtre. |
| Composants | shadcn/ui | Dialogues, tableaux, menus, champs et notifications personnalisables. |
| Icônes | Lucide React | Icônes fonctionnelles ; les logos logiciels viennent des métadonnées disponibles. |
| Moteur | Rust | Inventaire, validation, connecteurs système et orchestration des transactions. |
| Sérialisation | Serde | Contrats entre Rust et TypeScript. |
| Exécution des tâches | Runtime asynchrone Tauri ; Tokio si nécessaire | Travaux longs hors du fil de l’interface. |
| Autorisations | Polkit + service privilégié restreint | Autorisation et exécution des opérations système. |
| Stockage | SQLite via une bibliothèque Rust adaptée | Préférences et historique local. |
| Livraison initiale | Paquet `.deb` | Application, service, politiques d’autorisation et intégration bureau. |

Choisir les versions compatibles lors de l’initialisation, puis conserver les fichiers de verrouillage. Le cadrage impose Tauri **v2**, sans figer des numéros mineurs non vérifiés.

## 7. Architecture

### Interface React

L’interface affiche l’inventaire et les états d’opération, recueille les choix puis appelle les commandes Tauri. Elle ne construit aucune commande shell et ne décide pas seule de l’éligibilité d’une suppression.

Les ressources sont embarquées avec l’application. Les appels système sont exposés par des commandes métiers limitées ; les capacités Tauri et la politique de sécurité du contenu sont configurées explicitement.

### Moteur Rust non privilégié

Il normalise les résultats des connecteurs, fournit les métadonnées, prépare les aperçus et tient l’historique. Il peut déléguer aux API natives ou aux outils documentés du système.

Privilégier les sorties structurées ou les formats de requête contrôlés. Éviter de déduire les résultats à partir de textes humains dépendant de la langue. Les appels de processus utilisent des arguments séparés et validés, sans concaténation shell.

### Connecteurs

Chaque connecteur fournit les opérations suivantes : vérifier sa disponibilité, inventorier, obtenir les détails, préparer un retrait, exécuter le retrait autorisé et vérifier son résultat.

Un connecteur absent ou défaillant ne doit pas masquer les résultats des autres formats. L’inventaire signale alors qu’il est partiel.

### Service privilégié

Proposition d’implémentation : un petit service Rust sur D-Bus système, installé dans un emplacement protégé, dont chaque méthode sensible contrôle l’autorisation Polkit du demandeur.

Le service n’accepte ni commande arbitraire, ni exécutable choisi par React, ni script fourni par l’utilisateur. Il valide à nouveau les identifiants et les conséquences auprès du gestionnaire. Ses droits servent uniquement aux opérations système qui les nécessitent.

Les fichiers utilisateur, notamment les AppImage appartenant à l’utilisateur, sont traités avec les droits de cet utilisateur. L’interface graphique ne s’exécute pas en administrateur et ne collecte pas le mot de passe.

Les capacités Tauri limitent l’accès du frontend aux fonctions Rust ; elles ne remplacent pas les autorisations Linux.

## 8. Transactions et erreurs

Parcours nominal : inventaire → sélection → préparation → aperçu → confirmation → autorisation éventuelle → exécution → vérification → historique et actualisation.

- Associer chaque opération à un identifiant unique et empêcher les doubles soumissions.
- Pour la V1, exécuter une seule désinstallation à la fois ; les lectures peuvent être parallélisées.
- Gérer les verrous des gestionnaires et signaler une transaction extérieure en cours.
- Réévaluer les conséquences juste avant l’exécution ; si elles changent de manière significative, présenter un nouvel aperçu.
- Désactiver ou invalider une préparation expirée ou déjà utilisée.
- Distinguer annulation, refus d’autorisation, cible disparue, gestionnaire absent, conflit, échec et résultat incertain.
- Afficher une progression indéterminée si le gestionnaire ne fournit aucun pourcentage fiable.
- N’exposer une annulation en cours d’exécution que lorsque le gestionnaire permet de la traiter correctement.
- Après fermeture ou interruption, rapprocher l’opération persistée de l’état réel au prochain lancement ; ne pas afficher un succès supposé.

## 9. Contrats de données proposés

| Objet | Champs principaux |
| --- | --- |
| `ApplicationInstallation` | Clé stable, nom, version, format, identifiant natif, portée, référence, chemin éventuel, icône, taille disponible, état et capacité de retrait. |
| `UninstallPlan` | Identifiant opaque, cible, options, éléments affectés, effets sur les données, estimation disponible, autorisation requise et validité. |
| `Operation` | Identifiant, plan, état, étape, progression facultative, horodatages et résultat. |
| `OperationError` | Code stable, message utilisateur, détail technique filtré et possibilité de réessai. |
| `Preferences` | Apparence, dossiers AppImage et politique de conservation de l’historique. |

Les tailles peuvent être indisponibles ou estimées. La somme des tailles affichées n’est pas une promesse d’espace récupérable, notamment en présence de composants partagés.

Commandes Tauri envisagées : `list_installations`, `get_installation_details`, `scan_appimages`, `prepare_uninstall`, `confirm_uninstall`, `get_operation`, `list_history` et `update_preferences`.

`confirm_uninstall` reçoit une référence de préparation à revalider, pas une ligne de commande. Les événements ou canaux Tauri transmettent la progression ; une requête permet de retrouver l’état après reconnexion de l’interface.

Les données persistées ne constituent jamais une preuve suffisante de l’état d’installation courant.

## 10. Interface et expérience utilisateur

Prévoir une interface claire, aérée et adaptée aux fenêtres desktop, avec un thème clair initial et une navigation simple.

| Vue | Contenu |
| --- | --- |
| Applications | Recherche, filtres, liste, compteur et état de l’inventaire. |
| Détail | Métadonnées, format visible, portée, identifiant et action de désinstallation. |
| Aperçu de suppression | Cible exacte, conséquences, options applicables, confirmation ou motif de blocage. |
| Opération | Étape, progression, état de l’autorisation et messages utiles. |
| Historique | Date, application, format, résultat et diagnostic consultable. |
| Paramètres | Dossiers de recherche AppImage, apparence et conservation de l’historique. |

Prévoir les états chargement, liste vide, absence de résultat, gestionnaire indisponible, analyse partielle et erreur récupérable. Une icône de remplacement accompagne les applications sans logo.

Les boutons sensibles indiquent clairement l’action. Les dialogues prennent en charge le clavier, la gestion du focus et des libellés accessibles. Les messages techniques détaillés restent consultables sans surcharger le parcours courant.

## 11. Stockage et confidentialité

- Stocker préférences et historique dans les répertoires utilisateur appropriés, suivant les conventions XDG/Tauri.
- Utiliser des migrations versionnées pour SQLite.
- Conserver le diagnostic nécessaire sans enregistrer de mot de passe ou secret d’autorisation.
- Prévoir l’effacement de l’historique depuis les paramètres.
- Ne transmettre aucune télémétrie dans la première version.
- Fonctionner hors ligne pour l’inventaire local et les opérations compatibles avec l’état des gestionnaires.

## 12. Organisation du code proposée

| Emplacement | Contenu |
| --- | --- |
| `src/app/` | Navigation et structure de l’interface. |
| `src/features/applications/` | Inventaire, recherche, filtres et détails. |
| `src/features/uninstall/` | Aperçu, confirmation et suivi. |
| `src/features/history/` | Historique. |
| `src/features/settings/` | Préférences. |
| `src/components/ui/` | Composants shadcn/ui adaptés. |
| `src/lib/` | Client Tauri et utilitaires partagés. |
| `src-tauri/src/commands/` | Points d’entrée Tauri. |
| `src-tauri/src/domain/` | Modèles et règles métier. |
| `src-tauri/src/providers/` | Connecteurs DEB, Snap, Flatpak et AppImage. |
| `src-tauri/src/services/` | Orchestration et suivi. |
| `src-tauri/src/storage/` | SQLite et migrations. |
| `src-tauri/capabilities/` | Permissions du frontend. |
| `crates/panel-center-helper/` | Service privilégié indépendant de la WebView. |
| `packaging/` | D-Bus, Polkit, service système et intégration au paquet DEB. |
| `tests/` | Scénarios d’intégration et fixtures. |
| `docs/` | Décisions techniques et procédures de validation. |

Cette structure est indicative ; elle doit rester proportionnée à la première version.

## 13. Plan de réalisation

### Étape 1 — Valider l’intégration Ubuntu

Relever la version Ubuntu cible, l’architecture et les gestionnaires présents. Initialiser Tauri v2 avec React/TypeScript/Vite, intégrer les composants visuels et ouvrir une fenêtre sur la machine cible.

Produire un premier inventaire DEB en lecture seule. Tester dans une machine virtuelle le mécanisme d’autorisation du service avec une opération contrôlée avant d’étendre les suppressions.

**Validation :** démarrage de l’application, inventaire vérifié et autorisation acceptée/refusée correctement, sans privilèges administrateur pour l’interface.

### Étape 2 — Livrer l’inventaire unifié

Ajouter Snap, Flatpak et AppImage, les filtres et la vue détail. Vérifier les installations multiples, les portées Flatpak et les gestionnaires absents.

**Validation :** chaque entrée correspond à une cible locale identifiable ; une analyse incomplète est visible.

### Étape 3 — Réaliser les désinstallations

Développer les aperçus et transactions par format, avec suivi, autorisation, gestion des conflits et vérification finale.

**Validation :** un logiciel de test peut être retiré dans chacun des quatre formats sans atteindre une autre installation ni supprimer des données hors du périmètre annoncé.

### Étape 4 — Finaliser la livraison

Ajouter SQLite, historique, paramètres, accessibilité et traitement des interruptions. Construire et tester le DEB complet avec son service et ses politiques.

**Validation :** installation, lancement, utilisation et désinstallation du produit sur une machine virtuelle propre correspondant à la cible.

Aucune durée ferme n’est fixée avant la validation des intégrations système et des versions Ubuntu à supporter.

## 14. Critères d’acceptation et vérifications

| Scénario | Résultat attendu |
| --- | --- |
| DEB installé depuis un fichier local | Présent dans l’inventaire même si le fichier d’installation a été supprimé. |
| Même logiciel en DEB et Snap | Deux installations distinctes ; seul le format choisi est retiré. |
| Flatpak utilisateur et système | Portées affichées et cible exacte respectée. |
| AppImage dans un dossier configuré | Fichier détecté, chemin visible et retrait limité aux éléments identifiés. |
| Ajout manuel d’une AppImage | Analyse sans exécution du fichier et détection des doublons de chemin. |
| Gestionnaire absent | Autres formats utilisables, absence expliquée. |
| Retrait impliquant d’autres paquets | Conséquences affichées avant confirmation ; blocage si élément protégé. |
| Refus ou annulation Polkit | Aucune suppression exécutée et résultat correctement affiché. |
| Gestionnaire verrouillé | Message explicite et possibilité de réessai appropriée. |
| Double clic sur confirmation | Une seule opération créée. |
| Cible modifiée après aperçu | Nouvelle validation et nouvel aperçu si nécessaire. |
| Identifiant ou chemin malveillant | Rejet ; aucune exécution de commande arbitraire. |
| Arrêt pendant une opération | État réconcilié au redémarrage ; aucun succès inventé. |
| Suppression réussie | État local revérifié, liste actualisée et historique enregistré. |

Utiliser des tests unitaires pour les règles et la validation des entrées, des simulations pour les contrats et erreurs, puis des tests réels de bout en bout sur machine virtuelle avec logiciels de test. Ne pas désinstaller les logiciels personnels du poste de développement pour valider le produit.

Exécuter les contrôles TypeScript, le build frontend, les vérifications de format et d’analyse Rust, les tests pertinents et le build Tauri. Vérifier également la réactivité de l’interface lors d’une analyse ou d’une transaction longue.

Les résultats simulés et les transactions réellement exécutées doivent être distingués dans les comptes rendus.

## 15. Compatibilité et livraison

La version Ubuntu de référence et l’architecture restent à relever avant l’implémentation. La prise en charge d’autres versions demande une matrice de validation couvrant WebKitGTK, Polkit et les gestionnaires de paquets.

Tauri utilise WebKitGTK sous Linux. Tester le rendu sur la version cible et déterminer une base de compilation compatible ; ne pas annoncer une compatibilité universelle à partir d’un seul poste.

Le DEB doit embarquer ou déclarer les dépendances nécessaires, installer les fichiers du service avec les permissions adaptées et les retirer correctement lors de la désinstallation. La suppression du produit ne doit déclencher aucune suppression des applications qu’il gérait.

Les distributions Snap ou Flatpak de Panel Center restent à étudier séparément : leur confinement peut compliquer l’administration des logiciels de l’hôte. La capacité à gérer ces formats ne signifie pas que Panel Center doit lui-même être distribué dans ces formats.

## 16. Décisions et points à préciser

| Sujet | État |
| --- | --- |
| Tauri v2 + React + Rust | Stack de référence de ce cadrage demandé. |
| Tailwind, shadcn/ui et Lucide | Bibliothèques d’interface proposées dans la demande. |
| DEB, Snap, Flatpak et AppImage | Périmètre fonctionnel initial. |
| `.deb` pour la livraison | Recommandation technique à valider par un prototype installable. |
| Service Rust D-Bus + Polkit | Architecture proposée, à éprouver au premier incrément. |
| SQLite | Stockage local proposé. |
| Version Ubuntu et architecture | À relever sur le poste cible. |
| Liste des paquets et composants protégés | À définir et tester par version Ubuntu supportée. |
| Nom final et identité visuelle | Panel Center sert de nom de travail. |
| Fonctionnalités supplémentaires | Hors V1 tant qu’elles ne sont pas décidées. |

## 17. Sources officielles

Références consultées le 18 novembre 2026 ; vérifier leurs évolutions au moment de l’initialisation.

- [Tauri v2 — Architecture](https://v2.tauri.app/concept/architecture/)
- [Tauri v2 — Appels Rust depuis le frontend](https://v2.tauri.app/develop/calling-rust/)
- [Tauri v2 — Sécurité et capacités](https://v2.tauri.app/security/capabilities/)
- [Tauri v2 — Prérequis](https://v2.tauri.app/start/prerequisites/)
- [Tauri v2 — Distribution Debian](https://v2.tauri.app/distribute/debian/)
- [Tauri v2 — Versions de WebView](https://v2.tauri.app/reference/webview-versions/)
- [shadcn/ui — Installation Vite](https://ui.shadcn.com/docs/installation/vite)
- [Ubuntu — Gestion des paquets](https://documentation.ubuntu.com/server/how-to/software/package-management/)
- [Snap — Prise en main et suppression](https://snapcraft.io/docs/tutorials/get-started/)
- [Flatpak — Référence des commandes](https://docs.flatpak.org/en/latest/flatpak-command-reference.html)
- [AppImage — Fonctionnement et emplacements](https://docs.appimage.org/user-guide/faq.html)
- [Polkit — Modèle d’autorisation](https://www.freedesktop.org/software/polkit/docs/latest/polkit.8)
