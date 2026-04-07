module View.World3D exposing (view)

{-| 3D World renderer for the orienteering game.
First-person view with smooth terrain relief, colored roads, vegetation and buildings.
Terrain is a single TriangularMesh for performance. All features placed on terrain
via bilinear interpolation of the elevation grid.
-}

import Angle
import Array
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


type WorldCoordinates
    = WorldCoordinates


view : GameState -> Int -> Int -> Html Msg
view gs width height =
    let
        center =
            gs.playerPosition

        baseAlt =
            case gs.elevationGrid of
                Just grid -> grid.minAlt
                Nothing -> 0

        exag =
            1.5

        -- Camera direction
        elmAngle =
            Angle.degrees (90 - gs.playerBearing)

        lookDirection =
            Direction3d.xy elmAngle

        -- Player altitude from terrain
        playerZ =
            altAt gs.elevationGrid center baseAlt exag

        eyePoint =
            Point3d.meters 0 0 (playerZ + 1.7)

        focalPoint =
            Point3d.translateIn lookDirection (Length.meters 30) (Point3d.meters 0 0 (playerZ + 0.3))

        camera =
            Camera3d.lookAt
                { eyePoint = eyePoint
                , focalPoint = focalPoint
                , upDirection = Direction3d.z
                , fov = Camera3d.angle (Angle.degrees 75)
                , projection = Camera3d.Perspective
                }

        -- Distance culling
        maxDist =
            150

        nearbyVeg =
            List.filter (\z -> isNearby center maxDist z.coords) gs.vegetation

        nearbyBuildings =
            List.filter (\b -> isNearby center maxDist b.coords) gs.ign_buildings

        nearbyRoads =
            List.filter (\r -> isNearby center maxDist r.coords) gs.roads

        -- Terrain mesh (single entity!)
        terrainEntity =
            buildTerrainMesh gs.elevationGrid center baseAlt exag

        -- Features on terrain
        vegetationEntities =
            List.concatMap (vegetationToEntity center gs.elevationGrid baseAlt exag) nearbyVeg

        buildingEntities =
            List.concatMap (buildingToEntity center gs.elevationGrid baseAlt exag) nearbyBuildings

        roadEntities =
            List.concatMap (smoothRoadEntities center gs.elevationGrid baseAlt exag) nearbyRoads

        cpEntities =
            List.concatMap (controlPointToEntity center gs.elevationGrid baseAlt exag) gs.controlPoints

        skyColor =
            Color.rgb255 135 206 235
    in
    Scene3d.sunny
        { camera = camera
        , clipDepth = Length.meters 0.1
        , dimensions = ( Pixels.int width, Pixels.int height )
        , background = Scene3d.backgroundColor skyColor
        , entities = terrainEntity ++ vegetationEntities ++ roadEntities ++ buildingEntities ++ cpEntities
        , shadows = False
        , upDirection = Direction3d.z
        , sunlightDirection = Direction3d.negativeZ
        }



-- ELEVATION INTERPOLATION


{-| Bilinear interpolation of elevation grid at a lat/lon point.
Returns altitude in meters above sea level.
-}
sampleAlt : Maybe ElevationGrid -> Coordinate -> Float
sampleAlt maybeGrid pos =
    case maybeGrid of
        Nothing ->
            0

        Just grid ->
            let
                gridArr =
                    Array.fromList (List.map Array.fromList grid.grid)

                totalLatDeg =
                    grid.cellSizeM * toFloat (grid.rows - 1) / 111000.0

                totalLonDeg =
                    grid.cellSizeM * toFloat (grid.cols - 1) / (111000.0 * cos (pos.lat * pi / 180))

                -- Normalized position (0 to rows-1)
                rowF =
                    (pos.lat - grid.originLat) / totalLatDeg * toFloat (grid.rows - 1)

                colF =
                    (pos.lon - grid.originLon) / totalLonDeg * toFloat (grid.cols - 1)

                -- Clamp to grid bounds
                r =
                    clamp 0 (toFloat (grid.rows - 2)) rowF

                c =
                    clamp 0 (toFloat (grid.cols - 2)) colF

                r0 =
                    floor r

                c0 =
                    floor c

                -- Fractional parts for interpolation
                fr =
                    r - toFloat r0

                fc =
                    c - toFloat c0

                getAlt row col =
                    gridArr
                        |> Array.get row
                        |> Maybe.andThen (Array.get col)
                        |> Maybe.withDefault grid.minAlt

                -- Bilinear interpolation
                z00 =
                    getAlt r0 c0

                z10 =
                    getAlt r0 (c0 + 1)

                z01 =
                    getAlt (r0 + 1) c0

                z11 =
                    getAlt (r0 + 1) (c0 + 1)
            in
            z00 * (1 - fr) * (1 - fc) + z10 * (1 - fr) * fc + z01 * fr * (1 - fc) + z11 * fr * fc


