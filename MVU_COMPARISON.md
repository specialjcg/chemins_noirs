# MVU Architecture: Backend Rust ↔️ Frontend Elm

## Comparaison côte à côte

### MODEL

**Backend (Rust)** - `src/core/app.rs`:
```rust
#[derive(Debug, Clone)]
pub struct AppModel {
    pub is_running: bool,
    pub processed_events: u64,
}

impl Default for AppModel {
    fn default() -> Self {
        Self {
            is_running: true,
            processed_events: 0,
        }
    }
}
```

**Frontend (Elm)** - `ui/src/Main.elm`:
```elm
type alias Model =
    { boxes : List BoxSummary
    , selectedBox : Maybe BoxData
    , loadingBoxes : Bool
    , loadingDetail : Bool
    , error : Maybe String
    , lastUpdate : String
    }

init : () -> ( Model, Cmd Msg )
init _ =
    ( { boxes = []
      , selectedBox = Nothing
      , loadingBoxes = True
      , loadingDetail = False
      , error = Nothing
      , lastUpdate = "Never"
      }
    , fetchBoxes
    )
```

### MSG (Messages/Events)

**Backend (Rust)**:
```rust
#[derive(Debug, Clone)]
pub enum Msg {
    Input(InputEvent),
    BoxPersisted { box_id: String },
    Tick,
    Shutdown,
}
```

**Frontend (Elm)**:
```elm
type Msg
    = FetchBoxes
    | BoxesReceived (Result Http.Error (List BoxSummary))
    | SelectBox String
    | BoxDetailReceived (Result Http.Error BoxData)
    | CloseDetail
    | Tick Time.Posix
    | Refresh
```

### UPDATE (State transitions)

**Backend (Rust)**:
```rust
pub fn update(model: &AppModel, msg: Msg) -> (AppModel, Vec<Command>) {
    let mut next = model.clone();
    let mut cmds = Vec::new();

    match msg {
        Msg::Input(event) => {
            next.processed_events += 1;
            cmds.push(Command::HandleWorkflow(event));
        }
        Msg::BoxPersisted { .. } | Msg::Tick => {}
        Msg::Shutdown => {
            next.is_running = false;
        }
    }

    (next, cmds)
}
```

**Frontend (Elm)**:
```elm
update : Msg -> Model -> ( Model, Cmd Msg )
update msg model =
    case msg of
        FetchBoxes ->
            ( { model | loadingBoxes = True, error = Nothing }
            , fetchBoxes
            )

        BoxesReceived result ->
            case result of
                Ok boxes ->
                    ( { model
                        | boxes = boxes
                        , loadingBoxes = False
                        , lastUpdate = "Just now"
                      }
                    , Cmd.none
                    )

                Err error ->
                    ( { model
                        | loadingBoxes = False
                        , error = Just (httpErrorToString error)
                      }
                    , Cmd.none
                    )

        SelectBox boxId ->
            ( { model | loadingDetail = True }
            , fetchBoxDetail boxId
            )
```

### VIEW (Rendering)

**Backend (Rust)** - Console text:
```rust
pub fn view(model: &AppModel) -> String {
    format!(
        "[App] running={} events={}",
        model.is_running,
        model.processed_events
    )
}
```

**Frontend (Elm)** - HTML:
```elm
view : Model -> Html Msg
view model =
    div [ class "app-container" ]
        [ header []
            [ h1 [] [ text "🚂 Gare Promo Service" ]
            , button [ onClick Refresh ] [ text "🔄 Refresh" ]
            ]
        , case model.error of
            Just errorMsg ->
                div [ class "error-banner" ] [ text errorMsg ]
            Nothing ->
                text ""
        , main_ []
            [ if model.loadingBoxes then
                viewLoading
              else
                viewBoxes model.boxes
            ]
        ]

viewBoxes : List BoxSummary -> Html Msg
viewBoxes boxes =
    div [ class "boxes-grid" ]
        (List.map viewBoxCard boxes)

viewBoxCard : BoxSummary -> Html Msg
viewBoxCard box =
    div [ class "box-card", onClick (SelectBox box.boxId) ]
        [ h3 [] [ text box.boxId ]
        , div [] [ text (String.fromInt box.doneLines ++ "/" ++ String.fromInt box.totalLines) ]
        , progressBar box.doneLines box.totalLines
        ]
```

### RUNTIME (Event loop)

**Backend (Rust)** - `src/main.rs`:
```rust
#[tokio::main]
async fn main() -> Result<()> {
    let mut model = AppModel::default();

    loop {
        tokio::select! {
            Some(msg) = rx.recv() => {
                // 1. Update model
                let (next_model, cmds) = app::update(&model, msg);
                model = next_model;

                // 2. Execute side effects
                for cmd in cmds {
                    match cmd {
                        Command::HandleWorkflow(event) => {
                            workflow.handle_event(event).await?;
                        }
                        Command::None => {}
                    }
                }

                // 3. "View" (log to console)
                println!("{}", app::view(&model));
            }
        }
    }
}
```

