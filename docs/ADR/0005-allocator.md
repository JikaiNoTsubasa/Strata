# ADR-0005 — Allocateur : mimalloc vs glibc

**Statut** : Accepté pour J0 — **glibc par défaut** ; mimalloc reste disponible (feature) et sera re-mesuré en J3 (scénario 1 M lignes)

## Contexte
Le CDC impose `mimalloc` sous réserve de comparaison chiffrée avec glibc.

## Décision provisoire
- `#[global_allocator]` derrière une feature `mimalloc` (**désactivée par défaut** depuis les mesures J0), ce qui permet le banc A/B sans changer le code : `cargo xtask bench-mem --mimalloc`.
- Note : `#[global_allocator]` ne concerne que les allocations Rust ; GTK/GLib/Mesa continuent d'utiliser `malloc` de la glibc. Le gain attendu concerne surtout le cache de résultats et l'export.
- Côté glibc, tester aussi `MALLOC_ARENA_MAX=2` et `malloc_trim(0)` après fermeture d'onglets (critère « retour ≤ 70 Mo »).

## Protocole de mesure
Scénarios §5 : à vide, 500 tables, 1 M lignes défilées, retour après fermeture. PSS médiane sur 5 exécutions, pour `mimalloc` / `glibc` / `glibc+ARENA_MAX=2`. Résultats consignés dans ce fichier.

## Résultats J0 (16/11/2026) — scénario « à vide », session Wayland, médiane de 5 exécutions

| Allocateur | Renderer par défaut (GPU) | Renderer `cairo` |
|---|---|---|
| mimalloc | 71,1 Mo | 40,6 – 42,8 Mo |
| **glibc** | **62,9 – 65,0 Mo** | **32,8 – 35,0 Mo** |

mimalloc coûte **≈ 7–8 Mo de plus** au repos (réservation de segments par thread : thread principal + 2 workers Tokio + threads GTK), sans bénéfice à ce stade puisque l'application n'alloue presque rien. Décision : glibc par défaut. La comparaison sera refaite en J3, où la fragmentation du cache de résultats peut inverser le résultat.
