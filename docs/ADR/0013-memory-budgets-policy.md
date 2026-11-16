# ADR-0013 — Budgets mémoire : objectifs, pas de sacrifice fonctionnel

**Statut** : Accepté (décision utilisateur du 16/11/2026)

## Contexte
Le CDC §5 fixe des budgets PSS comme critères d'acceptation. Certains (fenêtre vide ≤ 40 Mo) sont probablement inatteignables avec GTK4 et son rendu GPU sans dégrader l'application.

## Décision
- Les budgets du CDC §5 sont des **objectifs** : on minimise la mémoire autant que possible, mais **jamais au détriment des fonctionnalités ni de la fluidité** (pas de renderer cairo forcé, pas de fonctionnalité M retirée).
- Les règles de conception du CDC §5 (streaming, chargement paresseux, valeurs compactes, onglets allégés…) restent **obligatoires** : ce sont elles qui gardent la mémoire basse.
- Le banc `xtask bench-mem` **bloque la CI sur les régressions** (> 10 % par rapport à la référence mesurée du dernier jalon validé), et affiche l'écart aux objectifs du CDC sans bloquer.
- Chaque jalon consigne ses mesures dans `docs/perf/<jalon>.md` ; tout dépassement d'objectif est signalé à l'utilisateur avec son explication.
