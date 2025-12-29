# Plan de migration : Seed (Rust/WASM) → Elm

## 1. État actuel du frontend

### Stack technique actuelle
- **Framework** : Seed (Rust/WebAssembly)
- **Build tool** : Trunk
- **Pattern** : MVU (Model-View-Update) - déjà compatible avec Elm !
- **Intégration carte** : MapLibre GL JS (JavaScript)
- **Types partagés** : Crate `shared` (Rust)

### Architecture actuelle (Seed)
```rust
// Model
pub struct Model {
    form: RouteForm,
    loop_form: LoopForm,
    waypoints: Vec<Coordinate>,
    pending: bool,
    last_response: Option<RouteResponse>,
    // ... ~15 champs
}

// Messages
pub enum Msg {
    StartLatChanged(String),
    Submit,
    MapClicked { lat: f64, lon: f64 },
    RouteFetched(Result<RouteResponse, String>),
    // ... ~25 variantes
}

// Update
pub fn update(msg: Msg, model: &mut Model, orders: &mut impl Orders<Msg>)

// View
pub fn view(model: &Model) -> Node<Msg>
```

## 2. Architecture cible Elm

### Stack technique future
- **Framework** : Elm 0.19.1
- **Build tool** : `elm make` + npm/webpack/vite
- **Pattern** : MVU (natif à Elm !)
- **Intégration carte** : MapLibre GL JS via **Elm Ports**
- **Types partagés** : Décodeurs JSON Elm

### Changements clés

| Aspect | Seed (Rust) | Elm |
|--------|-------------|-----|
| **Langage** | Rust → WASM | Elm → JavaScript |
| **Taille bundle** | ~300 KB WASM | ~30-50 KB JS (10x plus léger !) |
| **Vitesse compilation** | Lente (rustc + wasm) | Très rapide (elm make) |
| **Interop JS** | `wasm_bindgen` | **Ports** (plus simple) |
| **Type safety** | Forte | **Garantie absolue** (zero runtime errors) |
| **Immutabilité** | Manuelle (clone) | Native |
| **Gestion async** | `orders.perform_cmd` | `Cmd Msg` |
| **Hot reload** | ❌ | ✅ (avec elm-live) |

## 3. Avantages de la migration vers Elm

### ✅ Avantages

1. **Bundle 10x plus léger** : 30-50 KB vs 300 KB
2. **Temps de compilation 5-10x plus rapide** : 1-2s vs 10-30s
3. **Hot reload** : Modification instantanée du code en dev
4. **Debugging time-travel** : Elm Debugger natif
5. **Zero runtime errors** : Garantie du compilateur (pas de `unwrap()` qui panic)
6. **Ecosystem mature** : elm-ui, elm-spa, elm-test, etc.
7. **Courbe d'apprentissage douce** : Syntaxe simple, messages d'erreur pédagogiques
8. **Interop JS simple** : Ports Elm vs wasm_bindgen

### ⚠️ Points d'attention

1. **Pas de types partagés Rust→Elm** : Duplication des types (mais génération auto possible)
2. **Pas d'accès direct au DOM** : Tout passe par le Virtual DOM
3. **Pas de NPM natif** : Utilisation de Ports pour libs JS
4. **Pas de code asynchrone imbriqué** : Architecture purement event-driven

## 4. Structure de projet Elm proposée

```
frontend-elm/
├── elm.json                  # Configuration Elm
├── src/
│   ├── Main.elm             # Point d'entrée
│   ├── Types.elm            # Model, Msg
│   ├── Api.elm              # Appels HTTP
│   ├── Decoders.elm         # JSON decoders
│   ├── Encoders.elm         # JSON encoders
│   ├── Ports.elm            # Ports MapLibre
│   └── View/
│       ├── Form.elm         # Formulaire
│       ├── Preview.elm      # Prévisualisation route
│       └── LoopCandidates.elm  # Sélection boucles
├── public/
│   ├── index.html
│   ├── style.css
│   ├── maplibre_map.js      # Réutilisé tel quel !
│   └── main.js              # Initialisation Elm + Ports
└── package.json             # npm dependencies (MapLibre)
```

## 5. Exemple de code Elm équivalent

### 5.1 Types.elm

