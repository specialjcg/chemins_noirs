module View.Form exposing (view, viewSavedRoutesSection)

{-| Module de formulaire - Interface utilisateur pour la saisie des paramètres.
Approche fonctionnelle pure : génération déclarative du HTML.
-}

import Format
import Html exposing (..)
import Html.Attributes exposing (..)
import Html.Events exposing (on, onCheck, onClick, onInput, onMouseDown, preventDefaultOn)
import Json.Decode as Decode
import Dict
import DirectLegs
import Set
import Types exposing (..)


{-| Texte du bandeau d'attente, fonction du temps déjà écoulé.

Un graphe déjà en cache répond en une poignée de secondes ; s'il faut le
construire depuis le PBF, il faut compter deux à quatre minutes. Le message
annonçait 15 s dans tous les cas, et l'attente paraissait donc bloquée passé
ce délai — au point qu'on recharge la page avant la réponse.
-}
waitMessage : Int -> String
waitMessage seconds =
    if seconds < 15 then
        "Calcul du tracé…"

    else if seconds < 45 then
        "Calcul du tracé… " ++ String.fromInt seconds ++ " s"

    else
        "Première carte de cette zone : construction du graphe depuis les données OSM, "
            ++ String.fromInt (seconds // 60)
            ++ " min "
            ++ String.fromInt (modBy 60 seconds)
            ++ " s. Compter 2 à 4 min — ne pas recharger la page."


view : Model -> Html Msg
view model =
    Html.form [ class "controls" ]
        [ fieldset []
            [ legend [] [ text "Type de tracé" ]
            , div [ class "route-type" ]
                [ label []
                    [ input
                        [ type_ "radio"
                        , name "route-mode"
                        , checked (model.routeMode == PointToPoint)
                        , onClick (ToggleRouteMode PointToPoint)
                        ]
                        []
                    , span [] [ text "Aller simple" ]
                    ]
                , label []
                    [ input
                        [ type_ "radio"
                        , name "route-mode"
                        , checked (model.routeMode == Loop)
                        , onClick (ToggleRouteMode Loop)
                        ]
                        []
                    , span [] [ text "Boucle" ]
                    ]
                , label []
                    [ input
                        [ type_ "radio"
                        , name "route-mode"
                        , checked (model.routeMode == MultiPoint)
                        , onClick (ToggleRouteMode MultiPoint)
                        ]
                        []
                    , span [] [ text "Multi-points" ]
                    ]
                ]
            ]
        , viewCloseLoopToggle model
        , div [ class "chemin-noir-toggle" ]
            [ label [ class "toggle-label" ]
                [ div [ class "toggle-switch" ]
                    [ input
                        [ type_ "checkbox"
                        , checked model.cheminNoir
                        , onCheck (\_ -> ToggleCheminNoir)
                        , class "toggle-input"
                        ]
                        []
                    , span [ class "toggle-slider" ] []
                    ]
                , div [ class "toggle-text" ]
                    [ span [ class "toggle-title" ] [ text "Mode Chemin Noir" ]
                    , span [ class "toggle-desc" ]
                        [ text
                            (if model.cheminNoir then
                                "Actif — routes et zones habitées évitées"

                             else
                                "Inactif — poids personnalisables"
                            )
                        ]
                    ]
                ]
            ]

        -- Geocoding: centrer la carte
        , fieldset []
            [ legend [] [ text "Centrer la carte" ]
            , div [ class "address-search" ]
                [ div [ class "address-input-row" ]
                    [ input
                        [ Html.Attributes.value model.mapSearch
                        , onInput MapSearchChanged
                        , placeholder "Rechercher un lieu..."
                        , autocomplete False
                        , spellcheck False
                        , onEnter SearchMap
                        ]
                        []
                    , button
                        [ type_ "button"
                        , class "btn-address-search"
                        , onClick SearchMap
                        ]
                        [ text "Rechercher" ]
                    ]
                , if not (List.isEmpty model.mapSearchResults) then
                    div [ class "address-chips-wrapper" ]
                        [ small [ class "address-chips-hint" ] [ text "Choisissez une adresse :" ]
                        , div [ class "address-chips" ]
                            (List.map
                                (\geo ->
                                    let
                                        parts =
                                            String.split ", " geo.displayName

                                        line1 =
                                            List.head parts |> Maybe.withDefault ""

                                        line2 =
                                            parts
                                                |> List.drop 1
                                                |> List.take 2
                                                |> String.join ", "
                                    in
                                    button
                                        [ type_ "button"
                                        , class "address-chip"
                                        , onMouseDown (SelectMapSearchResult geo)
                                        ]
                                        [ span [ class "address-chip-line1" ] [ text line1 ]
                                        , span [ class "address-chip-line2" ] [ text line2 ]
                                        ]
                                )
                                (List.take 3 model.mapSearchResults)
                            )
                        ]

                  else
                    text ""
                ]
            ]
        , if not model.cheminNoir then
            fieldset []
                [ legend [] [ text "Poids" ]
                , inputField "Éviter population" model.form.wPop PopWeightChanged False
                , inputField "Éviter bitume" model.form.wPaved PavedWeightChanged False
                , inputField "Chercher la forêt" model.form.wForest ForestWeightChanged False
                ]

          else
            text ""
        , if model.routeMode == Loop then
            fieldset []
                [ legend [] [ text "Options boucle" ]
                , inputField "Distance cible (km)" model.loopForm.distanceKm LoopDistanceChanged False
                , inputField "Tolérance (km)" model.loopForm.toleranceKm LoopToleranceChanged False
                , inputField "Nombre de propositions" model.loopForm.candidateCount LoopCandidateCountChanged False
                , inputField "D+ max (m)" model.loopForm.maxAscentM LoopMaxAscentChanged False
                , inputField "D+ min (m)" model.loopForm.minAscentM LoopMinAscentChanged False
                , small [] [ text "Laissez D+ vide pour obtenir automatiquement la boucle la moins pentue." ]
                ]

          else
            text ""

        , viewDirectLegs model
        , div [ class "action-buttons" ]
            [ button
                [ type_ "button"
                , class "btn-gpx-import"
                , onClick ImportGpxClicked
                ]
                [ text "Importer GPX" ]
            ]
        , small [ class "waypoints-summary" ]
            [ text <|
                String.fromInt (List.length model.waypoints)
                    ++ " point(s) • Distance : "
                    -- Sans `Format.km`, le flottant du backend sortait brut :
                    -- « 7.016201634123582 km » après un import GPX.
                    ++ Format.km
                        (case model.lastResponse of
                            Just r ->
                                r.distanceKm

                            Nothing ->
                                0
                        )
            ]
        , viewFreehandPanel model
        , fieldset []
            [ legend [] [ text "Vue de la carte" ]
            , button
                [ type_ "button"
                , class "map-toggle"
                , onClick ToggleMapView
                ]
                [ text <|
                    case model.mapViewMode of
                        Topo ->
                            "Vue Plan IGN"

                        IgnPlan ->
                            "Vue Ortho IGN"

                        IgnOrtho ->
                            "Vue Satellite"

                        Satellite ->
                            "Vue Hybride"

                        Hybrid ->
                            "Vue Topo"
                ]
            ]
        , if model.lastResponse /= Nothing then
            fieldset []
                [ legend [] [ text "Sauvegarde" ]
                , div [ class "input-field" ]
                    [ Html.label [] [ text "Nom du tracé" ]
                    , input
                        [ Html.Attributes.value model.saveRouteName
                        , onInput SaveRouteNameChanged
                        , placeholder "Ma belle randonnée"
                        , autocomplete False
                        , spellcheck False
                        ]
                        []
                    ]
                , div [ class "input-field" ]
                    [ Html.label [] [ text "Description (optionnel)" ]
                    , input
                        [ Html.Attributes.value model.saveRouteDescription
                        , onInput SaveRouteDescriptionChanged
                        , placeholder "Description du tracé..."
                        , autocomplete False
                        , spellcheck False
                        ]
                        []
                    ]
                , div [ class "input-field" ]
                    [ Html.label [] [ text "Dossier (optionnel)" ]
                    , input
                        [ Html.Attributes.value model.saveRouteFolder
                        , onInput SaveRouteFolderChanged
                        , placeholder "Tours forêts — Sud"
                        , list "save-folder-options"
                        , autocomplete False
                        , spellcheck False
                        ]
                        []
                    , folderDatalist "save-folder-options" model.savedRoutes
                    ]
                , case model.loadedRouteId of
                    Just id ->
                        button
                            [ type_ "button"
                            , class "save-btn"
                            , onClick UpdateLoadedRoute
                            , disabled (String.isEmpty model.saveRouteName)
                            ]
                            [ text ("Mettre à jour l'itinéraire " ++ String.fromInt id) ]

                    Nothing ->
                        text ""
                , button
                    [ type_ "button"
                    , class "save-btn"
                    , onClick SaveRouteToDb
                    , disabled (String.isEmpty model.saveRouteName)
                    ]
                    [ text <|
                        -- Le libellé doit dire lequel des deux boutons crée une
                        -- ligne, sinon on écrase sans le vouloir.
                        case model.loadedRouteId of
                            Just _ ->
                                "Enregistrer comme nouvel itinéraire"

                            Nothing ->
                                "Sauvegarder dans la base"
                    ]
                ]

          else
            text ""
        , button
            [ type_ "button"
            , class "btn-submit"
            , onClick Submit
            , disabled model.pending
            ]
            [ text
                (if model.pending then
                    "Calcul en cours…"

                 else
                    "Tracer l'itinéraire"
                )
            ]
        , if model.pending then
            div [ class "route-loading" ]
                [ span [ class "route-spinner" ] []
                , span [] [ text (waitMessage model.pendingSeconds) ]
                ]

          else
            text ""
        , case model.error of
            Just error ->
                p [ class "error" ] [ text error ]

            Nothing ->
                text ""
        ]


inputField : String -> String -> (String -> Msg) -> Bool -> Html Msg
inputField label value msg isDisabled =
    div [ class "input-field" ]
        [ Html.label [] [ text label ]
        , input
            [ Html.Attributes.value value
            , onInput msg
            , autocomplete False
            , spellcheck False
            , disabled isDisabled
            ]
            []
        ]


onEnter : Msg -> Attribute Msg
onEnter msg =
    preventDefaultOn "keydown"
        (Decode.field "key" Decode.string
            |> Decode.andThen
                (\key ->
                    if key == "Enter" then
                        Decode.succeed ( msg, True )

                    else
                        Decode.fail "not Enter"
                )
        )


{-| Liste des itinéraires enregistrés.

Ce bloc était écrit deux fois, à l'identique, dans les deux branches du test
`model.lastResponse /= Nothing` : seul le formulaire qui le précède dépendait
de cette condition, pas la liste.

Il portait aussi son propre bouton « Mes tracés sauvegardés » et n'affichait
la liste que replié. Devenu un onglet, il n'a plus à s'ouvrir : l'onglet est
déjà la commande, et un second niveau de repli laisserait un panneau vide.
-}
viewSavedRoutesSection : Model -> Html Msg
viewSavedRoutesSection model =
    if List.isEmpty model.savedRoutes then
        p [ class "empty-state" ] [ text "Aucun tracé sauvegardé" ]

    else
        let
            groups =
                folderGroups model.savedRoutes

            -- Les flèches ne parcourent que les lignes affichées : un dossier
            -- replié ne doit pas avaler le focus.
            visible =
                groups
                    |> List.filter (\( key, _ ) -> Set.member key model.openFolders)
                    |> List.concatMap Tuple.second

            ids =
                List.map .id visible

            rang route =
                ids
                    |> List.indexedMap Tuple.pair
                    |> List.filter (\( _, id ) -> id == route.id)
                    |> List.head
                    |> Maybe.map Tuple.first
                    |> Maybe.withDefault 0
        in
        div
            [ class "saved-routes-panel"
            , attribute "aria-label" "Itinéraires enregistrés"
            ]
            (folderDatalist "move-folder-options" model.savedRoutes
                :: List.map
                    (\( key, routes ) ->
                        viewFolder model
                            key
                            routes
                            (\route -> viewSavedRoute model.referenceRoutes ids (rang route) route)
                    )
                    groups
            )


{-| Itinéraires par dossier, dossiers triés et « Sans dossier » en dernier.
-}
folderGroups : List SavedRoute -> List ( String, List SavedRoute )
folderGroups routes =
    let
        inFolder key =
            List.filter (\r -> folderKey r == key) routes

        loose =
            inFolder ""
    in
    List.map (\key -> ( key, inFolder key )) (savedFolders routes)
        ++ (if List.isEmpty loose then
                []

            else
                [ ( "", loose ) ]
           )


{-| Un dossier : son en-tête, et ses lignes s'il est ouvert.

L'en-tête porte le nombre d'itinéraires et leur longueur cumulée : pour un tour
découpé en tronçons, c'est la longueur du tour.
-}
viewFolder : Model -> String -> List SavedRoute -> (SavedRoute -> Html Msg) -> Html Msg
viewFolder model key routes viewRoute =
    let
        open =
            Set.member key model.openFolders

        label =
            if key == "" then
                "Sans dossier"

            else
                key

        resume =
            String.fromInt (List.length routes)
                ++ " · "
                ++ Format.km (List.sum (List.map .distanceKm routes))
    in
    div [ classList [ ( "route-folder", True ), ( "is-open", open ) ] ]
        [ button
            [ type_ "button"
            , class "route-folder-header"
            , onClick (ToggleFolder key)
            , attribute "aria-expanded"
                (if open then
                    "true"

                 else
                    "false"
                )
            ]
            [ span [ class "route-folder-caret", attribute "aria-hidden" "true" ]
                [ text
                    (if open then
                        "\u{25BE}"

                     else
                        "\u{25B8}"
                    )
                ]
            , span [ class "route-folder-name" ] [ text label ]
            , span [ class "route-folder-count t-metric" ] [ text resume ]
            ]
        , if open then
            div [ attribute "role" "list", attribute "aria-label" label ]
                (List.concatMap
                    (\route ->
                        [ viewRoute route
                        , viewMoveEditor model route
                        ]
                    )
                    routes
                )

          else
            text ""
        ]


{-| Champ de rangement, sous la ligne qu'on déplace.

Hors de la ligne : celle-ci capte Début, Fin et les flèches pour la navigation
et les rendrait inutilisables dans le champ.
-}
viewMoveEditor : Model -> SavedRoute -> Html Msg
viewMoveEditor model route =
    case model.movingRoute of
        Just ( id, folder ) ->
            if id == route.id then
                div [ class "route-move" ]
                    [ input
                        [ Html.Attributes.id (moveFolderDomId id)
                        , Html.Attributes.value folder
                        , onInput MoveRouteFolderChanged
                        , list "move-folder-options"
                        , placeholder "Dossier (vide : aucun)"
                        , attribute "aria-label" ("Dossier de " ++ route.name)
                        , autocomplete False
                        , spellcheck False
                        , on "keydown"
                            (Decode.field "key" Decode.string
                                |> Decode.andThen
                                    (\key ->
                                        case key of
                                            "Enter" ->
                                                Decode.succeed ConfirmMoveRoute

                                            "Escape" ->
                                                Decode.succeed CancelMoveRoute

                                            _ ->
                                                Decode.fail "touche ordinaire"
                                    )
                            )
                        ]
                        []
                    , button [ type_ "button", class "action-load", onClick ConfirmMoveRoute ] [ text "Ranger" ]
                    , button
                        [ type_ "button"
                        , class "action-delete"
                        , onClick CancelMoveRoute
                        , title "Annuler"
                        ]
                        [ text "\u{2715}" ]
                    ]

            else
                text ""

        Nothing ->
            text ""


{-| Dossiers existants proposés à la saisie.
-}
folderDatalist : String -> List SavedRoute -> Html msg
folderDatalist domId routes =
    datalist [ Html.Attributes.id domId ]
        (List.map (\f -> option [ Html.Attributes.value f ] []) (savedFolders routes))


{-| Une entrée de la liste des itinéraires, sur une seule ligne.

Chaque entrée occupait ~170 px : titre, description sur trois lignes, ligne de
chiffres et quatre boutons toujours visibles. Quinze itinéraires faisaient donc
2 550 px dans un panneau haut de 950, soit cinq visibles à la fois. La
description part dans l'infobulle et les actions n'apparaissent qu'au survol.
-}
viewSavedRoute : Dict.Dict Int ReferenceLayer -> List Int -> Int -> SavedRoute -> Html Msg
viewSavedRoute referenceRoutes ids index route =
    let
        dernier =
            List.length ids - 1

        focusRang rang =
            ids
                |> List.drop (clamp 0 dernier rang)
                |> List.head
                |> Maybe.withDefault route.id
                |> loadButtonDomId
                |> FocusElement
    in
    div
        [ classList
            [ ( "route-row", True )
            , ( "is-reference", Dict.member route.id referenceRoutes )
            ]
        , attribute "role" "listitem"
        , title (Maybe.withDefault route.name route.description)
        , onListKey
            (\key ->
                case key of
                    "ArrowDown" ->
                        Just (focusRang (index + 1))

                    "ArrowUp" ->
                        Just (focusRang (index - 1))

                    "Home" ->
                        Just (focusRang 0)

                    "End" ->
                        Just (focusRang dernier)

                    "Escape" ->
                        Just (FocusElement "panel-tab-routes")

                    _ ->
                        Nothing
            )
        ]
        [ span
            [ class
                ("route-row-dot "
                    ++ difficultyClass (Maybe.withDefault "" route.routeData.difficulty)
                )
            ]
            []
        , div [ class "route-row-main" ]
            [ span [ class "route-row-name" ]
                [ text route.name
                , if route.isFavorite then
                    span [ class "favorite-star" ] [ text "\u{2B50}" ]

                  else
                    text ""
                ]
            , span [ class "route-row-metrics t-metric" ] [ text (routeMetrics route) ]
            ]
        , div [ class "route-row-actions" ]
            [ button
                [ type_ "button"
                , class "action-load"
                , id (loadButtonDomId route.id)
                , onClick (LoadSavedRoute route.id)
                ]
                [ text "Charger" ]
            , button
                [ type_ "button"
                , classList
                    [ ( "action-layer", True )
                    , ( "active", Dict.member route.id referenceRoutes )
                    ]
                , onClick (ToggleReferenceRoute route.id)
                , title "Afficher en calque sur la carte, sans remplacer le tracé courant"
                ]
                [ text "\u{29C9}" ]
            , button
                [ type_ "button"
                , classList [ ( "action-fav", True ), ( "active", route.isFavorite ) ]
                , onClick (ToggleFavorite route.id)
                , title <|
                    if route.isFavorite then
                        "Retirer des favoris"

                    else
                        "Ajouter aux favoris"
                ]
                [ text "\u{2B50}" ]
            , button
                [ type_ "button"
                , class "action-move"
                , onClick (StartMoveRoute route.id)
                , title "Ranger dans un dossier"
                ]
                [ text "\u{1F4C1}" ]
            , button
                [ type_ "button"
                , class "action-delete"
                , onClick (DeleteSavedRoute route.id)
                , title "Supprimer"
                ]
                [ text "\u{2715}" ]
            ]
        ]


{-| Le bouton « Charger » sert de point d'ancrage du focus dans sa ligne.

Les flèches doivent déplacer le focus de ligne en ligne, mais une ligne est un
`div` : la focaliser demanderait un `tabindex` et la ferait entrer dans la
tabulation en plus de ses quatre boutons. Le premier bouton de la ligne fait
donc office de position courante, et c'est lui que les flèches visent.
-}
loadButtonDomId : Int -> String
loadButtonDomId routeId =
    "route-load-" ++ String.fromInt routeId


{-| Touches de déplacement dans la liste.

`preventDefault` est indispensable : une liste de quinze entrées dépasse la
hauteur du panneau, et sans lui chaque flèche déplacerait le focus *et* ferait
défiler le panneau, qui finirait par ne plus montrer la ligne focalisée.
-}
onListKey : (String -> Maybe Msg) -> Attribute Msg
onListKey versMsg =
    preventDefaultOn "keydown"
        (Decode.field "key" Decode.string
            |> Decode.andThen
                (\key ->
                    case versMsg key of
                        Just msg ->
                            Decode.succeed ( msg, True )

                        Nothing ->
                            Decode.fail "touche sans effet dans la liste"
                )
        )


{-| Distance, dénivelé et difficulté d'un itinéraire, en une ligne.

Les valeurs étaient affichées brutes : « 26.424826 km », « D+ 263.85m ». Un
relevé se lit au dixième de kilomètre et au mètre près, pas au micron.
-}
routeMetrics : SavedRoute -> String
routeMetrics route =
    let
        denivele =
            case route.totalAscentM of
                Just ascent ->
                    " · D+ " ++ Format.metres ascent

                Nothing ->
                    ""

        difficulte =
            case route.routeData.difficulty of
                Just d ->
                    " · " ++ difficultyLabel d

                Nothing ->
                    ""
    in
    Format.km route.distanceKm ++ denivele ++ difficulte


{-| Classe de la pastille de difficulté.

Les noms viennent de l'API en anglais ; la correspondance est écrite ici une
fois pour la couleur et une fois pour le libellé plutôt que de laisser une
chaîne anglaise atterrir dans une classe CSS française.
-}
difficultyClass : String -> String
difficultyClass niveau =
    case niveau of
        "easy" ->
            "d-easy"

        "moderate" ->
            "d-moderate"

        "difficult" ->
            "d-difficult"

        _ ->
            "d-expert"


{-| Le moteur classe en anglais, l'interface parle français. -}
difficultyLabel : String -> String
difficultyLabel niveau =
    case niveau of
        "easy" ->
            "facile"

        "moderate" ->
            "modéré"

        "difficult" ->
            "difficile"

        "expert" ->
            "expert"

        autre ->
            autre


{-| Referme le tracé multi-points : un dernier tronçon ramène au départ.

Le backend savait déjà boucler (`close_loop`), mais rien dans l'interface ne
l'activait. Hors multi-points le réglage n'a pas de sens : l'aller simple a
deux points fixes, et l'onglet Boucle génère ses propres boucles.
-}
viewCloseLoopToggle : Model -> Html Msg
viewCloseLoopToggle model =
    if model.routeMode == MultiPoint then
        div [ class "chemin-noir-toggle" ]
            [ label [ class "toggle-label" ]
                [ div [ class "toggle-switch" ]
                    [ input
                        [ type_ "checkbox"
                        , checked model.closeLoop
                        , onCheck (\_ -> ToggleCloseLoop)
                        , class "toggle-input"
                        ]
                        []
                    , span [ class "toggle-slider" ] []
                    ]
                , div [ class "toggle-text" ]
                    [ span [ class "toggle-title" ] [ text "Boucler" ]
                    , span [ class "toggle-desc" ]
                        [ text
                            (if model.closeLoop then
                                "Retour au point de départ"

                             else
                                "Le tracé s'arrête au dernier point"
                            )
                        ]
                    ]
                ]
            ]

    else
        text ""


viewFreehandPanel : Model -> Html Msg
viewFreehandPanel model =
    case ( model.routeMode, model.lastResponse ) of
        ( MultiPoint, Just _ ) ->
            if List.length model.waypoints >= 2 then
                div [ class "freehand-panel" ]
                    [ -- Toggle switch
                      label [ class "toggle-label" ]
                        [ div [ class "toggle-switch" ]
                            [ input
                                [ type_ "checkbox"
                                , checked model.freehandEnabled
                                , onCheck (\_ -> ToggleFreehandMode)
                                , class "toggle-input"
                                ]
                                []
                            , span [ class "toggle-slider" ] []
                            ]
                        , div [ class "toggle-text" ]
                            [ span [ class "toggle-title" ] [ text "Tracé libre" ]
                            , span [ class "toggle-desc" ]
                                [ text (freehandStatusText model) ]
                            ]
                        ]

                    -- Cancel button (visible during active drawing)
                    , case model.freehandDrawing of
                        Just _ ->
                            button
                                [ type_ "button"
                                , class "btn-cancel-freehand"
                                , onClick CancelFreehandDrawing
                                ]
                                [ text "Annuler le dessin" ]

                        Nothing ->
                            text ""

                    -- List of stored freehand segments
                    , if not (Dict.isEmpty model.freehandSegments) then
                        div [ class "freehand-segments-list" ]
                            (Dict.toList model.freehandSegments
                                |> List.map
                                    (\( idx, pts ) ->
                                        div [ class "freehand-segment-item" ]
                                            [ span []
                                                [ text
                                                    ("Segment "
                                                        ++ String.fromInt (idx + 1)
                                                        ++ " → "
                                                        ++ String.fromInt (idx + 2)
                                                        ++ " ("
                                                        ++ String.fromInt (List.length pts)
                                                        ++ " pts)"
                                                    )
                                                ]
                                            , button
                                                [ type_ "button"
                                                , class "btn-clear-segment"
                                                , onClick (ClearFreehandSegment idx)
                                                , title "Supprimer ce segment libre"
                                                ]
                                                [ text "\u{2715}" ]
                                            ]
                                    )
                            )

                      else
                        text ""
                    ]

            else
                text ""

        _ ->
            text ""


freehandStatusText : Model -> String
freehandStatusText model =
    if not model.freehandEnabled then
        "Dessinez des segments à main levée sur la carte"

    else
        case model.freehandDrawing of
            Nothing ->
                "Cliquez près d'un waypoint pour commencer"

            Just state ->
                "Segment "
                    ++ String.fromInt (state.fromIdx + 1)
                    ++ " → "
                    ++ String.fromInt (state.fromIdx + 2)
                    ++ " ("
                    ++ String.fromInt (List.length state.points)
                    ++ " pts) — cliquez près du waypoint suivant"




{-| Un interrupteur par tronçon : suivre les chemins, ou tirer tout droit.
-}
viewDirectLegs : Model -> Html Msg
viewDirectLegs model =
    let
        points =
            List.length model.waypoints

        count =
            DirectLegs.legCount points model.closeLoop

        legLabel leg =
            String.fromInt (leg + 1)
                ++ " → "
                ++ (if leg + 1 >= points then
                        "départ"

                    else
                        String.fromInt (leg + 2)
                   )
    in
    if model.routeMode /= MultiPoint || count == 0 then
        text ""

    else
        fieldset [ class "direct-legs" ]
            [ legend [] [ text "Tronçons" ]
            , small [] [ text "Cochez « tout droit » pour couper à travers, sans suivre de chemin." ]
            , ul [ class "direct-legs-list" ]
                (List.map
                    (\leg ->
                        li []
                            [ label []
                                [ input
                                    [ type_ "checkbox"
                                    , checked (Set.member leg model.directLegs)
                                    , onCheck (\_ -> ToggleDirectLeg leg)
                                    ]
                                    []
                                , text (" " ++ legLabel leg ++ " : tout droit")
                                ]
                            ]
                    )
                    (List.range 0 (count - 1))
                )
            ]
