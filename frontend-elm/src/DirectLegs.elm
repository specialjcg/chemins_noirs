module DirectLegs exposing (afterInsert, afterRemove, flags, legCount, lines, toggle)

{-| Tronçons tirés en ligne droite, repérés par leur index : le tronçon `i`
va du point `i` au point `i + 1`, et le dernier, en boucle, revient au départ.

Poser ou retirer un point décale la numérotation. Sans ces fonctions, le
tronçon marqué direct glisserait sur son voisin à chaque clic.

-}

import Set exposing (Set)
import Types exposing (Coordinate, RouteResponse)


{-| Nombre de tronçons d'un tracé à `points` points.
-}
legCount : Int -> Bool -> Int
legCount points closeLoop =
    if points < 2 then
        0

    else if closeLoop then
        points

    else
        points - 1


toggle : Int -> Set Int -> Set Int
toggle leg legs =
    if Set.member leg legs then
        Set.remove leg legs

    else
        Set.insert leg legs


{-| Un point posé à l'index `idx` coupe en deux le tronçon qui y passait :
les deux moitiés en gardent le mode. Les suivants reculent d'un cran.
-}
afterInsert : Int -> Set Int -> Set Int
afterInsert idx legs =
    Set.foldl
        (\leg acc ->
            if leg < idx - 1 then
                Set.insert leg acc

            else if leg == idx - 1 then
                acc |> Set.insert leg |> Set.insert idx

            else
                Set.insert (leg + 1) acc
        )
        Set.empty
        legs


{-| Retirer le point `idx` fond ses deux tronçons en un seul, direct s'ils
l'étaient tous les deux : un bout de chemin dans le lot, et la ligne droite
couperait ce que le marcheur avait choisi de suivre.
-}
afterRemove : Int -> Set Int -> Set Int
afterRemove idx legs =
    let
        merged =
            idx > 0 && Set.member (idx - 1) legs && Set.member idx legs
    in
    Set.foldl
        (\leg acc ->
            if leg < idx - 1 then
                Set.insert leg acc

            else if leg > idx then
                Set.insert (leg - 1) acc

            else
                acc
        )
        Set.empty
        legs
        |> (if merged then
                Set.insert (idx - 1)

            else
                identity
           )


{-| Un booléen par tronçon, dans l'ordre, pour la requête.
-}
flags : Int -> Set Int -> List Bool
flags count legs =
    List.map (\leg -> Set.member leg legs) (List.range 0 (count - 1))


{-| Les morceaux du tracé tirés tout droit, pour les dessiner à part.

Le backend ne découpe le tracé en étapes qu'à partir de deux tronçons : seul,
le tronçon direct est le tracé entier.

-}
lines : Set Int -> RouteResponse -> List (List Coordinate)
lines legs route =
    case route.segments of
        Just segments ->
            List.indexedMap
                (\leg segment ->
                    if Set.member leg legs then
                        Just
                            (route.path
                                |> List.drop segment.fromIndex
                                |> List.take (segment.toIndex - segment.fromIndex + 1)
                            )

                    else
                        Nothing
                )
                segments
                |> List.filterMap identity

        Nothing ->
            if Set.member 0 legs then
                [ route.path ]

            else
                []
