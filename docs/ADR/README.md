# Architecture Decision Records

Format : *Contexte → Décision → Alternatives → Conséquences*. Statuts : `Proposé`, `Accepté`, `À mesurer` (décision provisoire confirmée ou infirmée par mesure), `Remplacé par ADR-XXXX`.

| N° | Titre | Statut |
|---|---|---|
| [0001](0001-ui-toolkit-gtk4-libadwaita.md) | Toolkit UI : GTK4 + libadwaita + GtkSourceView 5 | Accepté |
| [0002](0002-database-drivers.md) | Drivers : tokio-postgres et mysql_async directs, trait Driver/Session | Accepté |
| [0003](0003-async-model.md) | Modèle async : Tokio 2 workers, pont GLib par canaux typés | Accepté |
| [0004](0004-secrets-storage.md) | Secrets : Secret Service via `oo7` | Accepté |
| [0005](0005-allocator.md) | Allocateur : mimalloc vs glibc | À mesurer |
| [0006](0006-preferences-storage.md) | Préférences : GSettings | Accepté |
| [0007](0007-i18n.md) | i18n : gettext (`gettext-rs`) | Accepté |
| [0008](0008-result-paging-and-spill.md) | Résultats : lecture à la demande, LRU, fichier de débordement | Accepté |
| [0009](0009-internal-storage-sqlite.md) | Stockage interne : rusqlite + migrations `user_version` | Accepté |
| [0010](0010-sql-splitting-and-completion.md) | Découpage SQL maison + sqlparser-rs pour la complétion/le formatage | Accepté |
| [0011](0011-release-profile.md) | Profil release et opt-level | À mesurer |
| [0012](0012-license.md) | Licence GPL-3.0-or-later | Accepté |
| [0013](0013-memory-budgets-policy.md) | Budgets mémoire : objectifs, pas de sacrifice fonctionnel | Accepté |
| [0014](0014-external-client-config-files.md) | Lecture de `~/.pgpass`, `pg_service.conf`, `~/.my.cnf` | Accepté |
| [0015](0015-single-window.md) | Fenêtre unique à onglets | Accepté |
| [0016](0016-application-id.md) | Identifiant d’application `fr.coregeek.Strata` | Accepté |