```elm
module Types exposing (..)

import Http

-- MODEL

type alias Model =
    { form : RouteForm
    , loopForm : LoopForm
    , waypoints : List Coordinate
    , closeLoop : Bool
    , pending : Bool
    , lastResponse : Maybe RouteResponse
    , loopCandidates : List LoopCandidate
    , selectedLoopIdx : Maybe Int
    , error : Maybe String
    , clickMode : ClickMode
    , routeMode : RouteMode
    , mapViewMode : MapViewMode
    , viewMode : ViewMode
    }

type alias RouteForm =
    { startLat : String
    , startLon : String
    , endLat : String
    , endLon : String
    , wPop : String
    , wPaved : String
    }

type alias Coordinate =
    { lat : Float
    , lon : Float
    }

-- MSG

type Msg
    = StartLatChanged String
    | StartLonChanged String
    | Submit
    | MapClicked Float Float
    | RouteFetched (Result Http.Error RouteResponse)
    | LoopRouteFetched (Result Http.Error LoopRouteResponse)
    | SelectLoopCandidate Int
    | AddWaypoint Coordinate
    | RemoveWaypoint Int
    | ToggleRouteMode RouteMode
    | ToggleMapView
    | SaveRoute
    | LoadRoute

type ClickMode
    = Start
    | End

type RouteMode
    = PointToPoint
    | Loop
    | MultiPoint

type MapViewMode
    = Standard
    | Satellite
```

### 5.2 Main.elm

