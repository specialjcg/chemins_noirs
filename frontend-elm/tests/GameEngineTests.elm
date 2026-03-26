module GameEngineTests exposing (..)

{-| Tests pour le moteur de jeu d'orientation.
Couvre : projection sur segments, conversion route->segments,
bearing, avance le long des routes, et avance le long d'un segment.
-}

import Expect
import GameEngine
import Test exposing (..)
import Types exposing (Coordinate, haversineMeters)


suite : Test
suite =
    describe "GameEngine"
        [ roadToSegmentsTests
        , projectOntoSegmentTests
        , bearingBetweenTests
        , advanceAlongSegmentTests
        , advanceAlongRoadTests
        ]



-- ROAD TO SEGMENTS


roadToSegmentsTests : Test
roadToSegmentsTests =
    describe "roadToSegments"
        [ test "liste vide donne aucun segment" <|
            \_ ->
                GameEngine.roadToSegments []
                    |> List.length
                    |> Expect.equal 0
        , test "un seul point donne aucun segment" <|
            \_ ->
                GameEngine.roadToSegments [ coord 45.0 4.0 ]
                    |> List.length
                    |> Expect.equal 0
        , test "deux points donnent un segment" <|
            \_ ->
                let
                    a =
                        coord 45.0 4.0

                    b =
                        coord 45.001 4.001

                    segments =
                        GameEngine.roadToSegments [ a, b ]
                in
                Expect.equal [ ( a, b ) ] segments
        , test "trois points donnent deux segments consecutifs" <|
            \_ ->
                let
                    a =
                        coord 45.0 4.0

                    b =
                        coord 45.001 4.001

                    c =
                        coord 45.002 4.002

                    segments =
                        GameEngine.roadToSegments [ a, b, c ]
                in
                Expect.equal [ ( a, b ), ( b, c ) ] segments
        , test "cinq points donnent quatre segments" <|
            \_ ->
                let
                    pts =
                        List.map (\i -> coord (45.0 + toFloat i * 0.001) (4.0 + toFloat i * 0.001)) (List.range 0 4)
                in
                GameEngine.roadToSegments pts
                    |> List.length
                    |> Expect.equal 4
        ]



-- PROJECT ONTO SEGMENT


projectOntoSegmentTests : Test
projectOntoSegmentTests =
    describe "projectOntoSegment"
        [ test "projection sur le milieu d'un segment horizontal" <|
            \_ ->
                let
                    a =
                        coord 45.93 4.57

                    b =
                        coord 45.93 4.58

                    p =
                        coord 45.931 4.575

                    proj =
                        GameEngine.projectOntoSegment p a b
                in
                Expect.all
                    [ \c -> Expect.within (Expect.Absolute 0.0001) 45.93 c.lat
                    , \c -> Expect.within (Expect.Absolute 0.001) 4.575 c.lon
                    ]
                    proj
        , test "projection clampee au debut du segment" <|
            \_ ->
                let
                    a =
                        coord 45.93 4.57

                    b =
                        coord 45.93 4.58

                    p =
                        coord 45.931 4.56

                    proj =
                        GameEngine.projectOntoSegment p a b
                in
                Expect.all
                    [ \c -> Expect.within (Expect.Absolute 0.0001) a.lat c.lat
                    , \c -> Expect.within (Expect.Absolute 0.0001) a.lon c.lon
                    ]
                    proj
        , test "projection clampee a la fin du segment" <|
            \_ ->
                let
                    a =
                        coord 45.93 4.57

                    b =
                        coord 45.93 4.58

                    p =
                        coord 45.931 4.59

                    proj =
                        GameEngine.projectOntoSegment p a b
                in
                Expect.all
                    [ \c -> Expect.within (Expect.Absolute 0.0001) b.lat c.lat
                    , \c -> Expect.within (Expect.Absolute 0.0001) b.lon c.lon
                    ]
                    proj
        , test "projection sur un segment vertical" <|
            \_ ->
                let
                    a =
                        coord 45.93 4.575

                    b =
                        coord 45.94 4.575

                    p =
                        coord 45.935 4.577

                    proj =
                        GameEngine.projectOntoSegment p a b
                in
                Expect.all
                    [ \c -> Expect.within (Expect.Absolute 0.0001) 45.935 c.lat
                    , \c -> Expect.within (Expect.Absolute 0.001) 4.575 c.lon
                    ]
                    proj
        , test "projection sur un point degenere (A == B)" <|
            \_ ->
                let
                    a =
                        coord 45.93 4.575

                    p =
                        coord 45.931 4.576

                    proj =
                        GameEngine.projectOntoSegment p a a
                in
                Expect.all
                    [ \c -> Expect.within (Expect.Absolute 0.0001) a.lat c.lat
                    , \c -> Expect.within (Expect.Absolute 0.0001) a.lon c.lon
                    ]
                    proj
        , test "projectOntoSegmentT retourne t=0.5 au milieu" <|
            \_ ->
                let
                    a =
                        coord 45.93 4.57

                    b =
                        coord 45.93 4.58

                    p =
                        coord 45.931 4.575

                    ( _, t ) =
                        GameEngine.projectOntoSegmentT p a b
                in
                Expect.within (Expect.Absolute 0.01) 0.5 t
        , test "projectOntoSegmentT retourne t=0 au debut" <|
            \_ ->
                let
                    a =
                        coord 45.93 4.57

                    b =
                        coord 45.93 4.58

                    p =
                        coord 45.931 4.56

                    ( _, t ) =
                        GameEngine.projectOntoSegmentT p a b
                in
                Expect.within (Expect.Absolute 0.01) 0 t
        ]



