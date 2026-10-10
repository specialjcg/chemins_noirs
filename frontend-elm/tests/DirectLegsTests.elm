module DirectLegsTests exposing (suite)

{-| Numérotation des tronçons directs quand on pose ou retire des points.
-}

import DirectLegs
import Expect
import Set
import Test exposing (..)
import Types exposing (Coordinate, RouteResponse)


suite : Test
suite =
    describe "DirectLegs"
        [ describe "legCount"
            [ test "aller simple : un tronçon de moins que de points" <|
                \_ -> DirectLegs.legCount 4 False |> Expect.equal 3
            , test "boucle : le retour au départ en ajoute un" <|
                \_ -> DirectLegs.legCount 4 True |> Expect.equal 4
            , test "un point seul n'a aucun tronçon, même en boucle" <|
                \_ -> DirectLegs.legCount 1 True |> Expect.equal 0
            ]
        , describe "afterInsert"
            [ test "les deux moitiés d'un tronçon direct restent directes" <|
                \_ ->
                    DirectLegs.afterInsert 2 (Set.fromList [ 1 ])
                        |> Expect.equal (Set.fromList [ 1, 2 ])
            , test "les tronçons suivants reculent d'un cran" <|
                \_ ->
                    DirectLegs.afterInsert 1 (Set.fromList [ 2 ])
                        |> Expect.equal (Set.fromList [ 3 ])
            , test "ajouter en fin de tracé ne touche à rien" <|
                \_ ->
                    DirectLegs.afterInsert 3 (Set.fromList [ 0, 1 ])
                        |> Expect.equal (Set.fromList [ 0, 1 ])
            ]
        , describe "afterRemove"
            [ test "deux tronçons directs fondus restent directs" <|
                \_ ->
                    DirectLegs.afterRemove 2 (Set.fromList [ 1, 2, 3 ])
                        |> Expect.equal (Set.fromList [ 1, 2 ])
            , test "un direct fondu avec un chemin redevient chemin" <|
                \_ ->
                    DirectLegs.afterRemove 2 (Set.fromList [ 1 ])
                        |> Expect.equal Set.empty
            , test "retirer le départ supprime son tronçon et décale les autres" <|
                \_ ->
                    DirectLegs.afterRemove 0 (Set.fromList [ 0, 2 ])
                        |> Expect.equal (Set.fromList [ 1 ])
            , test "retirer l'arrivée supprime le dernier tronçon" <|
                \_ ->
                    DirectLegs.afterRemove 3 (Set.fromList [ 0, 2 ])
                        |> Expect.equal (Set.fromList [ 0 ])
            ]
        , describe "lines"
            [ test "découpe le tracé sur les étapes des tronçons directs" <|
                \_ ->
                    DirectLegs.lines (Set.fromList [ 1 ]) (route (Just [ ( 0, 2 ), ( 2, 4 ) ]))
                        |> List.map (List.map .lon)
                        |> Expect.equal [ [ 2, 3, 4 ] ]
            , test "sans étapes, un seul tronçon direct est tout le tracé" <|
                \_ ->
                    DirectLegs.lines (Set.fromList [ 0 ]) (route Nothing)
                        |> List.map List.length
                        |> Expect.equal [ 5 ]
            ]
        , test "flags donne un booléen par tronçon, au-delà de l'ensemble aussi" <|
            \_ ->
                DirectLegs.flags 3 (Set.fromList [ 1, 7 ])
                    |> Expect.equal [ False, True, False ]
        ]


route : Maybe (List ( Int, Int )) -> RouteResponse
route bounds =
    { path = List.map (\i -> Coordinate 47 (toFloat i)) (List.range 0 4)
    , distanceKm = 0
    , gpxBase64 = ""
    , metadata = Nothing
    , elevationProfile = Nothing
    , snappedWaypoints = Nothing
    , estimatedTimeMinutes = Nothing
    , difficulty = Nothing
    , surfaceBreakdown = Nothing
    , segments =
        Maybe.map
            (List.map
                (\( from, to ) ->
                    { fromIndex = from, toIndex = to, distanceKm = 0, ascentM = 0, descentM = 0, avgSlopePct = 0 }
                )
            )
            bounds
    , pointSurfaces = Nothing
    , obstacles = []
    }
