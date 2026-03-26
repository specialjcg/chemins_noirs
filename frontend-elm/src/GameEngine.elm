module GameEngine exposing
    ( SegmentAdvanceResult
    , advanceAlongRoad
    , advanceAlongRoadDebug
    , advanceAlongSegment
    , advanceOnSegments
    , bearingBetween
    , findNearestSegment
    , projectOntoSegment
    , projectOntoSegmentT
    , roadToSegments
    , snapToNearestRoad
    )

{-| Pure game engine functions for the orienteering game.

Key concept: the player is always ON a road segment. They advance along it.
At junctions (segment endpoints), the bearing determines which branch to take.
The bearing is used ONLY at junctions, not to pick which segment to be on.
-}

import Types exposing (Coordinate, haversineMeters)


{-| Result of advancing along road segments.
Returns new position AND the bearing of the road at that point.
-}
type alias SegmentAdvanceResult =
    { position : Coordinate
    , roadBearing : Float
    , snapped : Bool
    , distanceMoved : Float
    }


{-| Snap a position to the nearest road segment.
Returns the projected point on the segment, or the original pos if no road nearby.
-}
snapToNearestRoad : Coordinate -> List (List Coordinate) -> Coordinate
snapToNearestRoad pos roads =
    case findNearestSegment pos roads of
        Just nearest ->
            if nearest.dist < 20 then
                nearest.proj

            else
                pos

        Nothing ->
            pos


{-| Find the nearest road segment to the player.
-}
findNearestSegment : Coordinate -> List (List Coordinate) -> Maybe { proj : Coordinate, segA : Coordinate, segB : Coordinate, dist : Float }
findNearestSegment pos roads =
    let
        allSegments =
            List.concatMap roadToSegments roads
    in
    allSegments
        |> List.filterMap
            (\( a, b ) ->
                let
                    proj =
                        projectOntoSegment pos a b

                    dist =
                        haversineMeters pos proj
                in
                if dist < 100 then
                    Just { proj = proj, segA = a, segB = b, dist = dist }

                else
                    Nothing
            )
        |> List.sortBy .dist
        |> List.head


{-| Advance the player along road segments by distM meters.

The player is ON or very near a road.
- If at a junction (close to multiple segment endpoints), bearing picks the branch.
- Otherwise, advance along the nearest segment.
- At segment end, pick next segment by bearing.
-}
advanceOnSegments : Coordinate -> Float -> Float -> List (List Coordinate) -> SegmentAdvanceResult
advanceOnSegments pos bearing distM roads =
    let
        allSegments =
            List.concatMap roadToSegments roads

        -- Check if we're at a junction: find segment ENDPOINTS within 5m
        nearbyEndpoints =
            allSegments
                |> List.concatMap
                    (\( a, b ) ->
                        let
                            dA = haversineMeters pos a
                            dB = haversineMeters pos b
                        in
                        (if dA < 5 then [ a ] else [])
                            ++ (if dB < 5 then [ b ] else [])
                    )

        atJunction =
            List.length nearbyEndpoints >= 2
    in
    if atJunction then
        -- At a junction: use bearing to pick direction
        advanceFromJunction pos bearing distM roads

    else
        -- On a segment: advance along nearest segment
        advanceAlongCurrentSegment pos bearing distM roads


{-| At a junction, find all departing segments and pick the one
most aligned with the player's bearing, then advance along it.
-}
advanceFromJunction : Coordinate -> Float -> Float -> List (List Coordinate) -> SegmentAdvanceResult
advanceFromJunction pos bearing distM roads =
    let
        allSegments =
            List.concatMap roadToSegments roads

        -- Find segments that have an endpoint near the player
        departingSegments =
            allSegments
                |> List.filterMap
                    (\( a, b ) ->
                        let
                            dA = haversineMeters pos a
                            dB = haversineMeters pos b
                        in
                        if dA < 5 then
                            let
                                segBear = bearingBetween a b
                                diff = angleDiff bearing segBear
                            in
                            Just { start = a, otherEnd = b, segBearing = segBear, angleDiff = diff, segLen = haversineMeters a b }

                        else if dB < 5 then
                            let
                                segBear = bearingBetween b a
                                diff = angleDiff bearing segBear
                            in
                            Just { start = b, otherEnd = a, segBearing = segBear, angleDiff = diff, segLen = haversineMeters b a }

                        else
                            Nothing
                    )
                |> List.sortBy .angleDiff
    in
    case List.head departingSegments of
        Nothing ->
            -- No departing segment, stay put
            { position = pos
            , roadBearing = bearing
            , snapped = False
            , distanceMoved = 0
            }

        Just best ->
            let
                distToEnd = best.segLen
            in
            if distM <= distToEnd then
                let
                    ratio = distM / max 0.01 distToEnd
                    newPos = interpolate best.start best.otherEnd ratio
                in
                { position = newPos
                , roadBearing = best.segBearing
                , snapped = True
                , distanceMoved = distM
                }

            else
                -- Reached end, continue
                case findNextSegment best.otherEnd best.start bearing (distM - distToEnd) roads of
                    Just nr ->
                        { nr | distanceMoved = distM }

                    Nothing ->
                        { position = best.otherEnd
                        , roadBearing = best.segBearing
                        , snapped = True
                        , distanceMoved = distToEnd
                        }


