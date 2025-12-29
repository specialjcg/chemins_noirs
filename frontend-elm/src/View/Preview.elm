module View.Preview exposing (view)

{-| Module de prévisualisation - Affichage des résultats de route.
-}

import Html exposing (..)
import Html.Attributes exposing (..)
import Html.Events exposing (onClick)
import Types exposing (..)


view : Model -> Html Msg
view model =
    case model.lastResponse of
        Just route ->
            div [ class "preview" ]
                [ viewLoopCandidates model
                , viewStats route
                , viewMetadata route.metadata
                , viewElevationProfile route.elevationProfile
                , viewPath route
                ]

        Nothing ->
            div [ class "preview" ]
                [ h2 [] [ text "En attente" ]
                , p [] [ text "Soumettez des points pour visualiser un itinéraire." ]
                ]


viewStats : RouteResponse -> Html Msg
viewStats route =
    div [ class "stats" ]
        [ h2 [] [ text "Dernier tracé" ]
        , p [] [ text <| String.fromFloat route.distanceKm ++ " km parcourus" ]
        , small [] [ text "Téléchargez le GPX via l'API (payload base64)" ]
        ]


viewMetadata : Maybe RouteMetadata -> Html Msg
viewMetadata maybeMeta =
    case maybeMeta of
        Just meta ->
            div [ class "metadata-grid" ]
                [ metadataCard "Points" (String.fromInt meta.pointCount)
                , metadataCard "Départ"
                    (String.fromFloat meta.start.lat
                        ++ " / "
                        ++ String.fromFloat meta.start.lon
                    )
                , metadataCard "Arrivée"
                    (String.fromFloat meta.end.lat
                        ++ " / "
                        ++ String.fromFloat meta.end.lon
                    )
                , metadataCard "BBox"
                    ("["
                        ++ String.fromFloat meta.bounds.minLat
                        ++ "↔"
                        ++ String.fromFloat meta.bounds.maxLat
                        ++ "] lat / ["
                        ++ String.fromFloat meta.bounds.minLon
                        ++ "↔"
                        ++ String.fromFloat meta.bounds.maxLon
                        ++ "] lon"
                    )
                ]

        Nothing ->
            text ""


metadataCard : String -> String -> Html Msg
metadataCard label content =
    div [ class "metadata-card" ]
        [ span [ class "label" ] [ text label ]
        , strong [] [ text content ]
        ]


viewElevationProfile : Maybe ElevationProfile -> Html Msg
viewElevationProfile maybeProfile =
    case maybeProfile of
        Just profile ->
            div [ class "elevation-section" ]
                [ h3 [] [ text "Profil d'élévation" ]
                , div [ class "metadata-grid" ]
                    [ metadataCard "Dénivelé +" (String.fromFloat profile.totalAscent ++ " m")
                    , metadataCard "Dénivelé -" (String.fromFloat profile.totalDescent ++ " m")
                    , metadataCard "Altitude min"
                        (case profile.minElevation of
                            Just e ->
                                String.fromFloat e ++ " m"

                            Nothing ->
                                "N/A"
                        )
                    , metadataCard "Altitude max"
                        (case profile.maxElevation of
                            Just e ->
                                String.fromFloat e ++ " m"

                            Nothing ->
                                "N/A"
                        )
                    ]
                ]

        Nothing ->
            text ""


viewPath : RouteResponse -> Html Msg
viewPath route =
    let
        pathPoints =
            List.indexedMap (viewPathPoint route.elevationProfile) route.path
    in
    ul [ class "path-preview" ] pathPoints


viewPathPoint : Maybe ElevationProfile -> Int -> Coordinate -> Html Msg
viewPathPoint maybeProfile idx coord =
    let
        elevation =
            maybeProfile
                |> Maybe.andThen
                    (\profile ->
                        profile.elevations
                            |> List.drop idx
                            |> List.head
                            |> Maybe.andThen identity
                    )

        elevationText =
            case elevation of
                Just e ->
                    " — " ++ String.fromFloat e ++ " m"

                Nothing ->
                    ""
    in
    li []
        [ text <|
            String.fromInt idx
                ++ ": "
                ++ String.fromFloat coord.lat
                ++ " / "
                ++ String.fromFloat coord.lon
                ++ elevationText
        ]


viewLoopCandidates : Model -> Html Msg
viewLoopCandidates model =
    if List.isEmpty model.loopCandidates then
        text ""

    else
        let
            heading =
                case model.loopMeta of
                    Just meta ->
                        "Boucles proposées – cible "
                            ++ String.fromFloat meta.targetDistanceKm
                            ++ " km (± "
                            ++ String.fromFloat meta.distanceToleranceKm
                            ++ " km)"

                    Nothing ->
                        "Boucles proposées"

            entries =
                List.indexedMap (viewLoopCandidate model.selectedLoopIdx) model.loopCandidates
        in
        div [ class "loop-candidates" ]
            [ h3 [] [ text heading ]
            , small [] [ text "Choisissez la boucle qui vous convient le mieux." ]
            , div [] entries
            ]


viewLoopCandidate : Maybe Int -> Int -> LoopCandidate -> Html Msg
viewLoopCandidate selectedIdx idx candidate =
    let
        ascentLabel =
            case candidate.route.elevationProfile of
                Just profile ->
                    String.fromFloat profile.totalAscent ++ " m D+"

                Nothing ->
                    "D+ ?"

        className =
            if selectedIdx == Just idx then
                "loop-choice selected"

            else
                "loop-choice"
    in
    button
        [ class className
        , onClick (SelectLoopCandidate idx)
        ]
        [ text <|
            "#"
                ++ String.fromInt idx
                ++ " – "
                ++ String.fromFloat candidate.route.distanceKm
                ++ " km • "
                ++ ascentLabel
                ++ " • Δ"
                ++ (if candidate.distanceErrorKm >= 0 then
                        "+"

                    else
                        ""
                   )
                ++ String.fromFloat candidate.distanceErrorKm
                ++ " km • cap "
                ++ String.fromFloat candidate.bearingDeg
                ++ "°"
        ]
