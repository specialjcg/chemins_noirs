module View.World3D exposing (view)

{-| 3D World renderer for the orienteering game.
First-person view with terrain relief from IGN elevation data.
- Terrain mesh from elevation grid
- Roads colored by type at real altitude
- Vegetation zones at real altitude
- 3D buildings at real altitude
-}

import Array
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
import Scene3d.Mesh as Mesh
import Sphere3d
import TriangularMesh
import Types exposing (..)
import Vector3d


type WorldCoordinates
    = WorldCoordinates


view : GameState -> Int -> Int -> Html Msg
view gs width height =
    let
        center =
            gs.playerPosition

        -- Elevation exaggeration factor (1.0 = real scale)
        elevScale =
            1.0

        baseAlt =
            case gs.elevationGrid of
                Just grid ->
                    grid.minAlt

                Nothing ->
                    0

        -- Camera direction: elm_angle = 90 - compass_bearing
        elmAngle =
            Angle.degrees (90 - gs.playerBearing)

        lookDirection =
            Direction3d.xy elmAngle

        -- Player altitude: sample terrain grid at player position
        playerAlt =
            (sampleElevation gs.elevationGrid center - baseAlt) * elevScale
                |> (\a -> if a < -50 || a > 200 then 0 else a)

        -- First-person camera at eye level
        playerPoint =
            Point3d.meters 0 0 playerAlt

        -- Eye at 1.7m above ground + 3m safety offset (terrain grid is coarse)
        eyePoint =
            Point3d.translateIn Direction3d.z (Length.meters 4.7) playerPoint

        -- Look at ground level 30m ahead (slightly below eye level to see the road)
        focalPoint =
            playerPoint
                |> Point3d.translateIn lookDirection (Length.meters 30)
                |> Point3d.translateIn Direction3d.z (Length.meters -0.5)

        camera =
            Camera3d.lookAt
                { eyePoint = eyePoint
                , focalPoint = focalPoint
                , upDirection = Direction3d.z
                , fov = Camera3d.angle (Angle.degrees 75)
                , projection = Camera3d.Perspective
                }

        -- Distance culling: only render objects within 200m of player
        maxDist =
            150

        nearbyVeg =
            List.filter (\z -> isNearby center maxDist z.coords) gs.vegetation

        nearbyBuildings =
            List.filter (\b -> isNearby center maxDist b.coords) gs.ign_buildings

        nearbyRoads =
            List.filter (\r -> isNearby center maxDist r.coords) gs.roads

        -- Terrain mesh or flat ground
        groundEntities =
            terrainEntities gs.elevationGrid center baseAlt elevScale

        -- Vegetation (nearby only)
        vegetationEntities =
            List.concatMap (vegetationToEntity center baseAlt elevScale gs.elevationGrid) nearbyVeg

        -- Buildings (nearby only)
        buildingEntities =
            List.concatMap (buildingToEntity center baseAlt elevScale gs.elevationGrid) nearbyBuildings

        -- Roads (nearby only)
        roadEntities =
            List.concatMap (styledRoadToEntities center baseAlt elevScale gs.elevationGrid) nearbyRoads

        -- Control points
        cpEntities =
            List.concatMap (controlPointToEntity center baseAlt elevScale gs.elevationGrid) gs.controlPoints

        skyColor =
            Color.rgb255 135 206 235
    in
    Scene3d.sunny
        { camera = camera
        , clipDepth = Length.meters 0.1
        , dimensions = ( Pixels.int width, Pixels.int height )
        , background = Scene3d.backgroundColor skyColor
        , entities = groundEntities ++ vegetationEntities ++ buildingEntities ++ roadEntities ++ cpEntities
        , shadows = False
        , upDirection = Direction3d.z
        , sunlightDirection = Direction3d.negativeZ
        }



-- DISTANCE CULLING


{-| Check if any point in a Coord3D list is within maxDistM meters of center.
Uses fast approximate distance (no haversine needed for culling).
-}
isNearby : Coordinate -> Float -> List Coord3D -> Bool
isNearby center maxDistM coords =
    let
        -- Approximate meters per degree at this latitude
        mPerDegLat =
            111000

        mPerDegLon =
            111000 * cos (center.lat * pi / 180)

        maxDistDegLat =
            maxDistM / mPerDegLat

        maxDistDegLon =
            maxDistM / mPerDegLon
    in
    List.any
        (\c ->
            abs (c.lat - center.lat) < maxDistDegLat && abs (c.lon - center.lon) < maxDistDegLon
        )
        coords



