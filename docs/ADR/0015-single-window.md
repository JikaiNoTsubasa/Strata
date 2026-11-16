# ADR-0015 — Fenêtre unique à onglets

**Statut** : Accepté (question ouverte n°2 du CDC)

## Décision
- Une seule `AdwApplicationWindow` ; toutes les connexions partagent la barre latérale et les onglets sont liés chacun à une connexion.
- `AdwApplication` en instance unique : relancer `strata` (ou ouvrir une URL `postgres://…`) réactive la fenêtre existante et ouvre un onglet.
- Pas de détachement d'onglet dans une nouvelle fenêtre (`AdwTabView::create-window` non géré).

## Conséquences
Un seul arbre de widgets, mémoire minimale ; état de fenêtre (taille, panneaux) persisté via GSettings.
