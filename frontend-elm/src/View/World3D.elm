module View.World3D exposing (view)

{-| 3D World renderer for the orienteering game.
Uses elm-3d-scene with IGN topo tile as ground texture.

Camera is always behind the player, looking in the direction of the road.
The road goes toward the top of the screen (third-person running view).
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
import Vector3d


type WorldCoordinates
    = WorldCoordinates


view : GameState -> Int -> Int -> Html Msg
view gs width height =
    let
        -- Center the scene on the PLAYER (not first control point)
        center =
            gs.playerPosition

        -- Camera above and behind the player, looking ahead and down at the map
        -- Direction3d.xy uses 0°=+X=East, but compass bearing uses 0°=North=+Y
        -- Conversion: elm_angle = 90 - compass_bearing
        playerPoint =
            latLonToPoint gs.playerPosition center

        elmAngle =
            Angle.degrees (90 - gs.playerBearing)

        lookDirection =
            Direction3d.xy elmAngle

        behindDirection =
            Direction3d.xy (Angle.degrees (90 - gs.playerBearing + 180))

        -- Eye point: 20m high, 15m behind the player
        eyePoint =
            playerPoint
                |> Point3d.translateIn Direction3d.z (Length.meters 20)
                |> Point3d.translateIn behindDirection (Length.meters 15)

        -- Look at a point 20m ahead of the player, on the ground
        focalPoint =
            Point3d.translateIn lookDirection (Length.meters 20) playerPoint

        camera =
            Camera3d.lookAt
                { eyePoint = eyePoint
                , focalPoint = focalPoint
                , upDirection = Direction3d.z
                , fov = Camera3d.angle (Angle.degrees 70)
                , projection = Camera3d.Perspective
                }

        -- Ground: topo tile grid if available, else green fallback
        groundEntities =
            if List.isEmpty gs.topoTiles then
                [ Scene3d.quad (Material.matte (Color.rgb255 90 120 60))
                    (Point3d.meters -500 -500 0)
                    (Point3d.meters 500 -500 0)
                    (Point3d.meters 500 500 0)
                    (Point3d.meters -500 500 0)
                ]

            else
                List.map (\tile -> topoGroundQuad tile.texture tile.bounds center) gs.topoTiles

        -- Player position marker on ground (red circle + direction arrow)
        playerMarkerEntities =
            playerMarker center gs.playerBearing

        -- Control point markers
        cpEntities =
            List.concatMap (controlPointToEntity center) gs.controlPoints

        -- Trees scattered around the player (relative to player position)
        treeEntities =
            List.concatMap treeEntity (scatteredTrees 60)

        -- Sky
        skyColor =
            Color.rgb255 135 206 235
    in
    Scene3d.sunny
        { camera = camera
        , clipDepth = Length.meters 0.5
        , dimensions = ( Pixels.int width, Pixels.int height )
        , background = Scene3d.backgroundColor skyColor
        , entities = groundEntities ++ playerMarkerEntities ++ cpEntities ++ treeEntities
        , shadows = False
        , upDirection = Direction3d.z
        , sunlightDirection = Direction3d.negativeZ
        }



-- TOPO GROUND WITH TEXTURE


{-| Create a textured ground quad from the IGN topo tile.
Maps the tile's geographic bounds to 3D coordinates.
-}
topoGroundQuad : Material.Texture Color.Color -> { minLat : Float, maxLat : Float, minLon : Float, maxLon : Float } -> Coordinate -> Scene3d.Entity WorldCoordinates
topoGroundQuad texture bounds center =
    let
        -- Convert tile bounds to 3D coordinates relative to center
        sw =
            latLonToPoint { lat = bounds.minLat, lon = bounds.minLon } center

        se =
            latLonToPoint { lat = bounds.minLat, lon = bounds.maxLon } center

        ne =
            latLonToPoint { lat = bounds.maxLat, lon = bounds.maxLon } center

        nw =
            latLonToPoint { lat = bounds.maxLat, lon = bounds.minLon } center

        -- Vertices with UV coordinates and normals
        upNormal =
            Vector3d.unitless 0 0 1

        v0 =
            { position = sw, normal = upNormal, uv = ( 0, 0 ) }

        v1 =
            { position = se, normal = upNormal, uv = ( 1, 0 ) }

        v2 =
            { position = ne, normal = upNormal, uv = ( 1, 1 ) }

        v3 =
            { position = nw, normal = upNormal, uv = ( 0, 1 ) }

        -- Create mesh with two triangles
        mesh =
            TriangularMesh.indexed
                (Array.fromList [ v0, v1, v2, v3 ])
                [ ( 0, 1, 2 ), ( 0, 2, 3 ) ]

        sceneMesh =
            Mesh.texturedFaces mesh
    in
    Scene3d.mesh (Material.texturedMatte texture) sceneMesh



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



-- ROADS (3D paths above the topo ground)


roadToEntities : Coordinate -> List Coordinate -> List (Scene3d.Entity WorldCoordinates)
roadToEntities center coords =
    let
        points =
            List.map (\c -> latLonToPoint c center) coords

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
                    -- Road width: 1.5m each side = 3m wide (more visible)
                    nx =
                        -dy / segLen * 1.5

                    ny =
                        dx / segLen * 1.5

                    z =
                        0.15
                in
                Just
                    (Scene3d.quad (Material.matte (Color.rgb255 180 140 90))
                        (Point3d.meters (fx + nx) (fy + ny) z)
                        (Point3d.meters (fx - nx) (fy - ny) z)
                        (Point3d.meters (tx - nx) (ty - ny) z)
                        (Point3d.meters (tx + nx) (ty + ny) z)
                    )
        )
        pairs



-- PLAYER MARKER (red dot + direction arrow on ground)


playerMarker : Coordinate -> Float -> List (Scene3d.Entity WorldCoordinates)
playerMarker center bearing =
    let
        -- Player is at center (0,0) since scene is centered on player
        z =
            0.2

        -- Direction arrow: a thin triangle pointing in bearing direction
        bearRad =
            bearing * pi / 180

        -- Arrow tip: 3m ahead
        tipX =
            sin bearRad * 3.0

        tipY =
            cos bearRad * 3.0

        -- Arrow base: 0.8m to each side, 0.5m behind
        perpX =
            cos bearRad * 0.8

        perpY =
            -(sin bearRad) * 0.8

        baseX =
            -(sin bearRad) * 0.5

        baseY =
            -(cos bearRad) * 0.5
    in
    [ -- Red dot at player feet
      Scene3d.sphere (Material.matte Color.red)
        (Sphere3d.atPoint
            (Point3d.meters 0 0 z)
            (Length.meters 0.4)
        )
    , -- Direction arrow (yellow triangle on ground)
      Scene3d.quad (Material.matte (Color.rgb255 255 220 0))
        (Point3d.meters tipX tipY z)
        (Point3d.meters (baseX + perpX) (baseY + perpY) z)
        (Point3d.meters baseX baseY z)
        (Point3d.meters (baseX - perpX) (baseY - perpY) z)
    ]



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
    , -- Marker sphere (bigger for visibility)
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
