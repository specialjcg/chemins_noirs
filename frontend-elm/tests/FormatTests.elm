module FormatTests exposing (suite)

{-| Écriture des grandeurs affichées.

Chaque bloc fige un défaut qui s'est réellement produit : le flottant brut du
backend dans le compteur de points, et l'arrondi signé qui faisait écrire
« -1,7 » pour -1,25 — les écarts de distance, les pentes en descente et les
longitudes à l'ouest de Greenwich sont tous négatifs.
-}

import Expect
import Format
import Test exposing (..)


suite : Test
suite =
    describe "Format"
        [ describe "km"
            [ test "arrondit le flottant du backend au dixième" <|
                \_ ->
                    -- La valeur exacte relevée après un import GPX.
                    Format.km 7.016201634123582
                        |> Expect.equal "7,0 km"
            , test "écrit la décimale même nulle" <|
                \_ ->
                    Format.km 29
                        |> Expect.equal "29,0 km"
            , test "arrondit au plus proche, pas par troncature" <|
                \_ ->
                    Format.km 8.579
                        |> Expect.equal "8,6 km"
            , test "la retenue passe sur l'unité" <|
                \_ ->
                    Format.km 8.97
                        |> Expect.equal "9,0 km"
            , test "une distance négative garde son signe" <|
                \_ ->
                    Format.km -1.25
                        |> Expect.equal "-1,3 km"
            , test "pas de moins zéro" <|
                \_ ->
                    Format.km -0.02
                        |> Expect.equal "0,0 km"
            ]
        , describe "deltaKm"
            [ test "un écart positif montre son signe" <|
                \_ ->
                    Format.deltaKm 0.34
                        |> Expect.equal "+0,3 km"
            , test "un écart négatif garde le sien" <|
                \_ ->
                    Format.deltaKm -0.34
                        |> Expect.equal "-0,3 km"
            , test "un écart nul à l'affichage n'est ni positif ni négatif" <|
                \_ ->
                    -- Quatre mètres d'écart : « +0,0 km » annoncerait un
                    -- dépassement que la valeur affichée ne montre pas.
                    Format.deltaKm 0.004
                        |> Expect.equal "0,0 km"
            ]
        , describe "metres"
            [ test "arrondit au mètre, sans décimale" <|
                \_ ->
                    Format.metres 409.29999999999995
                        |> Expect.equal "409 m"
            , test "un dénivelé négatif garde son signe" <|
                \_ ->
                    Format.metres -12.4
                        |> Expect.equal "-12 m"
            ]
        , describe "pourcent"
            [ test "une pente s'écrit au dixième" <|
                \_ ->
                    Format.pourcent 4.23
                        |> Expect.equal "4,2 %"
            , test "une pente descendante garde son signe" <|
                \_ ->
                    Format.pourcent -7.85
                        |> Expect.equal "-7,9 %"
            ]
        , describe "coord"
            [ test "cinq décimales, complétées par des zéros" <|
                \_ ->
                    -- Le zéro du milieu doit survivre : « 44,2884 » désignerait
                    -- un point à onze mètres de là.
                    Format.coord 44.2884521
                        |> Expect.equal "44,28845"
            , test "complète une partie décimale plus courte" <|
                \_ ->
                    Format.coord 5.4
                        |> Expect.equal "5,40000"
            , test "une longitude à l'ouest reste négative" <|
                \_ ->
                    Format.coord -1.6777926
                        |> Expect.equal "-1,67779"
            , test "les zéros de tête de la partie décimale tiennent" <|
                \_ ->
                    Format.coord 2.00042
                        |> Expect.equal "2,00042"
            ]
        , describe "cap"
            [ test "un cap s'écrit au degré" <|
                \_ ->
                    Format.cap 137.6
                        |> Expect.equal "138°"
            ]
        ]
