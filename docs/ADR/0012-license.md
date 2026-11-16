# ADR-0012 — Licence GPL-3.0-or-later

**Statut** : Proposé (hypothèse 1 du CDC)

## Décision
GPL-3.0-or-later, fichier `LICENSE`, en-tête SPDX `// SPDX-License-Identifier: GPL-3.0-or-later` dans chaque fichier source. `cargo deny` vérifie la compatibilité des licences des dépendances (MIT, Apache-2.0, BSD, MPL-2.0, Zlib, Unicode, LGPL pour GTK).

## Alternative
MIT/Apache-2.0 si une adoption commerciale plus large est souhaitée (question à confirmer).
