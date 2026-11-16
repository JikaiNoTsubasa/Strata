# Réponses aux questions ouvertes (CDC §17) — 16/11/2026

| Question | Réponse | Traduit dans |
|---|---|---|
| A. Débordement des pages de résultats sur disque | Oui | ADR-0008 |
| B. Budget mémoire J0 (40 Mo) | Le dépassement est acceptable ; minimiser sans réduire les fonctionnalités | ADR-0013 |
| 1. Lire `~/.pgpass`, `pg_service.conf`, `~/.my.cnf` | Oui | ADR-0014, J1-6b |
| 2. Fenêtres | Fenêtre unique à onglets | ADR-0015 |
| 3. Types spéciaux PostgreSQL | Affichage brut en v1 | ADR-0002 (mode texte) |
| 4. Mode CLI sans UI | Non | — (MIS-5 `strata --connect` reste : ouvre l'UI) |
| 5. Dépôt public | Oui ; aucune information critique dans le dépôt | `CLAUDE.md`, J0-8 (gitleaks), J0-10 |
| 6. Icône | Fournie par l'utilisateur | `docs/ICONS.md` |
| 7. Drivers | `tokio-postgres` + `mysql_async` directs | ADR-0002 |
| 8. Licence | Open source → GPL-3.0-or-later | ADR-0012 |
| — Identifiant d’application | `fr.coregeek.Strata` (domaine coregeek.fr détenu par l’auteur) | ADR-0016 |
| — Tunnels SSH (v2 dans le CDC) | Ajoutés en v1 à la demande de l'utilisateur | ADR-0017 |
