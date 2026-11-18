# Stockage local SQLite

Les préférences utilisateur, l’historique des opérations et le suivi des opérations interrompues sont stockés localement dans une base SQLite (`$XDG_DATA_HOME/panel-center/panel_center.db`, avec mode WAL et clés étrangères activées).

## Migrations versionnées

Les migrations sont gérées dans `migrations.rs` et tracées dans la table `schema_migrations` :

1. `001_initial_preferences_and_history` : tables `preferences` (ligne unique contrainte) et `operation_history` (indexée par horodatage décroissant).
2. `002_in_flight_operations` : table `in_flight_operations` permettant de réconcilier au démarrage toute opération interrompue par une fermeture brutale sans jamais inventer un succès.

Les éventuels anciens fichiers `preferences.json` et `history.json` sont importés automatiquement une seule fois lors de l’initialisation d’une base vide.
