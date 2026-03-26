module Main exposing (main)

{-| Application principale - Architecture MVU (Model-View-Update).
Approche fonctionnelle pure : pas de mutations, fonctions pures, gestion explicite des effets.
-}

import Api
import Array
import Browser
import Browser.Events
import Time
import Decoders
import Encoders
import GameEngine
import TopoTile
import Html exposing (..)
import Html.Attributes exposing (class, style)
import Html.Events
import Json.Decode
import Ports
import Dict exposing (Dict)
import Types exposing (..)
import View.Form as Form
import View.Game as Game
import View.Preview as Preview
import View.World3D as World3D



-- MAIN


main : Program () Model Msg
main =
    Browser.element
        { init = init
        , view = view
        , update = update
        , subscriptions = subscriptions
        }



-- INIT


init : () -> ( Model, Cmd Msg )
init _ =
    ( initialModel
    , Cmd.batch
        [ Ports.initMap ()
        , Api.listSavedRoutes SavedRoutesLoaded
        ]
    )



-- UPDATE


update : Msg -> Model -> ( Model, Cmd Msg )
update msg model =
    case msg of
        PopWeightChanged val ->
            let
                form =
                    model.form

                newForm =
                    { form | wPop = val }
            in
            ( { model | form = newForm }
            , resetLoopCandidatesCmd model
            )

        PavedWeightChanged val ->
            let
                form =
                    model.form

                newForm =
                    { form | wPaved = val }
            in
            ( { model | form = newForm }
            , resetLoopCandidatesCmd model
            )

        LoopDistanceChanged val ->
            let
                loopForm =
                    model.loopForm

                newLoopForm =
                    { loopForm | distanceKm = val }
            in
            ( { model | loopForm = newLoopForm }
            , resetLoopCandidatesCmd model
            )

        LoopToleranceChanged val ->
            let
                loopForm =
                    model.loopForm

                newLoopForm =
                    { loopForm | toleranceKm = val }
            in
            ( { model | loopForm = newLoopForm }
            , resetLoopCandidatesCmd model
            )

        LoopCandidateCountChanged val ->
            let
                loopForm =
                    model.loopForm

                newLoopForm =
                    { loopForm | candidateCount = val }
            in
            ( { model | loopForm = newLoopForm }
            , resetLoopCandidatesCmd model
            )

        LoopMaxAscentChanged val ->
            let
                loopForm =
                    model.loopForm

                newLoopForm =
                    { loopForm | maxAscentM = val }
            in
            ( { model | loopForm = newLoopForm }
            , resetLoopCandidatesCmd model
            )

        LoopMinAscentChanged val ->
            let
                loopForm =
                    model.loopForm

                newLoopForm =
                    { loopForm | minAscentM = val }
            in
            ( { model | loopForm = newLoopForm }
            , resetLoopCandidatesCmd model
            )

        Submit ->
            if model.pending then
                ( model, Cmd.none )

            else
                case model.routeMode of
                    PointToPoint ->
                        if List.length model.waypoints < 2 then
                            ( { model | error = Just "Placez 2 points sur la carte" }
                            , Cmd.none
                            )

                        else
                            let
                                startWp =
                                    List.head model.waypoints |> Maybe.withDefault { lat = 0, lon = 0 }

                                endWp =
                                    List.head (List.drop 1 model.waypoints) |> Maybe.withDefault { lat = 0, lon = 0 }

                                ( wPop, wPaved ) =
                                    if model.cheminNoir then
                                        ( 5.0, 8.0 )

                                    else
                                        ( String.toFloat model.form.wPop |> Maybe.withDefault 1.5
                                        , String.toFloat model.form.wPaved |> Maybe.withDefault 4.0
                                        )

                                request =
                                    { start = startWp
                                    , end = endWp
                                    , wPop = wPop
                                    , wPaved = wPaved
                                    }
                            in
                            ( { model | pending = True, error = Nothing }
                            , Api.fetchRoute request RouteFetched
                            )

                    Loop ->
                        if List.isEmpty model.waypoints then
                            ( { model | error = Just "Placez un point de départ sur la carte" }
                            , Cmd.none
                            )

                        else
                            case loopFormToRequest model.cheminNoir model.form model.loopForm model.waypoints of
                                Ok request ->
                                    ( { model
                                        | pending = True
                                        , error = Nothing
                                        , loopCandidates = []
                                        , loopMeta = Nothing
                                        , selectedLoopIdx = Nothing
                                      }
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
                                (multiPointRequest model)
                                RouteFetched
                            )

        RouteFetched result ->
            case result of
                Ok route ->
                    if model.routeMode == MultiPoint then
                        -- Use backend snapped positions (exact on-road projections at segment junctions)
                        -- Falls back to original click positions if backend doesn't provide them
                        let
                            markerPositions =
                                Maybe.withDefault model.waypoints route.snappedWaypoints

                            displayRoute =
                                applyFreehandOverrides model.freehandSegments model.freehandDrawing markerPositions route

                            updatedModel =
                                applyRoute model displayRoute
                        in
                        ( { updatedModel | waypoints = markerPositions, originalResponse = Just route }
                        , Cmd.batch
                            [ Ports.updateRoute displayRoute.path
                            , Ports.updateWaypointMarkers markerPositions
                            ]
                        )

                    else
                        ( applyRoute model route
                        , Cmd.batch
                            [ Ports.updateRoute route.path
                            , Ports.updateWaypointMarkers model.waypoints
                            , case route.metadata of
                                Just meta ->
                                    Ports.updateBbox meta.bounds

                                Nothing ->
                                    Cmd.none
                            , centerOnRouteCmd route
                            ]
                        )

                Err httpError ->
                    ( { model
                        | pending = False
                        , error = Just (httpErrorToString httpError)
                      }
                    , Cmd.batch
                        [ Ports.updateRoute []
                        , Ports.updateWaypointMarkers model.waypoints
                        ]
                    )

        LoopRouteFetched result ->
            case result of
                Ok response ->
                    if List.isEmpty response.candidates then
                        ( { model
                            | pending = False
                            , error = Just "Aucune boucle trouvée pour ces paramètres"
                            , loopMeta = Nothing
                          }
                        , Ports.updateRoute []
                        )

                    else
                        let
                            meta =
                                { targetDistanceKm = response.targetDistanceKm
                                , distanceToleranceKm = response.distanceToleranceKm
                                }

                            firstCandidate =
                                List.head response.candidates
                        in
                        case firstCandidate of
                            Just candidate ->
                                ( { model
                                    | pending = False
                                    , loopCandidates = response.candidates
                                    , loopMeta = Just meta
                                    , selectedLoopIdx = Just 0
                                    , error = Nothing
                                  }
                                    |> (\m -> applyRoute m candidate.route)
                                , Cmd.batch
                                    [ Ports.updateRoute candidate.route.path
                                    , case candidate.route.metadata of
                                        Just metadata ->
                                            Ports.updateBbox metadata.bounds

                                        Nothing ->
                                            Cmd.none
                                    ]
                                )

                            Nothing ->
                                ( { model | pending = False }
                                , Cmd.none
                                )

                Err httpError ->
                    ( { model
                        | pending = False
                        , error = Just (httpErrorToString httpError)
                      }
                    , Ports.updateRoute []
                    )

        SelectLoopCandidate idx ->
            case List.head (List.drop idx model.loopCandidates) of
                Just candidate ->
                    ( { model | selectedLoopIdx = Just idx }
                        |> (\m -> applyRoute m candidate.route)
                    , Cmd.batch
                        [ Ports.updateRoute candidate.route.path
                        , case candidate.route.metadata of
                            Just meta ->
                                Ports.updateBbox meta.bounds

                            Nothing ->
                                Cmd.none
                        ]
                    )

                Nothing ->
                    ( model, Cmd.none )

        MapClicked lat lon ->
            case model.appMode of
                Orienteering _ ->
                    update (PlayerClickedDestination lat lon) model

                Planning ->
                    handleMapClick lat lon model

        AddWaypoint coord ->
            let
                modelWithHistory =
                    pushWaypointHistory model

                newWaypoints =
                    modelWithHistory.waypoints ++ [ coord ]

                newModel =
                    { modelWithHistory | waypoints = newWaypoints, error = Nothing }
            in
            if List.length newWaypoints >= 2 then
                let
                    ( routedModel, routeCmd ) =
                        update ComputeMultiPointRoute newModel
                in
                ( routedModel
                , Cmd.batch
                    [ Ports.updateWaypointMarkers newWaypoints
                    , routeCmd
                    ]
                )

            else
                ( newModel
                , Ports.updateWaypointMarkers newWaypoints
                )

        InsertWaypoint idx coord ->
            let
                modelWithHistory =
                    pushWaypointHistory model

                newWaypoints =
                    List.take (idx + 1) modelWithHistory.waypoints
                        ++ [ coord ]
                        ++ List.drop (idx + 1) modelWithHistory.waypoints

                newModel =
                    { modelWithHistory | waypoints = newWaypoints, error = Nothing }
            in
            if List.length newWaypoints >= 2 then
                let
                    ( routedModel, routeCmd ) =
                        update ComputeMultiPointRoute newModel
                in
                ( routedModel
                , Cmd.batch
                    [ Ports.updateWaypointMarkers newWaypoints
                    , routeCmd
                    ]
                )

            else
                ( newModel
                , Ports.updateWaypointMarkers newWaypoints
                )

        RemoveWaypoint idx ->
            let
                modelWithHistory =
                    pushWaypointHistory model

                newWaypoints =
                    List.take idx modelWithHistory.waypoints
                        ++ List.drop (idx + 1) modelWithHistory.waypoints

                newModel =
                    { modelWithHistory | waypoints = newWaypoints }
            in
            if List.length newWaypoints >= 2 then
                let
                    ( routedModel, routeCmd ) =
                        update ComputeMultiPointRoute newModel
                in
                ( routedModel
                , Cmd.batch
                    [ Ports.updateWaypointMarkers newWaypoints
                    , routeCmd
                    ]
                )

            else
                ( newModel
                , Cmd.batch
                    [ Ports.updateWaypointMarkers newWaypoints
                    , Ports.updateRoute []
                    ]
                )

        MoveWaypoint idx lat lon ->
            let
                modelWithHistory =
                    pushWaypointHistory model

                newWaypoints =
                    List.indexedMap
                        (\i wp ->
                            if i == idx then
                                { lat = lat, lon = lon }

                            else
                                wp
                        )
                        modelWithHistory.waypoints

                newModel =
                    { modelWithHistory | waypoints = newWaypoints, error = Nothing }
            in
            if List.length newWaypoints >= 2 then
                let
                    ( routedModel, routeCmd ) =
                        update ComputeMultiPointRoute newModel
                in
                ( routedModel, routeCmd )

            else
                ( newModel, Cmd.none )

        ClearWaypoints ->
            let
                modelWithHistory =
                    pushWaypointHistory model
            in
            ( { modelWithHistory
                | waypoints = []
                , lastResponse = Nothing
                , error = Nothing
              }
            , Cmd.batch
                [ Ports.updateWaypointMarkers []
                , Ports.updateRoute []
                ]
            )

        UndoWaypoints ->
            case model.waypointHistory of
                prev :: rest ->
                    let
                        newModel =
                            { model
                                | waypoints = prev
                                , waypointHistory = rest
                                , waypointFuture = model.waypoints :: model.waypointFuture
                                , error = Nothing
                                , freehandSegments = Dict.empty
                                , freehandDrawing = Nothing
                            }
                    in
                    if List.length prev >= 2 then
                        let
                            ( routedModel, routeCmd ) =
                                update ComputeMultiPointRoute newModel
                        in
                        ( routedModel
                        , Cmd.batch
                            [ Ports.updateWaypointMarkers prev
                            , routeCmd
                            ]
                        )

                    else
                        ( { newModel | lastResponse = Nothing }
                        , Cmd.batch
                            [ Ports.updateWaypointMarkers prev
                            , Ports.updateRoute []
                            ]
                        )

                [] ->
                    ( model, Cmd.none )

        RedoWaypoints ->
            case model.waypointFuture of
                next :: rest ->
                    let
                        newModel =
                            { model
                                | waypoints = next
                                , waypointFuture = rest
                                , waypointHistory = model.waypoints :: model.waypointHistory
                                , error = Nothing
                                , freehandSegments = Dict.empty
                                , freehandDrawing = Nothing
                            }
                    in
                    if List.length next >= 2 then
                        let
                            ( routedModel, routeCmd ) =
                                update ComputeMultiPointRoute newModel
                        in
                        ( routedModel
                        , Cmd.batch
                            [ Ports.updateWaypointMarkers next
                            , routeCmd
                            ]
                        )

                    else
                        ( { newModel | lastResponse = Nothing }
                        , Cmd.batch
                            [ Ports.updateWaypointMarkers next
                            , Ports.updateRoute []
                            ]
                        )

                [] ->
                    ( model, Cmd.none )

        ToggleCloseLoop ->
            let
                newModel =
                    { model | closeLoop = not model.closeLoop }
            in
            if List.length newModel.waypoints >= 2 then
                update ComputeMultiPointRoute newModel

            else
                ( newModel, Cmd.none )

        ComputeMultiPointRoute ->
            update Submit model

        ToggleRouteMode mode ->
            ( { model
                | routeMode = mode
                , waypoints = []
                , lastResponse = Nothing
                , loopCandidates = []
                , loopMeta = Nothing
                , selectedLoopIdx = Nothing
                , error = Nothing
                , freehandSegments = Dict.empty
                , originalResponse = Nothing
                , freehandDrawing = Nothing
                , freehandEnabled = False
              }
            , Cmd.batch
                [ Ports.updateWaypointMarkers []
                , Ports.updateRoute []
                ]
            )

        ToggleMapView ->
            let
                newMode =
                    case model.mapViewMode of
                        Topo ->
                            Satellite

                        Satellite ->
                            Hybrid

                        Hybrid ->
                            Topo

                styleStr =
                    case newMode of
                        Topo ->
                            "topo"

                        Satellite ->
                            "satellite"

                        Hybrid ->
                            "hybrid"
            in
            ( { model | mapViewMode = newMode }
            , Ports.switchMapStyle styleStr
            )

        Toggle3DView ->
            let
                newMode =
                    case model.viewMode of
                        Map2D ->
                            Map3D

                        Map3D ->
                            Map2D
            in
            ( { model | viewMode = newMode }
            , Ports.toggleThree3DView (newMode == Map3D)
            )

        PlayAnimation ->
            ( { model | animationState = Playing }
            , Ports.startAnimation ()
            )

        PauseAnimation ->
            ( { model | animationState = Stopped }
            , Ports.stopAnimation ()
            )

        SaveRouteNameChanged name ->
            ( { model | saveRouteName = name }
            , Cmd.none
            )

        SaveRouteDescriptionChanged description ->
            ( { model | saveRouteDescription = description }
            , Cmd.none
            )

        SaveRouteToDb ->
            case model.lastResponse of
                Just route ->
                    let
                        -- Include original waypoints for multi-point routes
                        originalWaypoints =
                            if List.isEmpty model.waypoints then
                                Nothing
                            else
                                Just model.waypoints

                        request =
                            { name = model.saveRouteName
                            , description =
                                if String.isEmpty model.saveRouteDescription then
                                    Nothing

                                else
                                    Just model.saveRouteDescription
                            , tags = Nothing
                            , originalWaypoints = originalWaypoints
                            }
                    in
                    ( { model | pending = True }
                    , Api.saveRouteToDb request route RouteSaved
                    )

                Nothing ->
                    ( { model | error = Just "Aucune route à sauvegarder" }
                    , Cmd.none
                    )

        RouteSaved result ->
            case result of
                Ok savedRoute ->
                    ( { model
                        | pending = False
                        , error = Nothing
                        , saveRouteName = ""
                        , saveRouteDescription = ""
                        , savedRoutes = savedRoute :: model.savedRoutes
                      }
                    , Cmd.none
                    )

                Err error ->
                    ( { model
                        | pending = False
                        , error = Just (httpErrorToString error)
                      }
                    , Cmd.none
                    )

        LoadSavedRoutes ->
            ( { model | pending = True }
            , Api.listSavedRoutes SavedRoutesLoaded
            )

        SavedRoutesLoaded result ->
            case result of
                Ok routes ->
                    ( { model
                        | pending = False
                        , savedRoutes = routes
                        , showSavedRoutes = True
                        , error = Nothing
                      }
                    , Cmd.none
                    )

                Err error ->
                    ( { model
                        | pending = False
                        , error = Just (httpErrorToString error)
                      }
                    , Cmd.none
                    )

        LoadSavedRoute id ->
            ( { model | pending = True }
            , Api.getSavedRoute id SavedRouteLoaded
            )

        SavedRouteLoaded result ->
            case result of
                Ok savedRoute ->
                    let
                        route = savedRoute.routeData

                        -- Use saved waypoints if available, otherwise backward compatible extraction
                        waypoints =
                            case savedRoute.originalWaypoints of
                                Just wp -> wp
                                Nothing ->
                                    -- Backward compatibility: extract from path
                                    route.path
                                        |> List.drop 1
                                        |> List.reverse
                                        |> List.drop 1
                                        |> List.reverse

                        markerCmds =
                            [ Ports.updateWaypointMarkers waypoints ]
                    in
                    ( applySavedRoute { model | pending = False, error = Nothing } route waypoints
                    , Cmd.batch
                        ([ Ports.updateRoute route.path
                         , case route.metadata of
                            Just meta ->
                                Ports.centerOnMarkers { start = meta.start, end = meta.end }

                            Nothing ->
                                Cmd.none
                         ]
                            ++ markerCmds
                        )
                    )

                Err error ->
                    ( { model
                        | pending = False
                        , error = Just (httpErrorToString error)
                      }
                    , Cmd.none
                    )

        DeleteSavedRoute id ->
            ( { model | pending = True }
            , Api.deleteSavedRoute id RouteDeleted
            )

        RouteDeleted result ->
            case result of
                Ok () ->
                    ( { model | pending = False, error = Nothing }
                    , Api.listSavedRoutes SavedRoutesLoaded
                    )

                Err error ->
                    ( { model
                        | pending = False
                        , error = Just (httpErrorToString error)
                      }
                    , Cmd.none
                    )

        ToggleFavorite id ->
            ( { model | pending = True }
            , Api.toggleFavorite id FavoriteToggled
            )

        FavoriteToggled result ->
            case result of
                Ok updatedRoute ->
                    let
                        updateRoute r =
                            if r.id == updatedRoute.id then
                                updatedRoute

                            else
                                r
                    in
                    ( { model
                        | pending = False
                        , savedRoutes = List.map updateRoute model.savedRoutes
                        , error = Nothing
                      }
                    , Cmd.none
                    )

                Err error ->
                    ( { model
                        | pending = False
                        , error = Just (httpErrorToString error)
                      }
                    , Cmd.none
                    )

        ToggleSavedRoutesPanel ->
            ( { model | showSavedRoutes = not model.showSavedRoutes }
            , if not model.showSavedRoutes && List.isEmpty model.savedRoutes then
                Api.listSavedRoutes SavedRoutesLoaded

              else
                Cmd.none
            )

        SaveRoute ->
            case model.lastResponse of
                Just route ->
                    ( model
                    , Ports.saveRouteToLocalStorage (Encoders.encodeRouteResponse route)
                    )

                Nothing ->
                    ( model, Cmd.none )

        LoadRoute ->
            ( model
            , Ports.loadRouteFromLocalStorage ()
            )

        RouteLoadedFromStorage result ->
            case result of
                Ok route ->
                    ( applyRoute { model | error = Nothing } route
                    , Cmd.batch
                        [ Ports.updateRoute route.path
                        , case route.metadata of
                            Just meta ->
                                Ports.centerOnMarkers { start = meta.start, end = meta.end }

                            Nothing ->
                                Cmd.none
                        ]
                    )

                Err _ ->
                    ( { model | error = Just "Aucune route sauvegardée trouvée" }
                    , Cmd.none
                    )

        ExportGpx ->
            case model.lastResponse of
                Just route ->
                    ( model
                    , Ports.downloadGpx
                        { filename = "chemins-noirs.gpx"
                        , content = generateGpx route
                        }
                    )

                Nothing ->
                    ( model, Cmd.none )

        CopyShareLink ->
            let
                waypointStr =
                    model.waypoints
                        |> List.map (\c -> String.fromFloat c.lat ++ "," ++ String.fromFloat c.lon)
                        |> String.join ";"
            in
            ( model
            , Ports.copyToClipboard ("#w=" ++ waypointStr)
            )

        GotGeolocation lat lon ->
            update (AddWaypoint { lat = lat, lon = lon }) model

        RequestGeolocation ->
            ( model, Ports.requestGeolocation () )

        ToggleElevationChart ->
            ( { model | showElevationChart = not model.showElevationChart }
            , Cmd.none
            )

        ElevationChartHover idx ->
            let
                coordAtIndex =
                    case model.lastResponse of
                        Just route ->
                            route.path
                                |> List.drop idx
                                |> List.head

                        Nothing ->
                            Nothing

                hoverCmd =
                    case coordAtIndex of
                        Just c ->
                            Ports.setElevationHoverMarker (Just { lat = c.lat, lon = c.lon })

                        Nothing ->
                            Cmd.none
            in
            ( { model | elevationHoverIndex = Just idx }
            , hoverCmd
            )

        ElevationChartLeave ->
            ( { model | elevationHoverIndex = Nothing }
            , Ports.setElevationHoverMarker Nothing
            )

        ImportGpxClicked ->
            ( model, Ports.triggerGpxImport () )

        GpxWaypointsReceived rawCoords ->
            let
                coords =
                    List.map (\c -> { lat = c.lat, lon = c.lon }) rawCoords

                modelWithHistory =
                    pushWaypointHistory model

                newModel =
                    { modelWithHistory
                        | waypoints = coords
                        , routeMode = MultiPoint
                        , error = Nothing
                    }
            in
            if List.length coords >= 2 then
                let
                    ( routedModel, routeCmd ) =
                        update ComputeMultiPointRoute newModel
                in
                ( routedModel
                , Cmd.batch
                    [ Ports.updateWaypointMarkers coords
                    , routeCmd
                    ]
                )

            else
                ( newModel
                , Ports.updateWaypointMarkers coords
                )

        MapRouteHoverIndex idx ->
            ( { model | mapRouteHoverIndex = Just idx }
            , Cmd.none
            )

        MapRouteLeave ->
            ( { model | mapRouteHoverIndex = Nothing }
            , Cmd.none
            )

        ToggleCheminNoir ->
            ( { model | cheminNoir = not model.cheminNoir }
            , Cmd.none
            )

        MapSearchChanged val ->
            ( { model | mapSearch = val, mapSearchResults = [] }
            , Cmd.none
            )

        SearchMap ->
            if String.isEmpty (String.trim model.mapSearch) then
                ( model, Cmd.none )

            else
                ( model
                , Api.geocodeAddress model.mapSearch MapSearchResults
                )

        MapSearchResults result ->
            case result of
                Ok results ->
                    ( { model | mapSearchResults = results }
                    , Cmd.none
                    )

                Err _ ->
                    ( { model | mapSearchResults = [] }
                    , Cmd.none
                    )

        SelectMapSearchResult geo ->
            ( { model | mapSearchResults = [], mapSearch = "" }
            , Ports.centerMapOn { lat = geo.lat, lon = geo.lon }
            )

        ToggleFreehandMode ->
            let
                newEnabled =
                    not model.freehandEnabled
            in
            if newEnabled then
                ( { model | freehandEnabled = True }
                , Cmd.none
                )

            else
                -- Turning off: cancel any active drawing
                let
                    newModel =
                        { model | freehandEnabled = False, freehandDrawing = Nothing }
                in
                rebuildAndDisplayRoute newModel

        CancelFreehandDrawing ->
            let
                newModel =
                    { model | freehandDrawing = Nothing }
            in
            rebuildAndDisplayRoute newModel

        ClearFreehandSegment idx ->
            let
                newModel =
                    { model | freehandSegments = Dict.remove idx model.freehandSegments }
            in
            rebuildAndDisplayRoute newModel

        NoOp ->
            ( model, Cmd.none )

        -- Orienteering game messages
        EnterOrienteeringMode ->
            if List.length model.waypoints >= 2 then
                ( { model | appMode = Orienteering (initialGameState model.waypoints) }
                , Cmd.none
                )

            else
                ( { model | error = Just "Placez au moins 2 balises sur la carte" }
                , Cmd.none
                )

        ExitOrienteeringMode ->
            ( { model | appMode = Planning }
            , Ports.exitGameView ()
            )

        StartGame ->
            case model.appMode of
                Orienteering gs ->
                    let
                        initialRoads =
                            case model.lastResponse of
                                Just r ->
                                    [ r.path ]

                                Nothing ->
                                    []

                        -- Snap player to nearest road point (first point of route)
                        startPos =
                            case model.lastResponse of
                                Just r ->
                                    List.head r.path
                                        |> Maybe.withDefault gs.playerPosition

                                Nothing ->
                                    gs.playerPosition

                        routeArray =
                            case model.lastResponse of
                                Just r ->
                                    Array.fromList r.path

                                Nothing ->
                                    Array.empty

                        newGs =
                            { gs
                                | gameStatus = GameRunning
                                , roads = initialRoads
                                , routePath = routeArray
                                , routeIndex = 0
                                , playerPosition = startPos
                            }

                        routeCoords =
                            case model.lastResponse of
                                Just r ->
                                    List.map (\c -> { lat = c.lat, lon = c.lon }) r.path

                                Nothing ->
                                    []

                        cpData =
                            List.map
                                (\cp ->
                                    { lat = cp.position.lat
                                    , lon = cp.position.lon
                                    , label = cp.label
                                    }
                                )
                                gs.controlPoints
                    in
                    ( { model | appMode = Orienteering newGs }
                    , Cmd.batch
                        [ Ports.enterGameView
                            { lat = startPos.lat
                            , lon = startPos.lon
                            , bearing = gs.playerBearing
                            }
                        , Api.fetchIgnRoads startPos 0.01 RoadsFetched
                        , TopoTile.loadTopoGrid startPos.lat startPos.lon TopoTileLoaded
                        , logCmd ("START pos=" ++ String.fromFloat startPos.lat ++ "," ++ String.fromFloat startPos.lon)
                        ]
                    )

                _ ->
                    ( model, Cmd.none )

        GameTick elapsed ->
            case model.appMode of
                Orienteering gs ->
                    if gs.gameStatus == GameRunning then
                        ( { model | appMode = Orienteering { gs | elapsedMs = gs.elapsedMs + elapsed } }
                        , Cmd.none
                        )

                    else
                        ( model, Cmd.none )

                _ ->
                    ( model, Cmd.none )

        PlayerClickedDestination lat lon ->
            case model.appMode of
                Orienteering gs ->
                    if gs.gameStatus == GameRunning && not model.pending && gs.movePath == Nothing then
                        let
                            request =
                                { start = gs.playerPosition
                                , end = { lat = lat, lon = lon }
                                , wPop = 5.0
                                , wPaved = 8.0
                                }
                        in
                        ( { model | pending = True }
                        , Api.fetchRoute request GameRouteFetched
                        )

                    else
                        ( model, Cmd.none )

                _ ->
                    ( model, Cmd.none )

        GameRouteFetched result ->
            case ( model.appMode, result ) of
                ( Orienteering gs, Ok route ) ->
                    let
                        endPos =
                            List.reverse route.path
                                |> List.head
                                |> Maybe.withDefault gs.playerPosition

                        -- Calculate bearing towards next point on route (immediate direction)
                        newBearing =
                            case List.drop 1 route.path |> List.head of
                                Just dest ->
                                    let
                                        dLon =
                                            (dest.lon - gs.playerPosition.lon) * pi / 180

                                        lat1 =
                                            gs.playerPosition.lat * pi / 180

                                        lat2 =
                                            dest.lat * pi / 180

                                        y =
                                            sin dLon * cos lat2

                                        x =
                                            cos lat1 * sin lat2 - sin lat1 * cos lat2 * cos dLon
                                    in
                                    toFloat (modBy 360 (round (atan2 y x * 180 / pi) + 360))

                                Nothing ->
                                    gs.playerBearing

                        -- Check proximity to control points at new position
                        currentCp =
                            List.drop gs.currentPointIndex gs.controlPoints
                                |> List.head

                        ( updatedCps, nextIdx, finished ) =
                            case currentCp of
                                Just cp ->
                                    if haversineMeters endPos cp.position < 30 then
                                        let
                                            cps =
                                                List.indexedMap
                                                    (\i c ->
                                                        if i == gs.currentPointIndex then
                                                            { c | found = True }

                                                        else
                                                            c
                                                    )
                                                    gs.controlPoints

                                            newIdx =
                                                gs.currentPointIndex + 1
                                        in
                                        ( cps, newIdx, newIdx >= List.length gs.controlPoints )

                                    else
                                        ( gs.controlPoints, gs.currentPointIndex, False )

                                Nothing ->
                                    ( gs.controlPoints, gs.currentPointIndex, False )

                        newStatus =
                            if finished then
                                GameFinished

                            else
                                gs.gameStatus

                        newGs =
                            { gs
                                | playerPosition = endPos
                                , playerBearing = newBearing
                                , controlPoints = updatedCps
                                , currentPointIndex = nextIdx
                                , gameStatus = newStatus
                                , foundFlash = finished || (nextIdx > gs.currentPointIndex)
                            }
                    in
                    ( { model | appMode = Orienteering newGs, pending = False }
                    , Api.fetchIgnRoads endPos 0.005 RoadsFetched
                    )

                ( _, Err httpError ) ->
                    ( { model | pending = False, error = Just (httpErrorToString httpError) }
                    , Cmd.none
                    )

                _ ->
                    ( { model | pending = False }, Cmd.none )

        PlayerPositionUpdate lat lon ->
            case model.appMode of
                Orienteering gs ->
                    let
                        newPos =
                            { lat = lat, lon = lon }

                        -- Find distance to current target control point
                        currentCp =
                            List.drop gs.currentPointIndex gs.controlPoints
                                |> List.head

                        distToCurrent =
                            case currentCp of
                                Just cp ->
                                    Just (haversineMeters newPos cp.position)

                                Nothing ->
                                    Nothing

                        -- Check if within 10m = found!
                        justFound =
                            case distToCurrent of
                                Just d ->
                                    d < 10

                                Nothing ->
                                    False

                        ( updatedCps, nextIdx, finished ) =
                            if justFound then
                                let
                                    cps =
                                        List.indexedMap
                                            (\i c ->
                                                if i == gs.currentPointIndex then
                                                    { c | found = True }

                                                else
                                                    c
                                            )
                                            gs.controlPoints

                                    newIdx =
                                        gs.currentPointIndex + 1
                                in
                                ( cps, newIdx, newIdx >= List.length gs.controlPoints )

                            else
                                ( gs.controlPoints, gs.currentPointIndex, False )

                        newStatus =
                            if finished then
                                GameFinished

                            else
                                gs.gameStatus

                        newGs =
                            { gs
                                | playerPosition = newPos
                                , controlPoints = updatedCps
                                , currentPointIndex = nextIdx
                                , gameStatus = newStatus
                                , nearestCpDistance = distToCurrent
                                , foundFlash = justFound
                            }
                    in
                    ( { model | appMode = Orienteering newGs }
                    , Cmd.none
                    )

                _ ->
                    ( model, Cmd.none )

        PlayerBearingChanged bearing ->
            case model.appMode of
                Orienteering gs ->
                    ( { model | appMode = Orienteering { gs | playerBearing = bearing } }
                    , Cmd.none
                    )

                _ ->
                    ( model, Cmd.none )

        PlayerMovementFinished ->
            case model.appMode of
                Orienteering gs ->
                    ( { model | appMode = Orienteering { gs | movePath = Nothing, moveProgress = 0 } }
                    , Cmd.none
                    )

                _ ->
                    ( model, Cmd.none )

        ToggleTopoOverlay ->
            case model.appMode of
                Orienteering gs ->
                    let
                        newShow =
                            not gs.showTopoOverlay

                        newGs =
                            { gs | showTopoOverlay = newShow }

                    in
                    ( { model | appMode = Orienteering newGs }
                    , if newShow then
                        Ports.showTopoOverlay
                            { show = True
                            , lat = gs.playerPosition.lat
                            , lon = gs.playerPosition.lon
                            }

                      else
                        -- Retour en mode walk
                        Ports.updateGameCamera
                            { lat = gs.playerPosition.lat
                            , lon = gs.playerPosition.lon
                            , bearing = gs.playerBearing
                            }
                    )

                _ ->
                    ( model, Cmd.none )

        PauseGame ->
            case model.appMode of
                Orienteering gs ->
                    ( { model | appMode = Orienteering { gs | paused = True } }
                    , Cmd.none
                    )

                _ ->
                    ( model, Cmd.none )

        ResumeGame ->
            case model.appMode of
                Orienteering gs ->
                    ( { model | appMode = Orienteering { gs | paused = False } }
                    , Cmd.none
                    )

                _ ->
                    ( model, Cmd.none )

        GameSpeedUp ->
            case model.appMode of
                Orienteering gs ->
                    let
                        newSpeed =
                            Basics.min 5.0 (gs.speedMultiplier * 2)
                    in
                    ( { model | appMode = Orienteering { gs | speedMultiplier = newSpeed } }
                    , Cmd.none
                    )

                _ ->
                    ( model, Cmd.none )

        GameSpeedDown ->
            case model.appMode of
                Orienteering gs ->
                    let
                        newSpeed =
                            Basics.max 0.5 (gs.speedMultiplier / 2)
                    in
                    ( { model | appMode = Orienteering { gs | speedMultiplier = newSpeed } }
                    , Cmd.none
                    )

                _ ->
                    ( model, Cmd.none )

        SetTargetBearing tb ->
            case model.appMode of
                Orienteering gs ->
                    ( { model | appMode = Orienteering { gs | targetBearing = Just tb } }
                    , Cmd.none
                    )

                _ ->
                    ( model, Cmd.none )

        ClearTargetBearing ->
            case model.appMode of
                Orienteering gs ->
                    ( { model | appMode = Orienteering { gs | targetBearing = Nothing } }
                    , Cmd.none
                    )

                _ ->
                    ( model, Cmd.none )

        GameKeyLeft ->
            case model.appMode of
                Orienteering gs ->
                    let
                        newBearing =
                            toFloat (modBy 360 (round gs.playerBearing - 10 + 360))
                    in
                    ( { model | appMode = Orienteering { gs | playerBearing = newBearing } }
                    , Ports.updateGameCamera { lat = gs.playerPosition.lat, lon = gs.playerPosition.lon, bearing = newBearing }
                    )

                _ ->
                    ( model, Cmd.none )

        GameKeyRight ->
            case model.appMode of
                Orienteering gs ->
                    let
                        newBearing =
                            toFloat (modBy 360 (round gs.playerBearing + 10))
                    in
                    ( { model | appMode = Orienteering { gs | playerBearing = newBearing } }
                    , Ports.updateGameCamera { lat = gs.playerPosition.lat, lon = gs.playerPosition.lon, bearing = newBearing }
                    )

                _ ->
                    ( model, Cmd.none )

        RoadsFetched result ->
            case ( model.appMode, result ) of
                ( Orienteering gs, Ok roads ) ->
                    let
                        segCount =
                            List.sum (List.map (\r -> List.length r - 1) roads)

                        -- Snap player to nearest road SEGMENT (not just nearest point)
                        snapResult =
                            GameEngine.findNearestSegment gs.playerPosition roads

                        ( snappedPos, snapDist ) =
                            case snapResult of
                                Just nearest ->
                                    ( nearest.proj, nearest.dist )

                                Nothing ->
                                    ( gs.playerPosition, 0 )

                        -- Only snap if close enough (< 30m), otherwise keep position
                        finalPos =
                            if snapDist < 30 then
                                snappedPos

                            else
                                gs.playerPosition
                    in
                    ( { model | appMode = Orienteering { gs | roads = roads, playerPosition = finalPos } }
                    , Cmd.batch
                        [ logCmd ("ROADS " ++ String.fromInt (List.length roads) ++ "r " ++ String.fromInt segCount ++ "s snap=" ++ String.fromInt (round snapDist) ++ "m to " ++ String.fromFloat finalPos.lat ++ "," ++ String.fromFloat finalPos.lon)
                        , Ports.updateGameCamera { lat = snappedPos.lat, lon = snappedPos.lon, bearing = gs.playerBearing }
                        ]
                    )

                _ ->
                    ( model, Cmd.none )

        TopoTileLoaded bounds result ->
            case ( model.appMode, result ) of
                ( Orienteering gs, Ok texture ) ->
                    let
                        newTile =
                            { texture = texture, bounds = bounds }

                        -- Replace tile if same bounds already exists, otherwise add
                        existingFiltered =
                            List.filter
                                (\t ->
                                    not (t.bounds.minLat == bounds.minLat && t.bounds.minLon == bounds.minLon)
                                )
                                gs.topoTiles

                        newTiles =
                            newTile :: existingFiltered

                        newCenter =
                            Just gs.playerPosition
                    in
                    ( { model | appMode = Orienteering { gs | topoTiles = newTiles, topoTileCenter = newCenter } }
                    , Cmd.none
                    )

                _ ->
                    ( model, Cmd.none )

        BuildingsFetched result ->
            case ( model.appMode, result ) of
                ( Orienteering gs, Ok buildings ) ->
                    ( { model | appMode = Orienteering { gs | buildings = buildings } }
                    , Cmd.none
                    )

                _ ->
                    ( model, Cmd.none )

        GameKeyForward ->
            case model.appMode of
                Orienteering gs ->
                    if gs.gameStatus == GameRunning then
                        let
                            -- Manual forward = boost 10m along road
                            result =
                                GameEngine.advanceOnSegments gs.playerPosition gs.playerBearing 10.0 gs.roads

                            nextPos =
                                result.position

                            -- After moving, snap bearing to road direction
                            newBearing =
                                if result.snapped then
                                    result.roadBearing

                                else
                                    gs.playerBearing

                            -- Check control points
                            currentCp =
                                List.drop gs.currentPointIndex gs.controlPoints
                                    |> List.head

                            ( updatedCps, nextIdx, finished ) =
                                case currentCp of
                                    Just cp ->
                                        if haversineMeters nextPos cp.position < 30 then
                                            let
                                                cps =
                                                    List.indexedMap
                                                        (\i c ->
                                                            if i == gs.currentPointIndex then
                                                                { c | found = True }

                                                            else
                                                                c
                                                        )
                                                        gs.controlPoints

                                                newIdx2 =
                                                    gs.currentPointIndex + 1
                                            in
                                            ( cps, newIdx2, newIdx2 >= List.length gs.controlPoints )

                                        else
                                            ( gs.controlPoints, gs.currentPointIndex, False )

                                    Nothing ->
                                        ( gs.controlPoints, gs.currentPointIndex, False )

                            newStatus =
                                if finished then
                                    GameFinished

                                else
                                    gs.gameStatus

                            newGs =
                                { gs
                                    | playerPosition = nextPos
                                    , playerBearing = newBearing
                                    , controlPoints = updatedCps
                                    , currentPointIndex = nextIdx
                                    , gameStatus = newStatus
                                    , foundFlash = nextIdx > gs.currentPointIndex
                                    , totalDistanceM = gs.totalDistanceM + haversineMeters gs.playerPosition nextPos
                                }
                            -- Check if we need to reload tiles (moved > 80m from last tile center)
                            needReload =
                                case gs.topoTileCenter of
                                    Just tc ->
                                        haversineMeters nextPos tc > 80

                                    Nothing ->
                                        True

                            reloadCmds =
                                if needReload then
                                    [ Api.fetchIgnRoads nextPos 0.01 RoadsFetched
                                    , TopoTile.loadTopoGrid nextPos.lat nextPos.lon TopoTileLoaded
                                    ]

                                else
                                    []
                        in
                        ( { model | appMode = Orienteering newGs }
                        , Cmd.batch
                            ([ Ports.updateGameCamera
                                { lat = nextPos.lat
                                , lon = nextPos.lon
                                , bearing = newBearing
                                }
                             , logCmd ("FWD bear=" ++ String.fromInt (round newBearing) ++ " moved=" ++ String.fromInt (round (haversineMeters gs.playerPosition nextPos)) ++ "m snapped=" ++ (if result.snapped then "Y" else "N"))
                             ]
                                ++ reloadCmds
                            )
                        )

                    else
                        ( model, Cmd.none )

                _ ->
                    ( model, Cmd.none )

        GameMouseDown mouseX ->
            case model.appMode of
                Orienteering gs ->
                    ( { model | appMode = Orienteering { gs | isDragging = True, lastMouseX = mouseX, dragStartX = mouseX } }
                    , Cmd.none
                    )

                _ ->
                    ( model, Cmd.none )

        GameMouseUp mouseX ->
            case model.appMode of
                Orienteering gs ->
                    let
                        wasDrag =
                            abs (mouseX - gs.dragStartX) > 5

                        newGs =
                            { gs | isDragging = False }
                    in
                    if wasDrag || gs.gameStatus /= GameRunning then
                        ( { model | appMode = Orienteering (addLog ("DRAG dx=" ++ String.fromInt (round (mouseX - gs.dragStartX))) newGs) }
                        , logCmd ("MOUSEUP drag dx=" ++ String.fromInt (round (mouseX - gs.dragStartX)))
                        )

                    else
                        -- Click (pas de drag) → avancer
                        let
                            ( newModel, cmd ) =
                                update GameKeyForward { model | appMode = Orienteering (addLog "MOUSEUP→FWD" newGs) }
                        in
                        ( newModel, Cmd.batch [ cmd, logCmd "MOUSEUP click→FWD" ] )

                _ ->
                    ( model, Cmd.none )

        GameMouseDrag mouseX ->
            case model.appMode of
                Orienteering gs ->
                    if gs.isDragging then
                        let
                            deltaX =
                                mouseX - gs.lastMouseX

                            newBearing =
                                toFloat (modBy 360 (round (gs.playerBearing + deltaX * 0.4) + 360))
                        in
                        ( { model | appMode = Orienteering { gs | playerBearing = newBearing, lastMouseX = mouseX } }
                        , Ports.updateGameCamera { lat = gs.playerPosition.lat, lon = gs.playerPosition.lon, bearing = newBearing }
                        )

                    else
                        ( model, Cmd.none )

                _ ->
                    ( model, Cmd.none )

        GameMapClicked target ->
            case model.appMode of
                Orienteering gs ->
                    if gs.gameStatus == GameRunning then
                        let
                            clickCoord =
                                { lat = target.lat, lon = target.lon }

                            -- Snap click to nearest road segment
                            allSegments =
                                List.concatMap GameEngine.roadToSegments gs.roads

                            snappedPoint =
                                allSegments
                                    |> List.map (\( a, b ) -> GameEngine.projectOntoSegment clickCoord a b)
                                    |> List.sortBy (\proj -> haversineMeters clickCoord proj)
                                    |> List.head
                                    |> Maybe.withDefault clickCoord

                            -- Only move if the click is near a road (< 50m)
                            snapDist =
                                haversineMeters clickCoord snappedPoint

                            moveTarget =
                                if snapDist < 50 then
                                    snappedPoint

                                else
                                    clickCoord

                            -- Limit movement to 100m max per click
                            distToTarget =
                                haversineMeters gs.playerPosition moveTarget

                            finalTarget =
                                if distToTarget > 100 then
                                    let
                                        ratio =
                                            100 / distToTarget
                                    in
                                    { lat = gs.playerPosition.lat + (moveTarget.lat - gs.playerPosition.lat) * ratio
                                    , lon = gs.playerPosition.lon + (moveTarget.lon - gs.playerPosition.lon) * ratio
                                    }

                                else
                                    moveTarget

                            -- Update bearing towards target
                            newBearing =
                                GameEngine.bearingBetween gs.playerPosition finalTarget

                            -- Check control points
                            currentCp =
                                List.drop gs.currentPointIndex gs.controlPoints
                                    |> List.head

                            ( updatedCps, nextIdx, finished ) =
                                case currentCp of
                                    Just cp ->
                                        if haversineMeters finalTarget cp.position < 30 then
                                            let
                                                cps =
                                                    List.indexedMap
                                                        (\i c ->
                                                            if i == gs.currentPointIndex then
                                                                { c | found = True }

                                                            else
                                                                c
                                                        )
                                                        gs.controlPoints

                                                newIdx =
                                                    gs.currentPointIndex + 1
                                            in
                                            ( cps, newIdx, newIdx >= List.length gs.controlPoints )

                                        else
                                            ( gs.controlPoints, gs.currentPointIndex, False )

                                    Nothing ->
                                        ( gs.controlPoints, gs.currentPointIndex, False )

                            newStatus =
                                if finished then
                                    GameFinished

                                else
                                    gs.gameStatus

                            logMsg =
                                "CLICK (" ++ String.fromFloat (toFloat (round (target.lat * 100000)) / 100000) ++ "," ++ String.fromFloat (toFloat (round (target.lon * 100000)) / 100000) ++ ") snap=" ++ String.fromInt (round snapDist) ++ "m dist=" ++ String.fromInt (round (haversineMeters gs.playerPosition finalTarget)) ++ "m roads=" ++ String.fromInt (List.length gs.roads)

                            newGs =
                                addLog logMsg
                                    { gs
                                        | playerPosition = finalTarget
                                        , playerBearing = newBearing
                                        , controlPoints = updatedCps
                                        , currentPointIndex = nextIdx
                                        , gameStatus = newStatus
                                        , foundFlash = nextIdx > gs.currentPointIndex
                                    }
                        in
                        ( { model | appMode = Orienteering newGs }
                        , Cmd.batch
                            [ Ports.updateGameCamera
                                { lat = finalTarget.lat
                                , lon = finalTarget.lon
                                , bearing = newBearing
                                }
                            , logCmd ("MAPCLICK target=" ++ String.fromFloat target.lat ++ "," ++ String.fromFloat target.lon ++ " snap=" ++ String.fromInt (round snapDist) ++ "m moved=" ++ String.fromInt (round (haversineMeters gs.playerPosition finalTarget)) ++ "m")
                            ]
                        )

                    else
                        ( model, Cmd.none )

                _ ->
                    ( model, Cmd.none )



