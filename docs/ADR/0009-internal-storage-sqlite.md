# ADR-0009 — Stockage interne : rusqlite + migrations `user_version`

**Statut** : Proposé

## Décision
- `rusqlite` avec la feature `bundled` (SQLite récent garanti avec FTS5, indépendant de la distribution) ; coût binaire ≈ 1,5 Mo accepté.
- Migrations SQL embarquées (`include_str!`), appliquées en transaction, version dans `PRAGMA user_version`. Pas de crate de migration externe.
- `PRAGMA journal_mode=WAL; foreign_keys=ON; secure_delete=ON` (efface physiquement l'historique purgé).
- Création du répertoire en `0700` et du fichier en `0600` **avant** l'ouverture par SQLite (fichiers `-wal`/`-shm` hérités via umask restreint pendant l'ouverture).
- FTS5 en mode *external content* sur `history(sql)` avec triggers ; recherche par `MATCH` avec échappement des requêtes utilisateur.
- Toutes les opérations exécutées via `spawn_blocking` sur une connexion unique protégée par `Mutex` (accès rares et courts).

## Alternatives
- `sqlx` sqlite : async inutile ici, plus lourd. `diesel` : ORM inutile.