{-| Get terrain-relative z at a lat/lon, with exaggeration.
-}
altAt : Maybe ElevationGrid -> Coordinate -> Float -> Float -> Float
altAt maybeGrid pos baseAlt exag =
    (sampleAlt maybeGrid pos - baseAlt) * exag


{-| Convert Coord3D to x,y,z in scene coordinates, placed on terrain.
-}
toXYZ : Coord3D -> Coordinate -> Maybe ElevationGrid -> Float -> Float -> Float -> { x : Float, y : Float, z : Float }
toXYZ c center maybeGrid baseAlt exag zOffset =
    let
        earthRadius =
            6371000

        avgLat =
            center.lat * pi / 180
    in
    { x = (c.lon - center.lon) * pi / 180 * earthRadius * cos avgLat
    , y = (c.lat - center.lat) * pi / 180 * earthRadius
    , z = altAt maybeGrid { lat = c.lat, lon = c.lon } baseAlt exag + zOffset
    }



-- TERRAIN MESH (single entity)


buildTerrainMesh : Maybe ElevationGrid -> Coordinate -> Float -> Float -> List (Scene3d.Entity WorldCoordinates)
buildTerrainMesh maybeGrid center baseAlt exag =
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
                earthRadius =
                    6371000

                avgLat =
                    center.lat * pi / 180

                totalSizeX =
                    grid.cellSizeM * toFloat (grid.cols - 1)

                totalSizeY =
                    grid.cellSizeM * toFloat (grid.rows - 1)

                originX =
                    (grid.originLon - center.lon) * pi / 180 * earthRadius * cos avgLat

                originY =
                    (grid.originLat - center.lat) * pi / 180 * earthRadius

                gridArr =
                    Array.fromList (List.map Array.fromList grid.grid)

                getAlt row col =
                    gridArr
                        |> Array.get row
                        |> Maybe.andThen (Array.get col)
                        |> Maybe.withDefault grid.minAlt

                -- Build single terrain mesh using TriangularMesh.grid
                terrainMesh =
                    TriangularMesh.grid (grid.cols - 1) (grid.rows - 1)
                        (\u v ->
                            let
                                col =
                                    round (u * toFloat (grid.cols - 1))

                                row =
                                    round (v * toFloat (grid.rows - 1))

                                x =
                                    originX + u * totalSizeX

                                y =
                                    originY + v * totalSizeY

                                z =
                                    (getAlt row col - baseAlt) * exag
                            in
                            Point3d.meters x y z
                        )

                sceneMesh =
                    Mesh.indexedFacets terrainMesh

                -- Flat base ground beyond terrain
                avgZ =
                    ((grid.minAlt + grid.maxAlt) / 2 - baseAlt) * exag
            in
            [ Scene3d.mesh (Material.matte (Color.rgb255 75 135 45)) sceneMesh
            , Scene3d.quad (Material.matte (Color.rgb255 65 120 40))
                (Point3d.meters -1000 -1000 avgZ)
                (Point3d.meters 1000 -1000 avgZ)
                (Point3d.meters 1000 1000 avgZ)
                (Point3d.meters -1000 1000 avgZ)
            ]



-- DISTANCE CULLING


isNearby : Coordinate -> Float -> List Coord3D -> Bool
isNearby center maxDistM coords =
    let
        maxDegLat =
            maxDistM / 111000

        maxDegLon =
            maxDistM / (111000 * cos (center.lat * pi / 180))
    in
    List.any
        (\c ->
            abs (c.lat - center.lat) < maxDegLat && abs (c.lon - center.lon) < maxDegLon
        )
        coords



-- STYLED ROADS


