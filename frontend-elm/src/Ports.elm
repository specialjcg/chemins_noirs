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


port switchMapStyle : String -> Cmd msg


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


port downloadGpx : { filename : String, content : String } -> Cmd msg


port copyToClipboard : String -> Cmd msg


port requestGeolocation : () -> Cmd msg


port triggerGpxImport : () -> Cmd msg


port setElevationHoverMarker : Maybe { lat : Float, lon : Float } -> Cmd msg


port centerMapOn : { lat : Float, lon : Float } -> Cmd msg



-- PORTS IN (JavaScript → Elm)


port mapClickReceived : ({ lat : Float, lon : Float } -> msg) -> Sub msg


port waypointDragged : ({ index : Int, lat : Float, lon : Float } -> msg) -> Sub msg


port waypointDeleted : ({ index : Int } -> msg) -> Sub msg


port routeLoadedFromLocalStorage : (Encode.Value -> msg) -> Sub msg


port gotGeolocation : ({ lat : Float, lon : Float } -> msg) -> Sub msg


port elevationChartHover : ({ index : Int } -> msg) -> Sub msg


port undoRedoReceived : ({ action : String } -> msg) -> Sub msg


port gpxWaypointsReceived : (List { lat : Float, lon : Float } -> msg) -> Sub msg


port mapRouteHover : ({ index : Int } -> msg) -> Sub msg


port closeLoopRequested : (Bool -> msg) -> Sub msg