{-| Smoothly interpolate between two bearings (0-360), handling the 360/0 wrap.
Factor 0.0 = keep current, 1.0 = jump to target.
-}
smoothBearing : Float -> Float -> Float -> Float
smoothBearing current target factor =
    let
        diff =
            target - current

        wrappedDiff =
            if diff > 180 then
                diff - 360

            else if diff < -180 then
                diff + 360

            else
                diff

        result =
            current + wrappedDiff * factor
    in
    if result < 0 then
        result + 360

    else if result >= 360 then
        result - 360

    else
        result


addLog : String -> GameState -> GameState
addLog msg gs =
    { gs | debugLog = msg :: List.take 9 gs.debugLog }


logCmd : String -> Cmd Msg
logCmd msg =
    Api.sendLog msg (\_ -> NoOp)



-- VIEW


view : Model -> Html Msg
view model =
    case model.appMode of
        Orienteering gs ->
            div [ class "app-container game-mode" ]
                [ if gs.gameStatus == GameRunning && not gs.showTopoOverlay then
                    div
                        [ style "position" "fixed"
                        , style "top" "0"
                        , style "left" "0"
                        , style "width" "100vw"
                        , style "height" "100vh"
                        , style "z-index" "2"
                        , style "overflow" "hidden"
                        , style "cursor" "pointer"
                        ]
                        [ World3D.view gs 1600 900
                        ]

                  else
                    text ""
                , Game.view model gs
                ]

        Planning ->
            div [ class "app-container" ]
                [ header [ class "app-header" ]
                    [ h1 [] [ text "Chemins Noirs" ]
                    , p [ class "app-subtitle" ] [ text "Générateur GPX anti-bitume" ]
                    ]
                , Form.view model
                , Preview.view model
                ]



