# Strata — Instructions pour Claude Code

## Git
- **Ne jamais faire de commit (ni push, merge, rebase, reset ou création de branche) sans l'accord explicite de l'utilisateur.** Préparer les changements, proposer le message de commit au format Conventional Commits, puis attendre la validation.
- Cette règle prime sur la section 0.6 de `CDC.md` (« commits atomiques ») : les commits restent atomiques, mais chacun doit être approuvé.

## Dépôt public — aucune information sensible
Le dépôt est public. Ne jamais y écrire (code, tests, docs, fixtures, messages de commit, CI) :
- mots de passe, jetons, clés, certificats privés, chaînes de connexion réelles ;
- noms d'hôtes, adresses IP, noms de bases ou d'utilisateurs réels ; données personnelles (e-mails, noms) ;
- chemins absolus de la machine de l'utilisateur.
Les tests utilisent uniquement des identifiants générés pour des conteneurs éphémères (testcontainers). Les fichiers locaux (`*.db`, `.env`, rapports de bench locaux) sont dans `.gitignore`.

## Référence
- `CDC.md` fait foi pour le périmètre fonctionnel et technique.
- Architecture et décisions : `docs/ARCHITECTURE.md`, `docs/ADR/`, `docs/ROADMAP.md`, `docs/DECISIONS.md` (réponses de l'utilisateur).
- Budgets mémoire = objectifs, jamais au détriment des fonctionnalités (ADR-0013).