```elm
module Main exposing (main)

import Browser
import Types exposing (..)
import Api
import Ports
import View.Form
import View.Preview
import Html exposing (..)

-- INIT

init : () -> ( Model, Cmd Msg )
init _ =
    let
        initialModel =
            { form =
                { startLat = "45.9305"
                , startLon = "4.5776"
                , endLat = "45.9399"
                , endLon = "4.5757"
                , wPop = "1.5"
                , wPaved = "4.0"
                }
            , loopForm = defaultLoopForm
            , waypoints = []
            , closeLoop = False
            , pending = False
            , lastResponse = Nothing
            , loopCandidates = []
            , selectedLoopIdx = Nothing
            , error = Nothing
            , clickMode = Start
            , routeMode = PointToPoint
            , mapViewMode = Standard
            , viewMode = Map2D
            }
    in
    ( initialModel
    , Cmd.batch
        [ Ports.initMap ()
        , Ports.syncSelectionMarkers
            { start = Just { lat = 45.9305, lon = 4.5776 }
            , end = Just { lat = 45.9399, lon = 4.5757 }
            }
        ]
    )

-- UPDATE

update : Msg -> Model -> ( Model, Cmd Msg )
update msg model =
    case msg of
        StartLatChanged val ->
            let
                form = model.form
                newForm = { form | startLat = val }
            in
            ( { model | form = newForm }
            , syncMarkersCmd newForm
            )

        StartLonChanged val ->
            let
                form = model.form
                newForm = { form | startLon = val }
            in
            ( { model | form = newForm }
            , syncMarkersCmd newForm
            )

        Submit ->
            if model.pending then
                ( model, Cmd.none )
            else
                case model.routeMode of
                    PointToPoint ->
                        case formToRequest model.form of
                            Ok request ->
                                ( { model | pending = True, error = Nothing }
                                , Api.fetchRoute request RouteFetched
                                )

                            Err err ->
                                ( { model | error = Just err }
                                , Cmd.none
                                )

                    Loop ->
                        case loopFormToRequest model.form model.loopForm of
                            Ok request ->
                                ( { model | pending = True, error = Nothing }
                                , Api.fetchLoopRoute request LoopRouteFetched
                                )

                            Err err ->
                                ( { model | error = Just err }
                                , Cmd.none
                                )

                    MultiPoint ->
                        if List.length model.waypoints < 2 then
                            ( { model | error = Just "Au moins 2 points requis" }
                            , Cmd.none
                            )
                        else
                            ( { model | pending = True, error = Nothing }
                            , Api.fetchMultiPointRoute
                                { waypoints = model.waypoints
                                , closeLoop = model.closeLoop
                                , wPop = String.toFloat model.form.wPop |> Maybe.withDefault 1.0
                                , wPaved = String.toFloat model.form.wPaved |> Maybe.withDefault 1.0
                                }
                                RouteFetched
                            )

        RouteFetched result ->
            case result of
                Ok route ->
                    ( { model
                        | pending = False
                        , lastResponse = Just route
                        , error = Nothing
                      }
                    , Cmd.batch
                        [ Ports.updateRoute route.path
                        , Ports.updateBbox (routeBounds route)
                        ]
                    )

                Err httpError ->
                    ( { model
                        | pending = False
                        , error = Just (httpErrorToString httpError)
                      }
                    , Ports.updateRoute []
                    )

        MapClicked lat lon ->
            case model.routeMode of
                MultiPoint ->
                    update (AddWaypoint { lat = lat, lon = lon }) model

                _ ->
                    let
                        coord = { lat = lat, lon = lon }
                        form = model.form
                    in
                    case model.clickMode of
                        Start ->
                            let
                                newForm =
                                    { form
                                        | startLat = formatCoord lat
                                        , startLon = formatCoord lon
                                    }
                            in
                            ( { model | form = newForm }
                            , syncMarkersCmd newForm
                            )

                        End ->
                            let
                                newForm =
                                    { form
                                        | endLat = formatCoord lat
                                        , endLon = formatCoord lon
                                    }
                            in
                            ( { model | form = newForm }
                            , syncMarkersCmd newForm
                            )

        AddWaypoint coord ->
            let
                newWaypoints = model.waypoints ++ [ coord ]
            in
            ( { model | waypoints = newWaypoints, error = Nothing }
            , Ports.updateWaypointMarkers newWaypoints
            )

        RemoveWaypoint idx ->
            let
                newWaypoints =
                    List.take idx model.waypoints
                        ++ List.drop (idx + 1) model.waypoints
            in
            ( { model | waypoints = newWaypoints }
            , Ports.updateWaypointMarkers newWaypoints
            )

        ToggleRouteMode mode ->
            ( { model | routeMode = mode }
            , if mode /= MultiPoint && not (List.isEmpty model.waypoints) then
                Ports.updateWaypointMarkers []
              else
                Cmd.none
            )

        ToggleMapView ->
            let
                newMode =
                    case model.mapViewMode of
                        Standard -> Satellite
                        Satellite -> Standard
            in
            ( { model | mapViewMode = newMode }
            , Ports.toggleSatelliteView (newMode == Satellite)
            )

        SelectLoopCandidate idx ->
            case List.head (List.drop idx model.loopCandidates) of
                Just candidate ->
                    ( { model | selectedLoopIdx = Just idx }
                    , Cmd.batch
                        [ Ports.updateRoute candidate.route.path
                        , Ports.updateBbox (routeBounds candidate.route)
                        ]
                    )

                Nothing ->
                    ( model, Cmd.none )

        _ ->
            ( model, Cmd.none )

-- VIEW

view : Model -> Html Msg
view model =
    div [ class "app-container" ]
        [ h1 [] [ text "Chemins Noirs – générateur GPX anti-bitume" ]
        , View.Form.view model
        , View.Preview.view model
        ]

-- SUBSCRIPTIONS

subscriptions : Model -> Sub Msg
subscriptions model =
    Ports.mapClickReceived (\{ lat, lon } -> MapClicked lat lon)

-- MAIN

main : Program () Model Msg
main =
    Browser.element
        { init = init
        , view = view
        , update = update
        , subscriptions = subscriptions
        }

-- HELPERS

syncMarkersCmd : RouteForm -> Cmd Msg
syncMarkersCmd form =
    let
        start = parseCoordinate form.startLat form.startLon
        end = parseCoordinate form.endLat form.endLon
    in
    Ports.syncSelectionMarkers { start = start, end = end }

parseCoordinate : String -> String -> Maybe Coordinate
parseCoordinate latStr lonStr =
    Maybe.map2 Coordinate
        (String.toFloat latStr)
        (String.toFloat lonStr)

formatCoord : Float -> String
formatCoord value =
    String.fromFloat value

httpErrorToString : Http.Error -> String
httpErrorToString error =
    case error of
        Http.BadUrl url ->
            "URL invalide: " ++ url

        Http.Timeout ->
            "Timeout - le serveur ne répond pas"

        Http.NetworkError ->
            "Erreur réseau - vérifiez votre connexion"

        Http.BadStatus status ->
            "Erreur serveur: " ++ String.fromInt status

        Http.BadBody body ->
            "Réponse invalide: " ++ body
```