-- ELEVATION SAMPLING


{-| Sample elevation from the grid at a given coordinate.
Returns 0 if no grid available.
-}
sampleElevation : Maybe ElevationGrid -> Coordinate -> Float
sampleElevation maybeGrid pos =
    case maybeGrid of
        Nothing ->
            -- Return 0 here; caller should handle baseAlt offset
            0

        Just grid ->
            let
                -- Total grid span in degrees
                totalLatDeg =
                    grid.cellSizeM * toFloat (grid.rows - 1) / 111000.0

                totalLonDeg =
                    grid.cellSizeM * toFloat (grid.cols - 1) / (111000.0 * cos (pos.lat * pi / 180))

                -- Normalized position in grid (0-1)
                tLat =
                    if totalLatDeg < 0.00001 then 0.5
                    else (pos.lat - grid.originLat) / totalLatDeg

                tLon =
                    if totalLonDeg < 0.00001 then 0.5
                    else (pos.lon - grid.originLon) / totalLonDeg

                row =
                    clamp 0 (grid.rows - 1) (round (tLat * toFloat (grid.rows - 1)))

                col =
                    clamp 0 (grid.cols - 1) (round (tLon * toFloat (grid.cols - 1)))
            in
            grid.grid
                |> List.drop row
                |> List.head
                |> Maybe.andThen (\r -> List.drop col r |> List.head)
                |> Maybe.withDefault grid.minAlt



-- TERRAIN MESH


terrainEntities : Maybe ElevationGrid -> Coordinate -> Float -> Float -> List (Scene3d.Entity WorldCoordinates)
terrainEntities maybeGrid center baseAlt elevScale =
    case maybeGrid of
        Nothing ->
            -- Flat green ground fallback
            [ Scene3d.quad (Material.matte (Color.rgb255 75 135 45))
                (Point3d.meters -500 -500 0)
                (Point3d.meters 500 -500 0)
                (Point3d.meters 500 500 0)
                (Point3d.meters -500 500 0)
            ]

        Just grid ->
            let
                -- Grid cell size in meters (directly usable for 3D)
                cell =
                    grid.cellSizeM

                -- Origin of grid in 3D coordinates relative to player (center)
                earthRadius =
                    6371000

                avgLatRad =
                    center.lat * pi / 180

                originX =
                    (grid.originLon - center.lon) * pi / 180 * earthRadius * cos avgLatRad

                originY =
                    (grid.originLat - center.lat) * pi / 180 * earthRadius

                -- Convert grid to array for fast access
                gridArray =
                    Array.fromList (List.map Array.fromList grid.grid)

                getAlt row col =
                    gridArray
                        |> Array.get row
                        |> Maybe.andThen (Array.get col)
                        |> Maybe.withDefault grid.minAlt

                -- Generate terrain quads
                rowIndices =
                    List.range 0 (grid.rows - 2)

                colIndices =
                    List.range 0 (grid.cols - 2)

                quads =
                    List.concatMap
                        (\row ->
                            List.map
                                (\col ->
                                    let
                                        x0 =
                                            originX + toFloat col * cell

                                        y0 =
                                            originY + toFloat row * cell

                                        x1 =
                                            x0 + cell

                                        y1 =
                                            y0 + cell

                                        z00 =
                                            (getAlt row col - baseAlt) * elevScale

                                        z10 =
                                            (getAlt row (col + 1) - baseAlt) * elevScale

                                        z01 =
                                            (getAlt (row + 1) col - baseAlt) * elevScale

                                        z11 =
                                            (getAlt (row + 1) (col + 1) - baseAlt) * elevScale
                                    in
                                    Scene3d.quad (Material.matte (Color.rgb255 75 135 45))
                                        (Point3d.meters x0 y0 z00)
                                        (Point3d.meters x1 y0 z10)
                                        (Point3d.meters x1 y1 z11)
                                        (Point3d.meters x0 y1 z01)
                                )
                                colIndices
                        )
                        rowIndices

                -- Flat base ground at average terrain altitude
                avgAlt =
                    ((grid.minAlt + grid.maxAlt) / 2 - baseAlt) * elevScale

                baseGround =
                    Scene3d.quad (Material.matte (Color.rgb255 65 120 40))
                        (Point3d.meters -2000 -2000 avgAlt)
                        (Point3d.meters 2000 -2000 avgAlt)
                        (Point3d.meters 2000 2000 avgAlt)
                        (Point3d.meters -2000 2000 avgAlt)
            in
            baseGround :: quads



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
    in
    Point3d.meters (dLon * earthRadius * cos avgLat) (dLat * earthRadius) 0


