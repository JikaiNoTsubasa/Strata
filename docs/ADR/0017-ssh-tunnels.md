# ADR-0017 — Tunnels SSH

**Statut** : Accepté (16/11/2026) — demandé par l'utilisateur, avancé de la v2 (CDC §15) à la v1

## Contexte
Beaucoup de bases ne sont joignables que via un bastion SSH. Le CDC classait les tunnels en v2 ; l'utilisateur les veut maintenant.

## Décision
- **Implémentation en processus** avec `russh` (pur Rust, Tokio), et non le binaire `ssh` :
  - le CDC §5.4 interdit les processus annexes ;
  - `ssh` n'est pas disponible dans la sandbox Flatpak ;
  - aucun parsing de sortie de commande.
- **Fonctionnement équivalent à `ssh -L`** : Strata écoute sur `127.0.0.1:<port aléatoire>`. Chaque connexion acceptée est relayée par un canal `direct-tcpip` vers l'hôte et le port de la base, tels que vus depuis le serveur SSH.
  - Le pilote PostgreSQL se connecte au port local (`hostaddr`) mais garde le vrai nom d'hôte pour la vérification TLS.
  - Le même mécanisme servira à MySQL (J4).
- **Une connexion SSH par connexion ouverte**, partagée par toutes ses sessions (une par base) et rouverte automatiquement si elle tombe (CON-10).
- **Authentification** : agent SSH (`SSH_AUTH_SOCK`), mot de passe, ou clé privée avec phrase de passe facultative.
  - Le mot de passe ou la phrase de passe est stocké dans le trousseau (élément distinct, attribut `secret-kind=ssh`), demandé à chaque fois, ou absent.
- **Clés d'hôte** : vérifiées dans `~/.ssh/known_hosts` (lecture seule) et dans `$XDG_DATA_HOME/strata/known_hosts` (0600).
  - Une clé inconnue est présentée avec son empreinte SHA-256 ; l'utilisateur choisit de lui faire confiance (*trust on first use*), et elle est alors enregistrée dans le fichier de Strata.
  - Une **clé modifiée est toujours refusée**, sans possibilité de passer outre depuis l'application.
- **Limites v1** : pas de rebond (`ProxyJump`), pas de lecture de `~/.ssh/config`, pas de certificats d'hôte.

## Conséquences
- Le port local est accessible aux autres processus de l'utilisateur pendant la session, comme avec `ssh -L` ; les identifiants de la base restent nécessaires.
- Flatpak : ajouter `--socket=ssh-auth` (agent) et `--filesystem=~/.ssh:ro` (clés et `known_hosts`).
- Tests : `cargo test -p strata-engine --test ssh -- --include-ignored` (serveur OpenSSH Alpine + PostgreSQL 17 dans Docker).
