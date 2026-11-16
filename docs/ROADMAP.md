# Strata — Feuille de route

> Jalons du CDC §14 affinés en tâches. Chaque jalon se termine par : `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test`, mesure PSS (`cargo xtask bench-mem`) consignée dans `docs/perf/<jalon>.md`, et revue utilisateur.
> Légende : ☐ à faire · ◐ en cours · ☑ fait.

---

## J0 — Cadrage

| # | Tâche | Exigences |
|---|---|---|
| J0-1 | Prérequis poste de dev : `rustup` (stable), `pkg-config`, `libgtk-4-dev`, `libadwaita-1-dev`, `libgtksourceview-5-dev`, `gettext`, `libglib2.0-dev-bin`, `heaptrack`, `weston`/`xvfb` | — |
| J0-2 | ☑ `ARCHITECTURE.md`, ADR 0001–0012, `ROADMAP.md` | §0 |
| J0-3 | ☑ Validation par l'utilisateur + réponses aux questions ouvertes (CDC §17), reportées dans `docs/ADR/` et `docs/DECISIONS.md` | §0.2 |
| J0-4 | Squelette du workspace : crates vides documentées, `apps/strata`, `xtask`, lints communs (`[workspace.lints]` : `unsafe_code = "deny"`, `clippy::unwrap_used = "deny"`, `missing_docs = "warn"`), profil release, `rust-toolchain.toml`, `LICENSE`, `CHANGELOG.md` | §4, §6.1 |
| J0-5 | Fenêtre vide : `AdwApplication` + `AdwApplicationWindow` + page d'accueil « Nouvelle connexion », runtime Tokio + pont de canaux, `tracing`, gettext initialisé, mimalloc derrière feature | §6.3, §8, §9 |
| J0-6 | `cargo xtask bench-mem` : scénario « à vide » (lancement headless, lecture PSS `smaps_rollup`, rapport JSON, comparaison à `bench/baseline.json`) | §5, §12 |
| J0-7 | Mesures ADR-0005 (allocateur), ADR-0011 (opt-level), renderers GSK `ngl`/`vulkan`/`cairo` | §5 |
| J0-8 | CI GitHub Actions : fmt → clippy → test → `cargo deny` + `cargo audit` → **détection de secrets (`gitleaks`)** → build release → bench-mem (à vide) | §12 |
| J0-9 | `data/` : `.desktop`, AppStream minimal, emplacements d'icônes (icône fournie par l'utilisateur, cf. `docs/ICONS.md`), schéma GSettings + compilation en dev | §11, §13 |
| J0-10 | `.gitignore` (target, `*.db`, `.env`, rapports locaux), politique « aucune donnée sensible dans le dépôt » | public |

**Sortie** : architecture validée ☑ ; PSS de la fenêtre vide mesurée et enregistrée comme référence (objectif 40 Mo, non bloquant — ADR-0013).

---

## J1 — Fondations

| # | Tâche | Exigences |
|---|---|---|
| J1-1 | `strata-core` : `ConnectionConfig`, `Endpoint` (TCP/socket Unix), `TlsConfig`, `Environment`, `Secret` (zeroize), `Value`, `StrataError` + tests | CON-1/4/5/8/12 |
| J1-2 | `strata-store` : ouverture 0600/0700, migrations v1 (`connections`, `groups`, `meta`), CRUD connexions + duplication | CON-1, §11 |
| J1-3 | `strata-secrets` : trait `SecretStore`, `KeyringStore` (oo7), `MemoryStore`, détection d'indisponibilité | CON-2, §10.1 |
| J1-4 | `strata-db` : traits `DbDriver`/`DbSession`, backend PG minimal (`connect`, `ping`, `server_info`), TLS rustls 5 modes, socket Unix | CON-3/4/5 |
| J1-5 | `strata-engine` : `SessionManager` (connect/test/disconnect), commandes et événements de base | CON-3 |
| J1-6 | UI : liste des connexions dans la barre latérale (couleur, environnement), dialogue de connexion (création/édition/duplication/suppression, test avec durée et erreur détaillée, mode lecture seule, politique de mot de passe), dialogue « demander le mot de passe » | CON-1/2/3/8/12 |
| J1-6b | Lecture de `pg_service.conf` / `~/.pgpass` / `~/.my.cnf` : import pré-rempli des connexions et source de mot de passe en lecture seule (ADR-0014) | Q1 |
| J1-7 | Bannière prod + avertissement connexion non chiffrée vers hôte non local | CON-8, §10.2 |
| J1-8 | Tests : store (migrations, permissions), absence de mot de passe sur disque, intégration PG (testcontainers) connexion TLS/non-TLS | §16 |

