# Architecture Decision Records

Format : *Contexte → Décision → Alternatives → Conséquences*. Statuts : `Proposé`, `Accepté`, `À mesurer` (décision provisoire confirmée ou infirmée par mesure), `Remplacé par ADR-XXXX`.

| N° | Titre | Statut |
|---|---|---|
| [0001](0001-ui-toolkit-gtk4-libadwaita.md) | Toolkit UI : GTK4 + libadwaita + GtkSourceView 5 | Proposé |
| [0002](0002-database-drivers.md) | Drivers : tokio-postgres et mysql_async directs, trait Driver/Session | Proposé |
| [0003](0003-async-model.md) | Modèle async : Tokio 2 workers, pont GLib par canaux typés | Proposé |
| [0004](0004-secrets-storage.md) | Secrets : Secret Service via `oo7` | Proposé |
| [0005](0005-allocator.md) | Allocateur : mimalloc vs glibc | À mesurer |
| [0006](0006-preferences-storage.md) | Préférences : GSettings | Proposé |
| [0007](0007-i18n.md) | i18n : gettext (`gettext-rs`) | Proposé |
| [0008](0008-result-paging-and-spill.md) | Résultats : lecture à la demande, LRU, fichier de débordement | Proposé |
| [0009](0009-internal-storage-sqlite.md) | Stockage interne : rusqlite + migrations `user_version` | Proposé |
| [0010](0010-sql-splitting-and-completion.md) | Découpage SQL maison + sqlparser-rs pour la complétion/le formatage | Proposé |
| [0011](0011-release-profile.md) | Profil release et opt-level | À mesurer |
| [0012](0012-license.md) | Licence GPL-3.0-or-later | Proposé |
