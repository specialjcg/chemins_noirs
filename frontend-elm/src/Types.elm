module Types exposing (..)

import Array
import Color
import Http
import Json.Decode
import Dict exposing (Dict)


-- MODEL

type alias Model =
    { form : RouteForm
    , loopForm : LoopForm
    , waypoints : List Coordinate
    , closeLoop : Bool
    , pending : Bool
    , lastResponse : Maybe RouteResponse
    , loopCandidates : List LoopCandidate
    , loopMeta : Maybe LoopMeta
    , selectedLoopIdx : Maybe Int
    , error : Maybe String
    , routeMode : RouteMode
    , mapViewMode : MapViewMode
    , viewMode : ViewMode
    , animationState : AnimationState
    , savedRoutes : List SavedRoute
    , saveRouteName : String
    , saveRouteDescription : String
    , showSavedRoutes : Bool
    , showElevationChart : Bool
    , elevationHoverIndex : Maybe Int
    , waypointHistory : List (List Coordinate)
    , waypointFuture : List (List Coordinate)
    , mapRouteHoverIndex : Maybe Int
    , cheminNoir : Bool
    , mapSearch : String
    , mapSearchResults : List GeoResult
    , freehandSegments : Dict Int (List Coordinate)
    , originalResponse : Maybe RouteResponse
    , freehandDrawing : Maybe FreehandDrawingState
    , freehandEnabled : Bool
    , appMode : AppMode
    }


type alias RouteForm =
    { startLat : String
    , startLon : String
    , endLat : String
    , endLon : String
    , wPop : String
    , wPaved : String
    }


type alias LoopForm =
    { distanceKm : String
    , toleranceKm : String
    , candidateCount : String
    , maxAscentM : String
    , minAscentM : String
    }


type alias LoopMeta =
    { targetDistanceKm : Float
    , distanceToleranceKm : Float
    }


type alias FreehandDrawingState =
    { fromIdx : Int
    , points : List Coordinate
    }


defaultRouteForm : RouteForm
defaultRouteForm =
    { startLat = "45.9305"
    , startLon = "4.5776"
    , endLat = "45.9399"
    , endLon = "4.5757"
    , wPop = "1.5"
    , wPaved = "4.0"
    }


defaultLoopForm : LoopForm
defaultLoopForm =
    { distanceKm = "15"
    , toleranceKm = "2.5"
    , candidateCount = "5"
    , maxAscentM = ""
    , minAscentM = ""
    }


initialModel : Model
initialModel =
    { form = defaultRouteForm
    , loopForm = defaultLoopForm
    , waypoints = []
    , closeLoop = False
    , pending = False
    , lastResponse = Nothing
    , loopCandidates = []
    , loopMeta = Nothing
    , selectedLoopIdx = Nothing
    , error = Nothing
    , routeMode = PointToPoint
    , mapViewMode = Topo
    , viewMode = Map2D
    , animationState = Stopped
    , savedRoutes = []
    , saveRouteName = ""
    , saveRouteDescription = ""
    , showSavedRoutes = False
    , showElevationChart = False
    , elevationHoverIndex = Nothing
    , waypointHistory = []
    , waypointFuture = []
    , mapRouteHoverIndex = Nothing
    , cheminNoir = True
    , mapSearch = ""
    , mapSearchResults = []
    , freehandSegments = Dict.empty
    , originalResponse = Nothing
    , freehandDrawing = Nothing
    , freehandEnabled = False
    , appMode = Planning
    }



-- MSG


