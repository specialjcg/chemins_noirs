module EncoderTests exposing (..)

{-| Tests des encodeurs JSON des requêtes de tracé.
-}

import Encoders exposing (..)
import Expect
import Json.Decode as Decode
import Json.Encode as Encode
import Test exposing (..)


densityOf : Encode.Value -> Result Decode.Error Float
densityOf =
    Decode.decodeValue (Decode.field "w_density" Decode.float)


suite : Test
suite =
    describe "Encoders"
        [ test "une requête multi-points transmet le poids de densité" <|
            \_ ->
                { waypoints = []
                , closeLoop = False
                , wPop = 1.0
                , wPaved = 1.0
                , wForest = 0.0
                , wDensity = 3.0
                , directLegs = []
                }
                    |> encodeMultiPointRouteRequest
                    |> densityOf
                    |> Expect.equal (Ok 3.0)
        , test "une requête simple transmet le poids de densité" <|
            \_ ->
                { start = { lat = 45.0, lon = 5.0 }
                , end = { lat = 45.1, lon = 5.1 }
                , wPop = 1.0
                , wPaved = 1.0
                , wForest = 0.0
                , wDensity = 2.0
                }
                    |> encodeRouteRequest
                    |> densityOf
                    |> Expect.equal (Ok 2.0)
        , test "une boucle transmet le poids de densité" <|
            \_ ->
                { start = { lat = 45.0, lon = 5.0 }
                , targetDistanceKm = 20.0
                , distanceToleranceKm = 2.0
                , candidateCount = 6
                , wPop = 1.0
                , wPaved = 1.0
                , wForest = 0.0
                , wDensity = 1.5
                , maxTotalAscent = Nothing
                , minTotalAscent = Nothing
                }
                    |> encodeLoopRouteRequest
                    |> densityOf
                    |> Expect.equal (Ok 1.5)
        ]
