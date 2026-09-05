module View.ElevationBand exposing (fillGaps, view)

{-| Bande de dénivelé ancrée en bas de l'écran, sous la carte.

Un `input[type=range]` transparent recouvre le profil : le faire glisser
déplace un curseur dans la bande et un point sur le tracé de la carte
(via `Ports.setElevationHoverMarker`, piloté depuis `Main`).

Le profil est dessiné en SVG dans un repère 0-100 sur les deux axes avec
`preserveAspectRatio="none"`, pour occuper toute la largeur quelle que soit
la fenêtre. Les traits portent `vector-effect="non-scaling-stroke"` afin de
ne pas être étirés ; tout ce qui ne survivrait pas à cette déformation
(textes, pastille du curseur) est en HTML par-dessus.

-}

import Html exposing (Html, button, div, input, span, text)
import Html.Attributes as Attr exposing (class, classList, style)
import Html.Events exposing (on, onClick, targetValue)
import Json.Decode as Decode
import Svg
import Svg.Attributes as SA
import Types exposing (ElevationProfile, Model, Msg(..), RouteResponse)


view : Model -> Html Msg
view model =
    case model.lastResponse of
        Just route ->
            case route.elevationProfile of
                Just profile ->
                    let
                        elevations =
                            fillGaps profile.elevations
                    in
                    if List.length elevations < 2 then
                        text ""

                    else
                        viewBand model route profile elevations

                Nothing ->
                    text ""

        Nothing ->
            text ""


viewBand : Model -> RouteResponse -> ElevationProfile -> List Float -> Html Msg
viewBand model route profile elevations =
    let
        count =
            List.length elevations

        lastIdx =
            count - 1

        minE =
            profile.minElevation |> Maybe.withDefault (List.minimum elevations |> Maybe.withDefault 0)

        maxE =
            profile.maxElevation |> Maybe.withDefault (List.maximum elevations |> Maybe.withDefault 1)

        rangeE =
            Basics.max (maxE - minE) 1

        xPct i =
            toFloat i / toFloat (Basics.max lastIdx 1) * 100

        yPct e =
            (maxE - e) / rangeE * 100

        -- Curseur épinglé par le slider ; le survol du graphe du panneau
        -- latéral le masque temporairement.
        pinnedIdx =
            model.elevationCursorIndex
                |> Maybe.map (Basics.clamp 0 lastIdx)

        activeIdx =
            case model.elevationHoverIndex of
                Just i ->
                    Just (Basics.clamp 0 lastIdx i)

                Nothing ->
                    pinnedIdx

        elevationAt i =
            elevations |> List.drop i |> List.head

        kmAt i =
            toFloat i / toFloat (Basics.max lastIdx 1) * route.distanceKm
    in
    div
        [ class "elevation-band"
        , classList [ ( "is-collapsed", not model.showElevationBand ) ]
        ]
        [ div [ class "elevation-band-header" ]
            [ span [ class "elevation-band-title" ] [ text "Dénivelé" ]
            , span [ class "elevation-band-readout" ]
                (case activeIdx of
                    Just i ->
                        [ span [ class "elevation-band-km" ]
                            [ text (formatKm (kmAt i) ++ " km") ]
                        , span [ class "elevation-band-alt" ]
                            [ text
                                (case elevationAt i of
                                    Just e ->
                                        String.fromInt (round e) ++ " m"

                                    Nothing ->
                                        "—"
                                )
                            ]
                        ]

                    Nothing ->
                        [ span [ class "elevation-band-alt" ]
                            [ text
                                ("D+ "
                                    ++ String.fromInt (round profile.totalAscent)
                                    ++ " m · D- "
                                    ++ String.fromInt (round profile.totalDescent)
                                    ++ " m"
                                )
                            ]
                        ]
                )
            , button
                [ class "elevation-band-toggle"
                , onClick ToggleElevationBand
                , Attr.title
                    (if model.showElevationBand then
                        "Replier la bande de dénivelé"

                     else
                        "Déplier la bande de dénivelé"
                    )
                ]
                [ text
                    (if model.showElevationBand then
                        "▼"

                     else
                        "▲"
                    )
                ]
            ]
        , if model.showElevationBand then
            div [ class "elevation-band-plot" ]
                (viewProfile elevations xPct yPct
                    :: viewMapHoverCursor model lastIdx xPct
                    ++ viewCursor activeIdx elevationAt xPct yPct
                    ++ [ input
                            [ Attr.type_ "range"
                            , class "elevation-band-slider"
                            , Attr.min "0"
                            , Attr.max (String.fromInt lastIdx)
                            , Attr.step "1"
                            , Attr.value (String.fromInt (Maybe.withDefault 0 pinnedIdx))
                            , Attr.attribute "aria-label" "Position sur le profil altimétrique"
                            , on "input" (Decode.map ElevationCursorMoved targetInt)
                            ]
                            []
                       , span [ class "elevation-band-max" ] [ text (String.fromInt (round maxE) ++ " m") ]
                       , span [ class "elevation-band-min" ] [ text (String.fromInt (round minE) ++ " m") ]
                       ]
                )

          else
            text ""
        ]


