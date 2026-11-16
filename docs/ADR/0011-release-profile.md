# ADR-0011 — Profil release et opt-level

**Statut** : À mesurer

## Décision provisoire
Profil du CDC §4 (`opt-level=3`, `lto="fat"`, `codegen-units=1`, `panic="abort"`, `strip=true`). Un profil `release-small` (`opt-level="s"`) est ajouté pour comparaison.

## Mesures à réaliser
Taille du binaire, PSS au démarrage, temps de démarrage, temps de défilement 1 M lignes, débit d'export. Le code du binaire étant une faible part de la PSS (bibliothèques GTK partagées dominantes), `opt-level=3` est attendu gagnant ; à confirmer.

## Note
`panic="abort"` : un panic dans une tâche Tokio termine le processus. Les tâches du moteur ne doivent donc jamais paniquer (pas d'`unwrap`, lint `clippy::unwrap_used` en `deny` hors tests).

## Résultats
_À compléter._