**Frontend (Elm)** - Runtime Elm (caché):
```elm
-- Le runtime Elm gère automatiquement:
-- 1. Event listener (clic, HTTP response, Time tick)
-- 2. Appel de update() avec le Msg
-- 3. Exécution des Cmd (HTTP, ports, etc.)
-- 4. Appel de view() avec le nouveau Model
-- 5. Virtual DOM diff + patch
-- 6. [boucle]

main : Program () Model Msg
main =
    Browser.element
        { init = init           -- Model initial + Cmd
        , view = view           -- Model -> Html Msg
        , update = update       -- Msg -> Model -> (Model, Cmd Msg)
        , subscriptions = subscriptions  -- Model -> Sub Msg
        }

subscriptions : Model -> Sub Msg
subscriptions _ =
    Time.every 2000 Tick  -- Tick toutes les 2 secondes
```

## Flux de données identique

### Backend Rust

```
Hardware Event (Scanner)
      ↓
Sniffer TCP reçoit
      ↓
Msg::Input(Scan("BOX-001"))
      ↓
update(&model, msg)
      ↓
(new_model, [Command::HandleWorkflow])
      ↓
execute_command() → workflow.handle_event()
      ↓
Side effects (Storage, LED, etc.)
      ↓
Msg::BoxPersisted
      ↓
[boucle]
```

### Frontend Elm

```
User Event (clic bouton)
      ↓
onClick Refresh
      ↓
Msg: Refresh
      ↓
update Refresh model
      ↓
(new_model, Cmd: fetchBoxes)
      ↓
HTTP GET /api/boxes
      ↓
Response reçue
      ↓
Msg: BoxesReceived (Ok boxes)
      ↓
update BoxesReceived model
      ↓
(new_model with boxes, Cmd.none)
      ↓
view new_model → HTML
      ↓
[boucle]
```

## Différences clés

| Aspect | Backend Rust | Frontend Elm |
|--------|--------------|--------------|
| **Runtime** | Manuel (tokio::select!) | Automatique (Elm runtime) |
| **Side effects** | Async/await | Managed effects (Cmd/Sub) |
| **View** | Console text | HTML Virtual DOM |
| **Typing** | Static (rustc) | Static (elm compiler) |
| **Errors** | Result<T, E> | Maybe, Result |
| **Immutability** | Clone required | Built-in |
| **Concurrency** | tokio tasks | Single-threaded JS |
| **Testing** | cargo test | elm-test |

## Similarités

| Aspect | Backend | Frontend |
|--------|---------|----------|
| **Pattern** | ✅ MVU | ✅ MVU |
| **Pure functions** | ✅ update() | ✅ update |
| **Immutable state** | ✅ Clone | ✅ Built-in |
| **Type safety** | ✅ rustc | ✅ elm compiler |
| **No null** | ✅ Option<T> | ✅ Maybe a |
| **Error handling** | ✅ Result<T, E> | ✅ Result a b |
| **Pattern matching** | ✅ match | ✅ case of |
| **Commands** | ✅ Vec<Command> | ✅ Cmd Msg |
| **Subscriptions** | ✅ tokio::select! | ✅ Sub Msg |

## Types partagés (conceptuellement)

### BoxData

**Rust** - `src/models.rs`:
```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BoxData {
    pub box_id: String,
    pub lines: Vec<BoxLine>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BoxLine {
    pub line_id: String,
    pub article_code: String,
    pub quantity: u32,
    pub location: String,
    pub done: bool,
}
```

**Elm** - `ui/src/Main.elm`:
```elm
type alias BoxData =
    { boxId : String
    , lines : List BoxLine
    }

type alias BoxLine =
    { lineId : String
    , articleCode : String
    , quantity : Int
    , location : String
    , done : Bool
    }

-- Decoder pour parser le JSON du backend
boxDataDecoder : Decoder BoxData
boxDataDecoder =
    Decode.map2 BoxData
        (field "box_id" string)
        (field "lines" (list boxLineDecoder))

boxLineDecoder : Decoder BoxLine
boxLineDecoder =
    Decode.map5 BoxLine
        (field "line_id" string)
        (field "article_code" string)
        (field "quantity" int)
        (field "location" string)
        (field "done" bool)
```

### Sérialisation automatique