-- SUBSCRIPTIONS


subscriptions : Model -> Sub Msg
subscriptions model =
    Sub.batch
        [ Ports.mapClickReceived (\{ lat, lon } -> MapClicked lat lon)
        , Ports.waypointDragged (\{ index, lat, lon } -> MoveWaypoint index lat lon)
        , Ports.waypointDeleted (\{ index } -> RemoveWaypoint index)
        , Ports.routeLoadedFromLocalStorage
            (\value ->
                RouteLoadedFromStorage
                    (Json.Decode.decodeValue Decoders.decodeRouteResponse value)
            )
        , Ports.gotGeolocation (\{ lat, lon } -> GotGeolocation lat lon)
        , Ports.undoRedoReceived
            (\{ action } ->
                if action == "undo" then
                    UndoWaypoints

                else if action == "redo" then
                    RedoWaypoints

                else
                    NoOp
            )
        , Ports.gpxWaypointsReceived
            (\coords ->
                GpxWaypointsReceived (List.map (\c -> { lat = c.lat, lon = c.lon }) coords)
            )
        , Ports.mapRouteHover
            (\{ index } ->
                if index < 0 then
                    MapRouteLeave

                else
                    MapRouteHoverIndex index
            )
        , Ports.closeLoopRequested (\_ -> ToggleCloseLoop)
        , case model.appMode of
            Orienteering gs ->
                if gs.gameStatus == GameRunning then
                    Time.every 1000 (\_ -> GameTick 1000)

                else
                    Sub.none

            Planning ->
                Sub.none
        , Browser.Events.onMouseDown
            (Json.Decode.map GameMouseDown
                (Json.Decode.field "clientX" Json.Decode.float)
            )
        , Browser.Events.onMouseMove
            (Json.Decode.map GameMouseDrag
                (Json.Decode.field "clientX" Json.Decode.float)
            )
        , Browser.Events.onMouseUp
            (Json.Decode.map GameMouseUp
                (Json.Decode.field "clientX" Json.Decode.float)
            )
        , Ports.gameWheelReceived
            (\deltaY ->
                if deltaY > 0 then
                    GameKeyForward

                else
                    NoOp
            )
        , Ports.gameMapClicked GameMapClicked
        , Browser.Events.onKeyDown
            (Json.Decode.field "key" Json.Decode.string
                |> Json.Decode.map
                    (\key ->
                        case key of
                            "ArrowLeft" ->
                                GameKeyLeft

                            "q" ->
                                GameKeyLeft

                            "Q" ->
                                GameKeyLeft

                            "ArrowRight" ->
                                GameKeyRight

                            "d" ->
                                GameKeyRight

                            "D" ->
                                GameKeyRight

                            "ArrowUp" ->
                                GameKeyForward

                            "z" ->
                                GameKeyForward

                            "Z" ->
                                GameKeyForward

                            "w" ->
                                GameKeyForward

                            "W" ->
                                GameKeyForward

                            _ ->
                                NoOp
                    )
            )
        ]



