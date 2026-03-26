module View.World3D exposing (view)

{-| 3D World renderer for the orienteering game.
First-person view with simulated ground:
- Green grass ground
- Roads colored by type (asphalt=dark gray, chemin=golden, sentier=brown)
- Control point markers (poles with spheres)
- Scattered trees
-}

import Angle
import Camera3d
import Color
import Cylinder3d
import Direction3d
import Html exposing (Html)
import Length exposing (Meters)
import Pixels
import Point3d exposing (Point3d)
import Scene3d
import Scene3d.Material as Material
import Sphere3d
import Types exposing (..)


type WorldCoordinates
    = WorldCoordinates


view : GameState -> Int -> Int -> Html Msg
view gs width height =
    let
        -- Center the scene on the PLAYER
        center =
            gs.playerPosition

        -- Direction3d.xy uses 0°=+X=East, compass bearing uses 0°=North=+Y
        -- Conversion: elm_angle = 90 - compass_bearing
        playerPoint =
            latLonToPoint gs.playerPosition center

        elmAngle =
            Angle.degrees (90 - gs.playerBearing)

        lookDirection =
            Direction3d.xy elmAngle

        -- First-person camera: 1.7m eye height, looking 30m ahead
        eyePoint =
            Point3d.translateIn Direction3d.z (Length.meters 1.7) playerPoint

        focalPoint =
            playerPoint
                |> Point3d.translateIn lookDirection (Length.meters 30)
                |> Point3d.translateIn Direction3d.z (Length.meters 0.5)

        camera =
            Camera3d.lookAt
                { eyePoint = eyePoint
                , focalPoint = focalPoint
                , upDirection = Direction3d.z
                , fov = Camera3d.angle (Angle.degrees 75)
                , projection = Camera3d.Perspective
                }

        -- Green grass ground
        groundEntities =
            [ Scene3d.quad (Material.matte (Color.rgb255 75 135 45))
                (Point3d.meters -500 -500 0)
                (Point3d.meters 500 -500 0)
                (Point3d.meters 500 500 0)
                (Point3d.meters -500 500 0)
            ]

        -- Vegetation zones (colored ground patches)
        vegetationEntities =
            List.concatMap (vegetationToEntity center) gs.vegetation

        -- IGN Buildings (3D extruded blocks)
        buildingEntities =
            List.concatMap (buildingToEntity center) gs.ign_buildings

        -- Roads colored by nature
        roadEntities =
            List.concatMap (styledRoadToEntities center) gs.roads

        -- Control point markers
        cpEntities =
            List.concatMap (controlPointToEntity center) gs.controlPoints

        -- Trees (only if no real vegetation data)
        treeEntities =
            if List.isEmpty gs.vegetation then
                List.concatMap treeEntity (scatteredTrees 80)

            else
                []

        -- Sky
        skyColor =
            Color.rgb255 135 206 235
    in
    Scene3d.sunny
        { camera = camera
        , clipDepth = Length.meters 0.1
        , dimensions = ( Pixels.int width, Pixels.int height )
        , background = Scene3d.backgroundColor skyColor
        , entities = groundEntities ++ vegetationEntities ++ buildingEntities ++ roadEntities ++ cpEntities ++ treeEntities
        , shadows = False
        , upDirection = Direction3d.z
        , sunlightDirection = Direction3d.negativeZ
        }



-- COORDINATE CONVERSION


latLonToPoint : Coordinate -> Coordinate -> Point3d Meters WorldCoordinates
latLonToPoint coord center =
    let
        earthRadius =
            6371000

        dLat =
            (coord.lat - center.lat) * pi / 180

        dLon =
            (coord.lon - center.lon) * pi / 180

        avgLat =
            center.lat * pi / 180

        x =
            dLon * earthRadius * cos avgLat

        y =
            dLat * earthRadius
    in
    Point3d.meters x y 0



-- STYLED ROADS