**Rust** → JSON:
```rust
let box_data = BoxData {
    box_id: "BOX-001".into(),
    lines: vec![
        BoxLine {
            line_id: "L1".into(),
            article_code: "ART-123".into(),
            quantity: 10,
            location: "A1-B2".into(),
            done: false,
        }
    ],
};

// Serde sérialise automatiquement
Json(box_data)  // → {"box_id":"BOX-001","lines":[{"line_id":"L1",...}]}
```

**JSON → Elm**:
```elm
-- HTTP response automatiquement décodé
Http.get
    { url = "/api/boxes/BOX-001"
    , expect = Http.expectJson BoxDetailReceived boxDataDecoder
    }

-- Elm runtime parse le JSON et crée la valeur typée BoxData
```

## Avantages de l'architecture MVU unifiée

### 1. **Cohérence mentale**

Même pattern des deux côtés = courbe d'apprentissage réduite.

```
Rust:  (Model, Msg, update, view)
        ↕️
Elm:   (Model, Msg, update, view)
```

### 2. **Testabilité**

Les fonctions `update` sont pures des deux côtés :

```rust
// Backend test
#[test]
fn test_shutdown_msg() {
    let model = AppModel::default();
    let (next, _) = update(&model, Msg::Shutdown);
    assert!(!next.is_running);
}
```

```elm
-- Frontend test
test "FetchBoxes sets loading to True" <|
    \_ ->
        let
            model = { boxes = [], loadingBoxes = False, ... }
            (newModel, _) = update FetchBoxes model
        in
        Expect.equal newModel.loadingBoxes True
```

### 3. **Prévisibilité**

Flux de données unidirectionnel :

```
Event → Msg → Update → New Model → View → [wait for event]
```

Pas de mutations cachées, pas de callbacks imbriqués.

### 4. **Debugging**

**Backend**:
- Logs de tous les `Msg` reçus
- Snapshot du `Model` à chaque étape

**Frontend**:
- Elm Debugger (time-travel)
- Voir tous les `Msg` et états du `Model`

### 5. **Type Safety**

Les deux compilateurs vérifient :
- Toutes les branches de `match`/`case` sont couvertes
- Les types sont cohérents
- Pas de valeurs nulles non gérées
- Pas d'erreurs runtime

## Différences philosophiques

### Backend Rust : Performance & Safety

```rust
// Ownership, zero-cost abstractions
async fn handle_event(&mut self, event: InputEvent) -> Result<()> {
    // Borrow checker vérifie les accès mémoire
    self.workflow.process(event).await?;
    Ok(())
}

// Async/await pour concurrence
tokio::spawn(async move {
    sniffer::start(addr, tx).await
});
```

**Priorités**: Performance, concurrence, safety mémoire

### Frontend Elm : Simplicity & Reliability

```elm
-- Pas de runtime errors, jamais
update : Msg -> Model -> (Model, Cmd Msg)
update msg model =
    case msg of
        -- Le compilateur force à gérer tous les cas
        FetchBoxes -> (...)
        BoxesReceived result -> (...)
        -- Si j'ajoute un nouveau Msg, le code ne compile pas
        -- tant que je ne le gère pas ici

-- Pas de null, pas d'undefined
selectedBox : Maybe BoxData  -- Explicite
```

**Priorités**: Zero errors, simplicité, maintenabilité

## Quand utiliser MVU ?

### ✅ Excellent pour :

- Applications avec état complexe
- Interfaces utilisateur interactives
- Systèmes event-driven
- Applications où la fiabilité est critique
- Projets où le refactoring est fréquent

### ⚠️ Moins adapté pour :

- Scripts simples one-shot
- Performance extrême (hot path)
- Interop avec code legacy impératif
- Très petits projets (overhead)

## Ressources

### MVU Pattern
- [Elm Architecture](https://guide.elm-lang.org/architecture/)
- [Redux (MVU pour React)](https://redux.js.org/)
- [TEA (The Elm Architecture)](https://sporto.github.io/elm-patterns/architecture/)

### Rust MVU
- [Crux (Rust MVU framework)](https://github.com/redbadger/crux)
- [Iced (Rust GUI MVU)](https://github.com/iced-rs/iced)

### Elm
- [Elm Guide officiel](https://guide.elm-lang.org/)
- [Elm in Action (livre)](https://www.manning.com/books/elm-in-action)

## Conclusion

L'architecture MVU backend Rust + frontend Elm offre :

1. **Cohérence** : Même pattern, concepts partagés
2. **Type Safety** : Compilateurs stricts des deux côtés
3. **Testabilité** : Fonctions pures faciles à tester
4. **Maintenabilité** : Refactoring guidé par les types
5. **Fiabilité** : Moins de bugs, plus de confiance

C'est un stack idéal pour des applications critiques où la **fiabilité** et la **maintenabilité** sont prioritaires sur la vitesse de développement initiale.
