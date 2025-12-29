module Decoders exposing (..)

{-| Module de décodage JSON pour les réponses du backend Rust.
Approche fonctionnelle pure avec gestion d'erreurs explicite.
-}

import Json.Decode as Decode exposing (Decoder)
import Types exposing (..)


-- COORDINATE


decodeCoordinate : Decoder Coordinate
decodeCoordinate =
    Decode.map2 Coordinate
        (Decode.field "lat" Decode.float)
        (Decode.field "lon" Decode.float)



-- ROUTE RESPONSE


decodeRouteResponse : Decoder RouteResponse
decodeRouteResponse =
    Decode.map5 RouteResponse
        (Decode.field "path" (Decode.list decodeCoordinate))
        (Decode.field "distance_km" Decode.float)
        (Decode.field "gpx_base64" Decode.string)
        (Decode.maybe (Decode.field "metadata" decodeRouteMetadata))
        (Decode.maybe (Decode.field "elevation_profile" decodeElevationProfile))


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
