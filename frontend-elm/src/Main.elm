module Main exposing (main)

{-| Application principale - Architecture MVU (Model-View-Update).
Approche fonctionnelle pure : pas de mutations, fonctions pures, gestion explicite des effets.
-}

import Api
import Array
import Browser
import Browser.Dom
import Browser.Events
import Decoders
import Dict exposing (Dict)
import DirectLegs
import Encoders
import Html exposing (..)
import Html.Attributes exposing (class, style)
import Html.Events
import Http
import Json.Decode
import Json.Encode
import Ports
import Set
import Task
import Time
import Types exposing (..)
import View.ElevationBand as ElevationBand
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

        ToggleReferenceRoute id ->
            if Dict.member id model.referenceRoutes then
                let
                    next =
                        { model | referenceRoutes = Dict.remove id model.referenceRoutes }
                in
                ( next, referenceRoutesCmd next )

            else
                -- Le tracé complet n'est pas dans la liste : il faut aller le
                -- chercher, comme pour un chargement ordinaire.
                ( { model
                    | referenceRoutes =
                        Dict.insert id
                            { color = nextReferenceColor model.referenceRoutes, path = Nothing }
                            model.referenceRoutes
                  }
                , Api.getSavedRoute id (ReferenceRouteLoaded id)
                )

        ToggleFolderReferences key ->
            -- Tout le dossier d'une seule couleur : en 53 tronçons, une
            -- couleur par ligne ferait un patchwork illisible.
            let
                ids =
                    model.savedRoutes
                        |> List.filter (\route -> folderKey route == key)
                        |> List.map .id

                missing =
                    List.filter (\id -> not (Dict.member id model.referenceRoutes)) ids

                color =
                    ids
                        |> List.filterMap (\id -> Dict.get id model.referenceRoutes)
                        |> List.head
                        |> Maybe.map .color
                        |> Maybe.withDefault (nextReferenceColor model.referenceRoutes)
            in
            if List.isEmpty missing then
                update (RemoveReferenceRoutes ids) model

            else
                ( { model
                    | referenceRoutes =
                        List.foldl
                            (\id -> Dict.insert id { color = color, path = Nothing })
                            model.referenceRoutes
                            missing
                  }
                , Cmd.batch (List.map (\id -> Api.getSavedRoute id (ReferenceRouteLoaded id)) missing)
                )

        RemoveReferenceRoutes ids ->
            let
                next =
                    { model | referenceRoutes = List.foldl Dict.remove model.referenceRoutes ids }
            in
            ( next, referenceRoutesCmd next )

        ReferenceRouteLoaded id result ->
            -- Une réponse qui arrive après qu'on a décoché le calque ne doit
            -- pas le faire réapparaître.
            case ( Dict.get id model.referenceRoutes, result ) of
                ( Nothing, _ ) ->
                    ( model, Cmd.none )

                ( Just layer, Ok saved ) ->
                    let
                        next =
                            { model
                                | referenceRoutes =
                                    Dict.insert id
                                        { layer | path = Just saved.routeData.path }
                                        model.referenceRoutes
                            }
                    in
                    ( next, referenceRoutesCmd next )

                ( Just _, Err _ ) ->
                    let
                        next =
                            { model
                                | referenceRoutes = Dict.remove id model.referenceRoutes
                                , error = Just "Impossible d'afficher cet itinéraire en calque"
                            }
                    in
                    ( next, referenceRoutesCmd next )

        ForestWeightChanged val ->
            let
                form =
                    model.form

                newForm =
                    { form | wForest = val }
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

                                weights =
                                    if model.cheminNoir then
                                        cheminNoirWeights

                                    else
                                        { pop = String.toFloat model.form.wPop |> Maybe.withDefault 1.5
                                        , paved = String.toFloat model.form.wPaved |> Maybe.withDefault 4.0
                                        , forest = String.toFloat model.form.wForest |> Maybe.withDefault 0.0
                                        }

                                request =
                                    { start = startWp
                                    , end = endWp
                                    , wPop = weights.pop
                                    , wPaved = weights.paved
                                    , wForest = weights.forest
                                    }
                            in
                            ( { model | pending = True, pendingSeconds = 0, error = Nothing }
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
                                        , pendingSeconds = 0
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
                            ( { model | pending = True, pendingSeconds = 0, error = Nothing }
                            , Api.fetchMultiPointRoute
                                (multiPointRequest model)
                                RouteFetched
                            )

        SurfacesLoaded result ->
            case result of
                Ok ( surfaces, ign ) ->
                    ( { model
                        | lastResponse =
                            Maybe.map
                                (\route -> { route | pointSurfaces = Just surfaces, pointIgn = ign })
                                model.lastResponse
                      }
                    , Cmd.batch
                        [ Ports.setRouteSurfaces { surfaces = Just surfaces, ign = ign, elevations = Nothing }
                        , Api.sendLog
                            ("surfaces reçues : "
                                ++ String.fromInt (List.length (List.filter not surfaces))
                                ++ " points non goudronnés sur "
                                ++ String.fromInt (List.length surfaces)
                            )
                            (\_ -> NoOp)
                        ]
                    )

                -- La carte reste lisible sans revêtements : l'échec ne mérite pas
                -- un bandeau d'erreur, mais il doit laisser une trace, sinon rien
                -- ne distingue « en cours » de « tombé ».
                Err err ->
                    ( model
                    , Api.sendLog
                        ("surfaces indisponibles : " ++ httpErrorToString err)
                        (\_ -> NoOp)
                    )

        PendingTick ->
            ( { model | pendingSeconds = model.pendingSeconds + 1 }
            , Cmd.none
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
                            , Ports.setRouteSurfaces (surfaceLayers displayRoute)
                            , Ports.setDirectOverlay
                                { lines = DirectLegs.lines model.directLegs route
                                , obstacles =
                                    List.map
                                        (\o -> { lat = o.lat, lon = o.lon, kind = Encoders.obstacleKindKey o.kind })
                                        route.obstacles
                                }
                            , Ports.updateWaypointMarkers markerPositions
                            ]
                        )

                    else
                        ( applyRoute model route
                        , Cmd.batch
                            [ Ports.updateRoute route.path
                            , Ports.setRouteSurfaces (surfaceLayers route)
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
                                    , Ports.setRouteSurfaces (surfaceLayers candidate.route)
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
                        , Ports.setRouteSurfaces (surfaceLayers candidate.route)
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
            handleMapClick lat lon model

        AddWaypoint coord ->
            let
                modelWithHistory =
                    pushWaypointHistory model

                newWaypoints =
                    modelWithHistory.waypoints ++ [ coord ]

                newModel =
                    { modelWithHistory
                        | waypoints = newWaypoints

                        -- En boucle, le retour au départ passe derrière le
                        -- nouveau point et garde son mode.
                        , directLegs = DirectLegs.afterInsert (List.length model.waypoints) model.directLegs
                        , error = Nothing
                    }
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
                    { modelWithHistory
                        | waypoints = newWaypoints
                        , directLegs = DirectLegs.afterInsert (idx + 1) model.directLegs
                        , error = Nothing
                    }
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
                    { modelWithHistory
                        | waypoints = newWaypoints
                        , directLegs = DirectLegs.afterRemove idx model.directLegs
                    }
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
                , directLegs = Set.empty
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
                ( prev, prevDirect ) :: rest ->
                    let
                        newModel =
                            { model
                                | waypoints = prev
                                , directLegs = prevDirect
                                , waypointHistory = rest
                                , waypointFuture = ( model.waypoints, model.directLegs ) :: model.waypointFuture
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
                ( next, nextDirect ) :: rest ->
                    let
                        newModel =
                            { model
                                | waypoints = next
                                , directLegs = nextDirect
                                , waypointFuture = rest
                                , waypointHistory = ( model.waypoints, model.directLegs ) :: model.waypointHistory
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

        ToggleDirectLeg leg ->
            let
                modelWithHistory =
                    pushWaypointHistory model

                newModel =
                    { modelWithHistory | directLegs = DirectLegs.toggle leg model.directLegs }
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
                , directLegs = Set.empty
                , lastResponse = Nothing
                , loadedRouteId = Nothing
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
                            IgnPlan

                        IgnPlan ->
                            IgnOrtho

                        IgnOrtho ->
                            Satellite

                        Satellite ->
                            Hybrid

                        Hybrid ->
                            Topo

                styleStr =
                    case newMode of
                        Topo ->
                            "topo"

                        IgnPlan ->
                            "ign-plan"

                        IgnOrtho ->
                            "ign-ortho"

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

        ForestQueryChanged query ->
            ( { model | forestQuery = query }, Cmd.none )

        SearchForests ->
            if String.length (String.trim model.forestQuery) < 3 then
                ( { model | error = Just "Tapez au moins trois lettres du nom de la forêt." }, Cmd.none )

            else
                ( { model | pending = True, pendingSeconds = 0, error = Nothing }
                , Api.searchForests (String.trim model.forestQuery) ForestsFound
                )

        ForestsFound result ->
            case result of
                Ok [] ->
                    ( { model
                        | pending = False
                        , forestResults = []
                        , error = Just ("Aucune forêt publique ne s'appelle « " ++ model.forestQuery ++ " ».")
                      }
                    , Cmd.none
                    )

                Ok forests ->
                    ( { model | pending = False, forestResults = forests, error = Nothing }, Cmd.none )

                Err error ->
                    ( { model | pending = False, error = Just (httpErrorToString error) }, Cmd.none )

        ViaForest forest ->
            viaForest forest model

        SaveRouteFolderChanged folder ->
            ( { model | saveRouteFolder = folder }, Cmd.none )

        ToggleFolder key ->
            ( { model
                | openFolders =
                    if Set.member key model.openFolders then
                        Set.remove key model.openFolders

                    else
                        Set.insert key model.openFolders
              }
            , Cmd.none
            )

        StartMoveRoute id ->
            let
                current =
                    model.savedRoutes
                        |> List.filter (\r -> r.id == id)
                        |> List.head
                        |> Maybe.andThen .folder
                        |> Maybe.withDefault ""
            in
            ( { model | movingRoute = Just ( id, current ) }
            , focusOn (moveFolderDomId id)
            )

        MoveRouteFolderChanged folder ->
            ( { model | movingRoute = Maybe.map (\( id, _ ) -> ( id, folder )) model.movingRoute }
            , Cmd.none
            )

        ConfirmMoveRoute ->
            case model.movingRoute of
                Just ( id, folder ) ->
                    ( { model | pending = True, pendingSeconds = 0, movingRoute = Nothing }
                    , Api.setRouteFolder id (folderInput folder) RouteFolderSet
                    )

                Nothing ->
                    ( model, Cmd.none )

        CancelMoveRoute ->
            ( { model | movingRoute = Nothing }, Cmd.none )

        RouteFolderSet result ->
            case result of
                Ok updatedRoute ->
                    ( { model
                        | pending = False
                        , error = Nothing
                        , savedRoutes =
                            List.map
                                (\r ->
                                    if r.id == updatedRoute.id then
                                        updatedRoute

                                    else
                                        r
                                )
                                model.savedRoutes

                        -- Suivre l'itinéraire là où on vient de le ranger.
                        , openFolders = Set.insert (folderKey updatedRoute) model.openFolders
                        , saveRouteFolder =
                            if model.loadedRouteId == Just updatedRoute.id then
                                folderKey updatedRoute

                            else
                                model.saveRouteFolder
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

        SaveRouteToDb ->
            case model.lastResponse of
                Just route ->
                    ( { model | pending = True, pendingSeconds = 0 }
                    , Api.saveRouteToDb (saveRequestFrom model) route RouteSaved
                    )

                Nothing ->
                    ( { model | error = Just "Aucune route à sauvegarder" }
                    , Cmd.none
                    )

        SwitchPanelTab onglet ->
            if onglet == model.panelTab then
                ( model, Cmd.none )

            else
                ( model
                , Ports.demanderTransitionOnglet
                    { vers = tabKey onglet
                    , sens =
                        if onglet == TabRoutes then
                            "avant"

                        else
                            "arriere"
                    }
                )

        ApplyPanelTab cle ->
            let
                onglet =
                    tabFromKey cle
            in
            ( { model | panelTab = onglet }
            , focusOn (tabDomId onglet)
            )

        FocusElement domId ->
            ( model, focusOn domId )

        TogglePanel ->
            ( { model | panelCollapsed = not model.panelCollapsed }
            , Cmd.none
            )

        UpdateLoadedRoute ->
            case ( model.loadedRouteId, model.lastResponse ) of
                ( Just id, Just route ) ->
                    ( { model | pending = True, pendingSeconds = 0 }
                    , Api.updateSavedRoute id (saveRequestFrom model) route RouteSaved
                    )

                ( Nothing, _ ) ->
                    ( { model | error = Just "Aucun itinéraire chargé à mettre à jour" }
                    , Cmd.none
                    )

                ( _, Nothing ) ->
                    ( { model | error = Just "Aucune route à sauvegarder" }
                    , Cmd.none
                    )

        RouteSaved result ->
            case result of
                Ok savedRoute ->
                    let
                        -- Une mise à jour renvoie une ligne déjà présente :
                        -- l'empiler afficherait l'itinéraire deux fois.
                        known =
                            List.any (\r -> r.id == savedRoute.id) model.savedRoutes

                        savedRoutes =
                            if known then
                                List.map
                                    (\r ->
                                        if r.id == savedRoute.id then
                                            savedRoute

                                        else
                                            r
                                    )
                                    model.savedRoutes

                            else
                                savedRoute :: model.savedRoutes
                    in
                    ( { model
                        | pending = False
                        , error = Nothing
                        , savedRoutes = savedRoutes

                        -- Ce qui vient d'être écrit devient l'itinéraire chargé,
                        -- pour que la retouche suivante le reprenne au lieu d'en
                        -- créer un troisième.
                        , loadedRouteId = Just savedRoute.id
                        , openFolders = Set.insert (folderKey savedRoute) model.openFolders
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
            ( { model | pending = True, pendingSeconds = 0 }
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
            ( { model | pending = True, pendingSeconds = 0 }
            , Api.getSavedRoute id SavedRouteLoaded
            )

        SavedRouteLoaded result ->
            case result of
                Ok savedRoute ->
                    let
                        route =
                            savedRoute.routeData

                        -- Imported routes carry no waypoints: sample the path instead of using every point
                        waypoints =
                            Maybe.withDefault (sampleEveryKm 2 route.path) savedRoute.originalWaypoints

                        markerCmds =
                            [ Ports.updateWaypointMarkers waypoints ]

                        -- Un itinéraire enregistré ne porte pas ses revêtements :
                        -- on les demande au moteur plutôt que de recalculer le
                        -- tracé, qui sortirait différent de celui qu'on a ouvert.
                        surfacesCmd =
                            case route.pointSurfaces of
                                Just _ ->
                                    Cmd.none

                                Nothing ->
                                    Cmd.batch
                                        [ Api.fetchSurfaces route.path SurfacesLoaded

                                        -- Tracé dans le journal : la requête peut
                                        -- durer des minutes quand le graphe de la
                                        -- région n'est pas en cache, et sans cette
                                        -- ligne rien ne dit qu'elle est partie.
                                        , Api.sendLog
                                            ("surfaces demandées pour "
                                                ++ String.fromInt (List.length route.path)
                                                ++ " points"
                                            )
                                            (\_ -> NoOp)
                                        ]
                    in
                    ( applySavedRoute
                        { model
                            | pending = False
                            , error = Nothing
                            , loadedRouteId = Just savedRoute.id

                            -- Préremplis : une mise à jour qui part de champs
                            -- vides écrirait un nom vide sur la ligne.
                            , saveRouteName = savedRoute.name
                            , saveRouteDescription = Maybe.withDefault "" savedRoute.description
                            , saveRouteFolder = folderKey savedRoute
                            , openFolders = Set.insert (folderKey savedRoute) model.openFolders
                        }
                        route
                        waypoints
                    , Cmd.batch
                        ([ Ports.updateRoute route.path
                         , Ports.setRouteSurfaces (surfaceLayers route)
                         , frameRoute route.path
                         , surfacesCmd
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
            ( { model | pending = True, pendingSeconds = 0 }
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
            ( { model | pending = True, pendingSeconds = 0 }
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
                        , Ports.setRouteSurfaces (surfaceLayers route)
                        , frameRoute route.path
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
            ( { model | elevationHoverIndex = Just idx }
            , elevationMarkerCmd model (Just idx)
            )

        ElevationChartLeave ->
            -- Le curseur posé sur la bande survit à la sortie de souris : on lui
            -- rend le marqueur au lieu de l'effacer.
            ( { model | elevationHoverIndex = Nothing }
            , elevationMarkerCmd model model.elevationCursorIndex
            )

        ElevationCursorMoved idx ->
            ( { model | elevationCursorIndex = Just idx }
            , elevationMarkerCmd model (Just idx)
            )

        ToggleElevationBand ->
            ( { model | showElevationBand = not model.showElevationBand }
            , Cmd.none
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
                        , directLegs = Set.empty
                        , routeMode = MultiPoint
                        , error = Nothing
                    }

                -- Calcule le bbox des points importés et recadre la carte dessus.
                -- Sans ça, l'utilisateur reste sur la vue précédente (ex : Beaujolais)
                -- et ne voit pas ses points GPX qui sont parfois à 100+ km de là.
                centerCmd =
                    case bboxOfCoords coords of
                        Just ( sw, ne ) ->
                            Ports.centerOnMarkers { start = sw, end = ne }

                        Nothing ->
                            Cmd.none
            in
            if List.length coords >= 2 then
                let
                    ( routedModel, routeCmd ) =
                        update ComputeMultiPointRoute newModel
                in
                ( routedModel
                , Cmd.batch
                    [ Ports.updateWaypointMarkers coords
                    , centerCmd
                    , routeCmd
                    ]
                )

            else
                ( newModel
                , Cmd.batch
                    [ Ports.updateWaypointMarkers coords
                    , centerCmd
                    ]
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



-- VIEW


{-| Marqueur de survol sur la carte pour un index du tracé.
`Nothing`, ou un index hors du tracé, efface le marqueur.
-}
elevationMarkerCmd : Model -> Maybe Int -> Cmd Msg
elevationMarkerCmd model maybeIdx =
    let
        coord =
            Maybe.map2 Tuple.pair maybeIdx model.lastResponse
                |> Maybe.andThen
                    (\( idx, route ) -> route.path |> List.drop idx |> List.head)
    in
    Ports.setElevationHoverMarker
        (Maybe.map (\c -> { lat = c.lat, lon = c.lon }) coord)


{-| Barre d'onglets du panneau latéral.

Le formulaire et la liste partageaient un seul rouleau : titre, type de tracé,
mode, recherche, import, statistiques, vue de la carte, puis quinze
itinéraires. Séparés, chacun reçoit toute la hauteur du panneau.

-}
viewPanelTabs : Model -> Html Msg
viewPanelTabs model =
    let
        autre =
            case model.panelTab of
                TabPlan ->
                    TabRoutes

                TabRoutes ->
                    TabPlan

        onglet cible libelle =
            let
                actif =
                    model.panelTab == cible
            in
            Html.button
                [ Html.Attributes.classList
                    [ ( "panel-tab", True )
                    , ( "is-active", actif )
                    ]
                , Html.Attributes.type_ "button"
                , Html.Attributes.id (tabDomId cible)
                , Html.Attributes.attribute "role" "tab"
                , Html.Attributes.attribute "aria-selected" (boolAttr actif)
                , Html.Attributes.attribute "aria-controls" panelBodyDomId

                -- Tabulation mobile : un groupe d'onglets est un seul arrêt de
                -- tabulation, et les flèches circulent dedans. Deux arrêts pour
                -- deux onglets obligeraient à traverser le second pour atteindre
                -- le contenu du premier.
                , Html.Attributes.tabindex
                    (if actif then
                        0

                     else
                        -1
                    )
                , Html.Events.onClick (SwitchPanelTab cible)
                ]
                [ text libelle ]
    in
    div
        [ class "panel-tabs"
        , Html.Attributes.attribute "role" "tablist"
        , Html.Attributes.attribute "aria-label" "Sections du panneau"
        , onArrowSwitch (SwitchPanelTab autre)
        ]
        [ onglet TabPlan "Tracer"
        , onglet TabRoutes
            ("Mes tracés (" ++ String.fromInt (List.length model.savedRoutes) ++ ")")
        ]


{-| Identifiants DOM de la coquille.

Ils sont écrits une fois ici parce que trois endroits doivent s'accorder : le
bouton d'onglet que `SwitchPanelTab` va focaliser, l'`aria-controls` qui le
relie au corps du panneau, et le `aria-labelledby` qui fait le lien inverse.
Une faute de frappe dans l'un des trois ne casse rien de visible — elle rend
seulement l'annonce muette.

-}
tabDomId : PanelTab -> String
tabDomId onglet =
    case onglet of
        TabPlan ->
            "panel-tab-plan"

        TabRoutes ->
            "panel-tab-routes"


{-| Nom de l'onglet tel qu'il traverse le port.

Il faut un aller-retour par une chaîne parce que le JavaScript ouvre la
transition de vue entre les deux : `PanelTab` ne survit pas au voyage. Les deux
fonctions sont volontairement l'une sous l'autre, et `tabFromKey` retombe sur
`TabPlan` plutôt que de faillir — un nom inconnu ne peut venir que d'une faute
de frappe de notre côté, et l'onglet de planification est l'état de départ.

-}
tabKey : PanelTab -> String
tabKey onglet =
    case onglet of
        TabPlan ->
            "plan"

        TabRoutes ->
            "routes"


tabFromKey : String -> PanelTab
tabFromKey cle =
    if cle == "routes" then
        TabRoutes

    else
        TabPlan


panelBodyDomId : String
panelBodyDomId =
    "panel-body"


mapDomId : String
mapDomId =
    "map"


boolAttr : Bool -> String
boolAttr valeur =
    if valeur then
        "true"

    else
        "false"


{-| Déplace le focus clavier, en ignorant l'échec.

`Browser.Dom.focus` échoue si l'identifiant n'existe pas encore. C'est le cas
normal ici : la commande part dans le même tour que le changement de modèle,
donc avant que le nouveau HTML ne soit posé pour certains éléments. Rien à
signaler à l'utilisateur dans ce cas, le focus reste simplement sur place.

-}
focusOn : String -> Cmd Msg
focusOn domId =
    Browser.Dom.focus domId |> Task.attempt (\_ -> NoOp)


{-| Flèches gauche/droite dans le groupe d'onglets.

`preventDefault` est nécessaire : sans lui, la flèche fait aussi défiler le
panneau pendant que l'onglet change.

-}
onArrowSwitch : Msg -> Html.Attribute Msg
onArrowSwitch msg =
    Html.Events.preventDefaultOn "keydown"
        (Json.Decode.field "key" Json.Decode.string
            |> Json.Decode.andThen
                (\key ->
                    if List.member key [ "ArrowLeft", "ArrowRight", "ArrowUp", "ArrowDown" ] then
                        Json.Decode.succeed ( msg, True )

                    else
                        Json.Decode.fail "touche sans effet sur les onglets"
                )
        )


{-| Rappel de l'itinéraire affiché en calque.

Le seul indice était un bouton doré dans le panneau des itinéraires, invisible
une fois celui-ci refermé : on ne savait plus quel tracé on regardait, et trois
noms de la liste commencent par « Sologne ».

-}
viewReferenceBadges : Model -> Html Msg
viewReferenceBadges model =
    if Dict.isEmpty model.referenceRoutes then
        text ""

    else
        div [ class "reference-badges" ]
            (referenceBadgeGroups model
                |> List.map (viewReferenceBadge model.savedRoutes)
            )


{-| Un dossier entier en calque ferait 53 badges : ceux d'un même dossier et
d'une même couleur n'en font qu'un. Les itinéraires sans dossier restent seuls.
-}
referenceBadgeGroups : Model -> List ( String, List Int )
referenceBadgeGroups model =
    let
        folderOf id =
            model.savedRoutes
                |> List.filter (\route -> route.id == id)
                |> List.head
                |> Maybe.map folderKey
                |> Maybe.withDefault ""

        add ( id, layer ) groups =
            let
                folder =
                    folderOf id

                key =
                    if folder == "" then
                        ( "", layer.color, id )

                    else
                        ( folder, layer.color, 0 )
            in
            Dict.update key (\ids -> Just (id :: Maybe.withDefault [] ids)) groups
    in
    Dict.toList model.referenceRoutes
        |> List.foldl add Dict.empty
        |> Dict.toList
        |> List.map (\( ( _, color, _ ), ids ) -> ( color, List.reverse ids ))


viewReferenceBadge : List SavedRoute -> ( String, List Int ) -> Html Msg
viewReferenceBadge savedRoutes ( color, ids ) =
    let
        routes =
            List.filter (\route -> List.member route.id ids) savedRoutes

        name =
            case routes of
                [ route ] ->
                    route.name

                first :: _ :: _ ->
                    folderKey first ++ " · " ++ String.fromInt (List.length ids) ++ " itinéraires"

                [] ->
                    "Itinéraire de référence"
    in
    div [ class "reference-badge" ]
        [ span
            [ class "reference-badge-swatch"
            , style "border-top-color" color
            ]
            []
        , span [ class "reference-badge-name" ] [ text name ]
        , button
            [ class "reference-badge-close"
            , Html.Events.onClick (RemoveReferenceRoutes ids)
            , Html.Attributes.title "Retirer le calque"
            ]
            [ text "✕" ]
        ]


{-| Les calques dont le tracé est arrivé, à redessiner d'un bloc.
-}
referenceRoutesCmd : Model -> Cmd Msg
referenceRoutesCmd model =
    model.referenceRoutes
        |> Dict.values
        |> List.filterMap
            (\layer -> Maybe.map (\path -> { path = path, color = layer.color }) layer.path)
        |> Ports.setReferenceRoutes


{-| Couleurs des calques : vives, pour trancher sur le vert et le beige de
la carte topo, et loin du vert du tracé courant. Au-delà de la palette, on
recommence au début.
-}
referencePalette : List String
referencePalette =
    [ "#1f6feb", "#d6336c", "#7048e8", "#0c8599", "#e8590c", "#212529" ]


nextReferenceColor : Dict Int ReferenceLayer -> String
nextReferenceColor layers =
    let
        used =
            Dict.values layers |> List.map .color
    in
    referencePalette
        |> List.filter (\color -> not (List.member color used))
        |> List.head
        |> Maybe.withDefault
            (referencePalette
                |> List.drop (modBy (List.length referencePalette) (Dict.size layers))
                |> List.head
                |> Maybe.withDefault "#1f6feb"
            )


view : Model -> Html Msg
view model =
    -- `app-root` est en `display: contents` : il disparaît de la mise en page,
    -- mais il sort la bande du `.app-container`, qui deviendrait sinon un bloc
    -- conteneur pour les enfants `position: fixed` — par son `transform` quand
    -- il est replié.
    div
        [ Html.Attributes.classList
            [ ( "app-root", True )
            , ( "has-band", ElevationBand.isVisible model )
            ]
        ]
        [ -- Le panneau compte une soixantaine d'arrêts de tabulation : sans ce
          -- lien, atteindre la carte au clavier demande de les traverser tous.
          Html.a
            [ class "skip-link"
            , Html.Attributes.href ("#" ++ mapDomId)
            ]
            [ text "Aller à la carte" ]
        , div
            [ Html.Attributes.classList
                [ ( "app-container", True )
                , ( "is-collapsed", model.panelCollapsed )
                ]
            , Html.Attributes.id "app-panel"
            ]
            [ header [ class "app-header" ]
                [ h1 [] [ text "Chemins Noirs" ]
                , p [ class "app-subtitle" ] [ text "Générateur GPX anti-bitume" ]
                ]
            , viewPanelTabs model
            , div
                [ Html.Attributes.id panelBodyDomId
                , Html.Attributes.attribute "role" "tabpanel"
                , Html.Attributes.attribute "aria-labelledby" (tabDomId model.panelTab)
                ]
                [ case model.panelTab of
                    TabPlan ->
                        div []
                            [ Form.view model
                            , Preview.view model
                            ]

                    TabRoutes ->
                        Form.viewSavedRoutesSection model
                ]
            ]
        , Html.button
            [ Html.Attributes.classList
                [ ( "panel-toggle", True )
                , ( "is-collapsed", model.panelCollapsed )
                ]
            , Html.Attributes.type_ "button"

            -- Le chevron est décoratif : lu seul, il s'annonce « signe
            -- supérieur ». Le libellé porte l'action, `aria-expanded` l'état.
            , Html.Attributes.attribute "aria-label"
                (if model.panelCollapsed then
                    "Afficher les réglages"

                 else
                    "Masquer les réglages, agrandir la carte"
                )
            , Html.Attributes.attribute "aria-expanded" (boolAttr (not model.panelCollapsed))
            , Html.Attributes.attribute "aria-controls" "app-panel"
            , Html.Attributes.title
                (if model.panelCollapsed then
                    "Afficher les réglages"

                 else
                    "Masquer les réglages, agrandir la carte"
                )
            , Html.Events.onClick TogglePanel
            ]
            [ Html.span [ Html.Attributes.attribute "aria-hidden" "true" ]
                [ text
                    (if model.panelCollapsed then
                        "›"

                     else
                        "‹"
                    )
                ]
            ]
        , viewReferenceBadges model
        , ElevationBand.view model
        ]



-- SUBSCRIPTIONS


subscriptions : Model -> Sub Msg
subscriptions model =
    Sub.batch
        [ if model.pending then
            Time.every 1000 (\_ -> PendingTick)

          else
            Sub.none
        , Ports.mapClickReceived (\{ lat, lon } -> MapClicked lat lon)
        , Ports.routeLineClicked
            (\{ lat, lon } -> insertOnRouteLine { lat = lat, lon = lon } model)
        , Ports.waypointDragged (\{ index, lat, lon } -> MoveWaypoint index lat lon)
        , Ports.waypointDeleted (\{ index } -> RemoveWaypoint index)
        , Ports.routeLoadedFromLocalStorage
            (\value ->
                RouteLoadedFromStorage
                    (Json.Decode.decodeValue Decoders.decodeRouteResponse value)
            )
        , Ports.gotGeolocation (\{ lat, lon } -> GotGeolocation lat lon)
        , Ports.transitionOngletPrete ApplyPanelTab
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

    else if wpCount < maxWp then
        update (AddWaypoint coord) model

    else if model.routeMode == Loop then
        -- Une boucle n'a qu'un départ : cliquer ailleurs le déplace, comme on
        -- glisserait le marqueur. Refuser le clic laissait croire la carte
        -- bloquée. Les boucles proposées partaient de l'ancien départ : on les
        -- retire plutôt que d'afficher un tracé qui n'y passe plus.
        let
            ( moved, moveCmd ) =
                update (MoveWaypoint 0 lat lon) model
        in
        ( { moved
            | loopCandidates = []
            , loopMeta = Nothing
            , selectedLoopIdx = Nothing
            , lastResponse = Nothing
          }
        , Cmd.batch
            [ Ports.updateWaypointMarkers moved.waypoints
            , Ports.updateRoute []
            , moveCmd
            ]
        )

    else
        -- Le clic était avalé sans un mot : passé le plafond du mode, la carte
        -- devenait muette et rien ne disait que c'était voulu. On ne refuse pas
        -- en silence une action que l'utilisateur vient de tenter.
        ( { model | error = Just (clicIgnoreMessage model.routeMode) }
        , Cmd.none
        )


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
            , Cmd.batch
                [ Ports.updateRoute displayRoute.path
                , Ports.setRouteSurfaces (surfaceLayers displayRoute)
                ]
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


{-| Faire passer le tracé par une forêt.

Le point intérieur de la forêt devient une étape, insérée sur le segment le
plus proche : entre départ et arrivée pour un aller simple, qui passe alors en
multi-points puisqu'il en a trois. Sans départ ni arrivée, il n'y a rien à
détourner : on centre la carte sur la forêt et on garde la liste pour après.

-}
viaForest : Forest -> Model -> ( Model, Cmd Msg )
viaForest forest model =
    let
        coord =
            { lat = forest.lat, lon = forest.lon }

        done ( m, cmd ) =
            ( { m | forestResults = [], forestQuery = "" }, cmd )
    in
    case ( model.routeMode, model.waypoints ) of
        ( Loop, _ ) ->
            ( { model | error = Just "Une boucle se calcule depuis son départ : passez en aller simple ou en multi-points pour viser une forêt." }
            , Cmd.none
            )

        ( MultiPoint, _ :: _ :: _ ) ->
            done (update (InsertWaypoint (findInsertionIndex coord model.waypoints) coord) model)

        ( PointToPoint, [ _, _ ] ) ->
            done (update (InsertWaypoint 0 coord) { model | routeMode = MultiPoint })

        _ ->
            ( { model | error = Just "Placez d'abord le départ et l'arrivée, puis choisissez la forêt." }
            , Ports.centerMapOn coord
            )


pushWaypointHistory : Model -> Model
pushWaypointHistory model =
    { model
        | waypointHistory = List.take 50 (( model.waypoints, model.directLegs ) :: model.waypointHistory)
        , waypointFuture = []
        , freehandSegments = Dict.empty
        , freehandDrawing = Nothing
    }


{-| Pourquoi un clic de plus n'ajoute rien, selon le mode.

Chaque mode a son plafond — deux points en aller simple, un seul en boucle —
et le message nomme la sortie plutôt que la règle : ce que l'utilisateur veut
savoir, c'est quoi faire ensuite.

-}
clicIgnoreMessage : RouteMode -> String
clicIgnoreMessage mode =
    case mode of
        PointToPoint ->
            "Un aller simple n'a que deux points. Glissez un marqueur pour le déplacer, cliquez sur le tracé pour insérer une étape, ou cochez Multi-points pour en ajouter d'autres."

        Loop ->
            -- Un clic en boucle déplace le départ : on n'arrive plus ici.
            "Une boucle part d'un seul point."

        MultiPoint ->
            -- Le plafond du mode est à 999 : on n'arrive ici qu'en l'atteignant.
            "Trop de points sur ce tracé."


{-| Insère un point sur le tracé, au bon rang.

Le clic sur la ligne vise un endroit _entre_ deux points existants, et c'est
tout l'intérêt : jusqu'ici on ne pouvait qu'ajouter un point à la fin ou
déplacer un marqueur, donc détourner un tracé chargé demandait de défaire
l'ordre des étapes. `findInsertionIndex` cherche le segment le plus proche du
clic et rend le rang de son départ.

Hors multi-points le tracé n'a pas de segment intérieur où insérer quoi que ce
soit, et `InsertWaypoint` recalcule de toute façon en multi-points : on renvoie
donc vers le mode plutôt que de basculer dans le dos de l'utilisateur.

-}
surfaceLayers :
    RouteResponse
    -> { surfaces : Maybe (List Bool), ign : Maybe (List Bool), elevations : Maybe (List (Maybe Float)) }
surfaceLayers route =
    { surfaces = route.pointSurfaces
    , ign = route.pointIgn
    , elevations = Maybe.map .elevations route.elevationProfile
    }


insertOnRouteLine : Coordinate -> Model -> Msg
insertOnRouteLine coord model =
    if model.routeMode == MultiPoint && List.length model.waypoints >= 2 then
        InsertWaypoint (findInsertionIndex coord model.waypoints) coord

    else
        MapClicked coord.lat coord.lon


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


{-| Un point du tracé tous les `stepKm` environ, départ et arrivée inclus.
-}
sampleEveryKm : Float -> List Coordinate -> List Coordinate
sampleEveryKm stepKm path =
    case path of
        [] ->
            []

        first :: rest ->
            let
                step coord ( prev, sinceLast, kept ) =
                    let
                        dist =
                            sinceLast + haversineKm prev coord
                    in
                    if dist >= stepKm then
                        ( coord, 0, coord :: kept )

                    else
                        ( coord, dist, kept )

                ( last, _, sampled ) =
                    List.foldl step ( first, 0, [ first ] ) rest
            in
            if List.head sampled == Just last then
                List.reverse sampled

            else
                List.reverse (last :: sampled)


{-| Recadre la carte sur toute l'emprise du tracé. Start/end ne suffisent pas :
absents des tracés importés, et confondus sur une boucle.
-}
frameRoute : List Coordinate -> Cmd msg
frameRoute path =
    case bboxOfCoords path of
        Just ( sw, ne ) ->
            Ports.centerOnMarkers { start = sw, end = ne }

        Nothing ->
            Cmd.none


{-| Renvoie le bbox englobant de tous les coords sous forme (SW, NE).
Nothing si la liste est vide. Utilisé pour recadrer la carte sur un import GPX
sans dépendre du fait que premier/dernier point soient des extrêmes du tracé.
-}
bboxOfCoords : List Coordinate -> Maybe ( Coordinate, Coordinate )
bboxOfCoords coords =
    case coords of
        [] ->
            Nothing

        first :: rest ->
            let
                folded =
                    List.foldl
                        (\c acc ->
                            { minLat = min acc.minLat c.lat
                            , maxLat = max acc.maxLat c.lat
                            , minLon = min acc.minLon c.lon
                            , maxLon = max acc.maxLon c.lon
                            }
                        )
                        { minLat = first.lat, maxLat = first.lat, minLon = first.lon, maxLon = first.lon }
                        rest
            in
            Just
                ( { lat = folded.minLat, lon = folded.minLon }
                , { lat = folded.maxLat, lon = folded.maxLon }
                )


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
        , elevationCursorIndex = Nothing
        , error = Nothing
        , form = newForm

        -- Keep original waypoints unchanged - don't extract from calculated path
    }


{-| Métadonnées de sauvegarde lues dans le modèle.

Partagé par la création et la mise à jour : les deux écrivent le même corps,
seule l'URL change. Les waypoints ne sont joints que s'il y en a, sinon le
chargement suivant les réinventerait tous les 2 km.

-}
saveRequestFrom : Model -> SaveRouteRequest
saveRequestFrom model =
    { name = model.saveRouteName
    , description =
        if String.isEmpty model.saveRouteDescription then
            Nothing

        else
            Just model.saveRouteDescription
    , tags = Nothing
    , folder = folderInput model.saveRouteFolder
    , originalWaypoints =
        if List.isEmpty model.waypoints then
            Nothing

        else
            Just model.waypoints
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
        , elevationCursorIndex = Nothing
        , error = Nothing
        , form = newForm
        , waypoints = originalWaypoints -- Restore original waypoints

        -- Les itinéraires enregistrés ne gardent pas quels tronçons étaient
        -- directs : les points rechargés repartent tous sur les chemins.
        , directLegs = Set.empty
        , routeMode = routeMode
    }


loopFormToRequest : Bool -> RouteForm -> LoopForm -> List Coordinate -> Result String LoopRouteRequest
loopFormToRequest cheminNoir form loopForm waypoints =
    case List.head waypoints of
        Just start ->
            let
                weightsResult =
                    if cheminNoir then
                        Ok cheminNoirWeights

                    else
                        case ( String.toFloat form.wPop, String.toFloat form.wPaved, String.toFloat form.wForest ) of
                            ( Just pop, Just paved, Just forest ) ->
                                Ok { pop = pop, paved = paved, forest = forest }

                            _ ->
                                Err "Poids invalides"
            in
            case weightsResult of
                Ok weights ->
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
                                                , wPop = weights.pop
                                                , wPaved = weights.paved
                                                , wForest = weights.forest
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


{-| Poids d'une requête de tracé. -}
type alias Weights =
    { pop : Float
    , paved : Float
    , forest : Float
    }


{-| Le mode « chemin noir » cache les curseurs et embarque ses propres poids,
dont un penchant pour la forêt mesuré à +8 points de couvert sur le GR31.
-}
cheminNoirWeights : Weights
cheminNoirWeights =
    { pop = 5.0
    , paved = 8.0
    , forest = 2.0
    }


multiPointRequest : Model -> MultiPointRouteRequest
multiPointRequest model =
    if model.cheminNoir then
        { waypoints = model.waypoints
        , closeLoop = model.closeLoop
        , wPop = cheminNoirWeights.pop
        , wPaved = cheminNoirWeights.paved
        , wForest = cheminNoirWeights.forest
        , directLegs = directLegFlags model
        }

    else
        { waypoints = model.waypoints
        , closeLoop = model.closeLoop
        , wPop = String.toFloat model.form.wPop |> Maybe.withDefault 1.0
        , wPaved = String.toFloat model.form.wPaved |> Maybe.withDefault 1.0
        , wForest = String.toFloat model.form.wForest |> Maybe.withDefault 0.0
        , directLegs = directLegFlags model
        }


directLegFlags : Model -> List Bool
directLegFlags model =
    DirectLegs.flags (DirectLegs.legCount (List.length model.waypoints) model.closeLoop) model.directLegs
