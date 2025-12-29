module Encoders exposing (..)

{-| Module d'encodage JSON pour les requêtes vers le backend Rust.
Approche fonctionnelle pure.
-}

import Json.Encode as Encode
import Types exposing (..)


-- COORDINATE


encodeCoordinate : Coordinate -> Encode.Value
encodeCoordinate coord =
    Encode.object
        [ ( "lat", Encode.float coord.lat )
        , ( "lon", Encode.float coord.lon )
        ]



-- ROUTE REQUEST


encodeRouteRequest : RouteRequest -> Encode.Value
encodeRouteRequest req =
    Encode.object
        [ ( "start", encodeCoordinate req.start )
        , ( "end", encodeCoordinate req.end )
        , ( "w_pop", Encode.float req.wPop )
        , ( "w_paved", Encode.float req.wPaved )
        ]



-- MULTI-POINT ROUTE REQUEST


encodeMultiPointRouteRequest : MultiPointRouteRequest -> Encode.Value
encodeMultiPointRouteRequest req =
    Encode.object
        [ ( "waypoints", Encode.list encodeCoordinate req.waypoints )
        , ( "close_loop", Encode.bool req.closeLoop )
        , ( "w_pop", Encode.float req.wPop )
        , ( "w_paved", Encode.float req.wPaved )
        ]



-- LOOP ROUTE REQUEST


encodeLoopRouteRequest : LoopRouteRequest -> Encode.Value
encodeLoopRouteRequest req =
    Encode.object
        [ ( "start", encodeCoordinate req.start )
        , ( "target_distance_km", Encode.float req.targetDistanceKm )
        , ( "distance_tolerance_km", Encode.float req.distanceToleranceKm )
        , ( "candidate_count", Encode.int req.candidateCount )
        , ( "w_pop", Encode.float req.wPop )
        , ( "w_paved", Encode.float req.wPaved )
        , ( "max_total_ascent", encodeMaybe Encode.float req.maxTotalAscent )
        , ( "min_total_ascent", encodeMaybe Encode.float req.minTotalAscent )
        ]



-- ROUTE RESPONSE (for localStorage save/load)


encodeRouteBounds : RouteBounds -> Encode.Value
encodeRouteBounds bounds =
    Encode.object
        [ ( "min_lat", Encode.float bounds.minLat )
        , ( "max_lat", Encode.float bounds.maxLat )
        , ( "min_lon", Encode.float bounds.minLon )
        , ( "max_lon", Encode.float bounds.maxLon )
        ]


encodeRouteMetadata : RouteMetadata -> Encode.Value
encodeRouteMetadata metadata =
    Encode.object
        [ ( "point_count", Encode.int metadata.pointCount )
        , ( "bounds", encodeRouteBounds metadata.bounds )
        , ( "start", encodeCoordinate metadata.start )
        , ( "end", encodeCoordinate metadata.end )
        ]


encodeElevationProfile : ElevationProfile -> Encode.Value
encodeElevationProfile profile =
    Encode.object
        [ ( "elevations", Encode.list (encodeMaybe Encode.float) profile.elevations )
        , ( "min_elevation", encodeMaybe Encode.float profile.minElevation )
        , ( "max_elevation", encodeMaybe Encode.float profile.maxElevation )
        , ( "total_ascent", Encode.float profile.totalAscent )
        , ( "total_descent", Encode.float profile.totalDescent )
        ]


encodeRouteResponse : RouteResponse -> Encode.Value
encodeRouteResponse response =
    Encode.object
        [ ( "path", Encode.list encodeCoordinate response.path )
        , ( "distance_km", Encode.float response.distanceKm )
        , ( "gpx_base64", Encode.string response.gpxBase64 )
        , ( "metadata", encodeMaybe encodeRouteMetadata response.metadata )
        , ( "elevation_profile", encodeMaybe encodeElevationProfile response.elevationProfile )
        ]



-- SAVE ROUTE REQUEST (for PostgreSQL)


encodeSaveRouteRequest : SaveRouteRequest -> RouteResponse -> Encode.Value
encodeSaveRouteRequest req route =
    Encode.list identity
        [ Encode.object
            [ ( "name", Encode.string req.name )
            , ( "description"
              , case req.description of
                    Just desc ->
                        Encode.string desc

                    Nothing ->
                        Encode.null
              )
            , ( "tags"
              , case req.tags of
                    Just tagList ->
                        Encode.list Encode.string tagList

                    Nothing ->
                        Encode.null
              )
            , ( "original_waypoints"
              , case req.originalWaypoints of
                    Just waypoints ->
                        Encode.list encodeCoordinate waypoints

                    Nothing ->
                        Encode.null
              )
            ]
        , encodeRouteResponse route
        ]



-- HELPERS


encodeMaybe : (a -> Encode.Value) -> Maybe a -> Encode.Value
encodeMaybe encoder maybeValue =
    case maybeValue of
        Just value ->
            encoder value

        Nothing ->
            Encode.null
