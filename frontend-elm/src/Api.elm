module Api exposing (..)

{-| Module de communication HTTP avec le backend Rust.
Toutes les fonctions retournent des Cmd Msg (effets purs).
-}

import Decoders exposing (..)
import Encoders exposing (..)
import Http
import Types exposing (..)


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

