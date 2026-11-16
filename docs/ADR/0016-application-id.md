# ADR-0016 — Identifiant d'application : `fr.coregeek.Strata`

**Statut** : Accepté (16/11/2026) — remplace `org.strata.Strata` cité dans le CDC

## Contexte
Flathub exige un identifiant en notation DNS inversée correspondant à un domaine **contrôlé** par l'auteur. `strata.org` n'appartient pas au projet ; l'auteur détient `coregeek.fr`.

## Décision
- Identifiant : **`fr.coregeek.Strata`** (dernier composant en CamelCase, convention GNOME/Flathub ; les composants respectent les règles des noms de bus D-Bus).
- Défini **à un seul endroit** (`APP_ID` dans `apps/strata/build.rs`) et propagé partout :

| Élément | Valeur |
|---|---|
| `GApplication` / nom D-Bus | `fr.coregeek.Strata` |
| Schéma GSettings | id `fr.coregeek.Strata`, chemin `/fr/coregeek/Strata/` |
| Ressources GIO | préfixe `/fr/coregeek/Strata/` |
| `.desktop` | `data/fr.coregeek.Strata.desktop` |
| AppStream | `data/fr.coregeek.Strata.metainfo.xml` |
| Icônes | `fr.coregeek.Strata.svg`, `fr.coregeek.Strata-symbolic.svg` |
| Trousseau | attribut `application=fr.coregeek.Strata` |
| Build de développement | `fr.coregeek.Strata.Devel` (installable en parallèle) |

- **Ne plus changer après la première publication** : le changement casserait les préférences, les éléments du trousseau et les mises à jour Flathub des utilisateurs.

## Vérification Flathub (au moment de la publication, J8)
Flathub fournit un jeton à publier sur `https://coregeek.fr/.well-known/org.flathub.VerifiedApps.txt`. Le domaine doit donc rester actif et servi en HTTPS.