{-| Advance along the nearest segment (not at a junction).
-}
advanceAlongCurrentSegment : Coordinate -> Float -> Float -> List (List Coordinate) -> SegmentAdvanceResult
advanceAlongCurrentSegment pos bearing distM roads =
    case findNearestSegment pos roads of
        Nothing ->
            { position = pos
            , roadBearing = bearing
            , snapped = False
            , distanceMoved = 0
            }

        Just { proj, segA, segB } ->
            let
                bearingAB = bearingBetween segA segB
                bearingBA = bearingBetween segB segA
                diffAB = angleDiff bearing bearingAB
                diffBA = angleDiff bearing bearingBA

                ( forwardEnd, backEnd ) =
                    if diffAB <= diffBA then
                        ( segB, segA )
                    else
                        ( segA, segB )

                forwardBearing = bearingBetween proj forwardEnd
                distToForwardEnd = haversineMeters proj forwardEnd
            in
            if distToForwardEnd < 0.5 then
                -- At segment endpoint, transition to next
                case findNextSegment forwardEnd backEnd bearing distM roads of
                    Just nr -> nr
                    Nothing ->
                        { position = proj, roadBearing = forwardBearing, snapped = True, distanceMoved = 0 }

            else if distM <= distToForwardEnd then
                let
                    ratio = distM / distToForwardEnd
                    newPos = interpolate proj forwardEnd ratio
                in
                { position = newPos, roadBearing = forwardBearing, snapped = True, distanceMoved = distM }

            else
                let remaining = distM - distToForwardEnd
                in
                case findNextSegment forwardEnd backEnd bearing remaining roads of
                    Just nr -> { nr | distanceMoved = distM }
                    Nothing ->
                        { position = forwardEnd, roadBearing = forwardBearing, snapped = True, distanceMoved = distToForwardEnd }


{-| Find the next connected segment from a junction point and continue walking.
`fromPoint` is the junction, `prevPoint` is where we came from (to avoid U-turns).
-}
findNextSegment : Coordinate -> Coordinate -> Float -> Float -> List (List Coordinate) -> Maybe SegmentAdvanceResult
findNextSegment fromPoint prevPoint bearing remainingDist roads =
    let
        allSegments =
            List.concatMap roadToSegments roads

        -- Find segments that start or end at fromPoint (within 2m tolerance)
        connectedSegments =
            allSegments
                |> List.filterMap
                    (\( a, b ) ->
                        let
                            distA =
                                haversineMeters fromPoint a

                            distB =
                                haversineMeters fromPoint b
                        in
                        if distA < 2 then
                            let
                                distFromPrev =
                                    haversineMeters b prevPoint
                            in
                            if distFromPrev > 3 then
                                let
                                    segBearing =
                                        bearingBetween a b

                                    diff =
                                        angleDiff bearing segBearing
                                in
                                Just { otherEnd = b, start = a, bearing = segBearing, angleDiff = diff }

                            else
                                Nothing

                        else if distB < 2 then
                            let
                                distFromPrev =
                                    haversineMeters a prevPoint
                            in
                            if distFromPrev > 3 then
                                let
                                    segBearing =
                                        bearingBetween b a

                                    diff =
                                        angleDiff bearing segBearing
                                in
                                Just { otherEnd = a, start = b, bearing = segBearing, angleDiff = diff }

                            else
                                Nothing

                        else
                            Nothing
                    )
                |> List.sortBy .angleDiff
    in
    case List.head connectedSegments of
        Nothing ->
            Nothing

        Just best ->
            let
                distToEnd =
                    haversineMeters fromPoint best.otherEnd
            in
            if remainingDist <= distToEnd then
                let
                    ratio =
                        if distToEnd < 0.01 then
                            1.0

                        else
                            remainingDist / distToEnd

                    newPos =
                        interpolate fromPoint best.otherEnd ratio
                in
                Just
                    { position = newPos
                    , roadBearing = best.bearing
                    , snapped = True
                    , distanceMoved = remainingDist
                    }

            else
                case findNextSegment best.otherEnd fromPoint best.bearing (remainingDist - distToEnd) roads of
                    Just nr ->
                        Just nr

                    Nothing ->
                        Just
                            { position = best.otherEnd
                            , roadBearing = best.bearing
                            , snapped = True
                            , distanceMoved = distToEnd
                            }



