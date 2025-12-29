# Chemins Noirs - Frontend Elm

Frontend Elm pour Chemins Noirs - Générateur GPX anti-bitume avec architecture MVU pure.

## 🎯 Architecture

Ce projet utilise **The Elm Architecture (MVU)** :

```
┌─────────┐
│  Model  │  État immutable de l'application
└────┬────┘
     │
     ├──> update : Msg -> Model -> (Model, Cmd Msg)
     │    Fonction pure qui transforme l'état
     │
     ├──> view : Model -> Html Msg
     │    Génération déclarative du HTML
     │
     └──> subscriptions : Model -> Sub Msg
          Écoute des événements (ex: clics carte)
```

### Modules

```
src/
├── Main.elm              # Point d'entrée (init, update, view)
├── Types.elm             # Types (Model, Msg, domaine)
├── Decoders.elm          # Décodeurs JSON (backend → Elm)
├── Encoders.elm          # Encodeurs JSON (Elm → backend)
├── Ports.elm             # Ports (Elm ↔ JavaScript/MapLibre)
├── Api.elm               # Appels HTTP
└── View/
    ├── Form.elm          # Formulaires
    └── Preview.elm       # Prévisualisation routes
```

## 🚀 Installation

### Prérequis

- **Node.js** 18+ et npm
- **Elm** 0.19.1

```bash
# Installer Elm
npm install -g elm elm-format elm-test

# Installer les dépendances
cd frontend-elm
npm install
```

## 🛠️ Développement

### Lancer le serveur de développement

```bash
npm run dev
```

Ouvre http://localhost:3000

**Hot reload** : Le code Elm se recharge automatiquement à chaque modification !

### Compiler pour production

```bash
npm run build
```

Génère le bundle optimisé dans `dist/`

### Tester

```bash
# Tests unitaires (décodeurs, update, etc.)
npm test

# OU
elm-test
```

### Formater le code

```bash
elm-format src/ --yes
```

## 📂 Structure du projet

```
frontend-elm/
├── elm.json              # Configuration Elm + dépendances
├── package.json          # Dependencies npm (MapLibre, Vite)
├── vite.config.js        # Configuration build Vite
│
├── src/                  # Code Elm
│   ├── Main.elm
│   ├── Types.elm
│   ├── Decoders.elm
│   ├── Encoders.elm
│   ├── Ports.elm
│   ├── Api.elm
│   └── View/
│       ├── Form.elm
│       └── Preview.elm
│
├── tests/                # Tests unitaires Elm
│   └── DecoderTests.elm
│
└── public/               # Assets statiques
    ├── index.html
    ├── main.js           # Glue Elm ↔ MapLibre
    ├── maplibre_map.js   # Intégration MapLibre
    └── style.css
```

## 🌐 API Backend

Le frontend communique avec le backend Rust via HTTP :

- **POST** `/api/route` - Route point-to-point
- **POST** `/api/loops` - Génération de boucles
- **POST** `/api/route/multi` - Route multi-points

Les types sont partagés conceptuellement (JSON) :

```elm
-- Elm
type alias RouteRequest =
    { start : Coordinate
    , end : Coordinate
    , wPop : Float
    , wPaved : Float
    }
```

```rust
// Rust (backend)
#[derive(Serialize, Deserialize)]
pub struct RouteRequest {
    pub start: Coordinate,
    pub end: Coordinate,
    pub w_pop: f64,
    pub w_paved: f64,
}
```

## 🗺️ Intégration MapLibre

L'intégration avec MapLibre GL JS se fait via **Ports Elm** :

### Ports OUT (Elm → JS)

```elm
port updateRoute : List Coordinate -> Cmd msg
port toggleSatelliteView : Bool -> Cmd msg
```

### Ports IN (JS → Elm)

```elm
port mapClickReceived : ({ lat : Float, lon : Float } -> msg) -> Sub msg
```

### Connexion dans main.js