### 5.3 Ports.elm

```elm
port module Ports exposing (..)

import Types exposing (Coordinate)

-- Ports OUT (Elm → JavaScript)

port initMap : () -> Cmd msg

port updateRoute : List Coordinate -> Cmd msg

port updateSelectionMarkers :
    { start : Maybe Coordinate
    , end : Maybe Coordinate
    }
    -> Cmd msg

port updateWaypointMarkers : List Coordinate -> Cmd msg

port toggleSatelliteView : Bool -> Cmd msg

port toggleThree3DView : Bool -> Cmd msg

port updateBbox :
    { minLat : Float
    , maxLat : Float
    , minLon : Float
    , maxLon : Float
    }
    -> Cmd msg

port startAnimation : () -> Cmd msg

port stopAnimation : () -> Cmd msg

-- Ports IN (JavaScript → Elm)

port mapClickReceived : ({ lat : Float, lon : Float } -> msg) -> Sub msg
```

### 5.4 Api.elm

```elm
module Api exposing (..)

import Http
import Json.Decode as Decode
import Json.Encode as Encode
import Types exposing (..)
import Decoders exposing (..)

apiRoot : String
apiRoot =
    "http://localhost:8080/api/route"

loopApiRoot : String
loopApiRoot =
    "http://localhost:8080/api/loops"

-- Fetch point-to-point route
fetchRoute : RouteRequest -> (Result Http.Error RouteResponse -> msg) -> Cmd msg
fetchRoute request toMsg =
    Http.post
        { url = apiRoot
        , body = Http.jsonBody (encodeRouteRequest request)
        , expect = Http.expectJson toMsg decodeRouteResponse
        }

-- Fetch loop routes
fetchLoopRoute : LoopRouteRequest -> (Result Http.Error LoopRouteResponse -> msg) -> Cmd msg
fetchLoopRoute request toMsg =
    Http.post
        { url = loopApiRoot
        , body = Http.jsonBody (encodeLoopRouteRequest request)
        , expect = Http.expectJson toMsg decodeLoopRouteResponse
        }

-- Fetch multi-point route
fetchMultiPointRoute : MultiPointRouteRequest -> (Result Http.Error RouteResponse -> msg) -> Cmd msg
fetchMultiPointRoute request toMsg =
    Http.post
        { url = apiRoot ++ "/multi"
        , body = Http.jsonBody (encodeMultiPointRouteRequest request)
        , expect = Http.expectJson toMsg decodeRouteResponse
        }

-- Encoders

encodeRouteRequest : RouteRequest -> Encode.Value
encodeRouteRequest req =
    Encode.object
        [ ( "start", encodeCoordinate req.start )
        , ( "end", encodeCoordinate req.end )
        , ( "w_pop", Encode.float req.wPop )
        , ( "w_paved", Encode.float req.wPaved )
        ]

encodeCoordinate : Coordinate -> Encode.Value
encodeCoordinate coord =
    Encode.object
        [ ( "lat", Encode.float coord.lat )
        , ( "lon", Encode.float coord.lon )
        ]

encodeLoopRouteRequest : LoopRouteRequest -> Encode.Value
encodeLoopRouteRequest req =
    Encode.object
        [ ( "start", encodeCoordinate req.start )
        , ( "target_distance_km", Encode.float req.targetDistanceKm )
        , ( "distance_tolerance_km", Encode.float req.distanceToleranceKm )
        , ( "candidate_count", Encode.int req.candidateCount )
        , ( "w_pop", Encode.float req.wPop )
        , ( "w_paved", Encode.float req.wPaved )
        , ( "max_total_ascent", encodeMaybe Encode.float req.maxTotalAscent )
        , ( "min_total_ascent", encodeMaybe Encode.float req.minTotalAscent )
        ]

encodeMultiPointRouteRequest : MultiPointRouteRequest -> Encode.Value
encodeMultiPointRouteRequest req =
    Encode.object
        [ ( "waypoints", Encode.list encodeCoordinate req.waypoints )
        , ( "close_loop", Encode.bool req.closeLoop )
        , ( "w_pop", Encode.float req.wPop )
        , ( "w_paved", Encode.float req.wPaved )
        ]

encodeMaybe : (a -> Encode.Value) -> Maybe a -> Encode.Value
encodeMaybe encoder maybeValue =
    case maybeValue of
        Just value ->
            encoder value

        Nothing ->
            Encode.null
```

