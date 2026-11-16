# ADR-0012 — Licence GPL-3.0-or-later

**Statut** : Accepté (16/11/2026) — projet open source public

## Décision
GPL-3.0-or-later, fichier `LICENSE`, en-tête SPDX `// SPDX-License-Identifier: GPL-3.0-or-later` dans chaque fichier source. `cargo deny` vérifie la compatibilité des licences des dépendances (MIT, Apache-2.0, BSD, MPL-2.0, Zlib, Unicode, LGPL pour GTK).

## Pourquoi GPL plutôt que MIT/Apache
- **GPL-3.0** : n'importe qui peut utiliser, modifier et redistribuer Strata, mais toute version modifiée distribuée doit rester open source sous la même licence. Protège le projet contre une reprise fermée.
- **MIT/Apache-2.0** (permissives) : autorisent aussi l'intégration dans des produits propriétaires fermés.
- Pour une application de bureau (et non une bibliothèque destinée à être intégrée), la GPL est le choix habituel de l'écosystème GNOME. Changer de licence plus tard nécessite l'accord de tous les contributeurs : choix à faire maintenant.
