# Log d'exécution

- Modification de `frontend/src/types.ts`, `frontend/src/App.tsx`, et `frontend/src/activity.ts` pour transmettre et traiter l'URL d'image `image_url` lors de l'envoi de la notification.
- Modification de `frontend/src/components/MobileSetupWizard.tsx` pour enlever la bannière de succès.
- Injection de `use rand::Rng;` et `let jitter = rand::thread_rng().gen_range(1..=10);` dans `core/src/miner.rs` et `core/src/streak_miner.rs` via script PowerShell pour aléatoiriser les délais d'attente (sleep).
- Correction du traitement des flux hors ligne dans `streak_miner.rs` en renvoyant l'erreur `"Stream is offline"` permettant de l'afficher proprement sur l'UI et de sauter l'envoi de paquets superflus.
- L'acquisition des coffres verts est confirmée par le fonctionnement actuel qui scrute toutes les 60 secondes et déclenche la récupération si disponible.