### 5.5 Decoders.elm

```elm
module Decoders exposing (..)

import Json.Decode as Decode exposing (Decoder)
import Types exposing (..)

-- Coordinate
decodeCoordinate : Decoder Coordinate
decodeCoordinate =
    Decode.map2 Coordinate
        (Decode.field "lat" Decode.float)
        (Decode.field "lon" Decode.float)

-- RouteResponse
decodeRouteResponse : Decoder RouteResponse
decodeRouteResponse =
    Decode.map5 RouteResponse
        (Decode.field "path" (Decode.list decodeCoordinate))
        (Decode.field "distance_km" Decode.float)
        (Decode.field "gpx_base64" Decode.string)
        (Decode.maybe (Decode.field "metadata" decodeRouteMetadata))
        (Decode.maybe (Decode.field "elevation_profile" decodeElevationProfile))

decodeRouteMetadata : Decoder RouteMetadata
decodeRouteMetadata =
    Decode.map4 RouteMetadata
        (Decode.field "point_count" Decode.int)
        (Decode.field "bounds" decodeRouteBounds)
        (Decode.field "start" decodeCoordinate)
        (Decode.field "end" decodeCoordinate)

decodeRouteBounds : Decoder RouteBounds
decodeRouteBounds =
    Decode.map4 RouteBounds
        (Decode.field "min_lat" Decode.float)
        (Decode.field "max_lat" Decode.float)
        (Decode.field "min_lon" Decode.float)
        (Decode.field "max_lon" Decode.float)

decodeElevationProfile : Decoder ElevationProfile
decodeElevationProfile =
    Decode.map5 ElevationProfile
        (Decode.field "elevations" (Decode.list (Decode.nullable Decode.float)))
        (Decode.maybe (Decode.field "min_elevation" Decode.float))
        (Decode.maybe (Decode.field "max_elevation" Decode.float))
        (Decode.field "total_ascent" Decode.float)
        (Decode.field "total_descent" Decode.float)

-- LoopRouteResponse
decodeLoopRouteResponse : Decoder LoopRouteResponse
decodeLoopRouteResponse =
    Decode.map3 LoopRouteResponse
        (Decode.field "candidates" (Decode.list decodeLoopCandidate))
        (Decode.field "target_distance_km" Decode.float)
        (Decode.field "distance_tolerance_km" Decode.float)

decodeLoopCandidate : Decoder LoopCandidate
decodeLoopCandidate =
    Decode.map3 LoopCandidate
        (Decode.field "route" decodeRouteResponse)
        (Decode.field "distance_error_km" Decode.float)
        (Decode.field "bearing_deg" Decode.float)
```

### 5.6 Intégration MapLibre (public/main.js)

```javascript
// Initialiser l'application Elm
const app = Elm.Main.init({
  node: document.getElementById('app')
});

// Importer maplibre_map.js qui expose les fonctions MapLibre
import * as MapLibre from './maplibre_map.js';

// Port OUT : Elm → JavaScript

app.ports.initMap.subscribe(() => {
  MapLibre.initMap();
});

app.ports.updateRoute.subscribe((coords) => {
  MapLibre.updateRoute(coords);
});

app.ports.updateSelectionMarkers.subscribe(({ start, end }) => {
  MapLibre.updateSelectionMarkers(start, end);
});

app.ports.updateWaypointMarkers.subscribe((waypoints) => {
  MapLibre.updateWaypointMarkers(waypoints);
});

app.ports.toggleSatelliteView.subscribe((enabled) => {
  MapLibre.toggleSatelliteView(enabled);
});

app.ports.toggleThree3DView.subscribe((enabled) => {
  MapLibre.toggleThree3DView(enabled);
});

app.ports.updateBbox.subscribe((bounds) => {
  MapLibre.updateBbox(bounds);
});

app.ports.startAnimation.subscribe(() => {
  MapLibre.startAnimation();
});

app.ports.stopAnimation.subscribe(() => {
  MapLibre.stopAnimation();
});

// Port IN : JavaScript → Elm
// Le fichier maplibre_map.js doit émettre un event "map-click"

window.addEventListener('map-click', (event) => {
  app.ports.mapClickReceived.send({
    lat: event.detail.lat,
    lon: event.detail.lon
  });
});
```

