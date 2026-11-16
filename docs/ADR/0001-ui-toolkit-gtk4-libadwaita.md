# ADR-0001 — Toolkit UI : GTK4 + libadwaita + GtkSourceView 5

**Statut** : Proposé (imposé par le CDC §4)

## Contexte
Application native Linux, intégration GNOME, sans moteur web, budget RAM serré.

## Décision
- `gtk4` (feature `v4_12`), `libadwaita` (feature `v1_4`), `sourceview5` via les bindings gtk-rs.
- Interfaces en fichiers `.ui` (XML GtkBuilder) compilés dans une `gio::Resource` à la compilation (`glib-build-tools`). Blueprint écarté : ajoute un outil de build (`blueprint-compiler`) non disponible partout.
- Widgets principaux : `AdwApplicationWindow`, `AdwOverlaySplitView` (barre latérale repliable — mieux adapté qu'`AdwNavigationSplitView` pour une barre latérale redimensionnable sur bureau), `AdwTabView`, `AdwToastOverlay`, `AdwBanner`, `GtkColumnView`, `GtkTreeListModel`.
- Sous-classes GObject (`glib::subclass`) uniquement là où c'est nécessaire (`ResultModel`, objets d'items, widgets composites).

## Alternatives
- **iced / egui / Slint** : pas d'intégration GNOME/AT-SPI équivalente ; egui redessine en continu (CPU) ; écarté par le CDC.
- **Relm4** : surcouche élégante mais ajoute une abstraction et du code générique ; gtk-rs direct garde le contrôle sur la mémoire et le recyclage.

## Conséquences
- **Risque mémoire majeur** : le renderer GPU de GTK4 (`ngl`, `vulkan` par défaut en GTK ≥ 4.16) charge les pilotes Mesa (≈ 20–60 Mo PSS). Le critère J0 « fenêtre vide ≤ 40 Mo » devra être mesuré avec chaque renderer ; le résultat sera consigné ici et le budget éventuellement renégocié avec l'utilisateur.
- Versions minimales GTK 4.12 / libadwaita 1.4 (hypothèse 3) : fonctionnalités plus récentes (ex. `AdwToolbarView` 1.4 OK, `AdwDialog` 1.5) uniquement derrière détection de version ou évitées.