-- HELPERS


handleMapClick : Float -> Float -> Model -> ( Model, Cmd Msg )
handleMapClick lat lon model =
    let
        coord =
            { lat = lat, lon = lon }

        maxWp =
            case model.routeMode of
                PointToPoint ->
                    2

                Loop ->
                    1

                MultiPoint ->
                    999

        wpCount =
            List.length model.waypoints
    in
    if model.freehandEnabled && model.routeMode == MultiPoint && model.lastResponse /= Nothing then
        case model.freehandDrawing of
            Nothing ->
                case findNearestWaypoint coord model.waypoints of
                    Just ( idx, dist ) ->
                        if dist < 0.1 && idx < List.length model.waypoints - 1 then
                            let
                                newModel =
                                    { model | freehandDrawing = Just { fromIdx = idx, points = [] } }
                            in
                            rebuildAndDisplayRoute newModel

                        else
                            ( model, Cmd.none )

                    Nothing ->
                        ( model, Cmd.none )

            Just state ->
                let
                    targetIdx =
                        state.fromIdx + 1

                    targetWp =
                        getAt targetIdx model.waypoints
                in
                case targetWp of
                    Just target ->
                        if haversineKm coord target < 0.1 then
                            let
                                newModel =
                                    { model
                                        | freehandSegments = Dict.insert state.fromIdx state.points model.freehandSegments
                                        , freehandDrawing = Nothing
                                    }
                            in
                            rebuildAndDisplayRoute newModel

                        else
                            let
                                newState =
                                    { state | points = state.points ++ [ coord ] }

                                newModel =
                                    { model | freehandDrawing = Just newState }
                            in
                            rebuildAndDisplayRoute newModel

                    Nothing ->
                        ( model, Cmd.none )

    else if wpCount >= 2 && model.lastResponse /= Nothing && model.routeMode == MultiPoint then
        let
            idx =
                findInsertionIndex coord model.waypoints
        in
        update (InsertWaypoint idx coord) model

    else if wpCount < maxWp then
        update (AddWaypoint coord) model

    else
        ( model, Cmd.none )