-- BEARING BETWEEN


bearingBetweenTests : Test
bearingBetweenTests =
    describe "bearingBetween"
        [ test "plein nord = ~0 degres" <|
            \_ ->
                GameEngine.bearingBetween (coord 45.0 4.0) (coord 46.0 4.0)
                    |> Expect.within (Expect.Absolute 1) 0
        , test "plein est = ~90 degres" <|
            \_ ->
                GameEngine.bearingBetween (coord 45.0 4.0) (coord 45.0 5.0)
                    |> Expect.within (Expect.Absolute 1) 90
        , test "plein sud = ~180 degres" <|
            \_ ->
                GameEngine.bearingBetween (coord 46.0 4.0) (coord 45.0 4.0)
                    |> Expect.within (Expect.Absolute 1) 180
        , test "plein ouest = ~270 degres" <|
            \_ ->
                GameEngine.bearingBetween (coord 45.0 5.0) (coord 45.0 4.0)
                    |> Expect.within (Expect.Absolute 1) 270
        , test "nord-est = ~45 degres" <|
            \_ ->
                let
                    dlat =
                        0.01

                    dlon =
                        0.01 / cos (45.0 * pi / 180)
                in
                GameEngine.bearingBetween (coord 45.0 4.0) (coord (45.0 + dlat) (4.0 + dlon))
                    |> Expect.within (Expect.Absolute 2) 45
        ]



-- ADVANCE ALONG SEGMENT


advanceAlongSegmentTests : Test
advanceAlongSegmentTests =
    describe "advanceAlongSegment"
        [ test "joueur sur la route, avance le long du segment" <|
            \_ ->
                let
                    -- Joueur au milieu d'un segment nord-sud
                    pos =
                        coord 45.930 4.575

                    proj =
                        coord 45.930 4.575

                    forward =
                        coord 45.931 4.575

                    result =
                        GameEngine.advanceAlongSegment pos proj forward 5.0

                    dist =
                        haversineMeters pos result
                in
                Expect.all
                    [ \_ -> Expect.within (Expect.Absolute 1.0) 5.0 dist
                    , \_ -> Expect.greaterThan pos.lat result.lat
                    ]
                    ()
        , test "joueur loin de la route, se rapproche de la projection" <|
            \_ ->
                let
                    pos =
                        coord 45.930 4.576

                    proj =
                        coord 45.930 4.575

                    forward =
                        coord 45.931 4.575

                    result =
                        GameEngine.advanceAlongSegment pos proj forward 5.0

                    distToProj =
                        haversineMeters pos proj
                in
                -- La distance a la projection est ~77m, donc on ne fait que 5m vers elle
                Expect.all
                    [ \_ -> Expect.within (Expect.Absolute 1.0) 5.0 (haversineMeters pos result)
                    , \_ -> Expect.lessThan (haversineMeters pos proj) (haversineMeters result proj)
                    ]
                    ()
        , test "joueur proche de la route, snap puis avance" <|
            \_ ->
                let
                    -- Joueur a 2m de la route
                    pos =
                        coord 45.930 4.57502

                    proj =
                        coord 45.930 4.575

                    forward =
                        coord 45.931 4.575

                    result =
                        GameEngine.advanceAlongSegment pos proj forward 5.0
                in
                -- Doit snap a la route puis avancer vers le nord
                Expect.greaterThan pos.lat result.lat
        , test "joueur a la fin du segment, retourne le forward" <|
            \_ ->
                let
                    pos =
                        coord 45.9309 4.575

                    proj =
                        coord 45.9309 4.575

                    forward =
                        coord 45.931 4.575

                    distProjFwd =
                        haversineMeters proj forward

                    result =
                        GameEngine.advanceAlongSegment pos proj forward (distProjFwd + 10)
                in
                -- Demande plus que la distance restante, doit retourner forward
                Expect.all
                    [ \c -> Expect.within (Expect.Absolute 0.0001) forward.lat c.lat
                    , \c -> Expect.within (Expect.Absolute 0.0001) forward.lon c.lon
                    ]
                    result
        ]



