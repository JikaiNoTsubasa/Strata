# ADR-0011 — Profil release et opt-level

**Statut** : Accepté (mesuré en J0) — `opt-level = 3` conservé

## Décision provisoire
Profil du CDC §4 (`opt-level=3`, `lto="fat"`, `codegen-units=1`, `panic="abort"`, `strip=true`). Un profil `release-small` (`opt-level="s"`) est ajouté pour comparaison.

## Mesures à réaliser
Taille du binaire, PSS au démarrage, temps de démarrage, temps de défilement 1 M lignes, débit d'export. Le code du binaire étant une faible part de la PSS (bibliothèques GTK partagées dominantes), `opt-level=3` est attendu gagnant ; à confirmer.

## Note
`panic="abort"` : un panic dans une tâche Tokio termine le processus. Les tâches du moteur ne doivent donc jamais paniquer (pas d'`unwrap`, lint `clippy::unwrap_used` en `deny` hors tests).

## Résultats J0 (16/11/2026) — glibc, session Wayland, médiane de 5 exécutions

| Profil | Binaire | PSS (GPU) | PSS (cairo) | Démarrage |
|---|---|---|---|---|
| `release` (`opt-level=3`) | 1,64 Mo | 62,9 Mo | 32,8 Mo | 260–410 ms |
| `release-small` (`opt-level="s"`) | 1,55 Mo | 62,6 Mo | 34,5 Mo | 300–416 ms |

Différences dans le bruit de mesure : la PSS est dominée par les bibliothèques partagées (GTK, Mesa), pas par le code Strata. `opt-level=3` conservé pour les performances de défilement et d'export ; à re-mesurer en J3.
