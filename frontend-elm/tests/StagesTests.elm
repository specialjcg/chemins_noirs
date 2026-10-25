module StagesTests exposing (suite)

{-| Découpage en étapes : un GPX par étape, raccordés bout à bout. -}

import Expect
import Stages
import Test exposing (..)


{-| Points sur un méridien espacés d'un peu plus de 1 km (0,009° de latitude ≈ 1,0008 km). -}
meridian : Int -> List { lat : Float, lon : Float }
meridian count =
    List.range 0 (count - 1)
        |> List.map (\i -> { lat = 45 + toFloat i * 0.009, lon = 2 })


suite : Test
suite =
    describe "Stages"
        [ test "65 km en étapes de 30 km donne 3 étapes" <|
            \_ ->
                Stages.split 30 (meridian 66)
                    |> List.length
                    |> Expect.equal 3
        , test "la fin d'une étape est le départ de la suivante" <|
            \_ ->
                let
                    stages =
                        Stages.split 30 (meridian 66)
                in
                List.map2 (\a b -> a.last == b.first) stages (List.drop 1 stages)
                    |> Expect.equal [ True, True ]
        , test "la dernière étape finit au dernier point" <|
            \_ ->
                Stages.split 30 (meridian 66)
                    |> List.reverse
                    |> List.head
                    |> Maybe.map .last
                    |> Expect.equal (Just 65)
        , test "une coupe sur le dernier point ne crée pas d'étape vide" <|
            \_ ->
                Stages.split 30 (meridian 61)
                    |> List.length
                    |> Expect.equal 2
        , test "un tracé plus court qu'une étape tient en une étape" <|
            \_ ->
                Stages.split 30 (meridian 11)
                    |> List.map (\s -> ( s.first, s.last ))
                    |> Expect.equal [ ( 0, 10 ) ]
        , test "une longueur nulle ne coupe pas" <|
            \_ ->
                Stages.split 0 (meridian 66)
                    |> List.length
                    |> Expect.equal 1
        , test "un point seul n'a pas d'étape" <|
            \_ ->
                Stages.split 30 (meridian 1)
                    |> Expect.equal []
        , test "une étape enregistrée recalcule ses dénivelés" <|
            \_ ->
                let
                    full =
                        { path = meridian 5
                        , distanceKm = 4
                        , gpxBase64 = ""
                        , metadata = Nothing
                        , elevationProfile =
                            Just
                                { elevations = [ Just 100, Just 150, Nothing, Just 120, Just 200 ]
                                , minElevation = Just 100
                                , maxElevation = Just 200
                                , totalAscent = 130
                                , totalDescent = 30
                                }
                        , snappedWaypoints = Nothing
                        , estimatedTimeMinutes = Nothing
                        , difficulty = Nothing
                        , surfaceBreakdown = Nothing
                        , segments = Nothing
                        , pointSurfaces = Just [ True, True, False, False, True ]
                        , pointIgn = Nothing
                        , obstacles = []
                        }

                    stage =
                        Stages.route { first = 1, last = 3, distanceKm = 2 } full
                in
                Expect.equal
                    ( ( List.length stage.path, stage.pointSurfaces )
                    , Maybe.map (\p -> ( p.totalAscent, p.totalDescent, p.minElevation )) stage.elevationProfile
                    )
                    ( ( 3, Just [ True, False, False ] ), Just ( 0, 30, Just 120 ) )
        , test "slice garde les deux bornes" <|
            \_ ->
                Stages.slice { first = 2, last = 4, distanceKm = 0 } (List.range 0 9)
                    |> Expect.equal [ 2, 3, 4 ]
        ]
