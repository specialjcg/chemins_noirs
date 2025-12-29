# 🚀 Quickstart - Frontend Elm

## Installation et lancement en 3 étapes

### 1. Installer les dépendances

```bash
cd frontend-elm

# Installer Elm globalement (si pas déjà fait)
npm install -g elm elm-format elm-test

# Installer les dépendances npm (Vite, MapLibre, etc.)
npm install
```

### 2. Lancer le backend Rust (terminal 1)

```bash
cd ../backend
cargo run
```

Le backend démarre sur **http://localhost:8080**

### 3. Lancer le frontend Elm (terminal 2)

```bash
cd ../frontend-elm
npm run dev
```

Le frontend démarre sur **http://localhost:3000**

Ouvrez votre navigateur sur **http://localhost:3000** 🎉

## Commandes utiles

### Développement

```bash
# Dev server avec hot reload
npm run dev

# Tests unitaires
npm test

# Formater le code Elm
elm-format src/ --yes

# Compiler sans serveur
elm make src/Main.elm --output=public/elm.js
```

### Production

```bash
# Build optimisé
npm run build
# Résultat dans dist/

# Preview du build
npm run preview
```

## Vérifier que tout fonctionne

### ✅ Checklist

1. **Backend** : http://localhost:8080/api/route doit retourner 405 (Method Not Allowed)
2. **Frontend** : http://localhost:3000 affiche l'interface
3. **Carte** : La carte MapLibre s'affiche correctement
4. **Formulaire** : Les champs de coordonnées sont remplis
5. **Submit** : Cliquer sur "Tracer l'itinéraire" affiche une route

### 🐛 Debugging

Si problèmes :

1. **Backend ne démarre pas**
   ```bash
   cd backend
   cargo clean
   cargo build
   cargo run
   ```

2. **Frontend ne compile pas**
   ```bash
   cd frontend-elm
   rm -rf elm-stuff
   elm make src/Main.elm
   ```

3. **MapLibre ne s'affiche pas**
   - Vérifier que `public/maplibre_map.js` existe
   - Vérifier la console navigateur (F12) pour erreurs JS

4. **Erreur CORS**
   - Vérifier que le backend tourne sur port 8080
   - Vérifier `vite.config.js` proxy configuration

## Elm Debugger

Le **debugger Elm** est activé automatiquement en mode dev !

### Utilisation

1. Ouvrir http://localhost:3000
2. Cliquer sur l'icône Elm en bas à droite
3. **Time-travel debugging** :
   - Voir tous les `Msg` envoyés
   - Voir tous les états `Model`
   - Revenir en arrière dans le temps
   - Export/import d'états

### Exemple

```
1. StartLatChanged "45.9305"  → Model { form = { startLat = "45.9305", ... } }
2. StartLonChanged "4.5776"   → Model { form = { startLat = "45.9305", startLon = "4.5776", ... } }
3. Submit                     → Model { pending = True, ... }
4. RouteFetched (Ok route)    → Model { pending = False, lastResponse = Just route, ... }
```

Vous pouvez cliquer sur n'importe quel `Msg` pour **revenir à cet état** !

## Structure des fichiers

```
frontend-elm/
├── src/
│   ├── Main.elm          # Point d'entrée (init, update, view)
│   ├── Types.elm         # Tous les types (Model, Msg, domaine)
│   ├── Decoders.elm      # JSON → Elm
│   ├── Encoders.elm      # Elm → JSON
│   ├── Ports.elm         # Elm ↔ JavaScript
│   ├── Api.elm           # Appels HTTP
│   └── View/
│       ├── Form.elm      # Formulaires
│       └── Preview.elm   # Prévisualisation
│
├── tests/
│   └── DecoderTests.elm  # Tests unitaires
│
└── public/
    ├── index.html        # Point d'entrée HTML
    ├── main.js           # Glue Elm ↔ MapLibre
    ├── maplibre_map.js   # Intégration MapLibre
    └── style.css         # Styles CSS
```

## Flux de l'application

### Lancer une requête

```
User clique "Tracer l'itinéraire"
    ↓
Msg: Submit
    ↓
update Submit model
    ↓
Validation du formulaire (formToRequest)
    ↓
Api.fetchRoute request RouteFetched
    ↓
HTTP POST /api/route vers backend Rust
    ↓
Backend calcule la route
    ↓
JSON Response
    ↓
Msg: RouteFetched (Ok route)
    ↓
update RouteFetched model
    ↓
Model mis à jour + Cmd (Ports.updateRoute)
    ↓
Port OUT → JavaScript → MapLibre
    ↓
Carte affiche la route !
```

## Hot Reload

Le **hot reload** fonctionne automatiquement :

1. Modifier `src/Main.elm`
2. Sauvegarder (Ctrl+S)
3. Le navigateur **se recharge automatiquement**
4. L'état de l'app est **préservé** (grâce au debugger Elm)

### Exemple

```elm
-- Modifier View/Form.elm
button [ onClick Submit ]
    [ text "Tracer l'itinéraire" ]

↓ (sauvegarder)

button [ onClick Submit, class "primary-btn" ]  -- Ajout class
    [ text "🚀 Tracer l'itinéraire" ]           -- Ajout emoji
```

Sauvegarde → **Rechargement instantané** sans perdre l'état !

## Next Steps

### Améliorer le code

1. **Refactoring** : Extraire des fonctions réutilisables
2. **Tests** : Ajouter des tests pour `update`, helpers
3. **Styles** : Améliorer le CSS (ou utiliser `elm-ui`)
4. **Features** : Ajouter nouvelles fonctionnalités

### Apprendre Elm

- [Elm Guide officiel](https://guide.elm-lang.org/) - **Commencer ici !**
- [Elm Packages](https://package.elm-lang.org/) - Registry
- [Elm Town Podcast](https://elmtown.simplecast.com/)
- [Elm Radio](https://elm-radio.com/)

### Ressources

- **Documentation** : `README.md` (documentation complète)
- **Plan de migration** : `../ELM_MIGRATION_PLAN.md`
- **Architecture** : `../MVU_COMPARISON.md`

---

**Prêt à coder ?** 🎯

```bash
npm run dev
```

Ouvrez http://localhost:3000 et bon développement !
