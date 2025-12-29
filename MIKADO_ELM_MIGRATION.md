# 🌳 Graphe Mikado - Migration Seed → Elm

## 🎯 OBJECTIF PRINCIPAL
**Migrer le frontend de Seed (Rust/WASM) vers Elm tout en conservant 100% des fonctionnalités**

## 🌳 GRAPHE DE DÉPENDANCES

```
🎯 Frontend Elm Fonctionnel 100%
│
├── 📦 Build & Déploiement
│   ├── Configuration Vite + elm-plugin ⭐
│   ├── Script de build optimisé ⭐
│   └── Integration avec backend existant ⭐
│
├── 🎨 Interface Utilisateur Complète
│   ├── View/Form.elm (formulaires) ⭐
│   ├── View/Preview.elm (affichage route) ⭐
│   ├── View/LoopCandidates.elm (sélection boucles) ⭐
│   └── Styles CSS réutilisés ⭐
│
├── 🔄 Logique Métier (MVU)
│   ├── Main.elm (init, update, view, subscriptions)
│   │   ├── update() - Gestion de tous les Msg ⭐
│   │   ├── view() - Rendu HTML ⭐
│   │   └── init() - État initial ⭐
│   │
│   └── Types.elm (Model, Msg, types métier) ⭐
│
├── 🌐 Communication Backend
│   ├── Api.elm (fonctions HTTP)
│   │   ├── fetchRoute ⭐
│   │   ├── fetchLoopRoute ⭐
│   │   └── fetchMultiPointRoute ⭐
│   │
│   ├── Decoders.elm (JSON → Elm)
│   │   ├── decodeRouteResponse + TESTS ⭐
│   │   ├── decodeLoopRouteResponse + TESTS ⭐
│   │   └── decodeCoordinate + TESTS ⭐
│   │
│   └── Encoders.elm (Elm → JSON) ⭐
│
├── 🗺️ Intégration MapLibre
│   ├── Ports.elm (Elm ↔ JS)
│   │   ├── Ports OUT (updateRoute, updateMarkers, etc.) ⭐
│   │   └── Ports IN (mapClickReceived) ⭐
│   │
│   └── main.js (glue Elm ↔ maplibre_map.js)
│       ├── Initialisation app Elm ⭐
│       ├── Connexion ports OUT ⭐
│       └── Connexion ports IN ⭐
│
└── 🏗️ Infrastructure Projet
    ├── elm.json (configuration + dépendances) ⭐
    ├── Structure src/ (modules organisés) ⭐
    ├── index.html (point d'entrée) ⭐
    └── Tests unitaires (elm-test) ⭐
```

⭐ = **Feuille** (aucune dépendance - peut être fait immédiatement)

## 🚀 ORDRE D'EXÉCUTION (Méthode Mikado)

### Phase 1 : Infrastructure (Jour 1)
1. ✅ Créer elm.json avec dépendances
2. ✅ Créer structure src/
3. ✅ Configurer Vite + elm-plugin
4. ✅ Créer index.html minimal

### Phase 2 : Types & Fondations (Jour 2)
5. ✅ Types.elm - Tous les types (Model, Msg, etc.)
6. ✅ Decoders.elm - JSON decoders + TESTS TDD
7. ✅ Encoders.elm - JSON encoders

### Phase 3 : Communication (Jour 3)
8. ✅ Ports.elm - Définir tous les ports
9. ✅ Api.elm - Fonctions HTTP
10. ✅ main.js - Glue Elm ↔ MapLibre

### Phase 4 : Logique MVU (Jours 4-5)
11. ✅ Main.elm - init()
12. ✅ Main.elm - update() pour tous les Msg
13. ✅ Main.elm - subscriptions()

### Phase 5 : Interface (Jours 6-7)
14. ✅ View/Form.elm - Formulaires
15. ✅ View/Preview.elm - Affichage route
16. ✅ View/LoopCandidates.elm - Sélection boucles
17. ✅ Main.elm - view() qui assemble tout

### Phase 6 : Build & Tests (Jour 8)
18. ✅ Configuration build optimisé
19. ✅ Tests unitaires complets
20. ✅ Test d'intégration E2E

## 📋 CRITÈRES DE SUCCÈS

Chaque étape doit respecter :

1. **Compilation sans erreur** : `elm make src/Main.elm`
2. **Tests verts** : `elm-test` (si tests présents)
3. **Aucune régression** : Fonctionnalité équivalente à Seed
4. **Code fonctionnel pur** : Aucune mutation, functions pures

## 🎯 PROCHAINE ACTION IMMÉDIATE

**Étape 1** : Créer `elm.json` et structure de base
- **Fichiers** : `elm.json`, `src/`, `public/`, `package.json`
- **Temps estimé** : 30 min
- **Validation** : `elm make` compile sans erreur

---

**Note** : Cette approche Mikado garantit que chaque étape est **safe** et **testée** avant de passer à la suivante. Le compilateur Elm agit comme filet de sécurité.