applyFreehandOverrides : Dict Int (List Coordinate) -> Maybe FreehandDrawingState -> List Coordinate -> RouteResponse -> RouteResponse
applyFreehandOverrides freehandDict activeDrawing waypoints response =
    let
        hasOverrides =
            not (Dict.isEmpty freehandDict) || activeDrawing /= Nothing
    in
    if not hasOverrides then
        response

    else
        case response.segments of
            Nothing ->
                response

            Just segments ->
                let
                    originalPath =
                        response.path

                    buildSegment idx seg =
                        case Dict.get idx freehandDict of
                            Just pts ->
                                -- Stored freehand: waypoint → intermediate points → next waypoint
                                let
                                    wpA =
                                        getAt idx waypoints

                                    wpB =
                                        getAt (idx + 1) waypoints
                                in
                                case ( wpA, wpB ) of
                                    ( Just a, Just b ) ->
                                        let
                                            fullPath =
                                                [ a ] ++ pts ++ [ b ]
                                        in
                                        { path = fullPath
                                        , dist = freehandDistance fullPath
                                        }

                                    _ ->
                                        { path = List.take (seg.toIndex - seg.fromIndex + 1) (List.drop seg.fromIndex originalPath)
                                        , dist = seg.distanceKm
                                        }

                            Nothing ->
                                case activeDrawing of
                                    Just drawing ->
                                        if drawing.fromIdx == idx then
                                            -- Live preview of active drawing
                                            let
                                                wpA =
                                                    getAt idx waypoints

                                                wpB =
                                                    getAt (idx + 1) waypoints
                                            in
                                            case ( wpA, wpB ) of
                                                ( Just a, Just b ) ->
                                                    let
                                                        fullPath =
                                                            [ a ] ++ drawing.points ++ [ b ]
                                                    in
                                                    { path = fullPath
                                                    , dist = freehandDistance fullPath
                                                    }

                                                _ ->
                                                    { path = List.take (seg.toIndex - seg.fromIndex + 1) (List.drop seg.fromIndex originalPath)
                                                    , dist = seg.distanceKm
                                                    }

                                        else
                                            { path = List.take (seg.toIndex - seg.fromIndex + 1) (List.drop seg.fromIndex originalPath)
                                            , dist = seg.distanceKm
                                            }

                                    Nothing ->
                                        { path = List.take (seg.toIndex - seg.fromIndex + 1) (List.drop seg.fromIndex originalPath)
                                        , dist = seg.distanceKm
                                        }

                    built =
                        List.indexedMap buildSegment segments

                    mergedPath =
                        built
                            |> List.indexedMap
                                (\i piece ->
                                    if i == 0 then
                                        piece.path

                                    else
                                        List.drop 1 piece.path
                                )
                            |> List.concat

                    totalDist =
                        List.map .dist built |> List.sum

                    roundedDist =
                        toFloat (round (totalDist * 100)) / 100
                in
                { response
                    | path = mergedPath
                    , distanceKm = roundedDist
                }


