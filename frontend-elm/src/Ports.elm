port module Ports exposing (..)

{-| Module de ports pour la communication Elm ↔ JavaScript (MapLibre).
Approche fonctionnelle pure : les ports sont des effets contrôlés.
-}

import Json.Encode as Encode
import Types exposing (Coordinate, RouteBounds)


-- PORTS OUT (Elm → JavaScript)


port initMap : () -> Cmd msg


port updateRoute : List Coordinate -> Cmd msg


port updateSelectionMarkers :
    { start : Maybe Coordinate
    , end : Maybe Coordinate
    }
    -> Cmd msg


port updateWaypointMarkers : List Coordinate -> Cmd msg


port toggleSatelliteView : Bool -> Cmd msg


port toggleThree3DView : Bool -> Cmd msg


port updateBbox : RouteBounds -> Cmd msg


port centerOnMarkers :
    { start : Coordinate
    , end : Coordinate
    }
    -> Cmd msg


port startAnimation : () -> Cmd msg


port stopAnimation : () -> Cmd msg


port saveRouteToLocalStorage : Encode.Value -> Cmd msg


port loadRouteFromLocalStorage : () -> Cmd msg



-- PORTS IN (JavaScript → Elm)


port mapClickReceived : ({ lat : Float, lon : Float } -> msg) -> Sub msg


port routeLoadedFromLocalStorage : (Encode.Value -> msg) -> Sub msg