{-| Convert a 3D coordinate to a scene point.
Ignores WFS altitude — uses terrain grid sampling instead for consistency.
-}
coord3dToPoint : Coord3D -> Coordinate -> Float -> Float -> Maybe ElevationGrid -> Float -> Point3d Meters WorldCoordinates
coord3dToPoint coord center baseAlt elevScale maybeGrid zOffset =
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

        -- Sample terrain altitude at this point (consistent with terrain mesh)
        terrainAlt =
            sampleElevation maybeGrid { lat = coord.lat, lon = coord.lon }

        z =
            (terrainAlt - baseAlt) * elevScale + zOffset
    in
    Point3d.meters x y z



-- STYLED ROADS


roadStyle : String -> { color : Color.Color, halfWidth : Float, zOffset : Float }
roadStyle nature =
    if String.contains "1 chauss" nature || String.contains "2 chauss" nature || String.contains "Rond-point" nature then
        { color = Color.rgb255 55 55 55, halfWidth = 2.0, zOffset = 0.15 }

    else if String.contains "Chemin" nature then
        { color = Color.rgb255 194 160 80, halfWidth = 1.2, zOffset = 0.12 }

    else if String.contains "Sentier" nature then
        { color = Color.rgb255 165 125 75, halfWidth = 0.5, zOffset = 0.10 }

    else if String.contains "cyclable" nature then
        { color = Color.rgb255 160 160 160, halfWidth = 1.0, zOffset = 0.13 }

    else if String.contains "Escalier" nature then
        { color = Color.rgb255 140 140 140, halfWidth = 0.8, zOffset = 0.15 }

    else
        { color = Color.rgb255 150 110 60, halfWidth = 1.0, zOffset = 0.12 }


styledRoadToEntities : Coordinate -> Float -> Float -> Maybe ElevationGrid -> StyledRoad -> List (Scene3d.Entity WorldCoordinates)
styledRoadToEntities center baseAlt elevScale maybeGrid road =
    let
        style =
            roadStyle road.nature

        points =
            List.map (\c -> coord3dToPoint c center baseAlt elevScale maybeGrid style.zOffset) road.coords

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

                fz =
                    Length.inMeters (Point3d.zCoordinate from)

                tx =
                    Length.inMeters (Point3d.xCoordinate to)

                ty =
                    Length.inMeters (Point3d.yCoordinate to)

                tz =
                    Length.inMeters (Point3d.zCoordinate to)

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
                        (Point3d.meters (fx + nx) (fy + ny) fz)
                        (Point3d.meters (fx - nx) (fy - ny) fz)
                        (Point3d.meters (tx - nx) (ty - ny) tz)
                        (Point3d.meters (tx + nx) (ty + ny) tz)
                    )
        )
        pairs



-- VEGETATION


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