viewProfile : List Float -> (Int -> Float) -> (Float -> Float) -> Html Msg
viewProfile elevations xPct yPct =
    let
        points =
            elevations
                |> List.indexedMap
                    (\i e -> String.fromFloat (xPct i) ++ "," ++ String.fromFloat (yPct e))
                |> String.join " "
    in
    Svg.svg
        [ SA.viewBox "0 0 100 100"
        , SA.preserveAspectRatio "none"
        , SA.class "elevation-band-svg"
        , Attr.attribute "aria-hidden" "true"
        ]
        [ Svg.polygon
            [ SA.points (points ++ " 100,100 0,100")
            , SA.fill "rgba(77, 171, 123, 0.18)"
            , SA.stroke "none"
            ]
            []
        , Svg.polyline
            [ SA.points points
            , SA.fill "none"
            , SA.stroke "#4dab7b"
            , SA.strokeWidth "2"
            , Attr.attribute "vector-effect" "non-scaling-stroke"
            ]
            []
        ]


{-| Curseur du slider : trait vertical plein largeur + pastille sur le profil. -}
viewCursor : Maybe Int -> (Int -> Maybe Float) -> (Int -> Float) -> (Float -> Float) -> List (Html Msg)
viewCursor activeIdx elevationAt xPct yPct =
    case activeIdx of
        Just i ->
            [ div
                [ class "elevation-band-cursor"
                , style "left" (String.fromFloat (xPct i) ++ "%")
                ]
                (case elevationAt i of
                    Just e ->
                        [ div
                            [ class "elevation-band-dot"
                            , style "top" (String.fromFloat (yPct e) ++ "%")
                            ]
                            []
                        ]

                    Nothing ->
                        []
                )
            ]

        Nothing ->
            []


{-| Écho du survol du tracé sur la carte : trait doré pointillé. -}
viewMapHoverCursor : Model -> Int -> (Int -> Float) -> List (Html Msg)
viewMapHoverCursor model lastIdx xPct =
    case model.mapRouteHoverIndex of
        Just i ->
            if i >= 0 && i <= lastIdx then
                [ div
                    [ class "elevation-band-map-cursor"
                    , style "left" (String.fromFloat (xPct i) ++ "%")
                    ]
                    []
                ]

            else
                []

        Nothing ->
            []


targetInt : Decode.Decoder Int
targetInt =
    targetValue
        |> Decode.andThen
            (\raw ->
                case String.toInt raw of
                    Just i ->
                        Decode.succeed i

                    Nothing ->
                        Decode.fail ("index de profil illisible : " ++ raw)
            )


{-| Comble les altitudes manquantes en prolongeant la dernière connue.

`List.filterMap identity` décalerait les index dès qu'un point n'a pas
d'altitude, et le marqueur tomberait à côté sur la carte.

-}
fillGaps : List (Maybe Float) -> List Float
fillGaps xs =
    let
        firstKnown =
            xs |> List.filterMap identity |> List.head |> Maybe.withDefault 0
    in
    xs
        |> List.foldl
            (\maybeE ( previous, acc ) ->
                let
                    e =
                        Maybe.withDefault previous maybeE
                in
                ( e, e :: acc )
            )
            ( firstKnown, [] )
        |> Tuple.second
        |> List.reverse


formatKm : Float -> String
formatKm km =
    let
        tenths =
            round (km * 10)
    in
    String.fromInt (tenths // 10) ++ "," ++ String.fromInt (modBy 10 tenths)
