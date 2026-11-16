# ADR-0010 — Découpage SQL maison + sqlparser-rs pour complétion et formatage

**Statut** : Proposé

## Contexte
SQL-4 exige un découpage robuste (chaînes, commentaires imbriqués PG, dollar-quoting `$tag$`, identifiants quotés, `DELIMITER` MySQL). `sqlparser-rs` échoue sur du SQL partiel ou propriétaire et n'est pas conçu pour découper un script invalide.

## Décision
- **Lexer maison** dans `strata-sql` (sans allocation, sur `&str`, positions en octets) pour : découpage des scripts, instruction sous le curseur, classification (lecture/écriture/DDL, détection `UPDATE/DELETE` sans `WHERE`, `DROP`, `TRUNCATE`), détection des paramètres `:nom`/`$1`. Testé unitairement + `proptest` (propriété : concaténer les instructions découpées redonne le script, aucune coupure dans une chaîne/commentaire).
- **sqlparser-rs** (dialectes `PostgreSqlDialect`, `MySqlDialect`) utilisé en *best effort* : extraction des tables/alias du `FROM` pour la complétion, et formatage (re-génération). Si le parse échoue, la complétion se replie sur une heuristique à base de tokens ; le formatage refuse avec un message plutôt que de détruire le SQL.
- Formatage : formateur maison piloté par le lexer (casse des mots-clés, indentation des clauses) — `sqlparser`'s `Display` perd les commentaires, donc inacceptable pour un éditeur.
- Complétion : cache de métadonnées par connexion (noms seulement, chargés paresseusement), aucun serveur de langage.

## Conséquences
- Pas de dépendance au comportement de `Display` de sqlparser ; les commentaires sont préservés.