vegetationToEntity : Coordinate -> Float -> Float -> Maybe ElevationGrid -> VegetationZone -> List (Scene3d.Entity WorldCoordinates)
vegetationToEntity center baseAlt elevScale maybeGrid zone =
    let
        color =
            vegetationColor zone.nature

        meterPoints =
            List.map
                (\c ->
                    let
                        p =
                            coord3dToPoint c center baseAlt elevScale maybeGrid 0.05
                    in
                    ( Length.inMeters (Point3d.xCoordinate p)
                    , Length.inMeters (Point3d.yCoordinate p)
                    , Length.inMeters (Point3d.zCoordinate p)
                    )
                )
                zone.coords

        n =
            toFloat (List.length meterPoints)

        cx =
            List.sum (List.map (\( x, _, _ ) -> x) meterPoints) / max 1 n

        cy =
            List.sum (List.map (\( _, y, _ ) -> y) meterPoints) / max 1 n

        cz =
            List.sum (List.map (\( _, _, z ) -> z) meterPoints) / max 1 n

        centroid =
            Point3d.meters cx cy cz

        pairs =
            List.map2 Tuple.pair meterPoints (List.drop 1 meterPoints)
    in
    List.map
        (\( ( x1, y1, z1 ), ( x2, y2, z2 ) ) ->
            Scene3d.quad (Material.matte color)
                centroid
                (Point3d.meters x1 y1 z1)
                (Point3d.meters x2 y2 z2)
                centroid
        )
        pairs



-- BUILDINGS


buildingColor : String -> Color.Color
buildingColor nature =
    if String.contains "Industriel" nature || String.contains "agricole" nature then
        Color.rgb255 160 150 140

    else
        Color.rgb255 200 185 170


buildingToEntity : Coordinate -> Float -> Float -> Maybe ElevationGrid -> IgnBuilding -> List (Scene3d.Entity WorldCoordinates)
buildingToEntity center baseAlt elevScale maybeGrid building =
    let
        color =
            buildingColor building.nature

        roofColor =
            Color.rgb255 140 60 50

        h =
            max 3.0 building.hauteur

        meterPoints =
            List.map
                (\c ->
                    let
                        p =
                            coord3dToPoint c center baseAlt elevScale maybeGrid 0.1
                    in
                    ( Length.inMeters (Point3d.xCoordinate p)
                    , Length.inMeters (Point3d.yCoordinate p)
                    , Length.inMeters (Point3d.zCoordinate p)
                    )
                )
                building.coords

        pairs =
            List.map2 Tuple.pair meterPoints (List.drop 1 meterPoints)

        walls =
            List.map
                (\( ( x1, y1, z1 ), ( x2, y2, z2 ) ) ->
                    Scene3d.quad (Material.matte color)
                        (Point3d.meters x1 y1 z1)
                        (Point3d.meters x2 y2 z2)
                        (Point3d.meters x2 y2 (z2 + h))
                        (Point3d.meters x1 y1 (z1 + h))
                )
                pairs

        n =
            toFloat (List.length meterPoints)

        cx =
            List.sum (List.map (\( x, _, _ ) -> x) meterPoints) / max 1 n

        cy =
            List.sum (List.map (\( _, y, _ ) -> y) meterPoints) / max 1 n

        cz =
            List.sum (List.map (\( _, _, z ) -> z) meterPoints) / max 1 n

        roofCenter =
            Point3d.meters cx cy (cz + h)

        roof =
            List.map
                (\( ( x1, y1, z1 ), ( x2, y2, z2 ) ) ->
                    Scene3d.quad (Material.matte roofColor)
                        roofCenter
                        (Point3d.meters x1 y1 (z1 + h))
                        (Point3d.meters x2 y2 (z2 + h))
                        roofCenter
                )
                pairs
    in
    walls ++ roof



-- CONTROL POINTS


controlPointToEntity : Coordinate -> Float -> Float -> Maybe ElevationGrid -> ControlPoint -> List (Scene3d.Entity WorldCoordinates)
controlPointToEntity center baseAlt elevScale maybeGrid cp =
    let
        pos =
            latLonToPoint cp.position center

        x =
            Length.inMeters (Point3d.xCoordinate pos)

        y =
            Length.inMeters (Point3d.yCoordinate pos)

        z =
            (sampleElevation maybeGrid cp.position - baseAlt) * elevScale

        color =
            if cp.found then
                Color.green

            else
                Color.orange
    in
    [ Scene3d.cylinder (Material.matte Color.white)
        (Cylinder3d.startingAt
            (Point3d.meters x y z)
            Direction3d.z
            { radius = Length.meters 0.05
            , length = Length.meters 2.0
            }
        )
    , Scene3d.sphere (Material.matte color)
        (Sphere3d.atPoint
            (Point3d.meters x y (z + 2.2))
            (Length.meters 0.3)
        )
    ]