-- LEGACY FUNCTIONS (kept for compatibility)


advanceAlongRoad : Coordinate -> Float -> Float -> List (List Coordinate) -> Coordinate
advanceAlongRoad pos bearing distM roads =
    (advanceOnSegments pos bearing distM roads).position


advanceAlongRoadDebug : Coordinate -> Float -> Float -> List (List Coordinate) -> ( Coordinate, String, Float )
advanceAlongRoadDebug pos bearing distM roads =
    let
        result =
            advanceOnSegments pos bearing distM roads
    in
    ( result.position
    , "snapped=" ++ (if result.snapped then "Y" else "N") ++ " moved=" ++ String.fromInt (round result.distanceMoved) ++ "m"
    , result.roadBearing
    )


advanceAlongSegment : Coordinate -> Coordinate -> Coordinate -> Float -> Coordinate
advanceAlongSegment pos proj forward distM =
    let
        snapDist =
            haversineMeters pos proj

        projToForward =
            haversineMeters proj forward
    in
    if snapDist >= distM then
        interpolate pos proj (distM / snapDist)

    else
        let
            remaining =
                distM - snapDist
        in
        if projToForward < 0.1 then
            proj

        else if remaining >= projToForward then
            forward

        else
            interpolate proj forward (remaining / projToForward)


interpolate : Coordinate -> Coordinate -> Float -> Coordinate
interpolate a b ratio =
    { lat = a.lat + (b.lat - a.lat) * ratio
    , lon = a.lon + (b.lon - a.lon) * ratio
    }


angleDiff : Float -> Float -> Float
angleDiff a b =
    let
        d =
            abs (a - b)
    in
    if d > 180 then
        360 - d

    else
        d


roadToSegments : List Coordinate -> List ( Coordinate, Coordinate )
roadToSegments road =
    case road of
        a :: b :: rest ->
            ( a, b ) :: roadToSegments (b :: rest)

        _ ->
            []


projectOntoSegmentT : Coordinate -> Coordinate -> Coordinate -> ( Coordinate, Float )
projectOntoSegmentT p a b =
    let
        cosLat =
            cos (a.lat * pi / 180)

        ax =
            a.lon * cosLat

        ay =
            a.lat

        bx =
            b.lon * cosLat

        by =
            b.lat

        px =
            p.lon * cosLat

        py =
            p.lat

        dx =
            bx - ax

        dy =
            by - ay

        lenSq =
            dx * dx + dy * dy

        t =
            if lenSq < 1.0e-12 then
                0

            else
                clamp 0 1 (((px - ax) * dx + (py - ay) * dy) / lenSq)

        projX =
            ax + t * dx

        projY =
            ay + t * dy
    in
    ( { lat = projY, lon = projX / cosLat }, t )


projectOntoSegment : Coordinate -> Coordinate -> Coordinate -> Coordinate
projectOntoSegment p a b =
    Tuple.first (projectOntoSegmentT p a b)


bearingBetween : Coordinate -> Coordinate -> Float
bearingBetween from to =
    let
        dLon =
            (to.lon - from.lon) * pi / 180

        lat1 =
            from.lat * pi / 180

        lat2 =
            to.lat * pi / 180

        y =
            sin dLon * cos lat2

        x =
            cos lat1 * sin lat2 - sin lat1 * cos lat2 * cos dLon
    in
    toFloat (modBy 360 (round (atan2 y x * 180 / pi) + 360))
