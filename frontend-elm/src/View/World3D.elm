module View.World3D exposing (view)

{-| 3D World renderer for the orienteering game.
Uses elm-3d-scene for pure Elm WebGL rendering.
No JavaScript needed — everything is MVU.
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


view : GameState -> List (List Coordinate) -> Int -> Int -> Html Msg
view gs roads width height =
    let
        -- Use first control point as terrain center (or player start)
        center =
            case List.head gs.controlPoints of
                Just cp ->
                    cp.position

                Nothing ->
                    gs.playerPosition

        -- Camera at player position, looking in bearing direction
        playerPoint =
            latLonToPoint gs.playerPosition center

        eyePoint =
            Point3d.translateIn Direction3d.z (Length.meters 1.7) playerPoint

        bearingAngle =
            Angle.degrees gs.playerBearing

        lookDirection =
            Direction3d.xy bearingAngle

        focalPoint =
            Point3d.translateIn lookDirection (Length.meters 50) eyePoint
                |> Point3d.translateIn Direction3d.z (Length.meters -1.7)

        camera =
            Camera3d.lookAt
                { eyePoint = eyePoint
                , focalPoint = focalPoint
                , upDirection = Direction3d.z
                , fov = Camera3d.angle (Angle.degrees 70)
                , projection = Camera3d.Perspective
                }

        -- Ground plane
        ground =
            Scene3d.quad (Material.matte Color.darkGreen)
                (Point3d.meters -500 -500 0)
                (Point3d.meters 500 -500 0)
                (Point3d.meters 500 500 0)
                (Point3d.meters -500 500 0)

        -- Roads
        roadEntities =
            List.concatMap (roadToEntities center) roads

        -- Control point markers
        cpEntities =
            List.concatMap (controlPointToEntity center) gs.controlPoints

        -- Trees
        treeEntities =
            List.concatMap treeEntity defaultTrees

        -- Sky color
        skyColor =
            Color.rgb255 135 206 235
    in
    Scene3d.sunny
        { camera = camera
        , clipDepth = Length.meters 0.5
        , dimensions = ( Pixels.int width, Pixels.int height )
        , background = Scene3d.backgroundColor skyColor
        , entities = ground :: roadEntities ++ cpEntities ++ treeEntities
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



-- ROAD RENDERING


type alias RoadSegment =
    List Coordinate


roadToEntities : Coordinate -> RoadSegment -> List (Scene3d.Entity WorldCoordinates)
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
                midPoint =
                    Point3d.midpoint from to

                dx =
                    Length.inMeters (Point3d.xCoordinate to) - Length.inMeters (Point3d.xCoordinate from)

                dy =
                    Length.inMeters (Point3d.yCoordinate to) - Length.inMeters (Point3d.yCoordinate from)

                segLen =
                    sqrt (dx * dx + dy * dy)
            in
            if segLen < 0.1 then
                Nothing

            else
                Just
                    (Scene3d.quad (Material.matte (Color.rgb255 180 165 130))
                        (Point3d.meters (Length.inMeters (Point3d.xCoordinate from)) (Length.inMeters (Point3d.yCoordinate from) - 1) 0.05)
                        (Point3d.meters (Length.inMeters (Point3d.xCoordinate from)) (Length.inMeters (Point3d.yCoordinate from) + 1) 0.05)
                        (Point3d.meters (Length.inMeters (Point3d.xCoordinate to)) (Length.inMeters (Point3d.yCoordinate to) + 1) 0.05)
                        (Point3d.meters (Length.inMeters (Point3d.xCoordinate to)) (Length.inMeters (Point3d.yCoordinate to) - 1) 0.05)
                    )
        )
        pairs



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
            { radius = Length.meters 0.03
            , length = Length.meters 1.2
            }
        )
    , -- Orange marker sphere
      Scene3d.sphere (Material.matte color)
        (Sphere3d.atPoint
            (Point3d.meters x y 1.3)
            (Length.meters 0.15)
        )
    ]



-- TREES


type alias TreePos =
    { x : Float, y : Float, scale : Float }


defaultTrees : List TreePos
defaultTrees =
    List.indexedMap
        (\i _ ->
            let
                seed =
                    toFloat i

                x =
                    sin (seed * 137.508) * 400

                y =
                    cos (seed * 137.508 + 0.5) * 400

                s =
                    0.6 + sin (seed * 42.0) * 0.3
            in
            { x = x, y = y, scale = s }
        )
        (List.repeat 200 ())


treeEntity : TreePos -> List (Scene3d.Entity WorldCoordinates)
treeEntity tree =
    [ -- Trunk
      Scene3d.cylinder (Material.matte (Color.rgb255 92 64 51))
        (Cylinder3d.startingAt
            (Point3d.meters tree.x tree.y 0)
            Direction3d.z
            { radius = Length.meters (0.1 * tree.scale)
            , length = Length.meters (3 * tree.scale)
            }
        )
    , -- Leaves
      Scene3d.sphere (Material.matte Color.darkGreen)
        (Sphere3d.atPoint
            (Point3d.meters tree.x tree.y (4 * tree.scale))
            (Length.meters (1.5 * tree.scale))
        )
    ]
