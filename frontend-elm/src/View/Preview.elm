module View.Preview exposing (view)

{-| Module de prévisualisation - Affichage des résultats de route.
-}

import Format
import Html exposing (..)
import Html.Attributes exposing (..)
import Html.Events exposing (onClick, on)
import Json.Decode as Decode
import Types exposing (..)
import Svg
import View.ElevationBand as ElevationBand


view : Model -> Html Msg
view model =
    case model.lastResponse of
        Just route ->
            div [ class "preview" ]
                [ viewLoopCandidates model
                , viewStats route
                , viewActionButtons
                , viewSegmentStats route.segments
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
            [ p [] [ text <| Format.km route.distanceKm ]
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
        , title (name ++ " : " ++ Format.km dist)
        ]
        []


surfaceLegendItem : ( String, Float ) -> Html Msg
surfaceLegendItem ( name, dist ) =
    span [ class "surface-legend-item" ]
        [ span [ class ("surface-dot surface-" ++ String.toLower name) ] []
        , text (name ++ " " ++ Format.km dist)
        ]


viewSegmentStats : Maybe (List SegmentStats) -> Html Msg
viewSegmentStats maybeSegments =
    case maybeSegments of
        Just segments ->
            if List.length segments < 2 then
                text ""

            else
                div [ class "segment-stats" ]
                    [ h3 [] [ text "Stats par segment" ]
                    , div [] (List.indexedMap viewSegment segments)
                    ]

        Nothing ->
            text ""


viewSegment : Int -> SegmentStats -> Html Msg
viewSegment idx seg =
    div [ class "segment-card" ]
        [ span [ class "segment-label" ]
            [ text ("Segment " ++ String.fromInt (idx + 1)) ]
        , div [ class "segment-details" ]
            [ span [] [ text (Format.km seg.distanceKm) ]
            , span [] [ text ("D+ " ++ Format.metres seg.ascentM) ]
            , span [] [ text ("D- " ++ Format.metres seg.descentM) ]
            , span [] [ text (Format.pourcent seg.avgSlopePct ++ " de pente") ]
            ]
        ]


viewMetadata : Maybe RouteMetadata -> Html Msg
viewMetadata maybeMeta =
    case maybeMeta of
        Just meta ->
            div [ class "metadata-grid" ]
                [ metadataCard "Points" (String.fromInt meta.pointCount)
                , metadataCard "Départ"
                    (Format.coord meta.start.lat
                        ++ " / "
                        ++ Format.coord meta.start.lon
                    )
                , metadataCard "Arrivée"
                    (Format.coord meta.end.lat
                        ++ " / "
                        ++ Format.coord meta.end.lon
                    )
                , metadataCard "BBox"
                    ("["
                        ++ Format.coord meta.bounds.minLat
                        ++ "↔"
                        ++ Format.coord meta.bounds.maxLat
                        ++ "] lat / ["
                        ++ Format.coord meta.bounds.minLon
                        ++ "↔"
                        ++ Format.coord meta.bounds.maxLon
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
                    [ metadataCard "Dénivelé +" (Format.metres profile.totalAscent)
                    , metadataCard "Dénivelé -" (Format.metres profile.totalDescent)
                    , metadataCard "Altitude min"
                        (case profile.minElevation of
                            Just e ->
                                Format.metres e

                            Nothing ->
                                "inconnue"
                        )
                    , metadataCard "Altitude max"
                        (case profile.maxElevation of
                            Just e ->
                                Format.metres e

                            Nothing ->
                                "inconnue"
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
                -- `List.filterMap identity` décalerait les index dès qu'une
                -- altitude manque, et le marqueur tomberait à côté sur la carte.
                elevations =
                    ElevationBand.fillGaps profile.elevations

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
                            [ elevationSvg model elevations profile ]

                      else
                        text ""
                    ]

        Nothing ->
            text ""


elevationSvg : Model -> List Float -> ElevationProfile -> Html Msg
elevationSvg model elevations profile =
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

        -- Mouse move decoder: offsetX / clientWidth → index
        mouseMoveDecoder =
            Decode.map2
                (\offsetX clientWidth ->
                    let
                        ratio =
                            (offsetX - toFloat padding) / (clientWidth - 2 * toFloat padding)

                        idx =
                            Basics.max 0 (Basics.min (count - 1) (round (ratio * toFloat (count - 1))))
                    in
                    ElevationChartHover idx
                )
                (Decode.field "offsetX" Decode.float)
                (Decode.at [ "currentTarget", "clientWidth" ] Decode.float)

        -- Hover indicator: green circle at hovered elevation point
        chartHoverIndicator =
            case model.elevationHoverIndex of
                Just idx ->
                    if idx >= 0 && idx < count then
                        case List.drop idx elevations |> List.head of
                            Just e ->
                                let
                                    cx =
                                        toFloat padding + toFloat idx * xStep

                                    cy =
                                        toFloat padding + yScale e
                                in
                                [ Svg.circle
                                    [ attribute "cx" (String.fromFloat cx)
                                    , attribute "cy" (String.fromFloat cy)
                                    , attribute "r" "4"
                                    , attribute "class" "preview-point"
                                    , attribute "stroke-width" "1.5"
                                    ]
                                    []
                                , Svg.text_
                                    [ attribute "x" (String.fromFloat (cx + 6))
                                    , attribute "y" (String.fromFloat (cy - 6))
                                    , attribute "class" "preview-label-strong"
                                    , attribute "font-size" "10"
                                    , attribute "font-family" "var(--font-mono)"
                                    ]
                                    [ text (String.fromInt (round e) ++ "m") ]
                                ]

                            Nothing ->
                                []

                    else
                        []

                Nothing ->
                    []

        -- Map route hover: gold dashed vertical line
        mapHoverCursor =
            case model.mapRouteHoverIndex of
                Just idx ->
                    if idx >= 0 && idx < count then
                        let
                            cx =
                                toFloat padding + toFloat idx * xStep
                        in
                        [ Svg.line
                            [ attribute "x1" (String.fromFloat cx)
                            , attribute "y1" (String.fromInt padding)
                            , attribute "x2" (String.fromFloat cx)
                            , attribute "y2" (String.fromInt (height - padding))
                            , attribute "class" "preview-cursor"
                            , attribute "stroke-width" "1.5"
                            , attribute "stroke-dasharray" "4 3"
                            , attribute "opacity" "0.8"
                            ]
                            []
                        ]

                    else
                        []

                Nothing ->
                    []
    in
    Svg.svg
        [ attribute "viewBox" ("0 0 " ++ String.fromInt width ++ " " ++ String.fromInt height)
        , attribute "preserveAspectRatio" "xMidYMid meet"
        , on "mousemove" mouseMoveDecoder
        , on "mouseleave" (Decode.succeed ElevationChartLeave)
        ]
        ([ -- Area fill
           Svg.polygon
            [ attribute "points" areaPoints
            , attribute "fill" "rgba(77, 171, 123, 0.15)"
            , attribute "stroke" "none"
            ]
            []

         -- Line
         , Svg.polyline
            [ attribute "points" polyline
            , attribute "fill" "none"
            , attribute "class" "preview-profile"
            , attribute "stroke-width" "2"
            ]
            []

         -- Min elevation label
         , Svg.text_
            [ attribute "x" "2"
            , attribute "y" (String.fromFloat (toFloat (height - padding) - 2))
            , attribute "class" "preview-label"
            , attribute "font-size" "10"
            ]
            [ text (String.fromInt (round minE) ++ "m") ]

         -- Max elevation label
         , Svg.text_
            [ attribute "x" "2"
            , attribute "y" (String.fromFloat (toFloat padding + 10))
            , attribute "class" "preview-label"
            , attribute "font-size" "10"
            ]
            [ text (String.fromInt (round maxE) ++ "m") ]
         ]
            ++ mapHoverCursor
            ++ chartHoverIndicator
        )


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
                    " — " ++ Format.metres e

                Nothing ->
                    ""
    in
    li []
        [ text <|
            String.fromInt idx
                ++ ": "
                ++ Format.coord coord.lat
                ++ " / "
                ++ Format.coord coord.lon
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
                            ++ Format.km meta.targetDistanceKm
                            ++ " (± "
                            ++ Format.km meta.distanceToleranceKm
                            ++ ")"

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
                    Format.metres profile.totalAscent ++ " D+"

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
                ++ Format.km candidate.route.distanceKm
                ++ " • "
                ++ ascentLabel
                ++ " • Δ"
                ++ Format.deltaKm candidate.distanceErrorKm
                ++ " • cap "
                ++ Format.cap candidate.bearingDeg
        ]