roadStyle : String -> { color : Color.Color, halfWidth : Float, z : Float }
roadStyle nature =
    if String.contains "1 chauss" nature || String.contains "2 chauss" nature || String.contains "Rond-point" nature then
        { color = Color.rgb255 55 55 55, halfWidth = 2.0, z = 0.15 }

    else if String.contains "Chemin" nature then
        { color = Color.rgb255 194 160 80, halfWidth = 1.2, z = 0.12 }

    else if String.contains "Sentier" nature then
        { color = Color.rgb255 165 125 75, halfWidth = 0.5, z = 0.10 }

    else if String.contains "cyclable" nature then
        { color = Color.rgb255 160 160 160, halfWidth = 1.0, z = 0.13 }

    else
        { color = Color.rgb255 150 110 60, halfWidth = 1.0, z = 0.12 }


{-| Render a road as a smooth triangle strip with averaged normals at each vertex.
    This produces clean joins at bends without junction discs.
-}
smoothRoadEntities : Coordinate -> Maybe ElevationGrid -> Float -> Float -> StyledRoad -> List (Scene3d.Entity WorldCoordinates)
smoothRoadEntities center maybeGrid baseAlt exag road =
    let
        style =
            roadStyle road.nature

        pts =
            List.map (\c -> toXYZ c center maybeGrid baseAlt exag style.z) road.coords

        arr =
            Array.fromList pts

        n =
            Array.length arr

        get i =
            Array.get (clamp 0 (n - 1) i) arr
                |> Maybe.withDefault { x = 0, y = 0, z = 0 }

        -- Compute averaged normal at vertex i (perpendicular to average of adjacent segment directions)
        vertexNormal i =
            let
                prev = get (i - 1)
                curr = get i
                next = get (i + 1)

                -- Incoming direction
                dx1 = curr.x - prev.x
                dy1 = curr.y - prev.y
                len1 = sqrt (dx1 * dx1 + dy1 * dy1)

                -- Outgoing direction
                dx2 = next.x - curr.x
                dy2 = next.y - curr.y
                len2 = sqrt (dx2 * dx2 + dy2 * dy2)

                -- Normalized directions (or zero)
                ( ndx1, ndy1 ) =
                    if len1 < 0.01 then ( 0, 0 ) else ( dx1 / len1, dy1 / len1 )

                ( ndx2, ndy2 ) =
                    if len2 < 0.01 then ( 0, 0 ) else ( dx2 / len2, dy2 / len2 )

                -- Average tangent
                tx = ndx1 + ndx2
                ty = ndy1 + ndy2
                tLen = sqrt (tx * tx + ty * ty)

                -- Normal = perpendicular to tangent
                ( nx, ny ) =
                    if tLen < 0.01 then
                        -- Fallback: use outgoing segment normal
                        if len2 > 0.01 then ( -dy2 / len2, dx2 / len2 )
                        else if len1 > 0.01 then ( -dy1 / len1, dx1 / len1 )
                        else ( 0, 0 )
                    else
                        ( -ty / tLen, tx / tLen )

                -- Limit miter extension to 2x width to avoid spikes at very sharp angles
                miterScale =
                    if tLen < 0.01 then 1.0
                    else
                        let
                            cosHalfAngle = tLen / 2.0
                        in
                        min 2.0 (1.0 / max 0.3 cosHalfAngle)
            in
            { nx = nx * style.halfWidth * miterScale
            , ny = ny * style.halfWidth * miterScale
            }

        -- Build left/right points for each vertex
        edgePoints i =
            let
                p = get i
                norm = vertexNormal i
            in
            { left = Point3d.meters (p.x + norm.nx) (p.y + norm.ny) p.z
            , right = Point3d.meters (p.x - norm.nx) (p.y - norm.ny) p.z
            }

        edges =
            List.map edgePoints (List.range 0 (n - 1))

        edgePairs =
            List.map2 Tuple.pair edges (List.drop 1 edges)

        mat =
            Material.matte style.color
    in
    List.map
        (\( e1, e2 ) ->
            Scene3d.quad mat e1.left e1.right e2.right e2.left
        )
        edgePairs





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
        Color.rgb255 140 110 70
    else if String.contains "Verger" nature then
        Color.rgb255 100 150 60
    else if String.contains "Lande" nature then
        Color.rgb255 140 160 80
    else
        Color.rgb255 60 120 50


