# Twitch Miner V2

Application de mining Twitch modernisée avec Tauri V2 + Rust backend et React 19 frontend.

## 🚀 Fonctionnalités

### Drop Mining
- **Mining automatique** des drops Twitch avec détection intelligente des streams
- **Extraction Spade URL** robuste avec retry logic et jitter
- **Events temps réel** pour des mises à jour instantanées
- **Auto-claim** des drops complétés

### Streak Mining (Channel Points)
- **Mining parallèle** sur plusieurs channels simultanément
- **Modes séquentiel/parallèle** configurables
- **Statistiques détaillées** : pings réussis/échoués, cycles, taux de succès
- **Gestion automatique** des minute-watched events

### Interface
- **Design moderne** avec React 19 + Tailwind CSS + Radix UI
- **Contrôle indépendant** des deux mineurs
- **Monitoring temps réel** avec statistiques détaillées
- **Responsive design** adapté à tous les écrans

## 🛠️ Architecture

- **Backend**: Rust + Tauri V2 + Tokio (async runtime)
- **Frontend**: React 19 + TypeScript + Vite
- **UI**: Tailwind CSS + Radix UI components
- **Communication**: Events temps réel (pas de polling)
- **Stockage**: Tauri Store pour la configuration

## 📋 Prérequis

- **Node.js** 18+ et npm
- **Rust** 1.70+ avec Cargo
- **Git** pour le versionnage

## 🚀 Installation

### 1. Cloner le dépôt
```bash
git clone <repository-url>
cd twitch-miner
```

### 2. Installer les dépendances
```bash
# Installer les dépendances Node.js
npm install

# Installer les dépendances Rust (automatique avec Tauri)
cd src-tauri
cargo fetch
cd ..
```

### 3. Développement
```bash
# Lancer en mode développement
npm run tauri:dev
```

## 📦 Build & Déploiement

### Build de développement
```bash
npm run build
npm run tauri:build
```

### Build de production
```bash
npm run build
npm run tauri:build -- --target x86_64-pc-windows-msvc
```

Les builds seront générés dans `src-tauri/target/release/bundle/`

## 🎮 Utilisation

### 1. Authentification
- Lancez l'application
- Cliquez sur "Authenticate with Twitch"
- Scannez le QR code ou visitez l'URL affichée
- Entrez le code d'authentification

### 2. Drop Mining
- Cliquez sur "Start Drop Mining"
- L'application détectera automatiquement les drops disponibles
- Surveille les streams avec drops actifs
- Réclame automatiquement les drops complétés

### 3. Streak Mining
- Cliquez sur "Start Streak Mining"
- Mine les points de chaîne sur les channels configurés
- Surveillez les statistiques en temps réel
- Mode parallèle pour une efficacité maximale

## ⚙️ Configuration

Les channels pour le streak mining sont configurables dans le code source (`src-tauri/src/streak_miner.rs`):

```rust
fn get_streak_channels() -> Vec<String> {
    vec![
        "shroud".to_string(),
        "summit1g".to_string(),
        "lirik".to_string(),
        "pokimane".to_string(),
        "ninja".to_string(),
    ]
}
```

## 🔧 Dépannage

### Problèmes courants
- **Compilation échouée**: Vérifiez que Rust et Node.js sont correctement installés
- **Authentification échouée**: Assurez-vous d'avoir une connexion internet stable
- **Mining ne démarre pas**: Vérifiez que vous êtes bien authentifié

### Logs et debugging
- Les logs de développement sont affichés dans la console
- Les erreurs sont affichées dans l'interface utilisateur
- Utilisez `npm run tauri:dev` pour le debugging

## 📊 Performance

- **Faible consommation CPU**: < 5% en utilisation normale
- **Mémoire**: ~50-100MB RAM
- **Réseau**: Minimal grâce aux events temps réel
- **Async**: Architecture entièrement asynchrone avec Tokio

## 🛡️ Sécurité

- **Tokens stockés** localement avec Tauri Store
- **Pas de données envoyées** à des serveurs tiers
- **Communication locale** entre frontend et backend
- **Code source ouvert** pour audit

## 🤝 Contribuer

1. Fork le projet
2. Créer une branche feature (`git checkout -b feature/amazing-feature`)
3. Commit les changements (`git commit -m 'Add amazing feature'`)
4. Push vers la branche (`git push origin feature/amazing-feature`)
5. Ouvrir une Pull Request

## 📝 License

Ce projet est sous license MIT - voir le fichier LICENSE pour les détails.

## 🙏 Remerciements

- **Twitch** pour l'API et les drops
- **Tauri** pour le framework desktop moderne
- **React** pour l'interface utilisateur réactive
- **Rust** pour la performance et la sécurité

---

**Note**: Ce logiciel est à but éducatif. Utilisez-le responsable et respectez les conditions d'utilisation de Twitch.
