port module Ports exposing (..)

{-| Module de ports pour la communication Elm <-> JavaScript (MapLibre + Three.js).
Approche fonctionnelle pure : les ports sont des effets controles.
-}

import Json.Encode as Encode
import Types exposing (Coordinate, RouteBounds)


-- PORTS OUT (Elm -> JavaScript) — MapLibre map


port initMap : () -> Cmd msg


port updateRoute : List Coordinate -> Cmd msg


{-| Revêtement, provenance IGN et altitude de chaque point du tracé affiché.
`elevations = Nothing` garde les altitudes déjà connues (survol 3D).
-}
port setRouteSurfaces :
    { surfaces : Maybe (List Bool)
    , ign : Maybe (List Bool)
    , elevations : Maybe (List (Maybe Float))
    }
    -> Cmd msg


-- Tronçons tirés tout droit (en pointillé) et ce qu'ils traversent.
port setDirectOverlay :
    { lines : List (List Coordinate)
    , obstacles : List { lat : Float, lon : Float, kind : String }
    }
    -> Cmd msg


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


port setReferenceRoutes : List { path : List Coordinate, color : String } -> Cmd msg


port centerMapOn : { lat : Float, lon : Float } -> Cmd msg


-- PORTS IN (JavaScript -> Elm) — MapLibre events


port mapClickReceived : ({ lat : Float, lon : Float } -> msg) -> Sub msg


{-| Clic tombé sur le tracé lui-même et non sur la carte nue.

Le JS fait la distinction parce que lui seul sait où la ligne est dessinée à
l'écran : la même coordonnée est sur le tracé ou à côté selon le zoom.
-}
port routeLineClicked : ({ lat : Float, lon : Float } -> msg) -> Sub msg


{-| Transition de vue sur le changement d'onglet.

Le va-et-vient par le JavaScript n'est pas une complication gratuite :
`document.startViewTransition()` veut muter le DOM *dans* sa fonction de rappel,
or Elm ne repeint qu'au `requestAnimationFrame` suivant. Si Elm changeait le
modèle lui-même, la transition photographierait deux fois le même écran.

Donc `demanderTransitionOnglet` ne touche pas au modèle : il demande au JS
d'ouvrir la transition, le JS renvoie `transitionOngletPrete` depuis l'intérieur
du rappel, et c'est seulement là que le modèle bascule. `sens` vaut `"avant"` ou
`"arriere"` et sert à choisir le côté d'où glisse le nouveau contenu.

Si le navigateur ignore l'API, ou si le visiteur refuse les animations, le JS
rappelle immédiatement : l'onglet change sans transition, jamais rien ne bloque.
-}
port demanderTransitionOnglet : { vers : String, sens : String } -> Cmd msg


port transitionOngletPrete : (String -> msg) -> Sub msg


port waypointDragged : ({ index : Int, lat : Float, lon : Float } -> msg) -> Sub msg


port waypointDeleted : ({ index : Int } -> msg) -> Sub msg


port routeLoadedFromLocalStorage : (Encode.Value -> msg) -> Sub msg


port gotGeolocation : ({ lat : Float, lon : Float } -> msg) -> Sub msg


port elevationChartHover : ({ index : Int } -> msg) -> Sub msg


port undoRedoReceived : ({ action : String } -> msg) -> Sub msg


port gpxWaypointsReceived : (List { lat : Float, lon : Float } -> msg) -> Sub msg


port mapRouteHover : ({ index : Int } -> msg) -> Sub msg


port closeLoopRequested : (Bool -> msg) -> Sub msg