{-| Get color and half-width based on road nature from IGN BD TOPO.
-}
roadStyle : String -> { color : Color.Color, halfWidth : Float, z : Float }
roadStyle nature =
    if String.contains "1 chauss" nature || String.contains "2 chauss" nature || String.contains "Rond-point" nature then
        -- Paved road: dark asphalt
        { color = Color.rgb255 55 55 55, halfWidth = 2.0, z = 0.12 }

    else if String.contains "Chemin" nature then
        -- Dirt path: golden/sandy
        { color = Color.rgb255 194 160 80, halfWidth = 1.2, z = 0.10 }

    else if String.contains "Sentier" nature then
        -- Trail: light brown, narrow
        { color = Color.rgb255 165 125 75, halfWidth = 0.5, z = 0.08 }

    else if String.contains "cyclable" nature then
        -- Bike path: light gray
        { color = Color.rgb255 160 160 160, halfWidth = 1.0, z = 0.11 }

    else if String.contains "Escalier" nature then
        -- Stairs: gray
        { color = Color.rgb255 140 140 140, halfWidth = 0.8, z = 0.12 }

    else
        -- Default: brown path
        { color = Color.rgb255 150 110 60, halfWidth = 1.0, z = 0.10 }


styledRoadToEntities : Coordinate -> StyledRoad -> List (Scene3d.Entity WorldCoordinates)
styledRoadToEntities center road =
    let
        style =
            roadStyle road.nature

        points =
            List.map (\c -> latLonToPoint c center) road.coords

        pairs =
            List.map2 Tuple.pair points (List.drop 1 points)
    in
    List.filterMap
        (\( from, to ) ->
            let
                fx =
                    Length.inMeters (Point3d.xCoordinate from)

                fy =
                    Length.inMeters (Point3d.yCoordinate from)

                tx =
                    Length.inMeters (Point3d.xCoordinate to)

                ty =
                    Length.inMeters (Point3d.yCoordinate to)

                dx =
                    tx - fx

                dy =
                    ty - fy

                segLen =
                    sqrt (dx * dx + dy * dy)
            in
            if segLen < 0.1 then
                Nothing

            else
                let
                    nx =
                        -dy / segLen * style.halfWidth

                    ny =
                        dx / segLen * style.halfWidth
                in
                Just
                    (Scene3d.quad (Material.matte style.color)
                        (Point3d.meters (fx + nx) (fy + ny) style.z)
                        (Point3d.meters (fx - nx) (fy - ny) style.z)
                        (Point3d.meters (tx - nx) (ty - ny) style.z)
                        (Point3d.meters (tx + nx) (ty + ny) style.z)
                    )
        )
        pairs



-- VEGETATION ZONES (colored ground polygons)


vegetationColor : String -> Color.Color
vegetationColor nature =
    if String.contains "feuillus" nature then
        Color.rgb255 30 100 30

    else if String.contains "conif" nature then
        Color.rgb255 20 80 40

    else if String.contains "mixte" nature then
        Color.rgb255 25 90 35

    else if String.contains "Bois" nature then
        Color.rgb255 35 105 35

    else if String.contains "Vigne" nature then
        Color.rgb255 120 90 160

    else if String.contains "Verger" nature then
        Color.rgb255 100 150 60

    else if String.contains "Lande" nature then
        Color.rgb255 140 160 80

    else if String.contains "Haie" nature then
        Color.rgb255 40 110 40

    else
        Color.rgb255 60 120 50


{-| Render a vegetation zone as a flat colored polygon on the ground.
Uses triangle fan from centroid for simple polygon filling.
-}
vegetationToEntity : Coordinate -> VegetationZone -> List (Scene3d.Entity WorldCoordinates)
vegetationToEntity center zone =
    let
        color =
            vegetationColor zone.nature

        points =
            List.map (\c -> latLonToPoint c center) zone.coords

        z =
            0.05

        -- Use triangle fan from centroid
        xs =
            List.map (\p -> Length.inMeters (Point3d.xCoordinate p)) points

        ys =
            List.map (\p -> Length.inMeters (Point3d.yCoordinate p)) points

        n =
            toFloat (List.length xs)

        cx =
            List.sum xs / max 1 n

        cy =
            List.sum ys / max 1 n

        centroid =
            Point3d.meters cx cy z

        meterPoints =
            List.map2 (\x y -> ( x, y )) xs ys

        pairs =
            List.map2 Tuple.pair meterPoints (List.drop 1 meterPoints)
    in
    List.map
        (\( ( x1, y1 ), ( x2, y2 ) ) ->
            Scene3d.quad (Material.matte color)
                centroid
                (Point3d.meters x1 y1 z)
                (Point3d.meters x2 y2 z)
                centroid
        )
        pairs



