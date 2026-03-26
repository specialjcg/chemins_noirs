module Decoders exposing (..)

{-| Module de décodage JSON pour les réponses du backend Rust.
Approche fonctionnelle pure avec gestion d'erreurs explicite.
-}

import Json.Decode as Decode exposing (Decoder)
import Types exposing (..)


-- GEOCODING (Nominatim)


stringToFloat : String -> Decoder Float
stringToFloat str =
    case String.toFloat str of
        Just f ->
            Decode.succeed f

        Nothing ->
            Decode.fail ("Not a valid float: " ++ str)


decodeGeoResult : Decoder GeoResult
decodeGeoResult =
    Decode.map3 GeoResult
        (Decode.field "lat" Decode.string |> Decode.andThen stringToFloat)
        (Decode.field "lon" Decode.string |> Decode.andThen stringToFloat)
        (Decode.field "display_name" Decode.string)


decodeGeoResults : Decoder (List GeoResult)
decodeGeoResults =
    Decode.list decodeGeoResult



-- COORDINATE


decodeCoordinate : Decoder Coordinate
decodeCoordinate =
    Decode.map2 Coordinate
        (Decode.field "lat" Decode.float)
        (Decode.field "lon" Decode.float)



-- ROUTE RESPONSE


decodeRouteResponse : Decoder RouteResponse
decodeRouteResponse =
    Decode.map8
        (\path dist gpx meta elev snapped time diff ->
            \surface segments ->
                { path = path
                , distanceKm = dist
                , gpxBase64 = gpx
                , metadata = meta
                , elevationProfile = elev
                , snappedWaypoints = snapped
                , estimatedTimeMinutes = time
                , difficulty = diff
                , surfaceBreakdown = surface
                , segments = segments
                }
        )
        (Decode.field "path" (Decode.list decodeCoordinate))
        (Decode.field "distance_km" Decode.float)
        (Decode.field "gpx_base64" Decode.string)
        (Decode.maybe (Decode.field "metadata" decodeRouteMetadata))
        (Decode.maybe (Decode.field "elevation_profile" decodeElevationProfile))
        (Decode.maybe (Decode.field "snapped_waypoints" (Decode.list decodeCoordinate)))
        (Decode.maybe (Decode.field "estimated_time_minutes" Decode.int))
        (Decode.maybe (Decode.field "difficulty" Decode.string))
        |> Decode.andThen
            (\fn ->
                Decode.map2 fn
                    (Decode.maybe (Decode.field "surface_breakdown" decodeSurfaceBreakdown))
                    (Decode.maybe (Decode.field "segments" (Decode.list decodeSegmentStats)))
            )


decodeSegmentStats : Decoder SegmentStats
decodeSegmentStats =
    Decode.map6 SegmentStats
        (Decode.field "from_index" Decode.int)
        (Decode.field "to_index" Decode.int)
        (Decode.field "distance_km" Decode.float)
        (Decode.field "ascent_m" Decode.float)
        (Decode.field "descent_m" Decode.float)
        (Decode.field "avg_slope_pct" Decode.float)


decodeSurfaceBreakdown : Decoder (List ( String, Float ))
decodeSurfaceBreakdown =
    Decode.list (decodeTuple2 Decode.string Decode.float)


decodeTuple2 : Decoder a -> Decoder b -> Decoder ( a, b )
decodeTuple2 da db =
    Decode.map2 Tuple.pair
        (Decode.index 0 da)
        (Decode.index 1 db)


decodeRouteMetadata : Decoder RouteMetadata
decodeRouteMetadata =
    Decode.map4 RouteMetadata
        (Decode.field "point_count" Decode.int)
        (Decode.field "bounds" decodeRouteBounds)
        (Decode.field "start" decodeCoordinate)
        (Decode.field "end" decodeCoordinate)


decodeRouteBounds : Decoder RouteBounds
decodeRouteBounds =
    Decode.map4 RouteBounds
        (Decode.field "min_lat" Decode.float)
        (Decode.field "max_lat" Decode.float)
        (Decode.field "min_lon" Decode.float)
        (Decode.field "max_lon" Decode.float)


decodeElevationProfile : Decoder ElevationProfile
decodeElevationProfile =
    Decode.map5 ElevationProfile
        (Decode.field "elevations" (Decode.list (Decode.nullable Decode.float)))
        (Decode.maybe (Decode.field "min_elevation" Decode.float))
        (Decode.maybe (Decode.field "max_elevation" Decode.float))
        (Decode.field "total_ascent" Decode.float)
        (Decode.field "total_descent" Decode.float)



-- LOOP ROUTE RESPONSE


