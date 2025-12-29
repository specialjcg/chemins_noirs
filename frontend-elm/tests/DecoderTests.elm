module DecoderTests exposing (..)

{-| Tests TDD pour les décodeurs JSON.
Approche RED → GREEN → REFACTOR
-}

import Decoders exposing (..)
import Expect
import Json.Decode as Decode
import Test exposing (..)
import Types exposing (..)


suite : Test
suite =
    describe "Decoders"
        [ describe "decodeCoordinate"
            [ test "décode une coordonnée valide" <|
                \_ ->
                    let
                        json =
                            """{"lat": 45.9305, "lon": 4.5776}"""

                        result =
                            Decode.decodeString decodeCoordinate json
                    in
                    case result of
                        Ok coord ->
                            Expect.all
                                [ \c -> Expect.within (Expect.Absolute 0.0001) 45.9305 c.lat
                                , \c -> Expect.within (Expect.Absolute 0.0001) 4.5776 c.lon
                                ]
                                coord

                        Err _ ->
                            Expect.fail "Décodage échoué"
            , test "échoue sur JSON invalide" <|
                \_ ->
                    let
                        json =
                            """{"lat": "invalid", "lon": 4.5776}"""

                        result =
                            Decode.decodeString decodeCoordinate json
                    in
                    case result of
                        Ok _ ->
                            Expect.fail "Devrait échouer sur latitude invalide"

                        Err _ ->
                            Expect.pass
            ]
        , describe "decodeRouteBounds"
            [ test "décode des bounds valides" <|
                \_ ->
                    let
                        json =
                            """
                            {
                                "min_lat": 45.9,
                                "max_lat": 46.0,
                                "min_lon": 4.5,
                                "max_lon": 4.6
                            }
                            """

                        result =
                            Decode.decodeString decodeRouteBounds json
                    in
                    case result of
                        Ok bounds ->
                            Expect.all
                                [ \b -> Expect.within (Expect.Absolute 0.01) 45.9 b.minLat
                                , \b -> Expect.within (Expect.Absolute 0.01) 46.0 b.maxLat
                                , \b -> Expect.within (Expect.Absolute 0.01) 4.5 b.minLon
                                , \b -> Expect.within (Expect.Absolute 0.01) 4.6 b.maxLon
                                ]
                                bounds

                        Err _ ->
                            Expect.fail "Décodage échoué"
            ]
        , describe "decodeElevationProfile"
            [ test "décode un profil d'élévation avec toutes les valeurs" <|
                \_ ->
                    let
                        json =
                            """
                            {
                                "elevations": [100.5, 120.3, null, 150.0],
                                "min_elevation": 100.5,
                                "max_elevation": 150.0,
                                "total_ascent": 49.5,
                                "total_descent": 0.0
                            }
                            """

                        result =
                            Decode.decodeString decodeElevationProfile json
                    in
                    case result of
                        Ok profile ->
                            Expect.all
                                [ \p -> Expect.equal 4 (List.length p.elevations)
                                , \p -> Expect.equal (Just 100.5) p.minElevation
                                , \p -> Expect.equal (Just 150.0) p.maxElevation
                                , \p -> Expect.within (Expect.Absolute 0.1) 49.5 p.totalAscent
                                , \p -> Expect.within (Expect.Absolute 0.1) 0.0 p.totalDescent
                                ]
                                profile

                        Err _ ->
                            Expect.fail "Décodage échoué"
            , test "accepte des champs optionnels null" <|
                \_ ->
                    let
                        json =
                            """
                            {
                                "elevations": [],
                                "total_ascent": 0.0,
                                "total_descent": 0.0
                            }
                            """

                        result =
                            Decode.decodeString decodeElevationProfile json
                    in
                    case result of
                        Ok profile ->
                            Expect.all
                                [ \p -> Expect.equal Nothing p.minElevation
                                , \p -> Expect.equal Nothing p.maxElevation
                                ]
                                profile

                        Err _ ->
                            Expect.fail "Décodage échoué"
            ]
        , describe "decodeRouteResponse"
            [ test "décode une réponse complète" <|
                \_ ->
                    let
                        json =
                            """
                            {
                                "path": [
                                    {"lat": 45.9305, "lon": 4.5776},
                                    {"lat": 45.9320, "lon": 4.5780}
                                ],
                                "distance_km": 1.5,
                                "gpx_base64": "base64data",
                                "metadata": {
                                    "point_count": 2,
                                    "bounds": {
                                        "min_lat": 45.9305,
                                        "max_lat": 45.9320,
                                        "min_lon": 4.5776,
                                        "max_lon": 4.5780
                                    },
                                    "start": {"lat": 45.9305, "lon": 4.5776},
                                    "end": {"lat": 45.9320, "lon": 4.5780}
                                },
                                "elevation_profile": {
                                    "elevations": [100.0, 105.0],
                                    "min_elevation": 100.0,
                                    "max_elevation": 105.0,
                                    "total_ascent": 5.0,
                                    "total_descent": 0.0
                                }
                            }
                            """

                        result =
                            Decode.decodeString decodeRouteResponse json
                    in
                    case result of
                        Ok route ->
                            Expect.all
                                [ \r -> Expect.equal 2 (List.length r.path)
                                , \r -> Expect.within (Expect.Absolute 0.1) 1.5 r.distanceKm
                                , \r -> Expect.equal "base64data" r.gpxBase64
                                , \r ->
                                    case r.metadata of
                                        Just meta ->
                                            Expect.equal 2 meta.pointCount

                                        Nothing ->
                                            Expect.fail "Metadata devrait être présent"
                                ]
                                route

                        Err err ->
                            Expect.fail ("Décodage échoué: " ++ Decode.errorToString err)
            , test "décode une réponse minimale sans metadata ni elevation" <|
                \_ ->
                    let
                        json =
                            """
                            {
                                "path": [{"lat": 45.9305, "lon": 4.5776}],
                                "distance_km": 0.5,
                                "gpx_base64": "data"
                            }
                            """

                        result =
                            Decode.decodeString decodeRouteResponse json
                    in
                    case result of
                        Ok route ->
                            Expect.all
                                [ \r -> Expect.equal 1 (List.length r.path)
                                , \r -> Expect.equal Nothing r.metadata
                                , \r -> Expect.equal Nothing r.elevationProfile
                                ]
                                route

                        Err _ ->
                            Expect.fail "Décodage échoué"
            ]
        , describe "decodeLoopRouteResponse"
            [ test "décode une réponse avec plusieurs candidats" <|
                \_ ->
                    let
                        json =
                            """
                            {
                                "candidates": [
                                    {
                                        "route": {
                                            "path": [{"lat": 45.9305, "lon": 4.5776}],
                                            "distance_km": 15.2,
                                            "gpx_base64": "data1"
                                        },
                                        "distance_error_km": 0.2,
                                        "bearing_deg": 45.0
                                    },
                                    {
                                        "route": {
                                            "path": [{"lat": 45.9305, "lon": 4.5776}],
                                            "distance_km": 14.8,
                                            "gpx_base64": "data2"
                                        },
                                        "distance_error_km": -0.2,
                                        "bearing_deg": 135.0
                                    }
                                ],
                                "target_distance_km": 15.0,
                                "distance_tolerance_km": 2.5
                            }
                            """

                        result =
                            Decode.decodeString decodeLoopRouteResponse json
                    in
                    case result of
                        Ok response ->
                            Expect.all
                                [ \r -> Expect.equal 2 (List.length r.candidates)
                                , \r -> Expect.within (Expect.Absolute 0.1) 15.0 r.targetDistanceKm
                                , \r -> Expect.within (Expect.Absolute 0.1) 2.5 r.distanceToleranceKm
                                , \r ->
                                    case List.head r.candidates of
                                        Just candidate ->
                                            Expect.within (Expect.Absolute 0.1) 0.2 candidate.distanceErrorKm

                                        Nothing ->
                                            Expect.fail "Devrait avoir au moins un candidat"
                                ]
                                response

                        Err err ->
                            Expect.fail ("Décodage échoué: " ++ Decode.errorToString err)
            ]
        ]
