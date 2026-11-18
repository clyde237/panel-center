# Service privilégié à implémenter

Le socle initial n’installe et n’exécute aucun service privilégié.

Le futur service Rust D-Bus devra vérifier l’autorisation Polkit du demandeur pour chaque méthode sensible, valider à nouveau la cible et ses conséquences, puis utiliser uniquement les gestionnaires explicitement pris en charge. Il n’acceptera aucune ligne de commande arbitraire.

Développer et valider ce service dans une machine virtuelle avant d’activer les désinstallations. Cette documentation réserve son emplacement ; elle ne constitue pas une implémentation du service.
