module View.Form exposing (view)

{-| Module de formulaire - Interface utilisateur pour la saisie des paramètres.
Approche fonctionnelle pure : génération déclarative du HTML.
-}

import Html exposing (..)
import Html.Attributes exposing (..)
import Html.Events exposing (onCheck, onClick, onInput, onMouseDown, preventDefaultOn)
import Json.Decode as Decode
import Dict
import Types exposing (..)


view : Model -> Html Msg
view model =
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
        , div [ class "chemin-noir-toggle" ]
            [ label [ class "toggle-label" ]
                [ div [ class "toggle-switch" ]
                    [ input
                        [ type_ "checkbox"
                        , checked model.cheminNoir
                        , onCheck (\_ -> ToggleCheminNoir)
                        , class "toggle-input"
                        ]
                        []
                    , span [ class "toggle-slider" ] []
                    ]
                , div [ class "toggle-text" ]
                    [ span [ class "toggle-title" ] [ text "Mode Chemin Noir" ]
                    , span [ class "toggle-desc" ]
                        [ text
                            (if model.cheminNoir then
                                "Actif — routes et zones habitées évitées"

                             else
                                "Inactif — poids personnalisables"
                            )
                        ]
                    ]
                ]
            ]

        -- Geocoding: centrer la carte
        , fieldset []
            [ legend [] [ text "Centrer la carte" ]
            , div [ class "address-search" ]
                [ div [ class "address-input-row" ]
                    [ input
                        [ Html.Attributes.value model.mapSearch
                        , onInput MapSearchChanged
                        , placeholder "Rechercher un lieu..."
                        , autocomplete False
                        , spellcheck False
                        , onEnter SearchMap
                        ]
                        []
                    , button
                        [ type_ "button"
                        , class "btn-address-search"
                        , onClick SearchMap
                        ]
                        [ text "Rechercher" ]
                    ]
                , if not (List.isEmpty model.mapSearchResults) then
                    div [ class "address-chips-wrapper" ]
                        [ small [ class "address-chips-hint" ] [ text "Choisissez une adresse :" ]
                        , div [ class "address-chips" ]
                            (List.map
                                (\geo ->
                                    let
                                        parts =
                                            String.split ", " geo.displayName

                                        line1 =
                                            List.head parts |> Maybe.withDefault ""

                                        line2 =
                                            parts
                                                |> List.drop 1
                                                |> List.take 2
                                                |> String.join ", "
                                    in
                                    button
                                        [ type_ "button"
                                        , class "address-chip"
                                        , onMouseDown (SelectMapSearchResult geo)
                                        ]
                                        [ span [ class "address-chip-line1" ] [ text line1 ]
                                        , span [ class "address-chip-line2" ] [ text line2 ]
                                        ]
                                )
                                (List.take 3 model.mapSearchResults)
                            )
                        ]

                  else
                    text ""
                ]
            ]
        , if not model.cheminNoir then
            fieldset []
                [ legend [] [ text "Poids" ]
                , inputField "Éviter population" model.form.wPop PopWeightChanged False
                , inputField "Éviter bitume" model.form.wPaved PavedWeightChanged False
                ]

          else
            text ""
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
            div [ class "action-buttons" ]
                [ button
                    [ type_ "button"
                    , class "btn-gpx-import"
                    , onClick ImportGpxClicked
                    ]
                    [ text "Importer GPX" ]
                , if List.length model.waypoints >= 2 then
                    button
                        [ type_ "button"
                        , class "btn-game"
                        , onClick EnterOrienteeringMode
                        ]
                        [ text "Course d'Orientation" ]

                  else
                    text ""
                ]

          else
            text ""
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
        , viewFreehandPanel model
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


onEnter : Msg -> Attribute Msg
onEnter msg =
    preventDefaultOn "keydown"
        (Decode.field "key" Decode.string
            |> Decode.andThen
                (\key ->
                    if key == "Enter" then
                        Decode.succeed ( msg, True )

                    else
                        Decode.fail "not Enter"
                )
        )


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


viewFreehandPanel : Model -> Html Msg
viewFreehandPanel model =
    case ( model.routeMode, model.lastResponse ) of
        ( MultiPoint, Just _ ) ->
            if List.length model.waypoints >= 2 then
                div [ class "freehand-panel" ]
                    [ -- Toggle switch
                      label [ class "toggle-label" ]
                        [ div [ class "toggle-switch" ]
                            [ input
                                [ type_ "checkbox"
                                , checked model.freehandEnabled
                                , onCheck (\_ -> ToggleFreehandMode)
                                , class "toggle-input"
                                ]
                                []
                            , span [ class "toggle-slider" ] []
                            ]
                        , div [ class "toggle-text" ]
                            [ span [ class "toggle-title" ] [ text "Tracé libre" ]
                            , span [ class "toggle-desc" ]
                                [ text (freehandStatusText model) ]
                            ]
                        ]

                    -- Cancel button (visible during active drawing)
                    , case model.freehandDrawing of
                        Just _ ->
                            button
                                [ type_ "button"
                                , class "btn-cancel-freehand"
                                , onClick CancelFreehandDrawing
                                ]
                                [ text "Annuler le dessin" ]

                        Nothing ->
                            text ""

                    -- List of stored freehand segments
                    , if not (Dict.isEmpty model.freehandSegments) then
                        div [ class "freehand-segments-list" ]
                            (Dict.toList model.freehandSegments
                                |> List.map
                                    (\( idx, pts ) ->
                                        div [ class "freehand-segment-item" ]
                                            [ span []
                                                [ text
                                                    ("Segment "
                                                        ++ String.fromInt (idx + 1)
                                                        ++ " → "
                                                        ++ String.fromInt (idx + 2)
                                                        ++ " ("
                                                        ++ String.fromInt (List.length pts)
                                                        ++ " pts)"
                                                    )
                                                ]
                                            , button
                                                [ type_ "button"
                                                , class "btn-clear-segment"
                                                , onClick (ClearFreehandSegment idx)
                                                , title "Supprimer ce segment libre"
                                                ]
                                                [ text "\u{2715}" ]
                                            ]
                                    )
                            )

                      else
                        text ""
                    ]

            else
                text ""

        _ ->
            text ""


freehandStatusText : Model -> String
freehandStatusText model =
    if not model.freehandEnabled then
        "Dessinez des segments à main levée sur la carte"

    else
        case model.freehandDrawing of
            Nothing ->
                "Cliquez près d'un waypoint pour commencer"

            Just state ->
                "Segment "
                    ++ String.fromInt (state.fromIdx + 1)
                    ++ " → "
                    ++ String.fromInt (state.fromIdx + 2)
                    ++ " ("
                    ++ String.fromInt (List.length state.points)
                    ++ " pts) — cliquez près du waypoint suivant"


