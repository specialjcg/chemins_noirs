module Api exposing (..)

{-| Module de communication HTTP avec le backend Rust.
Toutes les fonctions retournent des Cmd Msg (effets purs).
-}

import Decoders exposing (..)
import Encoders exposing (..)
import Http
import Json.Decode
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


{-| Chaque déplacement de point relance le calcul : la requête précédente est
abandonnée, sa réponse ne serait jamais affichée. Le backend, voyant arriver
la nouvelle, lâche aussi la construction de graphe qu'il menait pour elle.

Elm exécute les commandes d'un batch à rebours : `Http.cancel` vient donc
après la requête dans la liste, pour tuer l'ancienne et non la nouvelle.

-}
fetchMultiPointRoute : MultiPointRouteRequest -> (Result Http.Error RouteResponse -> msg) -> Cmd msg
fetchMultiPointRoute request toMsg =
    Cmd.batch
        [ Http.request
            { method = "POST"
            , headers = []
            , url = apiRoot ++ "/multi"
            , body = Http.jsonBody (encodeMultiPointRouteRequest request)
            , expect = Http.expectJson toMsg decodeRouteResponse
            , timeout = Nothing
            , tracker = Just multiPointRouteTracker
            }
        , Http.cancel multiPointRouteTracker
        ]


multiPointRouteTracker : String
multiPointRouteTracker =
    "multi-point-route"



{-| Revêtement d'un tracé déjà calculé, point par point.

Un itinéraire relu depuis la base ne porte que ses coordonnées : la carte le
peint alors d'un seul vert, même quand il est non goudronné presque partout.
Cet appel récupère l'information sans retoucher le tracé.
-}
fetchSurfaces : List Coordinate -> (Result Http.Error ( List Bool, Maybe (List Bool) ) -> msg) -> Cmd msg
fetchSurfaces path toMsg =
    Http.post
        { url = "/api/surfaces"
        , body =
            Http.jsonBody
                (Json.Encode.object
                    [ ( "path", Json.Encode.list encodeCoordinate path ) ]
                )
        , expect =
            Http.expectJson toMsg
                (Json.Decode.map2 Tuple.pair
                    (Json.Decode.field "point_surfaces" (Json.Decode.list Json.Decode.bool))
                    (Json.Decode.maybe (Json.Decode.field "point_ign" (Json.Decode.list Json.Decode.bool)))
                )
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


{-| Remplacer un itinéraire déjà enregistré, au lieu d'en créer un autre.

Sert à reprendre un itinéraire chargé dont on a déplacé les points. Le corps
est celui de `saveRouteToDb` ; seule la méthode et l'URL changent.
-}
updateSavedRoute : Int -> SaveRouteRequest -> RouteResponse -> (Result Http.Error SavedRoute -> msg) -> Cmd msg
updateSavedRoute id request route toMsg =
    Http.request
        { method = "PUT"
        , headers = []
        , url = savedRoutesApiRoot ++ "/" ++ String.fromInt id
        , body = Http.jsonBody (encodeSaveRouteRequest request route)
        , expect = Http.expectJson toMsg decodeSavedRoute
        , timeout = Nothing
        , tracker = Nothing
        }


{-| Ranger un itinéraire dans un dossier, `Nothing` le sortant de tout dossier.

Ne renvoie que le dossier : passer par `updateSavedRoute` réécrirait tout le
tracé pour changer une étiquette.
-}
setRouteFolder : Int -> Maybe String -> (Result Http.Error SavedRoute -> msg) -> Cmd msg
setRouteFolder id folder toMsg =
    Http.request
        { method = "PATCH"
        , headers = []
        , url = savedRoutesApiRoot ++ "/" ++ String.fromInt id ++ "/folder"
        , body =
            Http.jsonBody
                (Json.Encode.object
                    [ ( "folder"
                      , Maybe.map Json.Encode.string folder
                            |> Maybe.withDefault Json.Encode.null
                      )
                    ]
                )
        , expect = Http.expectJson toMsg decodeSavedRoute
        , timeout = Nothing
        , tracker = Nothing
        }


{-| Forêts publiques dont le nom contient `query`, les plus grandes d'abord.
-}
searchForests : String -> (Result Http.Error (List Forest) -> msg) -> Cmd msg
searchForests query toMsg =
    Http.get
        { url = "/api/forests?q=" ++ Url.percentEncode query
        , expect = Http.expectJson toMsg (Json.Decode.list decodeForest)
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

