# Livraison

La commande `npm run desktop:build` prépare un paquet DEB pour le socle en lecture seule.

Les fichiers D-Bus, systemd et Polkit seront ajoutés avec le service privilégié. Aucun fichier de politique permissive ou service fictif n’est installé dans cette version.

La validation du paquet doit se faire dans une machine virtuelle Ubuntu propre. L’architecture et les bibliothèques du poste de construction ne suffisent pas à garantir la compatibilité avec toutes les versions Ubuntu.