## 6. Plan de migration étape par étape

### Phase 1 : Setup (1 jour)

1. **Installer Elm**
   ```bash
   npm install -g elm elm-format elm-test
   ```

2. **Initialiser le projet Elm**
   ```bash
   mkdir frontend-elm
   cd frontend-elm
   elm init
   ```

3. **Configurer elm.json**
   ```json
   {
     "type": "application",
     "source-directories": ["src"],
     "elm-version": "0.19.1",
     "dependencies": {
       "direct": {
         "elm/browser": "1.0.2",
         "elm/core": "1.0.5",
         "elm/html": "1.0.0",
         "elm/http": "2.0.0",
         "elm/json": "1.1.3"
       },
       "indirect": {}
     },
     "test-dependencies": {
       "direct": {},
       "indirect": {}
     }
   }
   ```

4. **Setup build (Vite ou webpack)**
   ```bash
   npm install --save-dev vite vite-plugin-elm
   ```

### Phase 2 : Types et Decoders (2 jours)

1. Créer `Types.elm` avec tous les types (Model, Msg, etc.)
2. Créer `Decoders.elm` pour parser le JSON du backend
3. Créer `Encoders.elm` pour encoder les requêtes
4. **Tester** les decoders avec `elm-test`

### Phase 3 : Ports MapLibre (1 jour)

1. Créer `Ports.elm` avec tous les ports
2. Créer `public/main.js` qui connecte Elm ↔ maplibre_map.js
3. **Tester** : Initialiser la carte, afficher un marker

### Phase 4 : API HTTP (1 jour)

1. Créer `Api.elm` avec les fonctions HTTP
2. Implémenter `fetchRoute`, `fetchLoopRoute`, `fetchMultiPointRoute`
3. **Tester** : Appeler le backend et décoder la réponse

### Phase 5 : Update logic (2 jours)

1. Implémenter `update` pour tous les `Msg`
2. Gérer les cas d'erreur
3. **Tester** : Toutes les interactions doivent marcher

### Phase 6 : View (2-3 jours)

1. Créer `View/Form.elm` (formulaires)
2. Créer `View/Preview.elm` (affichage route)
3. Créer `View/LoopCandidates.elm` (sélection boucles)
4. **Tester** : Rendu HTML correct, événements fonctionnels

### Phase 7 : Intégration finale (1 jour)

1. Connecter tous les modules dans `Main.elm`
2. Debugger avec Elm Debugger
3. **Tester** : Application complète end-to-end

### Phase 8 : Build et déploiement (1 jour)

1. Optimiser le build avec `elm make --optimize`
2. Minifier le JS avec UglifyJS
3. Configurer le serveur web (nginx, etc.)
4. **Déployer** !

**Total : ~10-12 jours** (avec tests et debugging)

## 7. Comparaison code Seed vs Elm

### Gestion de l'état mutable

**Seed (Rust)** :
```rust
pub fn update(msg: Msg, model: &mut Model, orders: &mut impl Orders<Msg>) {
    match msg {
        Msg::StartLatChanged(val) => {
            model.form.start_lat = val;  // Mutation !
            sync_selection_markers(&model.form);
            reset_loop_candidates(model);
        }
        // ...
    }
}
```

**Elm** :
```elm
update : Msg -> Model -> ( Model, Cmd Msg )
update msg model =
    case msg of
        StartLatChanged val ->
            let
                form = model.form
                newForm = { form | startLat = val }  -- Immutable !
            in
            ( { model | form = newForm }
            , syncMarkersCmd newForm
            )
```

### Appels HTTP

**Seed (Rust)** :
```rust
async fn send_route_request(payload: RouteRequest) -> Msg {
    let response = match Request::new(api_root())
        .method(Method::Post)
        .json(&payload)
    {
        Err(err) => Err(format!("{err:?}")),
        Ok(request) => match request.fetch().await {
            Err(err) => Err(format!("{err:?}")),
            Ok(raw) => match raw.check_status() {
                Err(status_err) => Err(format!("{status_err:?}")),
                Ok(resp) => match resp.json::<RouteResponse>().await {
                    Ok(route) => Ok(route),
                    Err(err) => Err(format!("{err:?}")),
                },
            },
        },
    };
    Msg::RouteFetched(response)
}
```

