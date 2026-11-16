# ADR-0008 — Résultats : lecture à la demande, cache LRU, fichier de débordement

**Statut** : Proposé — **point à valider explicitement par l'utilisateur** (écriture temporaire de données sur disque)

## Contexte
CDC §5 : jamais de résultat complet en RAM ; 1 M lignes × 10 colonnes défilables en ≤ 200 Mo ; fenêtre de 5 000 lignes, pages de 500.
Les curseurs serveur scrollables existent en PostgreSQL (`DECLARE … SCROLL CURSOR`, nécessite une transaction) mais **pas en MySQL**. Ré-exécuter avec `LIMIT/OFFSET` pour remonter n'est valide que pour des `SELECT` déterministes et coûteux en `OFFSET` élevé.

## Décision
1. **Requêtes libres** (onglet de requête) — stratégie uniforme pour les deux backends :
   - lecture du flux **à la demande** (500 premières lignes, puis lots supplémentaires quand le défilement approche de la fin connue, ou en continu sur action « Tout récupérer » / export / tri client) ;
   - `PageCache` : au plus **10 pages chaudes** en RAM (LRU) ; les pages évincées sont sérialisées dans un **fichier de débordement** sous `$XDG_CACHE_HOME/strata/spill/` créé en `0600` puis **supprimé immédiatement** (`O_TMPFILE` si disponible, sinon `unlink` après ouverture) : le contenu disparaît à la fermeture de l'onglet ou en cas de crash ;
   - relecture d'une page évincée : `pread` + décodage, ≈ 1 ms pour 500 lignes.
   - Tri côté client (GRD-2, résultat borné) : uniquement si le résultat complet est récupéré et ≤ 100 000 lignes ; au-delà, proposer de relancer avec `ORDER BY`.
2. **Ouverture de table** (onglet de données) : pas de spill ; **pagination keyset** sur la PK (repli `LIMIT/OFFSET` si pas de PK), tri/filtre côté serveur, pages re-demandées au besoin.
3. **Option de préférence** « Ne jamais écrire de résultats sur disque » : le cache devient une fenêtre glissante pure ; remonter au-delà de la fenêtre déclenche une ré-exécution (avec confirmation).

## Alternatives
- Curseur `SCROLL` PostgreSQL : pas d'équivalent MySQL, transaction longue côté serveur (verrous, *idle in transaction*), incompatible avec l'auto-commit par défaut.
- Matérialisation complète en RAM (comportement DBeaver) : contraire au CDC.
- Fenêtre glissante pure + ré-exécution : non déterministe, coûteux pour l'utilisateur.

## Conséquences
- Données de résultats temporairement sur disque (non nommées, permissions 0600, inaccessibles hors du processus après unlink). Impact sécurité limité mais à accepter explicitement ; l'option 3 permet de le désactiver.
- Encodage de page : `Value` sérialisée en format binaire maison (tag + longueur + octets), sans dépendance supplémentaire.
