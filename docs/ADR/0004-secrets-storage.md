# ADR-0004 — Secrets : Secret Service via `oo7`

**Statut** : Proposé

## Contexte
Aucun mot de passe en clair sur disque ; fonctionnement natif et en Flatpak.

## Décision
- Crate **`oo7`** (pur Rust, maintenu par l'équipe GNOME/bilelmoussaoui) : en hôte, utilise le Secret Service D-Bus ; **en sandbox Flatpak, utilise automatiquement le portail Secret** (fichier chiffré par application avec clé fournie par le trousseau), sans permission D-Bus large.
- Attributs de l'élément : `application=org.strata.Strata`, `connection_id=<uuid>`. `connections.secret_ref` stocke l'uuid.
- Trait `SecretStore` (`get/set/delete/is_available`) avec deux implémentations : `KeyringStore` (oo7) et `MemoryStore` (tests, et cache de session pour le mode « demander à chaque fois », vidé à la déconnexion).
- Si le service est indisponible : `set` renvoie `SecretError::Unavailable`, l'UI bascule la connexion en « demander à chaque fois » avec un message explicite.
- `Secret` encapsule `Zeroizing<String>` ; `Debug` affiche `***`.

## Alternatives
- `secret-service` : D-Bus seulement, pas de support du portail Flatpak, nécessite `--talk-name=org.freedesktop.secrets`.
- `keyring` : multiplateforme mais abstraction plus pauvre, même limite Flatpak.

## Conséquences
- En Flatpak, l'accès `org.freedesktop.secrets` n'est même plus nécessaire (portail) — permissions plus faibles que celles listées au CDC §10.8.
- Test d'acceptation : grep du mot de passe dans `strata.db`, les logs et `$XDG_CONFIG_HOME/strata` → aucune occurrence.