**Sortie** : création et test de connexion PG OK ; mot de passe dans le trousseau.

---

## J2 — Exploration (PostgreSQL)

| # | Tâche | Exigences |
|---|---|---|
| J2-1 | Backend PG : bases, schémas, tables, vues, vues matérialisées, séquences, fonctions/procédures (`pg_catalog`, requêtes paramétrées) | NAV-1 |
| J2-2 | `describe_table` : colonnes (type, nullable, défaut, commentaire), PK/FK, index, contraintes, déclencheurs, taille et lignes estimées | NAV-6 |
| J2-3 | Génération DDL PG (tables, index, contraintes, vues via `pg_get_viewdef`, fonctions via `pg_get_functiondef`) | NAV-5 |
| J2-4 | Arbre `GtkTreeListModel` paresseux avec nœud « chargement… » et erreurs par nœud | NAV-1/2 |
| J2-5 | Filtre instantané sur nœuds chargés + recherche serveur (`ILIKE` sur `pg_class`) | NAV-3 |
| J2-6 | Menu contextuel : ouvrir les données (placeholder J3), nouvelle requête, copier nom (quoté via `Dialect`), copier DDL, rafraîchir | NAV-4 |
| J2-7 | Onglets propriétés et DDL (sourceview lecture seule) | NAV-5/6 |
| J2-8 | Session métadonnées dédiée, fermeture sur inactivité, reconnexion avec indicateur d'état | CON-10/11 |
| J2-9 | Bench : base de 500 tables, dépliage ≤ 300 ms, PSS ≤ 80 Mo | §5 |

---

## J3 — Requêtes

| # | Tâche | Exigences |
|---|---|---|
| J3-1 | `strata-sql` : lexer, découpeur (`$$`, `DELIMITER`, commentaires), instruction sous le curseur, classification ; tests + proptest | SQL-3/4 |
| J3-2 | Onglets de requête (`AdwTabView`) liés à une connexion changeable, titre éditable, état modifié ; sourceview avec langage PG/MySQL | SQL-1/2 |
| J3-3 | Exécution (`Ctrl+Entrée`, `Ctrl+Maj+Entrée`, `F5`), session par onglet, `execute` streaming | SQL-3 |
| J3-4 | `PageCache` + fichier de débordement + option « jamais sur disque » (ADR-0008) | GRD-1, §5.1 |
| J3-5 | `ResultModel` (`gio::ListModel`) + `GtkColumnView` factories recyclées ; rendu NULL/vide/booléens/nombres/dates TZ/JSON/binaire/texte tronqué | GRD-1/9, §6.4 |
| J3-6 | Tri client borné / tri serveur pour onglet table ; redimensionnement, réordonnancement, masquage de colonnes | GRD-2 |
| J3-7 | Sélection cellules/lignes/colonnes, copie TSV/CSV/JSON/INSERT | GRD-4 |
| J3-8 | Annulation (bouton + `Échap`) avec retour UI ≤ 200 ms | SQL-7 |
| J3-9 | Résumé : durée, lignes, avertissements serveur ; erreurs avec SQLSTATE et soulignement de la position | SQL-8, §8 |
| J3-10 | Multi-résultats en sous-onglets | SQL-9 |
| J3-11 | Mode transaction manuel, Commit/Rollback, indicateur « transaction ouverte » | SQL-12 |
| J3-12 | Garde prod (confirmation avant écriture) et garde lecture seule client | CON-8/12 |
| J3-13 | Libération des résultats des onglets inactifs | §5.3 |
| J3-14 | Bench : 1 M × 10 défilé, PSS ≤ 200 Mo ; 3 connexions/5 onglets ≤ 120 Mo ; retour ≤ 70 Mo | §5 |

