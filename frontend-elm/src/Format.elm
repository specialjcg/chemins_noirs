module Format exposing (cap, coord, deltaKm, km, metres, pourcent)

{-| Écriture en français des grandeurs affichées.

Trois modules de vue formataient leurs kilomètres chacun pour soi — deux
`formatKm` identiques et un `formatDistance` qui arrondissait au centième —
et les autres grandeurs pas du tout : `String.fromFloat` livrait le flottant
du backend tel quel. « 7.016201634123582 km » après un import GPX,
« 409.29999999999995 m » de dénivelé pour une mesure que le relief ne donne
pas au millimètre.

Les règles tiennent en trois lignes : virgule décimale, décimale toujours
écrite même nulle (dans une colonne en chasse fixe, « 29 » et « 30,1 » ne
tombent pas au même endroit), et chaque grandeur arrondie à la précision que
la mesure possède vraiment — le dixième de kilomètre, le mètre, le degré.

@docs km, deltaKm, metres, pourcent, coord, cap

-}


{-| Distance au dixième de kilomètre : « 8,6 km ».
-}
km : Float -> String
km valeur =
    decimales 1 valeur ++ " km"


{-| Écart à une cible, signe toujours visible : « +0,3 km », « -1,2 km ».

Le signe se décide sur la valeur arrondie, pas sur la brute : un écart de
quatre mètres s'écrit « 0,0 km » et non « +0,0 km ».

-}
deltaKm : Float -> String
deltaKm valeur =
    (if round (valeur * 10) > 0 then
        "+"

     else
        ""
    )
        ++ km valeur


{-| Altitude ou dénivelé, au mètre : « 409 m ».
-}
metres : Float -> String
metres valeur =
    String.fromInt (round valeur) ++ " m"


{-| Pente au dixième : « 4,2 % ».
-}
pourcent : Float -> String
pourcent valeur =
    decimales 1 valeur ++ " %"


{-| Latitude ou longitude : « 44,28845 ».

Cinq décimales valent un peu plus d'un mètre de latitude — la précision d'un
relevé GPS de poignet, et la limite au-delà de laquelle les chiffres ne
décrivent plus rien.

-}
coord : Float -> String
coord valeur =
    decimales 5 valeur


{-| Cap, au degré : « 137° ».
-}
cap : Float -> String
cap valeur =
    String.fromInt (round valeur) ++ "°"



-- INTERNE


{-| Nombre à virgule fixe, à `chiffres` décimales.

Le signe est sorti avant l'arrondi parce que `//` tronque vers zéro quand
`modBy` reste positif : sur -1,25, le couple donnerait « -1,7 ». Les écarts
de distance, les pentes en descente et les longitudes à l'ouest de Greenwich
sont tous négatifs, le cas n'est pas théorique.

-}
decimales : Int -> Float -> String
decimales chiffres valeur =
    let
        facteur =
            puissanceDix chiffres

        unites =
            round (abs valeur * toFloat facteur)

        signe =
            -- Pas de « -0,0 » : à l'affichage, cette valeur-là est nulle.
            if valeur < 0 && unites /= 0 then
                "-"

            else
                ""
    in
    signe
        ++ String.fromInt (unites // facteur)
        ++ ","
        ++ String.padLeft chiffres '0' (String.fromInt (modBy facteur unites))


puissanceDix : Int -> Int
puissanceDix chiffres =
    List.foldl (\_ acc -> acc * 10) 1 (List.repeat chiffres ())
