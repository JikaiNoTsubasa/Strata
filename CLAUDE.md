# Strata — Instructions pour Claude Code

## Git
- **L'utilisateur s'occupe lui-même des commits.** Ne jamais lancer de commande git qui modifie l'historique ou les branches (commit, push, merge, rebase, reset, checkout/création de branche, stash, tag) sans demande explicite. Les commandes en lecture (`status`, `diff`, `log`) sont autorisées.
- En fin de tâche, signaler les fichiers modifiés et proposer un message au format Conventional Commits ; ne pas l'exécuter.
- Cette règle prime sur la section 0.6 de `CDC.md`.

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
