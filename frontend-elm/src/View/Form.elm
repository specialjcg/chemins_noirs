module View.Form exposing (view)

{-| Module de formulaire - Interface utilisateur pour la saisie des paramètres.
Approche fonctionnelle pure : génération déclarative du HTML.
-}

import Html exposing (..)
import Html.Attributes exposing (..)
import Html.Events exposing (onCheck, onClick, onInput)
import Types exposing (..)


view : Model -> Html Msg
view model =
    let
        disableEnd =
            model.routeMode == Loop
    in
    Html.form [ class "controls" ]
        [ fieldset []
            [ legend [] [ text "Type de tracé" ]
            , div [ class "route-type" ]
                [ label []
                    [ input
                        [ type_ "radio"
                        , name "route-mode"
                        , checked (model.routeMode == PointToPoint)
                        , onClick (ToggleRouteMode PointToPoint)
                        ]
                        []
                    , span [] [ text "Aller simple" ]
                    ]
                , label []
                    [ input
                        [ type_ "radio"
                        , name "route-mode"
                        , checked (model.routeMode == Loop)
                        , onClick (ToggleRouteMode Loop)
                        ]
                        []
                    , span [] [ text "Boucle" ]
                    ]
                , label []
                    [ input
                        [ type_ "radio"
                        , name "route-mode"
                        , checked (model.routeMode == MultiPoint)
                        , onClick (ToggleRouteMode MultiPoint)
                        ]
                        []
                    , span [] [ text "Multi-points" ]
                    ]
                ]
            ]
        , fieldset []
            [ legend [] [ text "Points" ]
            , inputField "Latitude départ" model.form.startLat StartLatChanged False
            , inputField "Longitude départ" model.form.startLon StartLonChanged False
            , inputField "Latitude arrivée" model.form.endLat EndLatChanged disableEnd
            , inputField "Longitude arrivée" model.form.endLon EndLonChanged disableEnd
            , if disableEnd then
                small [] [ text "Les coordonnées d'arrivée sont ignorées en mode boucle." ]

              else
                text ""
            ]
        , fieldset []
            [ legend [] [ text "Poids" ]
            , inputField "Éviter population" model.form.wPop PopWeightChanged False
            , inputField "Éviter bitume" model.form.wPaved PavedWeightChanged False
            ]
        , if model.routeMode == Loop then
            fieldset []
                [ legend [] [ text "Options boucle" ]
                , inputField "Distance cible (km)" model.loopForm.distanceKm LoopDistanceChanged False
                , inputField "Tolérance (km)" model.loopForm.toleranceKm LoopToleranceChanged False
                , inputField "Nombre de propositions" model.loopForm.candidateCount LoopCandidateCountChanged False
                , inputField "D+ max (m)" model.loopForm.maxAscentM LoopMaxAscentChanged False
                , inputField "D+ min (m)" model.loopForm.minAscentM LoopMinAscentChanged False
                , small [] [ text "Laissez D+ vide pour obtenir automatiquement la boucle la moins pentue." ]
                ]

          else
            text ""
        , if model.routeMode == MultiPoint then
            fieldset []
                [ legend [] [ text "Points du tracé" ]
                , viewWaypoints model
                , div [ class "input-field" ]
                    [ label []
                        [ input
                            [ type_ "checkbox"
                            , checked model.closeLoop
                            , onCheck (\_ -> ToggleCloseLoop)
                            ]
                            []
                        , span [] [ text " Boucler (retour au point de départ)" ]
                        ]
                    ]
                , div [ class "action-buttons" ]
                    [ button
                        [ type_ "button"
                        , class "btn-geoloc"
                        , onClick RequestGeolocation
                        ]
                        [ text "Ma position" ]
                    , button
                        [ type_ "button"
                        , class "btn-secondary btn-block"
                        , disabled (List.isEmpty model.waypoints)
                        , onClick ClearWaypoints
                        ]
                        [ text "Effacer tous les points" ]
                    ]
                , small [ class "waypoints-summary" ]
                    [ text <|
                        String.fromInt (List.length model.waypoints)
                            ++ " point(s) • Distance: "
                            ++ (case model.lastResponse of
                                    Just r ->
                                        String.fromFloat r.distanceKm ++ " km"

                                    Nothing ->
                                        "0.0 km"
                               )
                    ]
                ]

          else
            text ""
        , if model.routeMode /= MultiPoint then
            fieldset []
                [ legend [] [ text "Sélection via la carte" ]
                , div [ class "click-mode" ]
                    [ label []
                        [ input
                            [ type_ "radio"
                            , name "click-mode"
                            , checked (model.clickMode == Start)
                            , onClick (SetClickMode Start)
                            ]
                            []
                        , span [] [ text "Départ" ]
                        ]
                    , label []
                        [ input
                            [ type_ "radio"
                            , name "click-mode"
                            , checked (model.clickMode == End)
                            , onClick (SetClickMode End)
                            ]
                            []
                        , span [] [ text "Arrivée" ]
                        ]
                    ]
                , small [] [ text "Cliquez sur la carte pour remplir la position sélectionnée." ]
                ]

          else
            text ""
        , fieldset []
            [ legend [] [ text "Vue de la carte" ]
            , button
                [ type_ "button"
                , class "map-toggle"
                , onClick ToggleMapView
                ]
                [ text <|
                    case model.mapViewMode of
                        Topo ->
                            "Vue Satellite"

                        Satellite ->
                            "Vue Hybride"

                        Hybrid ->
                            "Vue Topo"
                ]
            ]
        , if model.lastResponse /= Nothing then
            fieldset []
                [ legend [] [ text "Sauvegarde" ]
                , div [ class "input-field" ]
                    [ Html.label [] [ text "Nom du tracé" ]
                    , input
                        [ Html.Attributes.value model.saveRouteName
                        , onInput SaveRouteNameChanged
                        , placeholder "Ma belle randonnée"
                        , autocomplete False
                        , spellcheck False
                        ]
                        []
                    ]
                , div [ class "input-field" ]
                    [ Html.label [] [ text "Description (optionnel)" ]
                    , input
                        [ Html.Attributes.value model.saveRouteDescription
                        , onInput SaveRouteDescriptionChanged
                        , placeholder "Description du tracé..."
                        , autocomplete False
                        , spellcheck False
                        ]
                        []
                    ]
                , button
                    [ type_ "button"
                    , class "save-btn"
                    , onClick SaveRouteToDb
                    , disabled (String.isEmpty model.saveRouteName)
                    ]
                    [ text "Sauvegarder dans la base" ]
                , button
                    [ type_ "button"
                    , class "load-btn"
                    , onClick ToggleSavedRoutesPanel
                    ]
                    [ text <|
                        if model.showSavedRoutes then
                            "Masquer les tracés"

                        else
                            "Mes tracés sauvegardés ("
                                ++ String.fromInt (List.length model.savedRoutes)
                                ++ ")"
                    ]
                , if model.showSavedRoutes then
                    div [ class "saved-routes-panel" ]
                        [ if List.isEmpty model.savedRoutes then
                            p [ class "empty-state" ]
                                [ text "Aucun tracé sauvegardé" ]

                          else
                            div [] (List.map viewSavedRoute model.savedRoutes)
                        ]

                  else
                    text ""
                ]

          else
            div []
                [ button
                    [ type_ "button"
                    , class "load-btn"
                    , onClick ToggleSavedRoutesPanel
                    ]
                    [ text <|
                        if model.showSavedRoutes then
                            "Masquer les tracés"

                        else
                            "Mes tracés sauvegardés ("
                                ++ String.fromInt (List.length model.savedRoutes)
                                ++ ")"
                    ]
                , if model.showSavedRoutes then
                    div [ class "saved-routes-panel" ]
                        [ if List.isEmpty model.savedRoutes then
                            p [ class "empty-state" ]
                                [ text "Aucun tracé sauvegardé" ]

                          else
                            div [] (List.map viewSavedRoute model.savedRoutes)
                        ]

                  else
                    text ""
                ]
        , button
            [ type_ "button"
            , class "btn-submit"
            , onClick Submit
            , disabled model.pending
            ]
            [ text "Tracer l'itinéraire" ]
        , case model.error of
            Just error ->
                p [ class "error" ] [ text error ]

            Nothing ->
                text ""
        ]


