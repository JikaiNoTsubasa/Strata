# Guide de développement

> Pour simplement lancer Strata, voir la section « Lancer depuis les sources » du [`README.md`](../README.md).
> Ce guide s'adresse aux personnes qui modifient le code.

---

## 1. Outillage

### Indispensable
Les paquets listés dans le README (GTK4, libadwaita, gettext, pkg-config) et Rust stable via rustup. La version de Rust est épinglée sur `stable` par `rust-toolchain.toml` ; `clippy` et `rustfmt` sont installés automatiquement.

### Recommandé

| Outil | Usage | Installation |
|---|---|---|
| `cargo-deny` | Licences, dépendances interdites (GTK hors UI, OpenSSL, WebKit), avis de sécurité | `cargo install --locked cargo-deny` |
| `cargo-audit` | Vulnérabilités connues | `cargo install --locked cargo-audit` |
| `weston` | Banc mémoire sans affichage (`--display headless`) | `sudo apt install weston` |
| `heaptrack` | Analyse détaillée des allocations | `sudo apt install heaptrack` |
| `desktop-file-utils`, `appstream` | Validation du `.desktop` et de l'AppStream | `sudo apt install desktop-file-utils appstream` |
| `docker` | Tests d'intégration avec de vraies bases (à partir de J1) | selon la distribution |

---

## 2. Organisation du dépôt

```
apps/strata/       binaire : logs, runtime Tokio, lancement de l'interface
crates/strata-*/   bibliothèques (voir docs/ARCHITECTURE.md §2)
crates/strata-ui/resources/   interfaces .ui et CSS, compilées dans une GResource
data/              .desktop, AppStream, schéma GSettings, icônes
po/                traductions (gettext)
xtask/             tâches de développement (cargo xtask …)
bench/             référence du banc mémoire
docs/              architecture, ADR, feuille de route, mesures
```

Règle d'architecture : **seuls `strata-ui` et `apps/strata` dépendent de GTK**. `cargo deny check` échoue si un autre crate en dépend.

---

## 3. Compiler et lancer

```sh
cargo run                    # debug : compilation rapide, exécution plus lente
cargo run --release          # release : profil des mesures (LTO, panic=abort)
cargo build --workspace      # tout compiler sans lancer
```

### Variables d'environnement

| Variable | Effet |
|---|---|
| `STRATA_LOG` | Filtre de logs `tracing`. Ex. : `debug`, `strata_engine=trace`, `warn,strata_ui=debug`. Défaut : `warn,strata=info,strata_ui=info,strata_engine=info`. Les requêtes SQL ne sont jamais loguées par défaut. |
| `LANGUAGE` | Langue de l'interface (`fr`, `en`). |
| `GSK_RENDERER` | Moteur de rendu GTK : `ngl`, `vulkan`, `cairo` (logiciel, moins de mémoire). |
| `GTK_DEBUG=interactive` | Ouvre l'inspecteur GTK (arbre des widgets, CSS en direct). Aussi : `Ctrl+Maj+D` si activé dans GNOME. |
| `STRATA_BENCH_SCENARIO` | Réservé au banc mémoire (lance une instance indépendante et signale quand elle est prête). |

### Ce qui se passe à la compilation
Le `build.rs` de `strata-ui` :
- compile les ressources (`resources/*.ui`, `style.css`) dans la GResource intégrée au binaire ;
- compile le schéma GSettings (`data/fr.coregeek.Strata.gschema.xml`) et les traductions (`po/*.po`) dans le répertoire de build, ce qui permet de lancer l'application **sans l'installer**.

### Préférences enregistrées
Les préférences (taille de fenêtre…) sont stockées par dconf sous `/fr/coregeek/Strata/`. Pour repartir de zéro :
```sh
dconf reset -f /fr/coregeek/Strata/
```

### Instance unique
Strata n'ouvre qu'une fenêtre (ADR-0015). Relancer `cargo run` alors qu'une instance tourne **réactive l'ancienne** au lieu d'exécuter le nouveau code. Fermez d'abord la fenêtre existante.

---

## 4. Vérifications avant de proposer une modification

Ce sont les mêmes étapes que la CI (`.github/workflows/ci.yml`) :

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo deny check
cargo audit
```

Règles de code (vérifiées par les lints du workspace) :
- pas d'`unsafe` ;
- pas d'`unwrap()`/`expect()` hors tests ;
- documentation `///` sur toute API publique ;
- en-tête `// SPDX-License-Identifier: GPL-3.0-or-later` dans chaque fichier source ;
- code, identifiants et commentaires en anglais ; commits au format Conventional Commits (`feat:`, `fix:`, `perf:`, `docs:`…).

**Dépôt public** : ne jamais committer de mot de passe, de jeton, de nom d'hôte ou de base réels, ni de fichier `.db` / `.env` (voir `.gitignore`).

---

## 5. Banc mémoire

```sh
cargo xtask bench-mem                              # scénario « à vide », session courante
cargo xtask bench-mem --display headless --renderer cairo   # comme la CI
cargo xtask bench-mem --runs 5 --mimalloc          # comparer avec l'allocateur mimalloc
cargo xtask bench-mem --profile release-small      # comparer avec opt-level = "s"
cargo xtask bench-mem --update-baseline            # enregistrer la nouvelle référence
cargo xtask --help                                 # toutes les options
```

- Mesure la **PSS** (`/proc/<pid>/smaps_rollup`) et le temps de démarrage ; rapport JSON dans `target/bench-mem/`.
- Échoue si la mémoire dépasse la référence (`bench/baseline.json`) de plus de 10 %. Les objectifs du cahier des charges sont affichés à titre indicatif (ADR-0013).
- La référence dépend de l'affichage et du moteur de rendu (clé `scénario/affichage/renderer`) : comparer des mesures `session` et `headless` n'a pas de sens.
- Ne mettre à jour la référence (`--update-baseline`) qu'en connaissance de cause, et consigner les mesures du jalon dans `docs/perf/`.

---

## 6. Traductions

Langue source : anglais. Langue fournie : français (`po/fr.po`).

**Ajouter une chaîne traduisible**
- Dans le code Rust : `tr("Texte")` (module `i18n` de `strata-ui`). Jamais de texte affiché en dur.
- Dans un fichier `.ui` : `<property name="label" translatable="yes">Texte</property>`.
- Si c'est un nouveau fichier, l'ajouter à `po/POTFILES.in`.

**Mettre à jour les catalogues**
```sh
cargo xtask i18n         # régénère po/strata.pot et fusionne dans po/*.po
```
Puis traduire les nouvelles entrées (vides ou marquées `fuzzy`) dans `po/fr.po`, par exemple avec Poedit ou Gtranslator, et vérifier :
```sh
msgfmt --check --statistics -o /dev/null po/fr.po
```

**Ajouter une langue** : ajouter son code dans `po/LINGUAS` puis lancer `cargo xtask i18n` (crée `po/<langue>.po`).

---

## 7. Interface

- Les interfaces sont décrites en XML GtkBuilder dans `crates/strata-ui/resources/ui/` et déclarées dans `resources.gresource.xml`.
- Les widgets composites utilisent `#[derive(gtk::CompositeTemplate)]` (voir `window.rs`).
- Respecter les [GNOME HIG](https://developer.gnome.org/hig/) et les widgets libadwaita.
- Le thread principal ne fait **jamais** d'I/O réseau ni de calcul lourd : envoyer une `Command` au moteur et réagir aux `UiEvent` (ADR-0003).

---

## 8. Icônes

Voir [`docs/ICONS.md`](ICONS.md).
