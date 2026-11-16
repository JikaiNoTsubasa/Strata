# Strata — Cahier des charges

> Client de gestion de bases de données pour Linux, natif, ultra-léger (alternative à DBeaver).
> Ce document est destiné à être donné à Claude Code (CLI) pour concevoir puis développer le projet.

---

## 0. Instructions pour Claude Code

**Lis ce document en entier avant toute action.** Il fait foi. Procède ainsi :

1. **Ne code rien au départ.** Produis d'abord, dans `docs/` :
   - `docs/ARCHITECTURE.md` (découpage en crates, flux de données, diagrammes Mermaid) ;
   - `docs/ADR/` (un fichier par décision structurante : toolkit, drivers, modèle async, stockage des secrets…) ;
   - `docs/ROADMAP.md` (jalons de la section 14 affinés en tâches).
2. **Valide l'architecture avec l'utilisateur** avant d'implémenter.
3. Implémente **jalon par jalon** (section 14). Chaque jalon doit compiler, passer `cargo clippy -- -D warnings`, `cargo fmt --check` et les tests.
4. **Mesure la RAM** à chaque jalon (section 5) et signale toute régression.
5. **Pose des questions** plutôt que de supposer quand une exigence est ambiguë. Les hypothèses déjà prises sont listées en section 17 : ne les remets en cause que si tu trouves un vrai blocage technique.
6. Commits atomiques, messages en anglais au format Conventional Commits (`feat:`, `fix:`, `perf:`, `refactor:`, `docs:`, `test:`, `chore:`).
7. Code, identifiants et commentaires en **anglais** ; documentation utilisateur et chaînes d'interface en **français et anglais** (i18n dès le départ, voir section 9).

---

## 1. Contexte et vision

DBeaver est complet mais lourd : JVM/Eclipse/SWT, souvent **600 Mo à 1,5 Go de RAM**, démarrage lent. Strata vise les développeurs et administrateurs qui veulent un client SQL **rapide, sobre en mémoire, natif Linux**, couvrant 90 % des usages quotidiens : se connecter, explorer le schéma, écrire des requêtes, consulter et modifier des données, exporter.

**Nom** : Strata (hypothèse : vérifier la disponibilité sur GitHub, crates.io, Flathub avant publication ; prévoir un nom de repli).

**Principe directeur** : *chaque fonctionnalité doit justifier son coût en mémoire et en complexité.* Si une fonctionnalité fait gonfler la RAM de façon notable, elle est chargée à la demande ou reportée.

---

## 2. Objectifs et non-objectifs