-- IGN BUILDINGS (3D extruded blocks)


buildingColor : String -> Color.Color
buildingColor nature =
    if String.contains "Industriel" nature || String.contains "agricole" nature then
        Color.rgb255 160 150 140

    else
        Color.rgb255 200 185 170


{-| Render a building as extruded walls + roof.
Uses pairs of consecutive polygon points to create wall quads.
-}
buildingToEntity : Coordinate -> IgnBuilding -> List (Scene3d.Entity WorldCoordinates)
buildingToEntity center building =
    let
        color =
            buildingColor building.nature

        roofColor =
            Color.rgb255 140 60 50

        h =
            max 3.0 building.hauteur

        points =
            List.map (\c -> latLonToPoint c center) building.coords

        meterPoints =
            List.map (\p -> ( Length.inMeters (Point3d.xCoordinate p), Length.inMeters (Point3d.yCoordinate p) )) points

        pairs =
            List.map2 Tuple.pair meterPoints (List.drop 1 meterPoints)

        -- Walls
        walls =
            List.map
                (\( ( x1, y1 ), ( x2, y2 ) ) ->
                    Scene3d.quad (Material.matte color)
                        (Point3d.meters x1 y1 0)
                        (Point3d.meters x2 y2 0)
                        (Point3d.meters x2 y2 h)
                        (Point3d.meters x1 y1 h)
                )
                pairs

        -- Roof (triangle fan from centroid)
        n =
            toFloat (List.length meterPoints)

        cx =
            List.sum (List.map Tuple.first meterPoints) / max 1 n

        cy =
            List.sum (List.map Tuple.second meterPoints) / max 1 n

        roofCenter =
            Point3d.meters cx cy h

        roof =
            List.map
                (\( ( x1, y1 ), ( x2, y2 ) ) ->
                    Scene3d.quad (Material.matte roofColor)
                        roofCenter
                        (Point3d.meters x1 y1 h)
                        (Point3d.meters x2 y2 h)
                        roofCenter
                )
                pairs
    in
    walls ++ roof



-- CONTROL POINTS


controlPointToEntity : Coordinate -> ControlPoint -> List (Scene3d.Entity WorldCoordinates)
controlPointToEntity center cp =
    let
        pos =
            latLonToPoint cp.position center

        x =
            Length.inMeters (Point3d.xCoordinate pos)

        y =
            Length.inMeters (Point3d.yCoordinate pos)

        color =
            if cp.found then
                Color.green

            else
                Color.orange
    in
    [ -- Pole
      Scene3d.cylinder (Material.matte Color.white)
        (Cylinder3d.startingAt
            (Point3d.meters x y 0)
            Direction3d.z
            { radius = Length.meters 0.05
            , length = Length.meters 2.0
            }
        )
    , -- Marker sphere
      Scene3d.sphere (Material.matte color)
        (Sphere3d.atPoint
            (Point3d.meters x y 2.2)
            (Length.meters 0.3)
        )
    ]



-- TREES


scatteredTrees : Int -> List { x : Float, y : Float, scale : Float }
scatteredTrees count =
    List.indexedMap
        (\i _ ->
            let
                seed =
                    toFloat (i + 500)

                x =
                    sin (seed * 137.508) * 350

                y =
                    cos (seed * 137.508 + 0.5) * 350

                s =
                    0.5 + sin (seed * 42.0) * 0.25
            in
            { x = x, y = y, scale = s }
        )
        (List.repeat count ())


treeEntity : { x : Float, y : Float, scale : Float } -> List (Scene3d.Entity WorldCoordinates)
treeEntity tree =
    [ Scene3d.cylinder (Material.matte (Color.rgb255 92 64 51))
        (Cylinder3d.startingAt
            (Point3d.meters tree.x tree.y 0)
            Direction3d.z
            { radius = Length.meters (0.1 * tree.scale)
            , length = Length.meters (3 * tree.scale)
            }
        )
    , Scene3d.sphere (Material.matte Color.darkGreen)
        (Sphere3d.atPoint
            (Point3d.meters tree.x tree.y (4 * tree.scale))
            (Length.meters (1.5 * tree.scale))
        )
    ]