```javascript
// Elm → JS
app.ports.updateRoute.subscribe((coords) => {
  MapLibreMap.updateRoute(coords);
});

// JS → Elm
window.addEventListener('map-click', (event) => {
  app.ports.mapClickReceived.send(event.detail);
});
```

## 🧪 Tests

Les tests utilisent `elm-test` :

```elm
describe "decodeCoordinate"
    [ test "décode une coordonnée valide" <|
        \_ ->
            let
                json = """{"lat": 45.9305, "lon": 4.5776}"""
                result = Decode.decodeString decodeCoordinate json
            in
            case result of
                Ok coord ->
                    Expect.all
                        [ \c -> Expect.within (Expect.Absolute 0.0001) 45.9305 c.lat
                        , \c -> Expect.within (Expect.Absolute 0.0001) 4.5776 c.lon
                        ]
                        coord
                Err _ ->
                    Expect.fail "Décodage échoué"
    ]
```

## 🎨 Principes fonctionnels

Ce projet respecte les principes de **programmation fonctionnelle pure** :

1. **Immutabilité** : Aucune mutation de données
2. **Fonctions pures** : Même entrée → même sortie, sans side-effects
3. **Composition** : Petites fonctions combinées
4. **Type safety** : Compilateur Elm garantit zero runtime errors
5. **Gestion explicite des effets** : `Cmd Msg` pour HTTP, ports, etc.

### Exemple d'update pur

```elm
update : Msg -> Model -> ( Model, Cmd Msg )
update msg model =
    case msg of
        StartLatChanged val ->
            let
                form = model.form
                newForm = { form | startLat = val }  -- Immutable update
            in
            ( { model | form = newForm }             -- Nouveau Model
            , syncSelectionMarkersCmd newForm        -- Effet (Cmd)
            )
```

## 🔧 Outils de développement

### Elm Debugger

Activé automatiquement en mode dev : **time-travel debugging** natif !

- Voir tous les `Msg` envoyés
- Voir tous les états `Model`
- Revenir en arrière dans le temps
- Export/import d'états pour reproduire des bugs

### Elm Reactor (alternatif)

```bash
elm reactor
# Ouvre http://localhost:8000
```

## 📦 Build optimisé

Le build production utilise :

1. `elm make --optimize` - Compilation optimisée
2. Vite - Bundling et minification
3. Tree-shaking - Suppression du code mort

**Résultat** : Bundle ~30-50 KB (vs ~300 KB WASM de Seed !)

## 🚀 Déploiement

### Build

```bash
npm run build
```

### Servir les fichiers statiques

Le dossier `dist/` contient :

- `index.html`
- `assets/main-xxx.js` (Elm compilé)
- `assets/style-xxx.css`
- `maplibre_map.js`

Servir avec nginx, Caddy, ou n'importe quel serveur web.

### Exemple nginx

```nginx
server {
    listen 80;
    root /path/to/frontend-elm/dist;
    index index.html;

    location / {
        try_files $uri $uri/ /index.html;
    }

    location /api {
        proxy_pass http://localhost:8080;
    }
}
```

## 📚 Ressources Elm

- [Elm Guide officiel](https://guide.elm-lang.org/)
- [Elm Packages](https://package.elm-lang.org/)
- [Elm Slack](https://elmlang.herokuapp.com/)
- [Elm Radio Podcast](https://elm-radio.com/)

## 🤝 Comparaison Seed vs Elm

| Aspect | Seed (Rust/WASM) | Elm |
|--------|------------------|-----|
| **Bundle size** | ~300 KB | ~30-50 KB |
| **Compile time** | 10-30s | 1-2s |
| **Hot reload** | ❌ | ✅ |
| **Runtime errors** | Possibles (unwrap) | **Zero garanti** |
| **Debugging** | Console logs | **Time-travel** |
| **Learning curve** | Steep (Rust + WASM) | Gentle |

## 📄 Licence

Même licence que le projet parent Chemins Noirs.