vegetationToEntity : Coordinate -> Maybe ElevationGrid -> Float -> Float -> VegetationZone -> List (Scene3d.Entity WorldCoordinates)
vegetationToEntity center maybeGrid baseAlt exag zone =
    let
        color = vegetationColor zone.nature
        pts = List.map (\c -> toXYZ c center maybeGrid baseAlt exag 0.05) zone.coords
        n = List.length pts
        nf = toFloat n
        cx = List.sum (List.map .x pts) / max 1 nf
        cy = List.sum (List.map .y pts) / max 1 nf
        cz = List.sum (List.map .z pts) / max 1 nf

        -- Ground: fan of quads from centroid to each edge (4 distinct points)
        centroid = { x = cx, y = cy, z = cz }
        pairs = List.map2 Tuple.pair pts (List.drop 1 pts ++ List.take 1 pts)

        ground =
            List.map
                (\( a, b ) ->
                    let
                        -- Midpoint between a and b as 4th distinct point
                        mx = (a.x + b.x) / 2
                        my = (a.y + b.y) / 2
                        mz = (a.z + b.z) / 2
                    in
                    Scene3d.quad (Material.matte color)
                        (Point3d.meters centroid.x centroid.y centroid.z)
                        (Point3d.meters a.x a.y a.z)
                        (Point3d.meters mx my mz)
                        (Point3d.meters b.x b.y b.z)
                )
                pairs

        -- 3D decorations (limited for performance)
        isVigne = String.contains "Vigne" zone.nature

        decor =
            if isForest zone.nature then
                scatterTrees zone.nature cx cy cz pts

            else if isVigne then
                vineRows cx cy cz pts

            else if String.contains "Verger" zone.nature then
                orchardTrees cx cy cz pts

            else
                []

        -- Vineyard ground: alternating strips of soil colors
        vineStrips =
            if isVigne then
                vineGroundStrips cx cy cz pts
            else
                []
    in
    ground ++ vineStrips ++ decor


isForest : String -> Bool
isForest nature =
    String.contains "feuillus" nature
        || String.contains "conif" nature
        || String.contains "mixte" nature
        || String.contains "Bois" nature


{-| Scatter trees inside a vegetation polygon. Max 12 per zone.
-}
scatterTrees : String -> Float -> Float -> Float -> List { x : Float, y : Float, z : Float } -> List (Scene3d.Entity WorldCoordinates)
scatterTrees nature cx cy cz pts =
    let
        xs = List.map .x pts
        ys = List.map .y pts
        minX = List.minimum xs |> Maybe.withDefault cx
        maxX = List.maximum xs |> Maybe.withDefault cx
        minY = List.minimum ys |> Maybe.withDefault cy
        maxY = List.maximum ys |> Maybe.withDefault cy
        step = 12.0

        pseudoRand ix iy =
            toFloat (modBy 1000 (abs (ix * 7919 + iy * 104729))) / 1000.0

        isConifer = String.contains "conif" nature
        crownColor = if isConifer then Color.rgb255 20 70 30 else Color.rgb255 35 110 35
    in
    List.concatMap
        (\ix ->
            List.filterMap
                (\iy ->
                    let
                        rx = pseudoRand ix iy
                        ry = pseudoRand iy ix
                        x = minX + toFloat ix * step + (rx - 0.5) * step * 0.5
                        y = minY + toFloat iy * step + (ry - 0.5) * step * 0.5
                    in
                    if pointInPolygon x y pts then
                        Just
                            (Scene3d.group
                                [ Scene3d.cylinder (Material.matte (Color.rgb255 90 60 30))
                                    (Cylinder3d.startingAt (Point3d.meters x y cz) Direction3d.z
                                        { length = Length.meters 2.5, radius = Length.meters 0.2 }
                                    )
                                , Scene3d.sphere (Material.matte crownColor)
                                    (Sphere3d.atPoint (Point3d.meters x y (cz + 3.5))
                                        (Length.meters 1.8)
                                    )
                                ]
                            )
                    else
                        Nothing
                )
                (List.range 0 (min 3 (round ((maxY - minY) / step))))
        )
        (List.range 0 (min 3 (round ((maxX - minX) / step))))
        |> List.take 12


