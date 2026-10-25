module Stages exposing (Stage, gpx, route, slice, split)

{-| Découpage d'un long tracé en étapes d'une longueur donnée.

Une étape est une plage d'indices du tracé. Le point de coupe appartient aux
deux étapes qu'il sépare : la fin de l'une est le départ de la suivante, sans
trou entre deux GPX.

-}

import Array exposing (Array)
import Types exposing (Coordinate, ElevationProfile, RouteResponse, haversineMeters)


type alias Stage =
    { first : Int
    , last : Int
    , distanceKm : Float
    }


{-| Coupe dès que la distance cumulée depuis le début de l'étape atteint
`stageKm`. Une longueur nulle ou négative donne une seule étape.
-}
split : Float -> List Coordinate -> List Stage
split stageKm path =
    case path of
        first :: rest ->
            let
                step coord ( prev, ( index, current, done ) ) =
                    let
                        km =
                            current.distanceKm + haversineMeters prev coord / 1000

                        reached =
                            { current | last = index, distanceKm = km }
                    in
                    if stageKm > 0 && km >= stageKm then
                        ( coord, ( index + 1, { first = index, last = index, distanceKm = 0 }, reached :: done ) )

                    else
                        ( coord, ( index + 1, reached, done ) )

                ( _, ( _, open, closed ) ) =
                    List.foldl step ( first, ( 1, { first = 0, last = 0, distanceKm = 0 }, [] ) ) rest
            in
            if List.isEmpty rest then
                []

            else if open.last == open.first then
                List.reverse closed

            else
                List.reverse (open :: closed)

        [] ->
            []


{-| Les éléments d'une liste compris dans l'étape, bornes incluses. -}
slice : Stage -> List a -> List a
slice stage items =
    items
        |> List.drop stage.first
        |> List.take (stage.last - stage.first + 1)


{-| Le tracé réduit à l'étape, prêt à enregistrer : profil d'altitude et
marquage surface/IGN découpés au même endroit, dénivelés recalculés. Les
statistiques propres au tracé entier (segments, obstacles, métadonnées) ne
valent plus pour un tronçon et sont retirées.
-}
route : Stage -> RouteResponse -> RouteResponse
route stage full =
    { path = slice stage full.path
    , distanceKm = stage.distanceKm
    , gpxBase64 = ""
    , metadata = Nothing
    , elevationProfile = Maybe.map (profile << slice stage << .elevations) full.elevationProfile
    , snappedWaypoints = Nothing
    , estimatedTimeMinutes = Nothing
    , difficulty = Nothing
    , surfaceBreakdown = Nothing
    , segments = Nothing
    , pointSurfaces = Maybe.map (slice stage) full.pointSurfaces
    , pointIgn = Maybe.map (slice stage) full.pointIgn
    , obstacles = []
    }


profile : List (Maybe Float) -> ElevationProfile
profile elevations =
    let
        known =
            List.filterMap identity elevations

        diffs =
            List.map2 (-) (List.drop 1 known) known
    in
    { elevations = elevations
    , minElevation = List.minimum known
    , maxElevation = List.maximum known
    , totalAscent = diffs |> List.filter (\d -> d > 0) |> List.sum
    , totalDescent = diffs |> List.filter (\d -> d < 0) |> List.sum |> negate
    }


{-| Trace GPX 1.1 ; une altitude manquante n'écrit pas de `<ele>`. -}
gpx : String -> List Coordinate -> List (Maybe Float) -> String
gpx name path elevations =
    let
        eles : Array (Maybe Float)
        eles =
            Array.fromList elevations

        point i coord =
            "      <trkpt lat=\""
                ++ String.fromFloat coord.lat
                ++ "\" lon=\""
                ++ String.fromFloat coord.lon
                ++ "\">\n"
                ++ (case Array.get i eles |> Maybe.andThen identity of
                        Just e ->
                            "        <ele>" ++ String.fromFloat e ++ "</ele>\n"

                        Nothing ->
                            ""
                   )
                ++ "      </trkpt>\n"
    in
    "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n"
        ++ "<gpx version=\"1.1\" creator=\"Chemins Noirs\"\n"
        ++ "  xmlns=\"http://www.topografix.com/GPX/1/1\">\n"
        ++ "  <trk>\n"
        ++ "    <name>"
        ++ name
        ++ "</name>\n"
        ++ "    <trkseg>\n"
        ++ String.concat (List.indexedMap point path)
        ++ "    </trkseg>\n"
        ++ "  </trk>\n"
        ++ "</gpx>\n"
