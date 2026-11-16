# ADR-0014 — Lecture de `~/.pgpass`, `pg_service.conf` et `~/.my.cnf`

**Statut** : Accepté (question ouverte n°1 du CDC : oui)

## Décision
Strata **lit** ces fichiers, **n'y écrit jamais**.

| Fichier | Emplacements (ordre de recherche) | Usage |
|---|---|---|
| `pg_service.conf` | `$PGSERVICEFILE`, `~/.pg_service.conf`, `$PGSYSCONFDIR/pg_service.conf`, `/etc/postgresql-common/pg_service.conf` | Import : proposer chaque service comme connexion pré-remplie ; ou connexion liée à un nom de service |
| `.pgpass` | `$PGPASSFILE`, `~/.pgpass` | Source de mot de passe à la connexion (règles libpq : correspondance `host:port:db:user`, jokers `*`, échappements `\:` `\\`) |
| `.my.cnf` | `~/.my.cnf`, `~/.mylogin.cnf` exclu (format chiffré, v2) | Sections `[client]` et `[mysql]` : hôte, port, socket, utilisateur, mot de passe |

- Nouvelle politique de mot de passe `PasswordPolicy::ClientConfigFile` : le mot de passe est relu dans le fichier à chaque connexion, **jamais copié** dans le trousseau ni dans SQLite (sauf action explicite « Enregistrer dans le trousseau »).
- Comme libpq, un `.pgpass` lisible par le groupe ou les autres est **ignoré** avec un avertissement.
- Parseurs dans `strata-core::client_config` (sans dépendance), testés unitairement ; aucune valeur lue n'est loguée.
- Flatpak : permissions en lecture seule ciblées `--filesystem=~/.pgpass:ro`, `--filesystem=~/.pg_service.conf:ro`, `--filesystem=~/.my.cnf:ro` (pas d'accès au home complet).

## Conséquences
- Planifié en J1 (tâche J1-6b), import `.my.cnf` testé en J4.