{-| Vine rows: parallel rows along Y axis, posts every 5m. Max 30 vines.
-}
vineRows : Float -> Float -> Float -> List { x : Float, y : Float, z : Float } -> List (Scene3d.Entity WorldCoordinates)
vineRows cx cy cz pts =
    let
        xs = List.map .x pts
        ys = List.map .y pts
        minX = List.minimum xs |> Maybe.withDefault cx
        maxX = List.maximum xs |> Maybe.withDefault cx
        minY = List.minimum ys |> Maybe.withDefault cy
        maxY = List.maximum ys |> Maybe.withDefault cy
        rowStep = 2.5
        postStep = 5.0
        vineGreen = Material.matte (Color.rgb255 80 130 40)
        postBrown = Material.matte (Color.rgb255 130 100 50)
    in
    List.concatMap
        (\ir ->
            let
                x = minX + toFloat ir * rowStep + rowStep / 2
            in
            List.filterMap
                (\ip ->
                    let
                        y = minY + toFloat ip * postStep + postStep / 2
                    in
                    if pointInPolygon x y pts then
                        Just
                            (Scene3d.group
                                [ Scene3d.cylinder postBrown
                                    (Cylinder3d.startingAt (Point3d.meters x y cz) Direction3d.z
                                        { length = Length.meters 1.2, radius = Length.meters 0.03 }
                                    )
                                , Scene3d.sphere vineGreen
                                    (Sphere3d.atPoint (Point3d.meters x y (cz + 1.0))
                                        (Length.meters 0.4)
                                    )
                                ]
                            )
                    else
                        Nothing
                )
                (List.range 0 (min 8 (round ((maxY - minY) / postStep))))
        )
        (List.range 0 (min 12 (round ((maxX - minX) / rowStep))))
        |> List.take 30


{-| Orchard trees: regular grid, max 10.
-}
orchardTrees : Float -> Float -> Float -> List { x : Float, y : Float, z : Float } -> List (Scene3d.Entity WorldCoordinates)
orchardTrees cx cy cz pts =
    let
        xs = List.map .x pts
        ys = List.map .y pts
        minX = List.minimum xs |> Maybe.withDefault cx
        maxX = List.maximum xs |> Maybe.withDefault cx
        minY = List.minimum ys |> Maybe.withDefault cy
        maxY = List.maximum ys |> Maybe.withDefault cy
        step = 8.0
    in
    List.concatMap
        (\ix ->
            List.filterMap
                (\iy ->
                    let
                        x = minX + toFloat ix * step + step / 2
                        y = minY + toFloat iy * step + step / 2
                    in
                    if pointInPolygon x y pts then
                        Just
                            (Scene3d.group
                                [ Scene3d.cylinder (Material.matte (Color.rgb255 100 70 35))
                                    (Cylinder3d.startingAt (Point3d.meters x y cz) Direction3d.z
                                        { length = Length.meters 1.8, radius = Length.meters 0.15 }
                                    )
                                , Scene3d.sphere (Material.matte (Color.rgb255 60 140 40))
                                    (Sphere3d.atPoint (Point3d.meters x y (cz + 2.5))
                                        (Length.meters 1.5)
                                    )
                                ]
                            )
                    else
                        Nothing
                )
                (List.range 0 (min 3 (round ((maxY - minY) / step))))
        )
        (List.range 0 (min 3 (round ((maxX - minX) / step))))
        |> List.take 10


{-| Vineyard ground: alternating strips of earth colors to simulate soil texture.
    Dark brown rows (plowed earth) alternating with lighter dry earth.
-}
vineGroundStrips : Float -> Float -> Float -> List { x : Float, y : Float, z : Float } -> List (Scene3d.Entity WorldCoordinates)
vineGroundStrips cx cy cz pts =
    let
        xs = List.map .x pts
        ys = List.map .y pts
        minX = List.minimum xs |> Maybe.withDefault cx
        maxX = List.maximum xs |> Maybe.withDefault cx
        minY = List.minimum ys |> Maybe.withDefault cy
        maxY = List.maximum ys |> Maybe.withDefault cy
        stripW = 1.2
        z = cz + 0.06

        -- Dark plowed earth between vines
        darkSoil = Material.matte (Color.rgb255 95 70 45)
        -- Lighter dry earth / gravel
        lightSoil = Material.matte (Color.rgb255 165 140 100)
        -- Slight green for vine row strip
        vineStrip = Material.matte (Color.rgb255 110 125 65)

        numStrips = min 20 (round ((maxX - minX) / stripW))
    in
    List.concatMap
        (\i ->
            let
                x1 = minX + toFloat i * stripW
                x2 = x1 + stripW
                mat =
                    if modBy 3 i == 0 then vineStrip
                    else if modBy 3 i == 1 then darkSoil
                    else lightSoil
            in
            -- One quad per strip segment along Y
            List.filterMap
                (\iy ->
                    let
                        y1 = minY + toFloat iy * 6.0
                        y2 = min maxY (y1 + 6.0)
                        midX = (x1 + x2) / 2
                        midY = (y1 + y2) / 2
                    in
                    if pointInPolygon midX midY pts then
                        Just
                            (Scene3d.quad mat
                                (Point3d.meters x1 y1 z)
                                (Point3d.meters x2 y1 z)
                                (Point3d.meters x2 y2 z)
                                (Point3d.meters x1 y2 z)
                            )
                    else
                        Nothing
                )
                (List.range 0 (min 15 (round ((maxY - minY) / 6.0))))
        )
        (List.range 0 numStrips)
        |> List.take 120