type Msg
    = PopWeightChanged String
    | PavedWeightChanged String
    | LoopDistanceChanged String
    | LoopToleranceChanged String
    | LoopCandidateCountChanged String
    | LoopMaxAscentChanged String
    | LoopMinAscentChanged String
    | Submit
    | ToggleRouteMode RouteMode
    | ToggleMapView
    | Toggle3DView
    | PlayAnimation
    | PauseAnimation
    | SaveRouteNameChanged String
    | SaveRouteDescriptionChanged String
    | SaveRouteToDb
    | RouteSaved (Result Http.Error SavedRoute)
    | LoadSavedRoutes
    | SavedRoutesLoaded (Result Http.Error (List SavedRoute))
    | LoadSavedRoute Int
    | SavedRouteLoaded (Result Http.Error SavedRoute)
    | DeleteSavedRoute Int
    | RouteDeleted (Result Http.Error ())
    | ToggleFavorite Int
    | FavoriteToggled (Result Http.Error SavedRoute)
    | ToggleSavedRoutesPanel
    | SaveRoute
    | LoadRoute
    | RouteLoadedFromStorage (Result Json.Decode.Error RouteResponse)
    | MapClicked Float Float
    | RouteFetched (Result Http.Error RouteResponse)
    | LoopRouteFetched (Result Http.Error LoopRouteResponse)
    | SelectLoopCandidate Int
    | AddWaypoint Coordinate
    | InsertWaypoint Int Coordinate
    | RemoveWaypoint Int
    | MoveWaypoint Int Float Float
    | ClearWaypoints
    | ToggleCloseLoop
    | ComputeMultiPointRoute
    | ExportGpx
    | CopyShareLink
    | GotGeolocation Float Float
    | RequestGeolocation
    | ToggleElevationChart
    | ElevationChartHover Int
    | ElevationChartLeave
    | UndoWaypoints
    | RedoWaypoints
    | ImportGpxClicked
    | GpxWaypointsReceived (List Coordinate)
    | MapRouteHoverIndex Int
    | MapRouteLeave
    | ToggleCheminNoir
    | MapSearchChanged String
    | SearchMap
    | MapSearchResults (Result Http.Error (List GeoResult))
    | SelectMapSearchResult GeoResult
    | ToggleFreehandMode
    | CancelFreehandDrawing
    | ClearFreehandSegment Int
    | NoOp
      -- Orienteering game
    | EnterOrienteeringMode
    | ExitOrienteeringMode
    | StartGame
    | GameTick Int
    | PlayerClickedDestination Float Float
    | PlayerPositionUpdate Float Float
    | PlayerBearingChanged Float
    | PlayerMovementFinished
    | GameRouteFetched (Result Http.Error RouteResponse)
    | ToggleTopoOverlay
    | PauseGame
    | ResumeGame
    | GameSpeedUp
    | GameSpeedDown
    | SetTargetBearing Float
    | ClearTargetBearing
    | GameKeyLeft
    | GameKeyRight
    | GameKeyForward
    | RoadsFetched (Result Http.Error (List StyledRoad))
    | VegetationFetched (Result Http.Error (List VegetationZone))
    | IgnBuildingsFetched (Result Http.Error (List IgnBuilding))
    | ElevationGridFetched (Result Http.Error ElevationGrid)
    | BuildingsFetched (Result Http.Error (List { center : Coordinate, polygon : List Coordinate }))
    | GameMouseDrag Float
    | GameMouseDown Float
    | GameMouseUp Float
    | GameMapClicked { lat : Float, lon : Float }



-- ENUMS


type RouteMode
    = PointToPoint
    | Loop
    | MultiPoint


type MapViewMode
    = Topo
    | Satellite
    | Hybrid


type ViewMode
    = Map2D
    | Map3D


type AnimationState
    = Stopped
    | Playing
    | Paused


type AppMode
    = Planning
    | Orienteering GameState


type alias GameState =
    { controlPoints : List ControlPoint
    , currentPointIndex : Int
    , playerPosition : Coordinate
    , playerBearing : Float
    , elapsedMs : Int
    , gameStatus : GameStatus
    , movePath : Maybe (List Coordinate)
    , moveProgress : Float
    , totalDistanceM : Float
    , showTopoOverlay : Bool
    , nearestCpDistance : Maybe Float
    , foundFlash : Bool
    , paused : Bool
    , speedMultiplier : Float
    , targetBearing : Maybe Float
    , roads : List StyledRoad
    , vegetation : List VegetationZone
    , ign_buildings : List IgnBuilding
    , elevationGrid : Maybe ElevationGrid
    , buildings : List { center : Coordinate, polygon : List Coordinate }
    , routePath : Array.Array Coordinate
    , routeIndex : Int
    , isDragging : Bool
    , lastMouseX : Float
    , dragStartX : Float
    , debugLog : List String
    }


type alias ControlPoint =
    { position : Coordinate
    , label : String
    , found : Bool
    }


type GameStatus
    = GameSetup
    | GameRunning
    | GameFinished



-- DOMAIN TYPES


type alias Coordinate =
    { lat : Float
    , lon : Float
    }


type alias Coord3D =
    { lat : Float
    , lon : Float
    , alt : Float
    }


type alias ElevationGrid =
    { grid : List (List Float)
    , minAlt : Float
    , maxAlt : Float
    , originLat : Float
    , originLon : Float
    , cellSizeM : Float
    , rows : Int
    , cols : Int
    }


type alias StyledRoad =
    { nature : String
    , coords : List Coord3D
    }


type alias VegetationZone =
    { nature : String
    , coords : List Coord3D
    }


type alias IgnBuilding =
    { nature : String
    , hauteur : Float
    , coords : List Coord3D
    }


type alias GeoResult =
    { lat : Float
    , lon : Float
    , displayName : String
    }


type alias RouteRequest =
    { start : Coordinate
    , end : Coordinate
    , wPop : Float
    , wPaved : Float
    }


type alias MultiPointRouteRequest =
    { waypoints : List Coordinate
    , closeLoop : Bool
    , wPop : Float
    , wPaved : Float
    }


type alias LoopRouteRequest =
    { start : Coordinate
    , targetDistanceKm : Float
    , distanceToleranceKm : Float
    , candidateCount : Int
    , wPop : Float
    , wPaved : Float
    , maxTotalAscent : Maybe Float
    , minTotalAscent : Maybe Float
    }


