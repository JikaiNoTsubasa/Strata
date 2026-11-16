# ADR-0006 — Préférences : GSettings

**Statut** : Proposé (hypothèse 7 du CDC conservée)

## Contexte
Préférences : thème, délai d'inactivité, rétention de l'historique, densité de la grille, police de l'éditeur, formatage SQL, raccourcis.

## Décision
- Schéma **`org.strata.Strata.gschema.xml`** dans `data/`, installé par Meson/Flatpak.
- **Développement via `cargo run`** : le `build.rs` de `apps/strata` compile le schéma (`glib-compile-schemas`) dans `target/` et le binaire positionne `GSETTINGS_SCHEMA_DIR` uniquement en build debug. Pas de blocage technique identifié.
- Accès via un wrapper typé `Preferences` côté UI ; le moteur reçoit les valeurs utiles (délais, rétention) par `Command::UpdateSettings`, il ne dépend pas de GIO.
- L'arborescence `$XDG_CONFIG_HOME/strata/` reste réservée aux fichiers éventuels (raccourcis personnalisés) ; en Flatpak, GSettings utilise le backend keyfile de la sandbox automatiquement.

## Alternatives
- Fichier TOML : plus simple en dev, mais pas de liaison de propriétés GTK (`settings.bind`) ni d'intégration `dconf`. Repli possible si le packaging pose problème.

## Conséquences
- Dépendance de build à `glib-compile-schemas` (présent avec les en-têtes GLib).
