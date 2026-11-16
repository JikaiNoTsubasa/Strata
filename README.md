# Strata

Lightweight, native database client for Linux (GTK4 + libadwaita), for PostgreSQL and MySQL/MariaDB.
*Client de bases de données natif et léger pour Linux (GTK4 + libadwaita), pour PostgreSQL et MySQL/MariaDB.*

> **Status / État** : early development — connections and schema browsing work (PostgreSQL); queries come next.
> Développement en cours — connexions et exploration du schéma (PostgreSQL) ; les requêtes arrivent ensuite.

## Run from source / Lancer depuis les sources

### 1. Requirements / Prérequis

GTK ≥ 4.12, libadwaita ≥ 1.4 and a stable Rust toolchain.
*GTK ≥ 4.12, libadwaita ≥ 1.4 et une chaîne Rust stable.*

```sh
# Debian / Ubuntu
sudo apt install build-essential pkg-config gettext libglib2.0-dev-bin libgtk-4-dev libadwaita-1-dev libgtksourceview-5-dev
# Fedora
sudo dnf install gcc pkgconf-pkg-config gettext glib2-devel gtk4-devel libadwaita-devel gtksourceview5-devel
# Arch Linux
sudo pacman -S --needed base-devel pkgconf gettext gtk4 libadwaita gtksourceview5

# Rust (https://rustup.rs), then open a new terminal or run: source ~/.cargo/env
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

### 2. Run / Lancer

```sh
git clone https://github.com/JikaiNoTsubasa/Strata.git
cd Strata
cargo run --release       # optimized build (recommended) / version optimisée (recommandée)
cargo run                 # debug build, faster to compile / version de débogage, compile plus vite
```

The binary is then available at `target/release/strata`.
*Le binaire se trouve ensuite dans `target/release/strata`.*

### 3. Useful options / Options utiles

| Command / Commande | Effect / Effet |
|---|---|
| `LANGUAGE=fr cargo run` | Force French / forcer le français |
| `LANGUAGE=en cargo run` | Force English / forcer l'anglais |
| `STRATA_LOG=debug cargo run` | Verbose logs / logs détaillés (`tracing` filter syntax) |
| `GSK_RENDERER=cairo cargo run` | Software rendering, lower memory / rendu logiciel, moins de mémoire |

System installation (desktop menu, Flatpak, `.deb`) is planned for milestone J8.
*L'installation système (menu du bureau, Flatpak, `.deb`) est prévue au jalon J8.*

Contributors: see [`docs/DEVELOPMENT.md`](docs/DEVELOPMENT.md) (tests, lints, memory benchmark, translations).
*Contributeurs : voir [`docs/DEVELOPMENT.md`](docs/DEVELOPMENT.md) (tests, lints, banc mémoire, traductions).*

## Documentation

- Specification / cahier des charges : [`CDC.md`](CDC.md)
- Architecture : [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md), decisions : [`docs/ADR/`](docs/ADR/)
- Roadmap : [`docs/ROADMAP.md`](docs/ROADMAP.md), measurements / mesures : [`docs/perf/`](docs/perf/)
- Development guide / guide de développement : [`docs/DEVELOPMENT.md`](docs/DEVELOPMENT.md)
- Icons / icônes : [`docs/ICONS.md`](docs/ICONS.md)

## License / Licence

[GPL-3.0-or-later](LICENSE)