type alias RouteResponse =
    { path : List Coordinate
    , distanceKm : Float
    , gpxBase64 : String
    , metadata : Maybe RouteMetadata
    , elevationProfile : Maybe ElevationProfile
    , snappedWaypoints : Maybe (List Coordinate)
    , estimatedTimeMinutes : Maybe Int
    , difficulty : Maybe String
    , surfaceBreakdown : Maybe (List ( String, Float ))
    , segments : Maybe (List SegmentStats)
    }


type alias SegmentStats =
    { fromIndex : Int
    , toIndex : Int
    , distanceKm : Float
    , ascentM : Float
    , descentM : Float
    , avgSlopePct : Float
    }


type alias RouteMetadata =
    { pointCount : Int
    , bounds : RouteBounds
    , start : Coordinate
    , end : Coordinate
    }


type alias RouteBounds =
    { minLat : Float
    , maxLat : Float
    , minLon : Float
    , maxLon : Float
    }


type alias ElevationProfile =
    { elevations : List (Maybe Float)
    , minElevation : Maybe Float
    , maxElevation : Maybe Float
    , totalAscent : Float
    , totalDescent : Float
    }


type alias LoopRouteResponse =
    { candidates : List LoopCandidate
    , targetDistanceKm : Float
    , distanceToleranceKm : Float
    }


type alias LoopCandidate =
    { route : RouteResponse
    , distanceErrorKm : Float
    , bearingDeg : Float
    }


type alias SavedRoute =
    { id : Int
    , name : String
    , description : Maybe String
    , createdAt : String
    , updatedAt : String
    , distanceKm : Float
    , totalAscentM : Maybe Float
    , totalDescentM : Maybe Float
    , isFavorite : Bool
    , tags : List String
    , originalWaypoints : Maybe (List Coordinate)
    , routeData : RouteResponse
    }


type alias SaveRouteRequest =
    { name : String
    , description : Maybe String
    , tags : Maybe (List String)
    , originalWaypoints : Maybe (List Coordinate)
    }



-- HELPERS


parseCoordinate : String -> String -> Maybe Coordinate
parseCoordinate latStr lonStr =
    Maybe.map2 Coordinate
        (String.toFloat latStr)
        (String.toFloat lonStr)


formatCoord : Float -> String
formatCoord value =
    String.fromFloat value
        |> (\s ->
                if String.length s > 7 then
                    String.left 7 s

                else
                    s
           )


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


haversineMeters : Coordinate -> Coordinate -> Float
haversineMeters a b =
    let
        r =
            6371000

        dLat =
            degrees (b.lat - a.lat)

        dLon =
            degrees (b.lon - a.lon)

        lat1 =
            degrees a.lat

        lat2 =
            degrees b.lat

        sinDLat =
            sin (dLat / 2)

        sinDLon =
            sin (dLon / 2)

        h =
            sinDLat * sinDLat + cos lat1 * cos lat2 * sinDLon * sinDLon
    in
    2 * r * asin (sqrt h)


isNearControlPoint : Coordinate -> ControlPoint -> Bool
isNearControlPoint playerPos cp =
    haversineMeters playerPos cp.position < 30


initialGameState : List Coordinate -> GameState
initialGameState waypoints =
    let
        -- First waypoint is start (not a control point to find)
        -- Remaining waypoints are control points
        cps =
            List.drop 1 waypoints

        controlPoints =
            List.indexedMap
                (\i coord ->
                    { position = coord
                    , label = String.fromInt (i + 1)
                    , found = False
                    }
                )
                cps

        startPos =
            List.head waypoints
                |> Maybe.withDefault { lat = 0, lon = 0 }
    in
    { controlPoints = controlPoints
    , currentPointIndex = 0
    , playerPosition = startPos
    , playerBearing = 0
    , elapsedMs = 0
    , gameStatus = GameSetup
    , movePath = Nothing
    , moveProgress = 0
    , totalDistanceM = 0
    , showTopoOverlay = False
    , nearestCpDistance = Nothing
    , foundFlash = False
    , paused = False
    , speedMultiplier = 1.0
    , targetBearing = Nothing
    , roads = []
    , vegetation = []
    , ign_buildings = []
    , elevationGrid = Nothing
    , buildings = []
    , routePath = Array.empty
    , routeIndex = 0
    , isDragging = False
    , lastMouseX = 0
    , dragStartX = 0
    , debugLog = []
    }


routeBounds : RouteResponse -> RouteBounds
routeBounds route =
    case route.metadata of
        Just meta ->
            meta.bounds

        Nothing ->
            calculateBounds route.path


calculateBounds : List Coordinate -> RouteBounds
calculateBounds coords =
    let
        lats =
            List.map .lat coords

        lons =
            List.map .lon coords

        minLat =
            List.minimum lats |> Maybe.withDefault 0

        maxLat =
            List.maximum lats |> Maybe.withDefault 0

        minLon =
            List.minimum lons |> Maybe.withDefault 0

        maxLon =
            List.maximum lons |> Maybe.withDefault 0
    in
    { minLat = minLat
    , maxLat = maxLat
    , minLon = minLon
    , maxLon = maxLon
    }