decodeLoopRouteResponse : Decoder LoopRouteResponse
decodeLoopRouteResponse =
    Decode.map3 LoopRouteResponse
        (Decode.field "candidates" (Decode.list decodeLoopCandidate))
        (Decode.field "target_distance_km" Decode.float)
        (Decode.field "distance_tolerance_km" Decode.float)


decodeLoopCandidate : Decoder LoopCandidate
decodeLoopCandidate =
    Decode.map3 LoopCandidate
        (Decode.field "route" decodeRouteResponse)
        (Decode.field "distance_error_km" Decode.float)
        (Decode.field "bearing_deg" Decode.float)



-- SAVED ROUTE


decodeSavedRoute : Decoder SavedRoute
decodeSavedRoute =
    Decode.map8
        (\id name desc createdAt updatedAt distanceKm ascentM descentM ->
            \isFav tags originalWp routeData ->
                { id = id
                , name = name
                , description = desc
                , createdAt = createdAt
                , updatedAt = updatedAt
                , distanceKm = distanceKm
                , totalAscentM = ascentM
                , totalDescentM = descentM
                , isFavorite = isFav
                , tags = tags
                , originalWaypoints = originalWp
                , routeData = routeData
                }
        )
        (Decode.field "id" Decode.int)
        (Decode.field "name" Decode.string)
        (Decode.maybe (Decode.field "description" Decode.string))
        (Decode.field "created_at" Decode.string)
        (Decode.field "updated_at" Decode.string)
        (Decode.field "distance_km" Decode.float)
        (Decode.maybe (Decode.field "total_ascent_m" Decode.float))
        (Decode.maybe (Decode.field "total_descent_m" Decode.float))
        |> Decode.andThen
            (\fn ->
                Decode.map4 fn
                    (Decode.field "is_favorite" Decode.bool)
                    (Decode.field "tags" (Decode.list Decode.string))
                    (Decode.maybe (Decode.field "original_waypoints" (Decode.list decodeCoordinate)))
                    (Decode.field "route_data" decodeRouteResponse)
            )


decodeSavedRoutesList : Decoder (List SavedRoute)
decodeSavedRoutesList =
    Decode.list decodeSavedRoute


decodeCoord3D : Decoder Coord3D
decodeCoord3D =
    Decode.map3 Coord3D
        (Decode.field "lat" Decode.float)
        (Decode.field "lon" Decode.float)
        (Decode.oneOf [ Decode.field "alt" Decode.float, Decode.succeed 0 ])


decodeRoadsResponse : Decoder (List StyledRoad)
decodeRoadsResponse =
    Decode.field "roads"
        (Decode.list
            (Decode.map2 StyledRoad
                (Decode.field "nature" Decode.string)
                (Decode.field "coords" (Decode.list decodeCoord3D))
            )
        )


decodeVegetationResponse : Decoder (List VegetationZone)
decodeVegetationResponse =
    Decode.field "zones"
        (Decode.list
            (Decode.map2 VegetationZone
                (Decode.field "nature" Decode.string)
                (Decode.field "coords" (Decode.list decodeCoord3D))
            )
        )


decodeElevationGrid : Decoder ElevationGrid
decodeElevationGrid =
    Decode.map8 ElevationGrid
        (Decode.field "grid" (Decode.list (Decode.list Decode.float)))
        (Decode.field "min_alt" Decode.float)
        (Decode.field "max_alt" Decode.float)
        (Decode.field "origin_lat" Decode.float)
        (Decode.field "origin_lon" Decode.float)
        (Decode.field "cell_size_m" Decode.float)
        (Decode.field "rows" Decode.int)
        (Decode.field "cols" Decode.int)


decodeIgnBuildingsResponse : Decoder (List IgnBuilding)
decodeIgnBuildingsResponse =
    Decode.field "buildings"
        (Decode.list
            (Decode.map3 IgnBuilding
                (Decode.field "nature" Decode.string)
                (Decode.field "hauteur" Decode.float)
                (Decode.field "coords" (Decode.list decodeCoord3D))
            )
        )


decodeBuildingsResponse : Decoder (List { center : Coordinate, polygon : List Coordinate })
decodeBuildingsResponse =
    Decode.field "buildings"
        (Decode.list
            (Decode.map2 (\c p -> { center = c, polygon = p })
                (Decode.field "center"
                    (Decode.map2 Coordinate
                        (Decode.field "lat" Decode.float)
                        (Decode.field "lon" Decode.float)
                    )
                )
                (Decode.field "polygon"
                    (Decode.list
                        (Decode.map2 Coordinate
                            (Decode.field "lat" Decode.float)
                            (Decode.field "lon" Decode.float)
                        )
                    )
                )
            )
        )