**Elm** :
```elm
fetchRoute : RouteRequest -> (Result Http.Error RouteResponse -> msg) -> Cmd msg
fetchRoute request toMsg =
    Http.post
        { url = apiRoot
        , body = Http.jsonBody (encodeRouteRequest request)
        , expect = Http.expectJson toMsg decodeRouteResponse
        }
```

**Beaucoup plus concis !**

### Interop JavaScript

**Seed (Rust)** :
```rust
#[wasm_bindgen(module = "/maplibre_map.js")]
extern "C" {
    #[wasm_bindgen(js_name = updateRoute)]
    fn update_route_js(coords: JsValue);
}

// Utilisation
if let Ok(value) = to_value(path) {
    update_route_js(value);
}
```

**Elm** :
```elm
-- Ports.elm
port updateRoute : List Coordinate -> Cmd msg

-- Utilisation
update msg model =
    ( model, Ports.updateRoute route.path )
```

**Plus simple et type-safe !**

## 8. Outils de développement Elm

### Elm Reactor (dev server)
```bash
elm reactor
# Ouvre http://localhost:8000
```

### Elm Live (hot reload)
```bash
npm install -g elm-live
elm-live src/Main.elm --open -- --output=public/elm.js
```

### Elm Format (formattage auto)
```bash
elm-format src/ --yes
```

### Elm Test
```bash
elm-test
```

### Elm Debugger
- Time-travel debugging natif
- Voir tous les `Msg` et `Model` dans le browser
- Export/import de l'état pour reproduire des bugs

## 9. Ressources

### Documentation officielle
- [Elm Guide](https://guide.elm-lang.org/) - Tutoriel officiel
- [Elm Packages](https://package.elm-lang.org/) - Registry de packages
- [Elm Syntax](https://elm-lang.org/docs/syntax) - Référence syntaxe

### Tutoriels migration
- [From JavaScript to Elm](https://github.com/elm-community/js-to-elm)
- [Elm in Action](https://www.manning.com/books/elm-in-action) - Livre complet

### Outils
- [elm-json](https://github.com/zwilias/elm-json) - Gestion dépendances
- [elm-verify-examples](https://github.com/stoeffel/elm-verify-examples) - Tests docs
- [elm-analyse](https://github.com/stil4m/elm-analyse) - Linter

## 10. Conclusion

### Pourquoi migrer vers Elm ?

1. **Performance** : Bundle 10x plus léger, build 5x plus rapide
2. **DX (Developer Experience)** : Hot reload, time-travel debugging, messages d'erreur clairs
3. **Fiabilité** : Zero runtime errors garantis par le compilateur
4. **Simplicité** : Moins de boilerplate que Seed/Rust
5. **Ecosystem** : Packages Elm matures (elm-ui, elm-spa, etc.)

### Migration recommandée ?

**✅ OUI** si :
- Vous voulez un bundle plus léger (~30 KB vs 300 KB)
- Vous voulez des temps de compilation rapides
- Vous voulez le hot reload en dev
- Vous voulez garantir zero runtime errors
- Vous aimez le pattern MVU (déjà utilisé avec Seed)

**⚠️ NON** si :
- Vous avez besoin de partager du code Rust frontend/backend
- Vous voulez utiliser directement des libs NPM complexes
- Vous avez besoin de performance extrême (mais Elm est déjà très rapide !)

### Verdict

Étant donné que :
1. Vous utilisez déjà le pattern MVU avec Seed
2. Le frontend est relativement simple (formulaires + carte)
3. L'interop JS se fait via `wasm_bindgen` (ports Elm sont plus simples)
4. Le bundle WASM est lourd (~300 KB)

**La migration vers Elm est fortement recommandée !** 🎯

Le temps de migration est raisonnable (~10-12 jours) et les gains sont significatifs :
- **10x plus léger**
- **5x plus rapide à compiler**
- **Zero runtime errors garantis**
- **Hot reload** en dev

Vous conservez le pattern MVU que vous connaissez déjà, tout en gagnant en simplicité et en performance.
