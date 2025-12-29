module Main exposing (main)

{-| Application principale - Architecture MVU (Model-View-Update).
Approche fonctionnelle pure : pas de mutations, fonctions pures, gestion explicite des effets.
-}

import Api
import Browser
import Decoders
import Encoders
import Html exposing (..)
import Html.Attributes exposing (class)
import Json.Decode
import Ports
import Types exposing (..)
import View.Form as Form
import View.Preview as Preview



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
    let
        model =
            initialModel

        start =
            parseCoordinate model.form.startLat model.form.startLon

        end =
            parseCoordinate model.form.endLat model.form.endLon
    in
    ( model
    , Cmd.batch
        [ Ports.initMap ()
        , Ports.updateSelectionMarkers { start = start, end = end }
        , case ( start, end ) of
            ( Just s, Just e ) ->
                Ports.centerOnMarkers { start = s, end = e }

            _ ->
                Cmd.none
        , Api.listSavedRoutes SavedRoutesLoaded
        ]
    )



-- UPDATE


update : Msg -> Model -> ( Model, Cmd Msg )
update msg model =
    case msg of
        StartLatChanged val ->
            let
                form =
                    model.form

                newForm =
                    { form | startLat = val }
            in
            ( { model | form = newForm }
            , Cmd.batch
                [ syncSelectionMarkersCmd newForm
                , resetLoopCandidatesCmd model
                ]
            )

        StartLonChanged val ->
            let
                form =
                    model.form

                newForm =
                    { form | startLon = val }
            in
            ( { model | form = newForm }
            , Cmd.batch
                [ syncSelectionMarkersCmd newForm
                , resetLoopCandidatesCmd model
                ]
            )

        EndLatChanged val ->
            let
                form =
                    model.form

                newForm =
                    { form | endLat = val }
            in
            ( { model | form = newForm }
            , syncSelectionMarkersCmd newForm
            )

        EndLonChanged val ->
            let
                form =
                    model.form

                newForm =
                    { form | endLon = val }
            in
            ( { model | form = newForm }
            , syncSelectionMarkersCmd newForm
            )

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
                    ( applyRoute model route
                    , Cmd.batch
                        [ Ports.updateRoute route.path
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
                    , Ports.updateRoute []
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
            if model.routeMode == MultiPoint then
                update (AddWaypoint { lat = lat, lon = lon }) model

            else
                let
                    coord =
                        { lat = lat, lon = lon }

                    form =
                        model.form
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
                        , Cmd.batch
                            [ syncSelectionMarkersCmd newForm
                            , resetLoopCandidatesCmd model
                            ]
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
                        , syncSelectionMarkersCmd newForm
                        )

        AddWaypoint coord ->
            let
                newWaypoints =
                    model.waypoints ++ [ coord ]
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
            , Cmd.batch
                [ Ports.updateWaypointMarkers newWaypoints
                , if List.length newWaypoints < 2 then
                    Cmd.batch
                        [ Ports.updateRoute []
                        ]

                  else
                    Cmd.none
                ]
            )

        ClearWaypoints ->
            ( { model
                | waypoints = []
                , lastResponse = Nothing
                , error = Nothing
              }
            , Cmd.batch
                [ Ports.updateWaypointMarkers []
                , Ports.updateRoute []
                ]
            )

        ToggleCloseLoop ->
            ( { model | closeLoop = not model.closeLoop }
            , Cmd.none
            )

        ComputeMultiPointRoute ->
            update Submit model

        SetClickMode mode ->
            ( { model | clickMode = mode }
            , Cmd.none
            )

        ToggleRouteMode mode ->
            ( { model
                | routeMode = mode
                , loopCandidates = []
                , loopMeta = Nothing
                , selectedLoopIdx = Nothing
              }
            , if mode /= MultiPoint && not (List.isEmpty model.waypoints) then
                Ports.updateWaypointMarkers []

              else
                Cmd.none
            )

        ToggleMapView ->
            let
                newMode =
                    case model.mapViewMode of
                        Standard ->
                            Satellite

                        Satellite ->
                            Standard
            in
            ( { model | mapViewMode = newMode }
            , Ports.toggleSatelliteView (newMode == Satellite)
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
                    in
                    ( applySavedRoute { model | pending = False, error = Nothing } route waypoints
                    , Cmd.batch
                        [ Ports.updateRoute route.path
                        , case route.metadata of
                            Just meta ->
                                Ports.centerOnMarkers { start = meta.start, end = meta.end }

                            Nothing ->
                                Cmd.none
                        ]
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

        NoOp ->
            ( model, Cmd.none )



-- VIEW


view : Model -> Html Msg
view model =
    div [ class "app-container" ]
        [ h1 [] [ text "Chemins Noirs – générateur GPX anti-bitume" ]
        , Form.view model
        , Preview.view model
        ]



-- SUBSCRIPTIONS


subscriptions : Model -> Sub Msg
subscriptions model =
    Sub.batch
        [ Ports.mapClickReceived (\{ lat, lon } -> MapClicked lat lon)
        , Ports.routeLoadedFromLocalStorage
            (\value ->
                RouteLoadedFromStorage
                    (Json.Decode.decodeValue Decoders.decodeRouteResponse value)
            )
        ]



-- HELPERS


syncSelectionMarkersCmd : RouteForm -> Cmd Msg
syncSelectionMarkersCmd form =
    let
        start =
            parseCoordinate form.startLat form.startLon

        end =
            parseCoordinate form.endLat form.endLon
    in
    Ports.updateSelectionMarkers { start = start, end = end }


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


formToRequest : RouteForm -> Result String RouteRequest
formToRequest form =
    case ( parseCoordinate form.startLat form.startLon, parseCoordinate form.endLat form.endLon ) of
        ( Just start, Just end ) ->
            case ( String.toFloat form.wPop, String.toFloat form.wPaved ) of
                ( Just wPop, Just wPaved ) ->
                    Ok
                        { start = start
                        , end = end
                        , wPop = wPop
                        , wPaved = wPaved
                        }

                _ ->
                    Err "Poids invalides"

        _ ->
            Err "Coordonnées invalides"


loopFormToRequest : RouteForm -> LoopForm -> Result String LoopRouteRequest
loopFormToRequest form loopForm =
    case parseCoordinate form.startLat form.startLon of
        Just start ->
            case String.toFloat form.wPop of
                Just wPop ->
                    case String.toFloat form.wPaved of
                        Just wPaved ->
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

                        Nothing ->
                            Err "Poids pavé invalide"

                Nothing ->
                    Err "Poids population invalide"

        Nothing ->
            Err "Coordonnées de départ invalides"


multiPointRequest : Model -> MultiPointRouteRequest
multiPointRequest model =
    { waypoints = model.waypoints
    , closeLoop = model.closeLoop
    , wPop = String.toFloat model.form.wPop |> Maybe.withDefault 1.0
    , wPaved = String.toFloat model.form.wPaved |> Maybe.withDefault 1.0
    }