---

## J4 — MySQL / MariaDB

| # | Tâche | Exigences |
|---|---|---|
| J4-1 | Backend `mysql_async` : connexion TCP/socket, TLS, `server_info` (distinction MySQL/MariaDB) | CON-3/4/5 |
| J4-2 | Métadonnées `information_schema` + `SHOW CREATE …` pour le DDL | NAV-1…6 |
| J4-3 | Streaming `query_iter`, annulation `KILL QUERY` sur connexion annexe | SQL-7 |
| J4-4 | `MySqlDialect` : quoting backtick, `?`, `DELIMITER`, coloration | SQL-2/4 |
| J4-5 | Suite d'intégration commune exécutée sur PG 14/15/16/17, MySQL 8.x, MariaDB 10.11/11.x | §12 |

---

## J5 — Édition de données

| # | Tâche | Exigences |
|---|---|---|
| J5-1 | Détection d'éditabilité (table unique, PK présente dans le résultat) + explication sinon | GRD-7 |
| J5-2 | Modèle de modifications en attente (update/insert/delete), rendu visuel, Annuler | GRD-5 |
| J5-3 | Génération SQL paramétrée via `Dialect` + aperçu | GRD-6/11 |
| J5-4 | Application en transaction, rollback et message explicite en cas d'échec | GRD-6 |
| J5-5 | Éditeur de valeur (gros texte/JSON/binaire), chargement à la demande | GRD-8 |
| J5-6 | Gardes-fous `UPDATE`/`DELETE` sans `WHERE`, `DROP`, `TRUNCATE` | SQL-16 |
| J5-7 | Filtre rapide par colonne → `WHERE` (onglet table) | GRD-3 |

---

## J6 — Confort

| # | Tâche | Exigences |
|---|---|---|
| J6-1 | Auto-complétion (`GtkSourceCompletionProvider`) : mots-clés, schémas, tables, colonnes selon alias/`FROM`, fonctions ; à la demande et à la frappe avec délai | SQL-5 |
| J6-2 | Formateur SQL configurable | SQL-6 |
| J6-3 | Recherche/remplacement, aller à la ligne, commenter/décommenter, multi-curseurs basiques (selon support GtkSourceView) | SQL-13 |
| J6-4 | Sauvegarde/restauration des onglets (`tabs_state`) | SQL-14 |
| J6-5 | Historique + FTS5 + rétention + désactivation par connexion | HIS-1…4 |
| J6-6 | Thème clair/sombre/auto (`AdwStyleManager`), préférences | MIS-1 |
| J6-7 | (S) Snippets, palette `Ctrl+K`, EXPLAIN en arbre | SQL-10/15, MIS-2, HIS-5 |

---

## J7 — Export

| # | Tâche | Exigences |
|---|---|---|
| J7-1 | `strata-export` : writers streaming CSV, TSV, JSON array, JSON Lines, SQL INSERT ; tests | EXP-1/2 |
| J7-2 | Options : séparateur, guillemets, encodage, en-têtes, format de date, NULL | EXP-3 |
| J7-3 | Export de résultat ou de table complète (nouvelle requête dédiée, pas via le cache), progression, annulation, ouverture du dossier (portail) | EXP-1/4 |
| J7-4 | Copie du résultat dans le presse-papiers | EXP-6 |
| J7-5 | Bench : 100 M lignes exportées, ≤ +50 Mo | EXP-2 |

---

## J8 — Finitions v1

| # | Tâche | Exigences |
|---|---|---|
| J8-1 | Traduction FR complète, vérification aucune chaîne en dur | §9 |
| J8-2 | Audit accessibilité (Orca, navigation clavier complète, contraste) | §8 |
| J8-3 | Flatpak (manifest, permissions minimales), `.deb` | §13 |
| J8-4 | README, guide utilisateur FR/EN, mise à jour ARCHITECTURE/ADR | §16 |
| J8-5 | Passe de performance, détection de blocages > 100 ms sur le thread principal | §5, §16 |
| J8-6 | (S) Moniteur de sessions, fenêtre de diagnostic mémoire, `strata --connect` / URL, import d'URL, groupes de connexions | MIS-3/4/5, CON-6/7 |