### Objectifs (v1)
- Application de bureau **native Linux**, interface GTK4 + libadwaita.
- Support **PostgreSQL** et **MySQL / MariaDB**.
- **Empreinte RAM minimale** (budgets chiffrés en section 5).
- Démarrage quasi instantané (< 500 ms jusqu'à la fenêtre utilisable).
- Édition de requêtes confortable (coloration, auto-complétion, multi-onglets).
- Exploration du schéma et consultation/édition des données.
- Stockage sécurisé des identifiants.

### Non-objectifs (v1)
- Pas de support Windows/macOS (le code ne doit pas l'empêcher à terme, mais aucun effort de portage).
- Pas de SQL Server, Oracle, SQLite comme **cible utilisateur** (SQLite sert uniquement au stockage interne de Strata). Prévoir une abstraction pour les ajouter en v2.
- Pas de diagramme ER, d'import de données, de comparaison de schémas, de tunnel SSH (v2, voir section 15).
- Pas de plugins, pas de moteur de scripting, pas de composant web embarqué (aucun WebKit/Electron/Tauri).
- Pas de synchronisation cloud, pas de télémétrie.

---

## 3. Public cible et cas d'usage

| Persona | Besoin principal |
|---|---|
| Développeur back-end | Lancer des requêtes ad hoc, inspecter des tables, corriger quelques lignes |
| Administrateur / SRE | Se connecter à plusieurs serveurs, inspecter schémas, index, volumes |
| Analyste data | Exécuter des requêtes, exporter en CSV/JSON/Excel-compatible |

Cas d'usage clés :
1. Créer une connexion, la tester, l'enregistrer.
2. Parcourir bases > schémas > tables > colonnes/index/contraintes.
3. Écrire et exécuter une requête, voir les résultats immédiatement (streaming).
4. Ouvrir une table et éditer des cellules directement dans la grille.
5. Annuler une requête longue sans figer l'interface.
6. Retrouver une requête passée (historique) ou favorite (snippets).
7. Exporter un résultat ou une table entière.

---

## 4. Stack technique (imposée)

| Couche | Choix | Notes |
|---|---|---|
| Langage | **Rust** (édition 2021 ou plus récente, toolchain stable) | Pas d'`unsafe` hors FFI justifiée |
| UI | **GTK4 + libadwaita** via `gtk4-rs` / `libadwaita-rs` | Widgets natifs, pas de moteur web |
| Éditeur SQL | **GtkSourceView 5** (`sourceview5`) | Coloration, indentation, numéros de ligne, recherche |
| Grille de résultats | `GtkColumnView` + `GListModel` personnalisé | **Virtualisée** : seules les lignes visibles sont instanciées |
| Runtime async | **Tokio** (runtime multi-thread à **2 workers max**) | Pont avec la boucle GLib via canaux (`async-channel`) |
| PostgreSQL | `tokio-postgres` (+ `rustls` pour TLS) | Ou `sqlx` si l'ADR le justifie |
| MySQL/MariaDB | `mysql_async` (+ `rustls`) | Idem |
| Parsing SQL | `sqlparser-rs` | Auto-complétion contextuelle, formatage |
| Stockage interne | **SQLite** via `rusqlite` | Connexions, historique, snippets, préférences |
| Secrets | **libsecret** (Secret Service D-Bus) via `oo7` ou `secret-service` | Jamais de mot de passe en clair sur disque |
| Allocateur | `mimalloc` | À comparer à glibc dans l'ADR avec mesures |
| i18n | `gettext-rs` ou `fluent` | FR + EN |
| Logs | `tracing` + `tracing-subscriber` | Niveau configurable, pas de log de données sensibles |
| Erreurs | `thiserror` (bibliothèques), `anyhow` (binaire) | |
| Tests | `cargo test`, `testcontainers` pour l'intégration | |
| Build | Cargo workspace, Meson optionnel pour Flatpak | |

**Profil release obligatoire** (`Cargo.toml`) :

```toml
[profile.release]
opt-level = 3        # comparer avec "s"/"z" et trancher par mesure
lto = "fat"
codegen-units = 1
panic = "abort"
strip = true
```

**Interdits** : Electron, Tauri/WebKitGTK, JVM, Python/Node embarqué, toute dépendance tirant un navigateur.

---

## 5. Exigences de performance et budgets mémoire

Mesures sur une machine de référence (8 Go RAM, x86_64, GNOME/Wayland), via `/proc/<pid>/smaps_rollup` (**PSS**) et `heaptrack`. Les budgets ci-dessous sont des **critères d'acceptation**.

| Scénario | Budget RAM (PSS) |
|---|---|
| Application lancée, aucune connexion | **≤ 60 Mo** |
| 1 connexion PostgreSQL ouverte, arbre du schéma déplié sur une base de 500 tables | **≤ 80 Mo** |
| 3 connexions, 5 onglets de requêtes ouverts | **≤ 120 Mo** |
| Résultat de 1 000 000 lignes × 10 colonnes consulté en défilement | **≤ 200 Mo** (fenêtre glissante, pas de matérialisation complète) |
| Après fermeture de tous les onglets et déconnexion | Retour à **≤ 70 Mo** sous 5 s (pas de fuite) |

| Scénario | Budget temps |
|---|---|
| Lancement → fenêtre interactive | ≤ 500 ms |
| Ouverture de l'arbre d'une base (500 tables) | ≤ 300 ms hors latence réseau |
| Premier lot de 500 lignes affiché après exécution | ≤ 50 ms après réception |
| Défilement de la grille | 60 fps constants |
| Annulation d'une requête | Retour UI ≤ 200 ms |

### Règles de conception imposées
1. **Jamais de résultat complet en mémoire.** Streaming via curseur serveur ou `LIMIT/OFFSET`/keyset ; fenêtre glissante côté client (ex. 5 000 lignes max en cache, éviction LRU par pages de 500).
2. **Schéma chargé paresseusement** : aucune requête `information_schema` / `pg_catalog` tant que l'utilisateur n'a pas déplié le nœud.
3. **Onglets inactifs allégés** : le résultat d'un onglet non visible depuis > N minutes est libéré (re-exécutable), seule la requête est conservée.
4. **Un seul processus**, aucun sidecar, aucun serveur de langage permanent.
5. **Valeurs de cellules compactes** : stockage en `Box<str>`/`Arc<str>`/enum compact, pas de `String` par cellule ; les BLOB/gros textes sont tronqués à l'affichage et chargés à la demande.
6. **Pool de connexions minimal** : 1 connexion « métadonnées » + 1 par onglet actif ; fermeture des connexions inactives après délai configurable.
7. **Un benchmark mémoire automatisé** dans la CI (voir section 12) qui échoue si un budget est dépassé de plus de 10 %.

---

## 6. Architecture

### 6.1 Découpage en crates (Cargo workspace)

```
strata/
├── Cargo.toml                  # workspace
├── crates/
│   ├── strata-core/            # types du domaine, traits, erreurs (sans UI, sans driver)
│   ├── strata-db/              # trait DbBackend + implémentations
│   │   ├── postgres/
│   │   └── mysql/
│   ├── strata-sql/             # parsing, formatage, moteur d'auto-complétion
│   ├── strata-store/           # SQLite interne : connexions, historique, snippets, prefs
│   ├── strata-secrets/         # abstraction libsecret
│   ├── strata-export/          # CSV, JSON, SQL INSERT, TSV (streaming)
│   └── strata-ui/              # GTK4/libadwaita (le seul crate dépendant de GTK)
├── apps/strata/                # binaire : assemblage, init runtime, CLI args
├── docs/
├── packaging/                  # flatpak, deb, desktop file, appstream, icônes
├── po/                         # traductions
└── tests/                      # intégration (testcontainers)
```

Règle : **`strata-core` et `strata-db` ne dépendent jamais de GTK.** L'UI est remplaçable et la logique testable sans affichage.

### 6.2 Abstraction base de données

Définir un trait asynchrone (à affiner dans l'ADR) :

```rust
#[async_trait]
pub trait DbBackend: Send + Sync {
    async fn connect(cfg: &ConnectionConfig, secret: Secret) -> Result<Self> where Self: Sized;
    async fn ping(&self) -> Result<()>;
    async fn server_info(&self) -> Result<ServerInfo>;

    // Métadonnées (toutes paresseuses et paginées)
    async fn list_databases(&self) -> Result<Vec<DbName>>;
    async fn list_schemas(&self, db: &DbName) -> Result<Vec<SchemaName>>;
    async fn list_tables(&self, schema: &SchemaRef) -> Result<Vec<TableInfo>>;
    async fn describe_table(&self, t: &TableRef) -> Result<TableDetails>; // colonnes, PK, FK, index, contraintes, DDL

    // Exécution
    async fn execute_stream(&self, sql: &str, params: Params, cancel: CancelToken)
        -> Result<RowStream>;          // flux de lots de lignes + métadonnées de colonnes
    async fn execute_non_query(&self, sql: &str, cancel: CancelToken) -> Result<ExecSummary>;

    // Transactions
    async fn begin(&self) -> Result<()>;
    async fn commit(&self) -> Result<()>;
    async fn rollback(&self) -> Result<()>;

    async fn cancel(&self) -> Result<()>;   // pg_cancel_request / KILL QUERY
}
```

Les différences de dialecte (quoting d'identifiants, types, `LIMIT`, métadonnées) sont encapsulées dans un trait `Dialect` utilisé par `strata-sql` et l'éditeur de grille.

### 6.3 Modèle de threads

- **Thread principal** : boucle GLib/GTK, ne fait **jamais** d'I/O réseau ni de calcul lourd.
- **Runtime Tokio** (2 workers) : toutes les opérations DB, export, parsing lourd.
- Communication : `async-channel` / `glib::MainContext::spawn_local` ; les messages vers l'UI sont des **événements typés** (`UiEvent::RowsBatch`, `UiEvent::QueryFinished`, …) et regroupés (coalescing) pour éviter de saturer la boucle principale.
- **Annulation** : chaque opération longue reçoit un `CancelToken`.

### 6.4 Modèle de données de la grille

- `ResultModel` implémente `gio::ListModel` avec **chargement par pages** et un cache LRU.
- Les cellules sont rendues via `GtkColumnView` + factories réutilisant les widgets (recyclage).
- Types affichés avec rendu adapté : NULL (style distinct), nombres alignés à droite, booléens, dates, JSON (aperçu), binaire (taille + aperçu hex), texte long tronqué.

---

## 7. Spécifications fonctionnelles

Priorités : **M** = obligatoire v1, **S** = souhaitable v1 (si le budget le permet), **C** = peut attendre v1.x.

### 7.1 Gestion des connexions

| ID | Exigence | Prio |
|---|---|---|
| CON-1 | Créer, modifier, dupliquer, supprimer une connexion (nom, type, hôte, port, base, utilisateur, couleur d'étiquette) | M |
| CON-2 | Mot de passe stocké dans le trousseau (libsecret) ; option « demander à chaque fois » | M |
| CON-3 | Bouton « Tester la connexion » avec message d'erreur clair et durée | M |
| CON-4 | TLS : modes disable / prefer / require / verify-ca / verify-full, sélection des certificats CA/client | M |
| CON-5 | Connexion via **socket Unix** (PostgreSQL et MySQL) | M |
| CON-6 | Import d'une URL de connexion (`postgres://…`, `mysql://…`) | S |
| CON-7 | Dossiers/groupes de connexions et recherche | S |
| CON-8 | Étiquette d'environnement (dev / staging / prod) avec couleur ; **prod = confirmation avant requêtes d'écriture** | M |
| CON-9 | Import des connexions depuis DBeaver (`data-sources.json`, sans les mots de passe) | C |
| CON-10 | Reconnexion automatique après coupure, avec indicateur d'état | M |
| CON-11 | Fermeture automatique des connexions inactives (délai configurable) | M |
| CON-12 | Mode lecture seule par connexion (transaction `READ ONLY` / garde côté client) | M |

### 7.2 Navigateur de schéma (arbre)

| ID | Exigence | Prio |
|---|---|---|
| NAV-1 | Arbre : connexion > base > schéma > (tables, vues, vues matérialisées, séquences, fonctions/procédures) > colonnes / index / contraintes / déclencheurs | M |
| NAV-2 | Chargement **paresseux** et asynchrone avec indicateur de chargement par nœud | M |
| NAV-3 | Filtre/recherche instantanée dans l'arbre (sur les nœuds déjà chargés + option de recherche serveur) | M |
| NAV-4 | Menu contextuel : ouvrir les données, nouvelle requête, copier le nom, copier le DDL, rafraîchir | M |
| NAV-5 | Affichage du DDL d'un objet (lecture seule, coloré) | M |
| NAV-6 | Propriétés d'une table : colonnes (type, nullable, défaut, commentaire), PK/FK, index, taille estimée, nombre de lignes estimé | M |
| NAV-7 | Navigation par clé étrangère (« voir la ligne référencée ») depuis la grille | S |
| NAV-8 | Création / modification / suppression d'objets via assistants (CREATE TABLE, ALTER…) | C |

### 7.3 Éditeur SQL

| ID | Exigence | Prio |
|---|---|---|
| SQL-1 | Onglets multiples, chacun lié à une connexion (changeable), titre éditable, état « modifié » | M |
| SQL-2 | Coloration syntaxique par dialecte (PostgreSQL / MySQL) | M |
| SQL-3 | Exécution : sélection, instruction sous le curseur, script complet (`Ctrl+Entrée`, `Ctrl+Maj+Entrée`, `F5`) | M |
| SQL-4 | Découpage correct des scripts multi-instructions (gestion des chaînes, commentaires, `$$` PostgreSQL, `DELIMITER` MySQL) | M |
| SQL-5 | **Auto-complétion contextuelle** : mots-clés, schémas, tables, colonnes (selon alias et `FROM`), fonctions ; déclenchée à la demande et à la frappe avec délai | M |
| SQL-6 | Formatage SQL (raccourci, configurable : casse des mots-clés, indentation) | M |
| SQL-7 | Annulation d'une requête en cours (bouton + `Échap`) | M |
| SQL-8 | Affichage du temps d'exécution, du nombre de lignes, des avertissements serveur | M |
| SQL-9 | Résultats multiples (plusieurs instructions → plusieurs sous-onglets de résultat) | M |
| SQL-10 | `EXPLAIN` / `EXPLAIN ANALYZE` avec rendu en arbre lisible (texte structuré, pas de moteur graphique lourd) | S |
| SQL-11 | Paramètres nommés (`:nom`, `$1`) avec boîte de saisie | S |
| SQL-12 | Mode transaction : auto-commit (défaut) ou manuel avec boutons Commit/Rollback et indicateur « transaction ouverte » visible | M |
| SQL-13 | Recherche/remplacement, aller à la ligne, commenter/décommenter, multi-curseurs basiques | M |
| SQL-14 | Sauvegarde automatique des onglets (restauration à la réouverture) | M |
| SQL-15 | Snippets (modèles réutilisables avec variables) | S |
| SQL-16 | Garde-fou : avertir avant `DELETE`/`UPDATE` sans `WHERE`, `DROP`, `TRUNCATE` | M |

### 7.4 Grille de résultats et édition des données

| ID | Exigence | Prio |
|---|---|---|
| GRD-1 | Affichage virtualisé, défilement fluide jusqu'à plusieurs millions de lignes (chargement par pages) | M |
| GRD-2 | Tri (côté serveur pour une table complète, côté client pour un résultat borné), redimensionnement/réordonnancement/masquage des colonnes | M |
| GRD-3 | Filtre rapide par colonne (génère un `WHERE` pour l'ouverture de table) | M |
| GRD-4 | Sélection de cellules/lignes/colonnes ; copie en TSV/CSV/JSON/SQL INSERT | M |
| GRD-5 | **Édition en place** (tables avec clé primaire) : modification, insertion, suppression de lignes ; modifications en attente visibles ; Appliquer / Annuler | M |
| GRD-6 | Les écritures sont exécutées dans une transaction avec requêtes paramétrées ; échec = rollback et message explicite | M |
| GRD-7 | Édition refusée (grille en lecture seule) pour les résultats sans clé primaire identifiable ou issus de jointures, avec explication | M |
| GRD-8 | Éditeur de valeur pour gros texte / JSON / binaire (fenêtre dédiée, chargement à la demande) | M |
| GRD-9 | Rendu distinct de NULL, vide, booléens, dates/heures avec fuseau | M |
| GRD-10 | Mode « vue enregistrement » (une ligne en colonnes verticales) | S |
| GRD-11 | Aperçu des SQL générés avant application des modifications | M |
| GRD-12 | Recherche dans les résultats déjà chargés | S |

### 7.5 Export

| ID | Exigence | Prio |
|---|---|---|
| EXP-1 | Export d'un résultat ou d'une table entière en **CSV, TSV, JSON (array et JSON Lines), SQL INSERT** | M |
| EXP-2 | **Streaming** : l'export d'une table de 100 M de lignes ne dépasse pas ~50 Mo de RAM supplémentaires | M |
| EXP-3 | Options : séparateur, guillemets, encodage, en-têtes, format de date, NULL | M |
| EXP-4 | Barre de progression, annulation, ouverture du dossier à la fin | M |
| EXP-5 | Export vers XLSX / Parquet | C |
| EXP-6 | Copie du résultat dans le presse-papiers | M |

### 7.6 Historique et favoris

| ID | Exigence | Prio |
|---|---|---|
| HIS-1 | Historique des requêtes exécutées (texte, connexion, date, durée, statut) dans SQLite interne | M |
| HIS-2 | Recherche plein texte dans l'historique (FTS5) | M |
| HIS-3 | Rétention configurable (nombre d'entrées / durée) et purge | M |
| HIS-4 | Option pour **ne pas** enregistrer l'historique d'une connexion (ex. prod) | M |
| HIS-5 | Favoris / snippets nommés avec tags | S |

### 7.7 Divers

| ID | Exigence | Prio |
|---|---|---|
| MIS-1 | Thème clair/sombre/automatique (suit le système via libadwaita) | M |
| MIS-2 | Raccourcis clavier configurables, palette de commandes (`Ctrl+K`) | S |
| MIS-3 | Moniteur de session : processus en cours (`pg_stat_activity` / `SHOW PROCESSLIST`) avec kill | S |
| MIS-4 | Page « À propos », vérification des dépendances, fenêtre de diagnostic mémoire (RAM courante de l'app) | S |
| MIS-5 | Ouverture directe : `strata --connect <nom>` / `strata postgres://…` | S |

---

## 8. Spécifications d'interface (UX)

- **Disposition** : barre latérale gauche redimensionnable/repliable (arbre) ; zone centrale à onglets (requêtes, tables, DDL) ; sous chaque éditeur, panneau de résultats redimensionnable ; barre d'état (connexion active, statut transaction, durée, lignes, RAM de l'app).
- Respect des **GNOME HIG** et de libadwaita (`AdwApplicationWindow`, `AdwTabView`, `AdwToastOverlay`, `AdwNavigationSplitView`).
- **Toutes les opérations longues sont non bloquantes** et annulables, avec retour visuel (spinner, progression).
- Messages d'erreur : cause lisible + détail technique dépliable (code SQLSTATE, position dans la requête avec soulignement dans l'éditeur).
- **Accessibilité** : navigation clavier complète, rôles ARIA/AT-SPI fournis par GTK, contraste suffisant, pas d'information portée par la couleur seule.
- **Densité** : mode compact pour la grille (option).
- Premier lancement : écran d'accueil minimal proposant « Nouvelle connexion ».
- Indicateur visuel fort pour les connexions **prod** (bandeau coloré) et pour une **transaction ouverte**.

---

## 9. Internationalisation

- Langues : **français et anglais**, anglais comme langue source.
- Toutes les chaînes via gettext (ou fluent) dès le premier écran ; pas de texte en dur.
- Formats de dates/nombres selon la locale pour l'affichage uniquement (jamais pour l'export, sauf option).

---

## 10. Sécurité et confidentialité

1. Mots de passe **uniquement** dans le Secret Service (libsecret). Si indisponible : refuser de les stocker et proposer « demander à chaque fois », avec message explicite.
2. TLS par défaut en `prefer` ; avertissement visible si la connexion est non chiffrée vers un hôte non local.
3. Requêtes d'édition de grille **toujours paramétrées** ; identifiants quotés via le `Dialect`.
4. **Aucune télémétrie**, aucun appel réseau autre que vers les bases configurées.
5. Les logs ne contiennent ni mots de passe ni valeurs de données ; les requêtes SQL ne sont loguées qu'au niveau `trace` et jamais par défaut.
6. Le fichier SQLite interne est en permissions `0600` dans `$XDG_DATA_HOME/strata/`.
7. Presse-papiers : pas de persistance côté Strata.
8. Sandbox Flatpak avec permissions minimales (`--share=network`, accès `org.freedesktop.secrets`, accès fichiers via portails).
9. Audit des dépendances en CI (`cargo audit`, `cargo deny`).

---

## 11. Stockage interne et configuration

- Emplacements : `$XDG_CONFIG_HOME/strata/` (préférences) et `$XDG_DATA_HOME/strata/strata.db` (SQLite).
- Préférences via **GSettings** (schéma `org.strata.Strata`) pour l'intégration GNOME, ou fichier TOML si GSettings est jugé trop contraignant (à trancher dans une ADR).
- Schéma SQLite versionné avec migrations (`user_version`) :

| Table | Contenu |
|---|---|
| `connections` | id, nom, type, hôte, port, base, utilisateur, options TLS, couleur, environnement, groupe, lecture seule, `secret_ref` |
| `groups` | id, nom, parent |
| `history` | id, connection_id, sql, exécuté_le, durée_ms, statut, lignes, erreur |
| `history_fts` | index FTS5 sur `sql` |
| `snippets` | id, nom, corps, tags |
| `tabs_state` | contenu des onglets non enregistrés, position du curseur |
| `meta` | version de schéma |

---

## 12. Qualité, tests et CI

### Tests
- **Unitaires** : parsing/découpage SQL, quoting, dialectes, modèle de pagination, export.
- **Intégration** : `testcontainers` avec PostgreSQL (14/15/16/17) et MySQL (8.x) + MariaDB (10.11/11.x) : connexion, métadonnées, exécution, streaming, annulation, édition de grille, TLS.
- **UI** : tests de logique de présentation séparés des widgets ; smoke test GTK en environnement headless (`xvfb`/`weston`) pour vérifier le démarrage.
- **Performance/mémoire** : un banc de test (`cargo xtask bench-mem`) qui exécute les scénarios de la section 5, mesure la PSS et produit un rapport JSON comparé à la référence ; échec de la CI en cas de dépassement.
- **Propriétés** (`proptest`) sur le découpeur de scripts SQL.

### CI (GitHub Actions ou équivalent)
`cargo fmt --check` → `cargo clippy -D warnings` → `cargo test` → `cargo deny/audit` → build release → bench mémoire → build Flatpak.

### Qualité de code
- Pas de `unwrap()`/`expect()` hors tests et initialisation.
- Documentation `///` sur toute API publique des crates.
- Couverture visée ≥ 70 % sur `strata-core`, `strata-db`, `strata-sql`, `strata-export`.

---

## 13. Packaging et distribution

| Format | Priorité |
|---|---|
| **Flatpak** (Flathub) | M |
| Paquet `.deb` | S |
| AppImage | C |
| AUR (PKGBUILD) | C |

- Fichiers `.desktop`, métadonnées **AppStream**, icône SVG + symbolique, association de schémas d'URL `postgres://` / `mysql://`.
- Versionnage SemVer, changelog tenu (`CHANGELOG.md`, Keep a Changelog).
- Licence : **GPL-3.0-or-later** (hypothèse, section 17).

---

## 14. Feuille de route (jalons)

| Jalon | Contenu | Critère de sortie |
|---|---|---|
| **J0 — Cadrage** | `ARCHITECTURE.md`, ADR, squelette du workspace, CI, bench mémoire à vide | Validé par l'utilisateur ; fenêtre vide ≤ 40 Mo |
| **J1 — Fondations** | Stockage SQLite, secrets, gestion des connexions (CON-1 à CON-5, CON-8, CON-12), test de connexion PostgreSQL | Création et test de connexion OK, mot de passe dans le trousseau |
| **J2 — Exploration** | Backend PostgreSQL complet (métadonnées), arbre paresseux, propriétés/DDL | NAV-1 à NAV-6 sur PostgreSQL, budget RAM atteint sur 500 tables |
| **J3 — Requêtes** | Éditeur, exécution en streaming, grille virtualisée, annulation, multi-résultats, transactions | SQL-1 à SQL-9, SQL-12, GRD-1/2/4/9 ; budget 1 M lignes tenu |
| **J4 — MySQL/MariaDB** | Backend MySQL, dialecte, tests d'intégration croisés | Parité fonctionnelle J2+J3 sur MySQL/MariaDB |
| **J5 — Édition de données** | Grille éditable, aperçu SQL, transactions, gardes-fous | GRD-5 à GRD-8, GRD-11, SQL-16 |
| **J6 — Confort** | Auto-complétion, formatage, historique FTS, snippets, thème, raccourcis | SQL-5/6/13/14, HIS-1 à HIS-4 |
| **J7 — Export** | Export streaming CSV/JSON/SQL | EXP-1 à EXP-4, EXP-6 |
| **J8 — Finitions v1** | i18n FR/EN, accessibilité, packaging Flatpak/deb, doc utilisateur, passe de perf | Tous les critères section 16 |

---

## 15. Hors périmètre v1 (pistes v2)

SQLite / SQL Server / Oracle / DuckDB comme cibles ; diagramme ER ; import de données (CSV → table) ; comparaison de schémas et génération de migrations ; tunnel SSH ; éditeur visuel de requêtes ; moniteur de performance avancé ; plugins ; collaboration ; import des connexions DBeaver (CON-9) ; export XLSX/Parquet.

---

## 16. Critères d'acceptation globaux de la v1

- [ ] Toutes les exigences **M** des sections 7.1 à 7.7 sont satisfaites.
- [ ] Les budgets mémoire et temps de la section 5 sont respectés et vérifiés par le banc automatisé.
- [ ] Aucun bloc de plus de 100 ms sur le thread principal pendant les scénarios de test.
- [ ] Tests d'intégration verts sur PostgreSQL 14 à 17, MySQL 8.x, MariaDB 10.11 et 11.x.
- [ ] Aucun mot de passe en clair sur disque ni dans les logs (vérifié par test).
- [ ] Application installable en Flatpak, interface complète en français et en anglais.
- [ ] Navigation au clavier complète, thème clair/sombre fonctionnels.
- [ ] Documentation : README, guide utilisateur, `ARCHITECTURE.md`, ADR à jour.
- [ ] `clippy` sans avertissement, `cargo audit` sans vulnérabilité connue non traitée.

---

## 17. Hypothèses prises et questions ouvertes

### Hypothèses (à contester si besoin)
1. **Licence GPL-3.0-or-later**, projet open source. *(Alternative : MIT/Apache-2.0 si vous voulez une adoption plus large ou un usage commercial.)*
2. Cible **GNOME / Wayland** en priorité ; fonctionnement attendu sous KDE et X11 mais sans optimisation spécifique.
3. Distributions de référence : Debian/Ubuntu récentes, Fedora, Arch ; **GTK ≥ 4.12** et **libadwaita ≥ 1.4**.
4. Une connexion « métadonnées » par serveur connecté, plus une connexion dédiée par onglet de requête actif.
5. Le nom **Strata** est retenu ; sa disponibilité reste à vérifier.
6. SQLite est utilisé **uniquement en interne** en v1 (pas comme cible utilisateur).
7. Préférences : GSettings par défaut ; repli sur TOML si cela complique le packaging Flatpak.
8. Pas de support de la lecture de fichiers de configuration externes (`~/.pgpass`, `~/.my.cnf`) en v1, sauf si le temps le permet (à décider).

### Questions ouvertes à poser à l'utilisateur pendant J0
1. Faut-il lire `~/.pgpass`, `pg_service.conf` et `~/.my.cnf` pour pré-remplir les connexions ?
2. Souhaitez-vous un **mode « multi-fenêtres »** (une fenêtre par connexion) ou une fenêtre unique à onglets ?
3. Quel niveau de support pour les **types spéciaux** PostgreSQL (tableaux, JSONB, hstore, géométrie/PostGIS, enum, domaines) : affichage brut, ou rendu enrichi ?
4. Faut-il une **API en ligne de commande** (exécuter un script et exporter sans UI) pour réutiliser les crates en CLI ?
5. Le projet sera-t-il public dès le début (nom de dépôt, organisation GitHub, charte de contribution) ?
6. Le logo et l'identité visuelle sont-ils à produire (icône SVG minimaliste, symbolique) ?
7. Préférez-vous `tokio-postgres`/`mysql_async` directs ou `sqlx` (plus simple mais un peu plus lourd et moins de contrôle sur le streaming et l'annulation) ? *(À trancher par ADR avec mesures.)*

---

## 18. Glossaire

- **PSS** : Proportional Set Size, mesure de mémoire qui répartit équitablement les pages partagées entre processus ; la plus fidèle pour comparer des applications GTK.
- **Keyset pagination** : pagination basée sur la dernière valeur de clé plutôt que `OFFSET`, plus performante sur de grandes tables.
- **ADR** : Architecture Decision Record, courte note documentant une décision technique et ses alternatives.
- **Virtualisation (grille)** : ne créer des widgets que pour les lignes visibles et les recycler au défilement.