getAt : Int -> List a -> Maybe a
getAt idx list =
    List.head (List.drop idx list)


haversineKm : Coordinate -> Coordinate -> Float
haversineKm a b =
    let
        r =
            6371.0

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


rebuildAndDisplayRoute : Model -> ( Model, Cmd Msg )
rebuildAndDisplayRoute model =
    case model.originalResponse of
        Just origResponse ->
            let
                displayRoute =
                    applyFreehandOverrides model.freehandSegments model.freehandDrawing model.waypoints origResponse
            in
            ( { model | lastResponse = Just displayRoute }
            , Ports.updateRoute displayRoute.path
            )

        Nothing ->
            ( model, Cmd.none )


findNearestWaypoint : Coordinate -> List Coordinate -> Maybe ( Int, Float )
findNearestWaypoint coord waypoints =
    waypoints
        |> List.indexedMap (\i wp -> ( i, haversineKm coord wp ))
        |> List.sortBy Tuple.second
        |> List.head


freehandDistance : List Coordinate -> Float
freehandDistance coords =
    let
        pairs =
            List.map2 Tuple.pair coords (List.drop 1 coords)
    in
    List.foldl (\( a, b ) acc -> acc + haversineKm a b) 0 pairs


pushWaypointHistory : Model -> Model
pushWaypointHistory model =
    { model
        | waypointHistory = List.take 50 (model.waypoints :: model.waypointHistory)
        , waypointFuture = []
        , freehandSegments = Dict.empty
        , freehandDrawing = Nothing
    }


