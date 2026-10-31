# Sécurité

## Signaler une faille

Merci de **ne pas** ouvrir d'issue publique pour une faille de sécurité.

Utilisez les **GitHub Security Advisories** du dépôt : onglet **Security › Advisories › Report a vulnerability** (<https://github.com/Thinsh0/TwitchMiner/security/advisories/new>). Le signalement reste privé entre vous et le mainteneur jusqu'à la publication d'un correctif.

Indiquez si possible :
- la version (`twitchminer --version`, ou l'écran Réglages de l'app) et la plateforme ;
- comment le serveur est exposé (Tailscale, réseau local, reverse proxy, Internet…) et avec quel jeton (complet, agent, invité) ;
- les étapes pour reproduire, et ce qu'un attaquant obtient.

Vous recevrez une première réponse dès que possible. Le correctif est publié dans une nouvelle version, puis l'avis est rendu public, avec votre nom si vous le souhaitez.

## Versions suivies

Seule la dernière version publiée reçoit des correctifs : l'app et le daemon préviennent quand une nouvelle version existe.

## Modèle de menace

Ce que TwitchMiner protège par défaut, et ce qu'il attend de son environnement : voir [docs/SERVER.md](docs/SERVER.md#garde-fous) (jetons, invités, agents IA, exposition hors Tailscale) et [docs/AGENT.md](docs/AGENT.md).
