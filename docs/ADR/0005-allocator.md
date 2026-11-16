# ADR-0005 — Allocateur : mimalloc vs glibc

**Statut** : À mesurer (J0, puis revalidé en J3)

## Contexte
Le CDC impose `mimalloc` sous réserve de comparaison chiffrée avec glibc.

## Décision provisoire
- `#[global_allocator]` derrière une feature `mimalloc` (activée par défaut), ce qui permet le banc A/B sans changer le code.
- Note : `#[global_allocator]` ne concerne que les allocations Rust ; GTK/GLib/Mesa continuent d'utiliser `malloc` de la glibc. Le gain attendu concerne surtout le cache de résultats et l'export.
- Côté glibc, tester aussi `MALLOC_ARENA_MAX=2` et `malloc_trim(0)` après fermeture d'onglets (critère « retour ≤ 70 Mo »).

## Protocole de mesure
Scénarios §5 : à vide, 500 tables, 1 M lignes défilées, retour après fermeture. PSS médiane sur 5 exécutions, pour `mimalloc` / `glibc` / `glibc+ARENA_MAX=2`. Résultats consignés dans ce fichier.

## Résultats
_À compléter._