findInsertionIndex : Coordinate -> List Coordinate -> Int
findInsertionIndex point waypoints =
    let
        pairs =
            List.map2 Tuple.pair
                (List.indexedMap Tuple.pair waypoints)
                (List.drop 1 waypoints)

        distances =
            List.map
                (\( ( idx, a ), b ) ->
                    ( idx, distToSegment point a b )
                )
                pairs
    in
    distances
        |> List.sortBy Tuple.second
        |> List.head
        |> Maybe.map Tuple.first
        |> Maybe.withDefault (List.length waypoints - 1)


distToSegment : Coordinate -> Coordinate -> Coordinate -> Float
distToSegment p a b =
    let
        dx =
            b.lon - a.lon

        dy =
            b.lat - a.lat

        lenSq =
            dx * dx + dy * dy
    in
    if lenSq == 0 then
        let
            ex =
                p.lon - a.lon

            ey =
                p.lat - a.lat
        in
        sqrt (ex * ex + ey * ey)

    else
        let
            t =
                clamp 0 1 (((p.lon - a.lon) * dx + (p.lat - a.lat) * dy) / lenSq)

            projLon =
                a.lon + t * dx

            projLat =
                a.lat + t * dy

            ex =
                p.lon - projLon

            ey =
                p.lat - projLat
        in
        sqrt (ex * ex + ey * ey)


