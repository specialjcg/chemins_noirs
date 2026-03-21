module View.Game exposing (view)

import Html exposing (..)
import Html.Attributes exposing (..)
import Html.Events exposing (onClick)
import Types exposing (..)


view : Model -> GameState -> Html Msg
view model gs =
    div [ class "game-hud" ]
        [ compass gs.playerBearing gs.targetBearing
        , timer gs.elapsedMs
        , controlPointList gs
        , nextBaliseHint gs
        , case gs.gameStatus of
            GameSetup ->
                div [ class "game-start-overlay" ]
                    [ h2 [] [ text "Course d'Orientation" ]
                    , p [] [ text (String.fromInt (List.length gs.controlPoints) ++ " balises a trouver") ]
                    , p [ class "game-rules" ]
                        [ text "Naviguez a la boussole et a la carte. "
                        , text "Les balises n'apparaissent qu'a 10m. "
                        , text "Ouvrez la carte topo pour voir les balises (sans votre position)."
                        ]
                    , button [ class "game-btn start-btn", onClick StartGame ] [ text "Lancer la course" ]
                    , button [ class "game-btn back-btn", onClick ExitOrienteeringMode ] [ text "Retour" ]
                    ]

            GameRunning ->
                div [ class "game-controls" ]
                    [ div [ class "game-status" ]
                        [ text
                            (if model.pending then
                                "En route..."

                             else
                                "Cliquez sur un chemin — Balise "
                                    ++ String.fromInt (List.length (List.filter .found gs.controlPoints) + 1)
                                    ++ "/"
                                    ++ String.fromInt (List.length gs.controlPoints)
                            )
                        ]
                    , button [ class "game-btn map-btn", onClick ToggleTopoOverlay ]
                        [ text
                            (if gs.showTopoOverlay then
                                "Fermer carte"

                             else
                                "Carte topo"
                            )
                        ]
                    , button [ class "game-btn abandon-btn", onClick ExitOrienteeringMode ] [ text "Stop" ]
                    ]

            GameFinished ->
                div [ class "game-finish-overlay" ]
                    [ h2 [] [ text "Course terminee !" ]
                    , p [ class "final-time" ] [ text (formatTime gs.elapsedMs) ]
                    , p [] [ text (String.fromInt (List.length gs.controlPoints) ++ " balises trouvees") ]
                    , button [ class "game-btn back-btn", onClick ExitOrienteeringMode ] [ text "Retour a la carte" ]
                    ]
        , if gs.foundFlash then
            div [ class "game-found-flash" ] [ text "BALISE TROUVEE !" ]

          else
            text ""
        ]


compass : Float -> Maybe Float -> Html Msg
compass bearing targetBearing =
    let
        rotation =
            "rotate(" ++ String.fromFloat (-bearing) ++ "deg)"

        deg =
            round bearing |> modBy 360

        targetMarker =
            case targetBearing of
                Just tb ->
                    let
                        targetRotation =
                            "rotate(" ++ String.fromFloat tb ++ "deg)"
                    in
                    div
                        [ class "compass-target-marker"
                        , style "transform" targetRotation
                        ]
                        []

                Nothing ->
                    text ""
    in
    div [ class "game-compass" ]
        [ div [ class "compass-body" ]
            [ div [ class "compass-outer-ring" ]
                [ span [ class "compass-deg compass-deg-0" ] [ text "0" ]
                , span [ class "compass-deg compass-deg-90" ] [ text "90" ]
                , span [ class "compass-deg compass-deg-180" ] [ text "180" ]
                , span [ class "compass-deg compass-deg-270" ] [ text "270" ]
                ]
            , div
                [ class "compass-dial"
                , style "transform" rotation
                ]
                [ span [ class "compass-cardinal compass-n" ] [ text "N" ]
                , span [ class "compass-cardinal compass-e" ] [ text "E" ]
                , span [ class "compass-cardinal compass-s" ] [ text "S" ]
                , span [ class "compass-cardinal compass-w" ] [ text "W" ]
                , div [ class "compass-needle-n" ] []
                , div [ class "compass-needle-s" ] []
                , div [ class "compass-pivot" ] []
                ]
            , targetMarker
            , div [ class "compass-lubber" ] []
            ]
        , div [ class "compass-readout" ]
            [ text (String.fromInt deg ++ "deg")
            , case targetBearing of
                Just tb ->
                    span [ class "compass-target-text" ]
                        [ text (" -> " ++ String.fromInt (round tb) ++ "deg") ]

                Nothing ->
                    text ""
            ]
        , div [ class "compass-adjust" ]
            [ button [ class "compass-adj-btn", onClick (SetTargetBearing (toFloat ((deg + 350) |> modBy 360))) ] [ text "-10" ]
            , button [ class "compass-adj-btn", onClick (SetTargetBearing (toFloat deg)) ] [ text "Cap" ]
            , button [ class "compass-adj-btn", onClick (SetTargetBearing (toFloat ((deg + 10) |> modBy 360))) ] [ text "+10" ]
            , case targetBearing of
                Just _ ->
                    button [ class "compass-adj-btn compass-clear", onClick ClearTargetBearing ] [ text "X" ]

                Nothing ->
                    text ""
            ]
        ]


timer : Int -> Html Msg
timer ms =
    div [ class "game-timer" ]
        [ text (formatTime ms) ]


formatTime : Int -> String
formatTime ms =
    let
        totalSec =
            ms // 1000

        minutes =
            totalSec // 60

        seconds =
            remainderBy 60 totalSec

        pad n =
            if n < 10 then
                "0" ++ String.fromInt n

            else
                String.fromInt n
    in
    pad minutes ++ ":" ++ pad seconds


controlPointList : GameState -> Html Msg
controlPointList gs =
    div [ class "game-cp-list" ]
        (List.indexedMap
            (\i cp ->
                div
                    [ class
                        ("game-cp-item"
                            ++ (if cp.found then
                                    " found"

                                else if i == gs.currentPointIndex then
                                    " current"

                                else
                                    ""
                               )
                        )
                    ]
                    [ span [ class "cp-number" ] [ text cp.label ]
                    , if cp.found then
                        span [ class "cp-status" ] [ text "OK" ]

                      else if i == gs.currentPointIndex then
                        span [ class "cp-status" ] [ text ">>>" ]

                      else
                        text ""
                    ]
            )
            gs.controlPoints
        )


speedLabel : Float -> String
speedLabel s =
    if s == 0.5 then
        "0.5"

    else if s == 1.0 then
        "1"

    else if s == 2.0 then
        "2"

    else if s == 4.0 then
        "4"

    else
        String.fromFloat s


nextBaliseHint : GameState -> Html Msg
nextBaliseHint gs =
    case gs.nearestCpDistance of
        Just dist ->
            if dist < 50 then
                div [ class "game-proximity" ]
                    [ text ("Balise a " ++ String.fromInt (round dist) ++ "m") ]

            else
                text ""

        Nothing ->
            text ""