{-| Point-in-polygon test (ray casting).
-}
pointInPolygon : Float -> Float -> List { x : Float, y : Float, z : Float } -> Bool
pointInPolygon px py pts =
    let
        edges =
            List.map2 Tuple.pair pts (List.drop 1 pts ++ List.take 1 pts)
    in
    List.foldl
        (\( a, b ) count ->
            if (a.y > py) /= (b.y > py) && px < a.x + (py - a.y) / (b.y - a.y) * (b.x - a.x) then
                count + 1
            else
                count
        )
        0
        edges
        |> modBy 2
        |> (==) 1



-- BUILDINGS


buildingToEntity : Coordinate -> Maybe ElevationGrid -> Float -> Float -> IgnBuilding -> List (Scene3d.Entity WorldCoordinates)
buildingToEntity center maybeGrid baseAlt exag building =
    let
        wallColor =
            if String.contains "Industriel" building.nature || String.contains "agricole" building.nature then
                Color.rgb255 160 150 140
            else
                Color.rgb255 200 185 170

        roofColor = Color.rgb255 140 60 50
        h = max 3.0 building.hauteur
        pts = List.map (\c -> toXYZ c center maybeGrid baseAlt exag 0) building.coords
        pairs = List.map2 Tuple.pair pts (List.drop 1 pts)

        walls =
            List.map
                (\( a, b ) ->
                    Scene3d.quad (Material.matte wallColor)
                        (Point3d.meters a.x a.y a.z)
                        (Point3d.meters b.x b.y b.z)
                        (Point3d.meters b.x b.y (b.z + h))
                        (Point3d.meters a.x a.y (a.z + h))
                )
                pairs

        n = toFloat (List.length pts)
        rcx = List.sum (List.map .x pts) / max 1 n
        rcy = List.sum (List.map .y pts) / max 1 n
        rcz = List.sum (List.map .z pts) / max 1 n
        roofCenter = Point3d.meters rcx rcy (rcz + h)

        roof =
            List.map
                (\( a, b ) ->
                    Scene3d.quad (Material.matte roofColor)
                        roofCenter
                        (Point3d.meters a.x a.y (a.z + h))
                        (Point3d.meters b.x b.y (b.z + h))
                        roofCenter
                )
                pairs
    in
    walls ++ roof



-- CONTROL POINTS


controlPointToEntity : Coordinate -> Maybe ElevationGrid -> Float -> Float -> ControlPoint -> List (Scene3d.Entity WorldCoordinates)
controlPointToEntity center maybeGrid baseAlt exag cp =
    let
        earthRadius = 6371000
        avgLat = center.lat * pi / 180
        x = (cp.position.lon - center.lon) * pi / 180 * earthRadius * cos avgLat
        y = (cp.position.lat - center.lat) * pi / 180 * earthRadius
        z = altAt maybeGrid cp.position baseAlt exag

        color =
            if cp.found then Color.green else Color.orange
    in
    [ Scene3d.cylinder (Material.matte Color.white)
        (Cylinder3d.startingAt
            (Point3d.meters x y z)
            Direction3d.z
            { radius = Length.meters 0.05, length = Length.meters 2.0 }
        )
    , Scene3d.sphere (Material.matte color)
        (Sphere3d.atPoint (Point3d.meters x y (z + 2.2)) (Length.meters 0.3))
    ]
