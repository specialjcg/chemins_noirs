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
                , viewActionButtons
                , viewMetadata route.metadata
                , viewElevationProfile route.elevationProfile
                , viewElevationChart model route
                , viewPath route
                ]

        Nothing ->
            div [ class "preview" ]
                [ h2 [] [ text "En attente" ]
                , p [] [ text "Soumettez des points pour visualiser un itinéraire." ]
                ]


viewActionButtons : Html Msg
viewActionButtons =
    div [ class "action-buttons" ]
        [ button
            [ class "btn-gpx-export"
            , onClick ExportGpx
            ]
            [ text "Export GPX" ]
        , button
            [ class "btn-share-link"
            , onClick CopyShareLink
            ]
            [ text "Copier le lien" ]
        ]


viewStats : RouteResponse -> Html Msg
viewStats route =
    div [ class "stats" ]
        [ h2 [] [ text "Dernier tracé" ]
        , div [ class "stats-row" ]
            [ p [] [ text <| formatDistance route.distanceKm ++ " km" ]
            , case route.estimatedTimeMinutes of
                Just minutes ->
                    p [ class "stat-time" ] [ text <| formatTime minutes ]

                Nothing ->
                    text ""
            , case route.difficulty of
                Just diff ->
                    span [ class ("difficulty-badge difficulty-" ++ diff) ]
                        [ text (difficultyLabel diff) ]

                Nothing ->
                    text ""
            ]
        , viewSurfaceBreakdown route.surfaceBreakdown
        ]


formatDistance : Float -> String
formatDistance km =
    let
        rounded =
            toFloat (round (km * 100)) / 100
    in
    String.fromFloat rounded


formatTime : Int -> String
formatTime minutes =
    let
        h =
            minutes // 60

        m =
            modBy 60 minutes
    in
    if h > 0 then
        String.fromInt h ++ "h" ++ String.padLeft 2 '0' (String.fromInt m)

    else
        String.fromInt m ++ " min"


difficultyLabel : String -> String
difficultyLabel diff =
    case diff of
        "easy" ->
            "Facile"

        "moderate" ->
            "Modéré"

        "difficult" ->
            "Difficile"

        "expert" ->
            "Expert"

        _ ->
            diff


viewSurfaceBreakdown : Maybe (List ( String, Float )) -> Html Msg
viewSurfaceBreakdown maybeSurfaces =
    case maybeSurfaces of
        Just surfaces ->
            if List.isEmpty surfaces then
                text ""

            else
                let
                    total =
                        List.foldl (\( _, d ) acc -> acc + d) 0 surfaces
                in
                if total <= 0 then
                    text ""

                else
                    div [ class "surface-breakdown" ]
                        [ div [ class "surface-bar" ]
                            (List.map (surfaceSegment total) surfaces)
                        , div [ class "surface-legend" ]
                            (List.map surfaceLegendItem surfaces)
                        ]

        Nothing ->
            text ""


surfaceSegment : Float -> ( String, Float ) -> Html Msg
surfaceSegment total ( name, dist ) =
    let
        pct =
            dist / total * 100
    in
    div
        [ class ("surface-segment surface-" ++ String.toLower name)
        , style "width" (String.fromFloat pct ++ "%")
        , title (name ++ ": " ++ formatDistance dist ++ " km")
        ]
        []


surfaceLegendItem : ( String, Float ) -> Html Msg
surfaceLegendItem ( name, dist ) =
    span [ class "surface-legend-item" ]
        [ span [ class ("surface-dot surface-" ++ String.toLower name) ] []
        , text (name ++ " " ++ formatDistance dist ++ " km")
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


viewElevationChart : Model -> RouteResponse -> Html Msg
viewElevationChart model route =
    case route.elevationProfile of
        Just profile ->
            let
                elevations =
                    profile.elevations
                        |> List.filterMap identity

                count =
                    List.length elevations
            in
            if count < 2 then
                text ""

            else
                div [ class "elevation-chart-container" ]
                    [ div
                        [ class "elevation-chart-header"
                        , onClick ToggleElevationChart
                        ]
                        [ span [] [ text "Profil altimétrique" ]
                        , span []
                            [ text
                                (if model.showElevationChart then
                                    "▼"

                                 else
                                    "▶"
                                )
                            ]
                        ]
                    , if model.showElevationChart then
                        div [ class "elevation-chart-body" ]
                            [ elevationSvg elevations profile ]

                      else
                        text ""
                    ]

        Nothing ->
            text ""


elevationSvg : List Float -> ElevationProfile -> Html Msg
elevationSvg elevations profile =
    let
        width =
            600

        height =
            150

        padding =
            30

        count =
            List.length elevations

        minE =
            profile.minElevation |> Maybe.withDefault 0

        maxE =
            profile.maxElevation |> Maybe.withDefault 1000

        rangeE =
            Basics.max (maxE - minE) 1

        xStep =
            toFloat (width - 2 * padding) / toFloat (Basics.max (count - 1) 1)

        yScale e =
            toFloat (height - 2 * padding) - (e - minE) / rangeE * toFloat (height - 2 * padding)

        points =
            List.indexedMap
                (\i e ->
                    String.fromFloat (toFloat padding + toFloat i * xStep)
                        ++ ","
                        ++ String.fromFloat (toFloat padding + yScale e)
                )
                elevations

        polyline =
            String.join " " points

        -- Area fill (closed polygon under the line)
        areaPoints =
            polyline
                ++ " "
                ++ String.fromFloat (toFloat padding + toFloat (count - 1) * xStep)
                ++ ","
                ++ String.fromInt (height - padding)
                ++ " "
                ++ String.fromInt padding
                ++ ","
                ++ String.fromInt (height - padding)

        -- Y-axis labels
        midE =
            (minE + maxE) / 2
    in
    Html.node "svg"
        [ attribute "viewBox" ("0 0 " ++ String.fromInt width ++ " " ++ String.fromInt height)
        , attribute "preserveAspectRatio" "xMidYMid meet"
        ]
        [ -- Area fill
          Html.node "polygon"
            [ attribute "points" areaPoints
            , attribute "fill" "rgba(77, 171, 123, 0.15)"
            , attribute "stroke" "none"
            ]
            []

        -- Line
        , Html.node "polyline"
            [ attribute "points" polyline
            , attribute "fill" "none"
            , attribute "stroke" "#4dab7b"
            , attribute "stroke-width" "2"
            ]
            []

        -- Min elevation label
        , Html.node "text"
            [ attribute "x" "2"
            , attribute "y" (String.fromFloat (toFloat (height - padding) - 2))
            , attribute "fill" "#9b9484"
            , attribute "font-size" "10"
            ]
            [ text (String.fromInt (round minE) ++ "m") ]

        -- Max elevation label
        , Html.node "text"
            [ attribute "x" "2"
            , attribute "y" (String.fromFloat (toFloat padding + 10))
            , attribute "fill" "#9b9484"
            , attribute "font-size" "10"
            ]
            [ text (String.fromInt (round maxE) ++ "m") ]
        ]


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
