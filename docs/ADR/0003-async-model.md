# ADR-0003 — Modèle async : Tokio 2 workers, pont GLib par canaux typés

**Statut** : Proposé

## Contexte
Le thread GTK ne doit jamais bloquer (> 100 ms interdit, §16) ; les drivers sont Tokio-natifs.

## Décision
- Runtime `tokio::runtime::Builder::new_multi_thread().worker_threads(2).max_blocking_threads(4).thread_stack_size(1 MiB)`, créé dans `apps/strata` avant GTK, exposé via un `Handle`.
- Pont : `async_channel::bounded::<Command>(256)` UI → moteur ; `async_channel::bounded::<UiEvent>(1024)` moteur → UI, consommé par une tâche `glib::MainContext::default().spawn_local`.
- **Coalescing** côté moteur : un acteur « EventSink » regroupe les événements de progression par clé (résultat, export) et les émet au plus toutes les 16 ms.
- Le moteur est structuré en **acteurs** (une tâche par session, un `SessionManager`, un `Exporter`) communiquant par canaux ; aucun `Mutex` partagé avec l'UI.
- Annulation : `tokio_util::sync::CancellationToken` partagé ; la tâche de session fait un `select!` entre le flux et le jeton.
- I/O SQLite et fichiers : `spawn_blocking`.

## Alternatives
- `glib::MainContext` comme exécuteur pour les drivers : impossible (drivers liés à Tokio) et risque de bloquer l'UI.
- Runtime `current_thread` dédié : plus léger mais un export lourd retarderait l'annulation des requêtes ; 2 workers est le compromis imposé.

## Conséquences
- Toute logique métier est testable avec `#[tokio::test]` sans GTK.
- Les types échangés sont `Send + 'static` ; les objets GObject restent du côté UI.