resetLoopCandidatesCmd : Model -> Cmd Msg
resetLoopCandidatesCmd model =
    Cmd.none


centerOnRouteCmd : RouteResponse -> Cmd Msg
centerOnRouteCmd route =
    case ( List.head route.path, List.head (List.reverse route.path) ) of
        ( Just start, Just end ) ->
            Ports.centerOnMarkers { start = start, end = end }

        _ ->
            Cmd.none


applyRoute : Model -> RouteResponse -> Model
applyRoute model route =
    let
        startCoord =
            List.head route.path

        endCoord =
            List.head (List.reverse route.path)

        form =
            model.form

        newForm =
            case ( startCoord, endCoord ) of
                ( Just start, Just end ) ->
                    { form
                        | startLat = formatCoord start.lat
                        , startLon = formatCoord start.lon
                        , endLat = formatCoord end.lat
                        , endLon = formatCoord end.lon
                    }

                _ ->
                    form
    in
    { model
        | pending = False
        , lastResponse = Just route
        , error = Nothing
        , form = newForm
        -- Keep original waypoints unchanged - don't extract from calculated path
    }


applySavedRoute : Model -> RouteResponse -> List Coordinate -> Model
applySavedRoute model route originalWaypoints =
    let
        -- For multi-point routes, use the original waypoints (click positions)
        -- for form start/end instead of the path (which includes snap projections).
        -- This ensures recalculating produces the same route.
        ( startCoord, endCoord ) =
            if not (List.isEmpty originalWaypoints) then
                ( List.head originalWaypoints
                , List.head (List.reverse originalWaypoints)
                )
            else
                ( List.head route.path
                , List.head (List.reverse route.path)
                )

        form =
            model.form

        newForm =
            case ( startCoord, endCoord ) of
                ( Just start, Just end ) ->
                    { form
                        | startLat = formatCoord start.lat
                        , startLon = formatCoord start.lon
                        , endLat = formatCoord end.lat
                        , endLon = formatCoord end.lon
                    }

                _ ->
                    form

        -- Set route mode based on whether we have waypoints
        routeMode =
            if List.isEmpty originalWaypoints then
                PointToPoint
            else
                MultiPoint
    in
    { model
        | pending = False
        , lastResponse = Just route
        , error = Nothing
        , form = newForm
        , waypoints = originalWaypoints  -- Restore original waypoints
        , routeMode = routeMode
    }


loopFormToRequest : Bool -> RouteForm -> LoopForm -> List Coordinate -> Result String LoopRouteRequest
loopFormToRequest cheminNoir form loopForm waypoints =
    case List.head waypoints of
        Just start ->
            let
                weightsResult =
                    if cheminNoir then
                        Ok ( 5.0, 8.0 )

                    else
                        case ( String.toFloat form.wPop, String.toFloat form.wPaved ) of
                            ( Just wPop, Just wPaved ) ->
                                Ok ( wPop, wPaved )

                            _ ->
                                Err "Poids invalides"
            in
            case weightsResult of
                Ok ( wPop, wPaved ) ->
                    case String.toFloat loopForm.distanceKm of
                        Just distanceKm ->
                            case String.toFloat loopForm.toleranceKm of
                                Just toleranceKm ->
                                    case String.toInt loopForm.candidateCount of
                                        Just candidateCount ->
                                            Ok
                                                { start = start
                                                , targetDistanceKm = distanceKm
                                                , distanceToleranceKm = toleranceKm
                                                , candidateCount = max 1 candidateCount
                                                , wPop = wPop
                                                , wPaved = wPaved
                                                , maxTotalAscent = String.toFloat loopForm.maxAscentM
                                                , minTotalAscent = String.toFloat loopForm.minAscentM
                                                }

                                        Nothing ->
                                            Err "Nombre de candidats invalide"

                                Nothing ->
                                    Err "Tolérance invalide"

                        Nothing ->
                            Err "Distance invalide"

                Err err ->
                    Err err

        Nothing ->
            Err "Placez un point de départ sur la carte"


generateGpx : RouteResponse -> String
generateGpx route =
    let
        header =
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n"
                ++ "<gpx version=\"1.1\" creator=\"Chemins Noirs\"\n"
                ++ "  xmlns=\"http://www.topografix.com/GPX/1/1\">\n"
                ++ "  <trk>\n"
                ++ "    <name>Chemins Noirs</name>\n"
                ++ "    <trkseg>\n"

        elevations =
            route.elevationProfile
                |> Maybe.map .elevations
                |> Maybe.withDefault []

        points =
            List.indexedMap
                (\i coord ->
                    let
                        ele =
                            elevations
                                |> List.drop i
                                |> List.head
                                |> Maybe.andThen identity
                                |> Maybe.map (\e -> "        <ele>" ++ String.fromFloat e ++ "</ele>\n")
                                |> Maybe.withDefault ""
                    in
                    "      <trkpt lat=\""
                        ++ String.fromFloat coord.lat
                        ++ "\" lon=\""
                        ++ String.fromFloat coord.lon
                        ++ "\">\n"
                        ++ ele
                        ++ "      </trkpt>\n"
                )
                route.path

        footer =
            "    </trkseg>\n"
                ++ "  </trk>\n"
                ++ "</gpx>\n"
    in
    header ++ String.concat points ++ footer


multiPointRequest : Model -> MultiPointRouteRequest
multiPointRequest model =
    if model.cheminNoir then
        { waypoints = model.waypoints
        , closeLoop = model.closeLoop
        , wPop = 5.0
        , wPaved = 8.0
        }

    else
        { waypoints = model.waypoints
        , closeLoop = model.closeLoop
        , wPop = String.toFloat model.form.wPop |> Maybe.withDefault 1.0
        , wPaved = String.toFloat model.form.wPaved |> Maybe.withDefault 1.0
        }
