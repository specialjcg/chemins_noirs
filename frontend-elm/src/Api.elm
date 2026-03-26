module Api exposing (..)

{-| Module de communication HTTP avec le backend Rust.
Toutes les fonctions retournent des Cmd Msg (effets purs).
-}

import Decoders exposing (..)
import Encoders exposing (..)
import Http
import Json.Encode
import Types exposing (..)
import Url


-- API ROOTS


apiRoot : String
apiRoot =
    "/api/route"


loopApiRoot : String
loopApiRoot =
    "/api/loops"


savedRoutesApiRoot : String
savedRoutesApiRoot =
    "/api/routes"



-- GEOCODING (Nominatim OSM)


geocodeAddress : String -> (Result Http.Error (List GeoResult) -> msg) -> Cmd msg
geocodeAddress query toMsg =
    Http.get
        { url =
            "https://nominatim.openstreetmap.org/search?q="
                ++ Url.percentEncode query
                ++ "&format=json&limit=5&countrycodes=fr"
        , expect = Http.expectJson toMsg decodeGeoResults
        }



-- FETCH POINT-TO-POINT ROUTE


fetchRoute : RouteRequest -> (Result Http.Error RouteResponse -> msg) -> Cmd msg
fetchRoute request toMsg =
    Http.post
        { url = apiRoot
        , body = Http.jsonBody (encodeRouteRequest request)
        , expect = Http.expectJson toMsg decodeRouteResponse
        }



-- FETCH LOOP ROUTES


fetchLoopRoute : LoopRouteRequest -> (Result Http.Error LoopRouteResponse -> msg) -> Cmd msg
fetchLoopRoute request toMsg =
    Http.post
        { url = loopApiRoot
        , body = Http.jsonBody (encodeLoopRouteRequest request)
        , expect = Http.expectJson toMsg decodeLoopRouteResponse
        }



-- FETCH MULTI-POINT ROUTE


fetchMultiPointRoute : MultiPointRouteRequest -> (Result Http.Error RouteResponse -> msg) -> Cmd msg
fetchMultiPointRoute request toMsg =
    Http.post
        { url = apiRoot ++ "/multi"
        , body = Http.jsonBody (encodeMultiPointRouteRequest request)
        , expect = Http.expectJson toMsg decodeRouteResponse
        }



-- ROADS (for game 3D view)


fetchRoads : Coordinate -> Float -> (Result Http.Error (List StyledRoad) -> msg) -> Cmd msg
fetchRoads center marginDeg toMsg =
    Http.post
        { url = "/api/roads"
        , body =
            Http.jsonBody
                (Encoders.encodeRoadsRequest center marginDeg)
        , expect = Http.expectJson toMsg Decoders.decodeRoadsResponse
        }


{-| Fetch IGN BD TOPO road geometries (matches IGN topo tiles perfectly).
Used in game mode for movement that follows the visible map.
-}
fetchIgnRoads : Coordinate -> Float -> (Result Http.Error (List StyledRoad) -> msg) -> Cmd msg
fetchIgnRoads center marginDeg toMsg =
    Http.post
        { url = "/api/ign-roads"
        , body =
            Http.jsonBody
                (Encoders.encodeRoadsRequest center marginDeg)
        , expect = Http.expectJson toMsg Decoders.decodeRoadsResponse
        }



fetchIgnVegetation : Coordinate -> Float -> (Result Http.Error (List VegetationZone) -> msg) -> Cmd msg
fetchIgnVegetation center marginDeg toMsg =
    Http.post
        { url = "/api/ign-vegetation"
        , body =
            Http.jsonBody
                (Encoders.encodeRoadsRequest center marginDeg)
        , expect = Http.expectJson toMsg Decoders.decodeVegetationResponse
        }


fetchIgnBuildings : Coordinate -> Float -> (Result Http.Error (List IgnBuilding) -> msg) -> Cmd msg
fetchIgnBuildings center marginDeg toMsg =
    Http.post
        { url = "/api/ign-buildings"
        , body =
            Http.jsonBody
                (Encoders.encodeRoadsRequest center marginDeg)
        , expect = Http.expectJson toMsg Decoders.decodeIgnBuildingsResponse
        }


fetchElevationGrid : Coordinate -> Float -> Int -> (Result Http.Error ElevationGrid -> msg) -> Cmd msg
fetchElevationGrid center sizeM resolution toMsg =
    Http.post
        { url = "/api/elevation-grid"
        , body =
            Http.jsonBody
                (Json.Encode.object
                    [ ( "center_lat", Json.Encode.float center.lat )
                    , ( "center_lon", Json.Encode.float center.lon )
                    , ( "size_m", Json.Encode.float sizeM )
                    , ( "resolution", Json.Encode.int resolution )
                    ]
                )
        , expect = Http.expectJson toMsg Decoders.decodeElevationGrid
        }


fetchBuildings : Coordinate -> Float -> (Result Http.Error (List { center : Coordinate, polygon : List Coordinate }) -> msg) -> Cmd msg
fetchBuildings center marginDeg toMsg =
    Http.post
        { url = "/api/buildings"
        , body =
            Http.jsonBody
                (Encoders.encodeRoadsRequest center marginDeg)
        , expect = Http.expectJson toMsg Decoders.decodeBuildingsResponse
        }



{-| Send a debug log message to the backend (written to frontend_debug.log).
Fire-and-forget — response is ignored.
-}
sendLog : String -> (Result Http.Error () -> msg) -> Cmd msg
sendLog msg toMsg =
    Http.post
        { url = "/api/log"
        , body = Http.jsonBody (Json.Encode.object [ ( "msg", Json.Encode.string msg ) ])
        , expect = Http.expectWhatever toMsg
        }



-- SAVED ROUTES (PostgreSQL)


saveRouteToDb : SaveRouteRequest -> RouteResponse -> (Result Http.Error SavedRoute -> msg) -> Cmd msg
saveRouteToDb request route toMsg =
    Http.post
        { url = savedRoutesApiRoot
        , body = Http.jsonBody (encodeSaveRouteRequest request route)
        , expect = Http.expectJson toMsg decodeSavedRoute
        }


listSavedRoutes : (Result Http.Error (List SavedRoute) -> msg) -> Cmd msg
listSavedRoutes toMsg =
    Http.get
        { url = savedRoutesApiRoot
        , expect = Http.expectJson toMsg decodeSavedRoutesList
        }


getSavedRoute : Int -> (Result Http.Error SavedRoute -> msg) -> Cmd msg
getSavedRoute id toMsg =
    Http.get
        { url = savedRoutesApiRoot ++ "/" ++ String.fromInt id
        , expect = Http.expectJson toMsg decodeSavedRoute
        }


deleteSavedRoute : Int -> (Result Http.Error () -> msg) -> Cmd msg
deleteSavedRoute id toMsg =
    Http.request
        { method = "DELETE"
        , headers = []
        , url = savedRoutesApiRoot ++ "/" ++ String.fromInt id
        , body = Http.emptyBody
        , expect = Http.expectWhatever toMsg
        , timeout = Nothing
        , tracker = Nothing
        }


toggleFavorite : Int -> (Result Http.Error SavedRoute -> msg) -> Cmd msg
toggleFavorite id toMsg =
    Http.post
        { url = savedRoutesApiRoot ++ "/" ++ String.fromInt id ++ "/favorite"
        , body = Http.emptyBody
        , expect = Http.expectJson toMsg decodeSavedRoute
        }

