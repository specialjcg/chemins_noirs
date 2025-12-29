module Types exposing (..)

import Http
import Json.Decode


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
    , clickMode : ClickMode
    , routeMode : RouteMode
    , mapViewMode : MapViewMode
    , viewMode : ViewMode
    , animationState : AnimationState
    , savedRoutes : List SavedRoute
    , saveRouteName : String
    , saveRouteDescription : String
    , showSavedRoutes : Bool
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
    , clickMode = Start
    , routeMode = PointToPoint
    , mapViewMode = Standard
    , viewMode = Map2D
    , animationState = Stopped
    , savedRoutes = []
    , saveRouteName = ""
    , saveRouteDescription = ""
    , showSavedRoutes = False
    }



-- MSG


type Msg
    = StartLatChanged String
    | StartLonChanged String
    | EndLatChanged String
    | EndLonChanged String
    | PopWeightChanged String
    | PavedWeightChanged String
    | LoopDistanceChanged String
    | LoopToleranceChanged String
    | LoopCandidateCountChanged String
    | LoopMaxAscentChanged String
    | LoopMinAscentChanged String
    | Submit
    | SetClickMode ClickMode
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
    | RemoveWaypoint Int
    | ClearWaypoints
    | ToggleCloseLoop
    | ComputeMultiPointRoute
    | NoOp



-- ENUMS


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


type ViewMode
    = Map2D
    | Map3D


type AnimationState
    = Stopped
    | Playing
    | Paused



-- DOMAIN TYPES


type alias Coordinate =
    { lat : Float
    , lon : Float
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