-- ADVANCE ALONG ROAD


advanceAlongRoadTests : Test
advanceAlongRoadTests =
    describe "advanceAlongRoad"
        [ test "sans routes, reste sur place (pas de hors-piste)" <|
            \_ ->
                let
                    result =
                        GameEngine.advanceAlongRoad (coord 45.93 4.575) 0 5.0 []
                in
                Expect.within (Expect.Absolute 0.0001) 45.93 result.lat
        , test "joueur SUR un point de route, avance vers le point suivant" <|
            \_ ->
                let
                    road =
                        [ coord 45.929 4.575, coord 45.930 4.575, coord 45.931 4.575 ]

                    result =
                        GameEngine.advanceAlongRoad (coord 45.930 4.575) 0 10.0 [ road ]
                in
                -- Doit avancer au nord vers 45.931
                Expect.greaterThan 45.930 result.lat
        , test "pas d'oscillation - 10 clics avancent toujours" <|
            \_ ->
                let
                    road =
                        [ coord 45.929 4.575
                        , coord 45.9295 4.575
                        , coord 45.930 4.575
                        , coord 45.9305 4.575
                        , coord 45.931 4.575
                        , coord 45.9315 4.575
                        , coord 45.932 4.575
                        ]

                    -- Simuler 10 clics bearing nord
                    walk pos n =
                        if n <= 0 then
                            pos

                        else
                            walk (GameEngine.advanceAlongRoad pos 0 10.0 [ road ]) (n - 1)

                    start =
                        coord 45.930 4.575

                    final =
                        walk start 10
                in
                -- Apres 10 clics vers le nord, doit etre bien au nord du depart
                Expect.greaterThan (start.lat + 0.0003) final.lat
        , test "scenario reel - carrefour (45.929587, 4.576883) ne bloque pas" <|
            \_ ->
                let
                    -- Routes reelles du carrefour problematique
                    road1 =
                        [ coord 45.929918 4.577186, coord 45.930085 4.577359, coord 45.930388 4.577121, coord 45.929587 4.576883 ]

                    road2 =
                        [ coord 45.929587 4.576883, coord 45.930560 4.577018 ]

                    road3 =
                        [ coord 45.929587 4.576883, coord 45.929649 4.576474 ]

                    road4 =
                        [ coord 45.929587 4.576883, coord 45.929909 4.576041 ]

                    roads =
                        [ road1, road2, road3, road4 ]

                    -- Joueur au carrefour, bearing nord (6°)
                    pos =
                        coord 45.929587 4.576883

                    result =
                        GameEngine.advanceAlongRoad pos 6 10.0 roads
                in
                -- Doit bouger (pas rester sur place)
                Expect.greaterThan 1.0 (haversineMeters pos result)
        , test "scenario reel - joueur snappe sur (45.9305261, 4.577655) bearing 171" <|
            \_ ->
                let
                    -- Points reels autour de cette position
                    road =
                        [ coord 45.930678 4.577361, coord 45.930526 4.577655, coord 45.930512 4.577541, coord 45.930521 4.577368 ]

                    pos =
                        coord 45.930526 4.577655

                    result =
                        GameEngine.advanceAlongRoad pos 171 10.0 [ road ]
                in
                -- Doit bouger (pas rester au meme point)
                Expect.greaterThan 1.0 (haversineMeters pos result)
        , test "bearing lateral trouve quand meme un point" <|
            \_ ->
                let
                    road =
                        [ coord 45.929 4.575, coord 45.931 4.575 ]

                    -- Joueur sur la route, bearing EST (90) — perpendiculaire
                    result =
                        GameEngine.advanceAlongRoad (coord 45.930 4.575) 90 10.0 [ road ]
                in
                -- Doit quand meme bouger (score favorise les proches meme si angle grand)
                Expect.greaterThan 1.0 (haversineMeters (coord 45.930 4.575) result)
        ]



-- HELPERS


coord : Float -> Float -> Coordinate
coord lat lon =
    { lat = lat, lon = lon }