inputField : String -> String -> (String -> Msg) -> Bool -> Html Msg
inputField label value msg isDisabled =
    div [ class "input-field" ]
        [ Html.label [] [ text label ]
        , input
            [ Html.Attributes.value value
            , onInput msg
            , autocomplete False
            , spellcheck False
            , disabled isDisabled
            ]
            []
        ]


viewWaypoints : Model -> Html Msg
viewWaypoints model =
    div [ class "waypoints-list" ]
        [ if List.isEmpty model.waypoints then
            p [ class "empty-state" ]
                [ text "Cliquez sur la carte pour ajouter des points" ]

          else
            div []
                (List.indexedMap viewWaypoint model.waypoints)
        ]


viewWaypoint : Int -> Coordinate -> Html Msg
viewWaypoint idx coord =
    div [ class "waypoint-item" ]
        [ span [ class "waypoint-coord" ]
            [ text <|
                String.fromInt (idx + 1)
                    ++ ". ("
                    ++ String.left 6 (String.fromFloat coord.lat)
                    ++ ", "
                    ++ String.left 6 (String.fromFloat coord.lon)
                    ++ ")"
            ]
        , button
            [ type_ "button"
            , class "waypoint-remove"
            , onClick (RemoveWaypoint idx)
            ]
            [ text "\u{2715}" ]
        ]


viewSavedRoute : SavedRoute -> Html Msg
viewSavedRoute route =
    div [ class "saved-route-item" ]
        [ div [ class "saved-route-header" ]
            [ div [ class "saved-route-info" ]
                [ h4 [ class "saved-route-name" ]
                    [ text route.name
                    , if route.isFavorite then
                        span [ class "favorite-star" ] [ text "\u{2B50}" ]

                      else
                        text ""
                    ]
                , case route.description of
                    Just desc ->
                        p [ class "saved-route-desc" ]
                            [ text desc ]

                    Nothing ->
                        text ""
                , div [ class "saved-route-stats" ]
                    [ text <|
                        String.fromFloat route.distanceKm
                            ++ " km"
                            ++ (case ( route.totalAscentM, route.totalDescentM ) of
                                    ( Just ascent, Just descent ) ->
                                        " \u{2022} D+ "
                                            ++ String.fromFloat ascent
                                            ++ "m \u{2022} D- "
                                            ++ String.fromFloat descent
                                            ++ "m"

                                    _ ->
                                        ""
                               )
                    ]
                ]
            ]
        , div [ class "saved-route-actions" ]
            [ button
                [ type_ "button"
                , class "action-load"
                , onClick (LoadSavedRoute route.id)
                ]
                [ text "Charger" ]
            , button
                [ type_ "button"
                , classList [ ( "action-fav", True ), ( "active", route.isFavorite ) ]
                , onClick (ToggleFavorite route.id)
                , title <|
                    if route.isFavorite then
                        "Retirer des favoris"

                    else
                        "Ajouter aux favoris"
                ]
                [ text "\u{2B50}" ]
            , button
                [ type_ "button"
                , class "action-delete"
                , onClick (DeleteSavedRoute route.id)
                , title "Supprimer"
                ]
                [ text "\u{2715}" ]
            ]
        ]
