module TopoTile exposing (TileBounds, loadTopoGrid, tileBoundsForPosition, tileBoundsForXY, tileUrl, tileUrlForXY, tileXY)

{-| Calculate IGN topo tile URLs and load them as textures.
Uses Web Mercator (EPSG:3857) tile math.

Supports loading a 3x3 grid of tiles around a position for better coverage.
-}

import Color
import Scene3d.Material as Material
import Task
import WebGL.Texture


type alias TileBounds =
    { minLat : Float, maxLat : Float, minLon : Float, maxLon : Float }


{-| Zoom level for topo tiles. 18 gives ~0.6m/pixel, good detail for first-person view.
-}
topoZoom : Int
topoZoom =
    18


{-| Calculate tile X/Y for a given lat/lon at the topo zoom level.
-}
tileXY : Float -> Float -> ( Int, Int )
tileXY lat lon =
    let
        n =
            2 ^ toFloat topoZoom

        x =
            floor ((lon + 180) / 360 * n)

        latRad =
            lat * pi / 180

        y =
            floor ((1 - logBase e (tan latRad + 1 / cos latRad) / pi) / 2 * n)
    in
    ( x, y )


{-| Get the geographic bounds of a tile by its X/Y coordinates.
-}
tileBoundsForXY : Int -> Int -> TileBounds
tileBoundsForXY x y =
    let
        n =
            2 ^ toFloat topoZoom

        minLon =
            toFloat x / n * 360 - 180

        maxLon =
            toFloat (x + 1) / n * 360 - 180

        maxLat =
            atan (sinh (pi * (1 - 2 * toFloat y / n))) * 180 / pi

        minLat =
            atan (sinh (pi * (1 - 2 * toFloat (y + 1) / n))) * 180 / pi
    in
    { minLat = minLat, maxLat = maxLat, minLon = minLon, maxLon = maxLon }


{-| Get the geographic bounds of the tile containing a position.
-}
tileBoundsForPosition : Float -> Float -> TileBounds
tileBoundsForPosition lat lon =
    let
        ( x, y ) =
            tileXY lat lon
    in
    tileBoundsForXY x y


{-| Build the IGN WMTS tile URL for a given tile X/Y.
-}
tileUrlForXY : Int -> Int -> String
tileUrlForXY x y =
    "https://data.geopf.fr/wmts?SERVICE=WMTS&REQUEST=GetTile&VERSION=1.0.0"
        ++ "&LAYER=GEOGRAPHICALGRIDSYSTEMS.PLANIGNV2"
        ++ "&STYLE=normal&TILEMATRIXSET=PM"
        ++ "&TILEMATRIX="
        ++ String.fromInt topoZoom
        ++ "&TILEROW="
        ++ String.fromInt y
        ++ "&TILECOL="
        ++ String.fromInt x
        ++ "&FORMAT=image/png"


{-| Build the IGN WMTS tile URL for a given lat/lon.
-}
tileUrl : Float -> Float -> String
tileUrl lat lon =
    let
        ( x, y ) =
            tileXY lat lon
    in
    tileUrlForXY x y


{-| Load a 3x3 grid of topo tiles around a position.
Each tile load sends a message with its bounds when complete.
-}
loadTopoGrid : Float -> Float -> (TileBounds -> Result WebGL.Texture.Error (Material.Texture Color.Color) -> msg) -> Cmd msg
loadTopoGrid lat lon toMsg =
    let
        ( cx, cy ) =
            tileXY lat lon

        offsets =
            [ ( -1, -1 ), ( 0, -1 ), ( 1, -1 )
            , ( -1, 0 ), ( 0, 0 ), ( 1, 0 )
            , ( -1, 1 ), ( 0, 1 ), ( 1, 1 )
            ]

        loadTile ( dx, dy ) =
            let
                tx =
                    cx + dx

                ty =
                    cy + dy

                bounds =
                    tileBoundsForXY tx ty

                url =
                    tileUrlForXY tx ty
            in
            Material.load url
                |> Task.attempt (toMsg bounds)
    in
    Cmd.batch (List.map loadTile offsets)


{-| Hyperbolic sine (not in Elm's Basics).
-}
sinh : Float -> Float
sinh x =
    (e ^ x - e ^ -x) / 2
