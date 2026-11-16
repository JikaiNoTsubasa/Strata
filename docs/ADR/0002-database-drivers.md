# ADR-0002 — Drivers : tokio-postgres et mysql_async directs, trait Driver/Session

**Statut** : Proposé — répond à la question ouverte n°7 du CDC

## Contexte
Il faut du streaming réel ligne par ligne, une annulation serveur fiable, l'accès aux métadonnées de colonnes (table/colonne d'origine pour l'éditabilité), aux avertissements serveur, et à la position d'erreur.

## Décision
- PostgreSQL : **`tokio-postgres`** + `tokio-postgres-rustls`. `query_raw` donne un flux de lignes ; `Client::cancel_token()` donne l'annulation ; `Column::table_oid()/column_id()` permettent GRD-7 ; `DbError` expose SQLSTATE, position, détail, hint. Les notices (avertissements) sont captées via la tâche `Connection`.
- MySQL/MariaDB : **`mysql_async`** (feature `rustls-tls`, sans `default` pour éviter OpenSSL). `query_iter` fournit un flux non bufferisé ; annulation par `KILL QUERY <connection_id>` sur une connexion annexe ; `Column::org_table()/org_name()` pour GRD-7.
- Lecture des valeurs en **mode texte** côté UI pour les types exotiques (affichage brut), conversion typée pour les types courants (entiers, flottants, booléens, dates, JSON, bytea).
- Trait en deux niveaux `DbDriver` (fabrique) / `DbSession` (une connexion physique) au lieu d'un `DbBackend` unique : une session porte un état transactionnel et un seul flux en cours, ce qui rend `begin/commit` explicites et correspond au modèle « 1 session par onglet ».
- `async-trait` pour permettre `Box<dyn DbSession>` (les `async fn` natives en trait ne sont pas *dyn-compatible*). Le coût d'une allocation par appel est négligeable face à la latence réseau.

## Alternatives
- **sqlx** : pool et macros inutiles ici, streaming moins contrôlable (pas d'accès simple aux métadonnées d'origine des colonnes ni à l'annulation PG par jeton), dépendances plus lourdes. Écarté.
- **Dispatch par enum** au lieu de `dyn` : un peu plus rapide, mais complique l'ajout de backends v2 (SQLite, SQL Server).

## Conséquences
- Deux implémentations à maintenir, factorisées via `Dialect` et des tests d'intégration croisés communs (même suite exécutée sur chaque backend).
- Dépendances TLS unifiées sur `rustls` (pas d'OpenSSL dans le binaire).
