use seed::{prelude::*, virtual_dom::AtValue, *};
use serde::{Deserialize, Serialize};
use serde_wasm_bindgen::to_value;
use shared::{
    default_distance_tolerance_km, default_loop_candidate_count, Coordinate, LoopCandidate,
    LoopRouteRequest, LoopRouteResponse, MultiPointRouteRequest, RouteRequest, RouteResponse,
};
use wasm_bindgen::{
    prelude::{wasm_bindgen, JsValue},
    JsCast,
};

// Structure pour les routes sauvegardées
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SavedRouteInfo {
    pub filename: String,
    pub name: String,
    pub distance_km: f64,
    pub saved_at: String,
}

#[derive(Debug, Serialize)]
pub struct SaveRouteRequest {
    pub name: String,
    pub route: RouteResponse,
}

#[wasm_bindgen(module = "/maplibre_map.js")]
extern "C" {
    #[wasm_bindgen(js_name = initMap)]
    fn init_map();
    #[wasm_bindgen(js_name = updateRoute)]
    fn update_route_js(coords: JsValue);
    #[wasm_bindgen(js_name = updateSelectionMarkers)]
    fn update_selection_markers(start: JsValue, end: JsValue);
    #[wasm_bindgen(js_name = updateWaypointMarkers)]
    fn update_waypoint_markers(waypoints: JsValue);
    #[wasm_bindgen(js_name = toggleSatelliteView)]
    fn toggle_satellite_view(enabled: bool);
    #[wasm_bindgen(js_name = updateBbox)]
    fn update_bbox_js(bounds: JsValue);
    #[wasm_bindgen(js_name = toggleThree3DView)]
    fn toggle_three_3d_view(enabled: bool);
    #[wasm_bindgen(js_name = centerOnMarkers)]
    fn center_on_markers(start: JsValue, end: JsValue);
    #[wasm_bindgen(js_name = startAnimation)]
    fn start_animation();
    #[wasm_bindgen(js_name = stopAnimation)]
    fn stop_animation();
}

fn api_root() -> String {
    if let Some(url) = option_env!("FRONTEND_API_ROOT") {
        return url.trim_end_matches('/').to_string();
    }
    "http://localhost:8080/api/route".to_string()
}

fn loop_api_root() -> String {
    if let Some(url) = option_env!("FRONTEND_LOOP_API_ROOT") {
        return url.trim_end_matches('/').to_string();
    }
    "http://localhost:8080/api/loops".to_string()
}

pub struct Model {
    form: RouteForm,
    loop_form: LoopForm,
    waypoints: Vec<Coordinate>,
    close_loop: bool,
    pending: bool,
    last_response: Option<RouteResponse>,
    loop_candidates: Vec<LoopCandidate>,
    loop_meta: Option<LoopMeta>,
    selected_loop_idx: Option<usize>,
    error: Option<String>,
    click_mode: ClickMode,
    route_mode: RouteMode,
    map_view_mode: MapViewMode,
    view_mode: ViewMode,
    animation_state: AnimationState,
    saved_routes: Vec<SavedRouteInfo>,
    show_routes_list: bool,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum ClickMode {
    Start,
    End,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RouteMode {
    PointToPoint,
    Loop,
    MultiPoint,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum MapViewMode {
    Standard,
    Satellite,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum ViewMode {
    Map2D,
    Map3D,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum AnimationState {
    Stopped,
    Playing,
    Paused,
}

#[derive(Default, Clone)]
struct RouteForm {
    start_lat: String,
    start_lon: String,
    end_lat: String,
    end_lon: String,
    w_pop: String,
    w_paved: String,
}

impl RouteForm {
    fn to_request(&self) -> Result<RouteRequest, String> {
        let parse = |field: &str, label: &str| {
            field
                .trim()
                .parse::<f64>()
                .map_err(|_| format!("Champ {label} invalide"))
        };
        Ok(RouteRequest {
            start: Coordinate {
                lat: parse(&self.start_lat, "lat départ")?,
                lon: parse(&self.start_lon, "lon départ")?,
            },
            end: Coordinate {
                lat: parse(&self.end_lat, "lat arrivée")?,
                lon: parse(&self.end_lon, "lon arrivée")?,
            },
            w_pop: parse(&self.w_pop, "poids densité")?,
            w_paved: parse(&self.w_paved, "poids bitume")?,
        })
    }

    fn parse_weights(&self) -> Result<(f64, f64), String> {
        let w_pop = parse_field(&self.w_pop, "poids densité")?;
        let w_paved = parse_field(&self.w_paved, "poids bitume")?;
        Ok((w_pop, w_paved))
    }
}

#[derive(Clone)]
struct LoopMeta {
    target_distance_km: f64,
    distance_tolerance_km: f64,
}

#[derive(Clone)]
struct LoopForm {
    distance_km: String,
    tolerance_km: String,
    candidate_count: String,
    max_ascent_m: String,
    min_ascent_m: String,
}

impl Default for LoopForm {
    fn default() -> Self {
        Self {
            distance_km: "15".into(),
            tolerance_km: format!("{:.1}", default_distance_tolerance_km()),
            candidate_count: default_loop_candidate_count().to_string(),
            max_ascent_m: String::new(),
            min_ascent_m: String::new(),
        }
    }
}

impl LoopForm {
    fn to_request(&self, form: &RouteForm) -> Result<LoopRouteRequest, String> {
        let start = form
            .coordinate_pair(&form.start_lat, &form.start_lon)
            .ok_or_else(|| "Coordonnées de départ invalides".to_string())?;
        let (w_pop, w_paved) = form.parse_weights()?;
        let distance_km = parse_field(&self.distance_km, "distance cible (km)")?;
        let tolerance_km = if self.tolerance_km.trim().is_empty() {
            default_distance_tolerance_km()
        } else {
            parse_field(&self.tolerance_km, "tolérance (km)")?
        };
        let candidate_count =
            parse_field::<usize>(&self.candidate_count, "nombre de boucles")?.max(1);
        let max_ascent = parse_optional_field(&self.max_ascent_m)?;
        let min_ascent = parse_optional_field(&self.min_ascent_m)?;

        Ok(LoopRouteRequest {
            start,
            target_distance_km: distance_km,
            distance_tolerance_km: tolerance_km,
            candidate_count,
            w_pop,
            w_paved,
            max_total_ascent: max_ascent,
            min_total_ascent: min_ascent,
        })
    }
}

fn parse_optional_field(value: &str) -> Result<Option<f64>, String> {
    if value.trim().is_empty() {
        Ok(None)
    } else {
        parse_field(value, "dénivelé").map(Some)
    }
}

fn parse_field<T>(value: &str, label: &str) -> Result<T, String>
where
    T: std::str::FromStr,
{
    value
        .trim()
        .parse::<T>()
        .map_err(|_| format!("Champ {label} invalide"))
}

pub enum Msg {
    StartLatChanged(String),
    StartLonChanged(String),
    EndLatChanged(String),
    EndLonChanged(String),
    PopWeightChanged(String),
    PavedWeightChanged(String),
    LoopDistanceChanged(String),
    LoopToleranceChanged(String),
    LoopCandidateCountChanged(String),
    LoopMaxAscentChanged(String),
    LoopMinAscentChanged(String),
    Submit,
    SetClickMode(ClickMode),
    ToggleRouteMode(RouteMode),
    ToggleMapView,
    Toggle3DView,
    PlayAnimation,
    PauseAnimation,
    SaveRoute,
    LoadRoute,
    RefreshRoutesList,
    ToggleRoutesList,
    LoadSpecificRoute(String),
    ListRoutesFetched(Result<Vec<SavedRouteInfo>, String>),
    MapClicked { lat: f64, lon: f64 },
    RouteFetched(Result<RouteResponse, String>),
    LoopRouteFetched(Result<LoopRouteResponse, String>),
    SelectLoopCandidate(usize),
    AddWaypoint(Coordinate),
    RemoveWaypoint(usize),
    ClearWaypoints,
    ToggleCloseLoop,
    ComputeMultiPointRoute,
}

pub fn init(_: Url, orders: &mut impl Orders<Msg>) -> Model {
    orders.stream(streams::window_event(Ev::from("map-click"), |event| {
        let event = event
            .dyn_into::<web_sys::CustomEvent>()
            .expect("map-click event must be CustomEvent");
        let detail = event.detail();
        let payload: MapClickPayload = serde_wasm_bindgen::from_value(detail)
            .unwrap_or(MapClickPayload { lat: 0.0, lon: 0.0 });
        web_sys::console::debug_1(
            &format!(
                "[frontend] map click lat={:.5} lon={:.5}",
                payload.lat, payload.lon
            )
            .into(),
        );
        Msg::MapClicked {
            lat: payload.lat,
            lon: payload.lon,
        }
    }));

    let model = Model {
        form: RouteForm {
            // Combefort, Le Bois-d'Oingt (69620)
            start_lat: "45.9305".into(),
            start_lon: "4.5776".into(),
            end_lat: "45.9399".into(),
            end_lon: "4.5757".into(),
            w_pop: "1.5".into(),
            w_paved: "4.0".into(),
        },
        loop_form: LoopForm::default(),
        waypoints: Vec::new(),
        close_loop: false,
        pending: false,
        last_response: None,
        loop_candidates: Vec::new(),
        loop_meta: None,
        selected_loop_idx: None,
        error: None,
        click_mode: ClickMode::Start,
        route_mode: RouteMode::PointToPoint,
        map_view_mode: MapViewMode::Standard,
        view_mode: ViewMode::Map2D,
        animation_state: AnimationState::Stopped,
        saved_routes: Vec::new(),
        show_routes_list: false,
    };

    sync_selection_markers(&model.form);

    // Center map on initial start and end coordinates
    if let (Some(start), Some(end)) = (
        model
            .form
            .coordinate_pair(&model.form.start_lat, &model.form.start_lon),
        model
            .form
            .coordinate_pair(&model.form.end_lat, &model.form.end_lon),
    ) {
        if let (Ok(start_js), Ok(end_js)) = (to_value(&start), to_value(&end)) {
            center_on_markers(start_js, end_js);
        }
    }

    model
}

pub fn update(msg: Msg, model: &mut Model, orders: &mut impl Orders<Msg>) {
    match msg {
        Msg::StartLatChanged(val) => {
            model.form.start_lat = val;
            sync_selection_markers(&model.form);
            reset_loop_candidates(model);
        }
        Msg::StartLonChanged(val) => {
            model.form.start_lon = val;
            sync_selection_markers(&model.form);
            reset_loop_candidates(model);
        }
        Msg::EndLatChanged(val) => {
            model.form.end_lat = val;
            sync_selection_markers(&model.form);
        }
        Msg::EndLonChanged(val) => {
            model.form.end_lon = val;
            sync_selection_markers(&model.form);
        }
        Msg::PopWeightChanged(val) => {
            model.form.w_pop = val;
            reset_loop_candidates(model);
        }
        Msg::PavedWeightChanged(val) => {
            model.form.w_paved = val;
            reset_loop_candidates(model);
        }
        Msg::LoopDistanceChanged(val) => {
            model.loop_form.distance_km = val;
            reset_loop_candidates(model);
        }
        Msg::LoopToleranceChanged(val) => {
            model.loop_form.tolerance_km = val;
            reset_loop_candidates(model);
        }
        Msg::LoopCandidateCountChanged(val) => {
            model.loop_form.candidate_count = val;
            reset_loop_candidates(model);
        }
        Msg::LoopMaxAscentChanged(val) => {
            model.loop_form.max_ascent_m = val;
            reset_loop_candidates(model);
        }
        Msg::LoopMinAscentChanged(val) => {
            model.loop_form.min_ascent_m = val;
            reset_loop_candidates(model);
        }
        Msg::Submit => {
            if model.pending {
                return;
            }
            model.error = None;
            match model.route_mode {
                RouteMode::PointToPoint => match model.form.to_request() {
                    Ok(payload) => {
                        model.pending = true;
                        orders.perform_cmd(send_route_request(payload));
                    }
                    Err(err) => model.error = Some(err),
                },
                RouteMode::Loop => match model.loop_form.to_request(&model.form) {
                    Ok(payload) => {
                        model.pending = true;
                        reset_loop_candidates(model);
                        orders.perform_cmd(send_loop_request(payload));
                    }
                    Err(err) => model.error = Some(err),
                },
                RouteMode::MultiPoint => {
                    orders.send_msg(Msg::ComputeMultiPointRoute);
                }
            }
        }
        Msg::RouteFetched(result) => {
            model.pending = false;
            match result {
                Ok(route) => {
                    apply_route(model, route);
                    reset_loop_candidates(model);
                }
                Err(err) => {
                    push_route_to_map(&[]);
                    model.error = Some(err);
                    reset_loop_candidates(model);
                }
            }
        }
        Msg::LoopRouteFetched(result) => {
            model.pending = false;
            match result {
                Ok(response) => {
                    if response.candidates.is_empty() {
                        push_route_to_map(&[]);
                        model.loop_meta = None;
                        model.error = Some("Aucune boucle trouvée pour ces paramètres".to_string());
                        return;
                    }
                    model.loop_meta = Some(LoopMeta {
                        target_distance_km: response.target_distance_km,
                        distance_tolerance_km: response.distance_tolerance_km,
                    });
                    model.loop_candidates = response.candidates;
                    model.error = None;
                    if let Some(first) = model.loop_candidates.get(0) {
                        model.selected_loop_idx = Some(0);
                        apply_route(model, first.route.clone());
                    }
                }
                Err(err) => {
                    push_route_to_map(&[]);
                    model.error = Some(err);
                }
            }
        }
        Msg::SelectLoopCandidate(idx) => {
            if let Some(candidate) = model.loop_candidates.get(idx) {
                model.selected_loop_idx = Some(idx);
                apply_route(model, candidate.route.clone());
            }
        }
        Msg::SetClickMode(mode) => {
            model.click_mode = mode;
        }
        Msg::ToggleRouteMode(mode) => {
            model.route_mode = mode;
            reset_loop_candidates(model);
            // Clear waypoints when switching away from MultiPoint mode
            if mode != RouteMode::MultiPoint && !model.waypoints.is_empty() {
                model.waypoints.clear();
                if let Ok(empty_array) = to_value(&Vec::<Coordinate>::new()) {
                    update_waypoint_markers(empty_array);
                }
            }
        }
        Msg::ToggleMapView => {
            model.map_view_mode = match model.map_view_mode {
                MapViewMode::Standard => MapViewMode::Satellite,
                MapViewMode::Satellite => MapViewMode::Standard,
            };
            toggle_satellite_view(model.map_view_mode == MapViewMode::Satellite);
        }
        Msg::Toggle3DView => {
            model.view_mode = match model.view_mode {
                ViewMode::Map2D => ViewMode::Map3D,
                ViewMode::Map3D => ViewMode::Map2D,
            };
            // Maplibre toggles terrain view directly
            toggle_three_3d_view(model.view_mode == ViewMode::Map3D);
        }
        Msg::PlayAnimation => {
            start_animation();
            model.animation_state = AnimationState::Playing;
        }
        Msg::PauseAnimation => {
            stop_animation();
            model.animation_state = AnimationState::Stopped;
        }
        Msg::SaveRoute => {
            if let Some(ref route) = model.last_response {
                // Demander un nom à l'utilisateur
                if let Some(window) = web_sys::window() {
                    if let Ok(Some(name)) = window.prompt_with_message("Nom de la route :") {
                        if !name.trim().is_empty() {
                            save_route_to_disk_with_name(route, &name);
                            // Rafraîchir la liste après sauvegarde
                            orders.send_msg(Msg::RefreshRoutesList);
                        }
                    }
                }
            }
        }
        Msg::LoadRoute => {
            // Afficher la liste des routes sauvegardées
            orders.send_msg(Msg::RefreshRoutesList);
            model.show_routes_list = !model.show_routes_list;
        }
        Msg::RefreshRoutesList => {
            orders.perform_cmd(async {
                match fetch_saved_routes_list().await {
                    Ok(routes) => Msg::ListRoutesFetched(Ok(routes)),
                    Err(e) => Msg::ListRoutesFetched(Err(e)),
                }
            });
        }
        Msg::ToggleRoutesList => {
            model.show_routes_list = !model.show_routes_list;
        }
        Msg::LoadSpecificRoute(filename) => {
            orders.perform_cmd(async move {
                match load_route_from_disk_by_filename(&filename).await {
                    Ok(route) => {
                        web_sys::console::log_1(&format!("Route '{}' chargée", filename).into());
                        Msg::RouteFetched(Ok(route))
                    }
                    Err(e) => Msg::RouteFetched(Err(e)),
                }
            });
            model.show_routes_list = false;
        }
        Msg::ListRoutesFetched(result) => {
            match result {
                Ok(routes) => {
                    model.saved_routes = routes;
                }
                Err(e) => {
                    web_sys::console::error_1(&format!("Failed to load routes list: {}", e).into());
                }
            }
        }
        Msg::MapClicked { lat, lon } => {
            if model.route_mode == RouteMode::MultiPoint {
                // In MultiPoint mode, add waypoint directly
                orders.send_msg(Msg::AddWaypoint(Coordinate { lat, lon }));
                return;
            }

            let lat_str = format_coord(lat);
            let lon_str = format_coord(lon);
            web_sys::console::debug_1(
                &format!(
                    "[frontend] MapClicked mode={:?} lat={lat:.5} lon={lon:.5}",
                    model.click_mode
                )
                .into(),
            );
            match model.click_mode {
                ClickMode::Start => {
                    model.form.start_lat = lat_str;
                    model.form.start_lon = lon_str;
                    reset_loop_candidates(model);
                }
                ClickMode::End => {
                    model.form.end_lat = lat_str;
                    model.form.end_lon = lon_str;
                }
            }
            sync_selection_markers(&model.form);
        }
        Msg::AddWaypoint(coord) => {
            model.waypoints.push(coord);
            model.error = None;
            // Update waypoint markers on map
            if let Ok(waypoints_js) = to_value(&model.waypoints) {
                update_waypoint_markers(waypoints_js);
            }
            // Don't auto-compute - let user click "Calculer" button when ready
        }
        Msg::RemoveWaypoint(idx) => {
            if idx < model.waypoints.len() {
                model.waypoints.remove(idx);
                // Update waypoint markers on map
                if let Ok(waypoints_js) = to_value(&model.waypoints) {
                    update_waypoint_markers(waypoints_js);
                }
                // Clear route if less than 2 waypoints remain
                if model.waypoints.len() < 2 {
                    push_route_to_map(&[]);
                    model.last_response = None;
                }
            }
        }
        Msg::ClearWaypoints => {
            model.waypoints.clear();
            // Clear waypoint markers from map
            if let Ok(empty_array) = to_value(&Vec::<Coordinate>::new()) {
                update_waypoint_markers(empty_array);
            }
            push_route_to_map(&[]);
            model.last_response = None;
            model.error = None;
        }
        Msg::ToggleCloseLoop => {
            model.close_loop = !model.close_loop;
            // Don't auto-recalculate - let user click "Calculer" when ready
        }
        Msg::ComputeMultiPointRoute => {
            if model.waypoints.len() < 2 {
                model.error = Some("Au moins 2 points requis".to_string());
                return;
            }
            if model.pending {
                return;
            }
            model.pending = true;
            model.error = None;
            orders.perform_cmd(send_multipoint_route_request(
                model.waypoints.clone(),
                model.close_loop,
                model.form.w_pop.parse().unwrap_or(1.0),
                model.form.w_paved.parse().unwrap_or(1.0),
            ));
        }
    }
}

async fn send_route_request(payload: RouteRequest) -> Msg {
    web_sys::console::debug_1(
        &format!(
            "[frontend] sending route request start=({:.5},{:.5}) end=({:.5},{:.5})",
            payload.start.lat, payload.start.lon, payload.end.lat, payload.end.lon
        )
        .into(),
    );
    let response = match Request::new(api_root()).method(Method::Post).json(&payload) {
        Err(err) => Err(format!("{err:?}")),
        Ok(request) => match request.fetch().await {
            Err(err) => Err(format!("{err:?}")),
            Ok(raw) => match raw.check_status() {
                Err(status_err) => Err(format!("{status_err:?}")),
                Ok(resp) => match resp.json::<RouteResponse>().await {
                    Ok(route) => Ok(route),
                    Err(err) => Err(format!("{err:?}")),
                },
            },
        },
    };

    Msg::RouteFetched(response)
}

async fn send_multipoint_route_request(
    waypoints: Vec<Coordinate>,
    close_loop: bool,
    w_pop: f64,
    w_paved: f64,
) -> Msg {
    web_sys::console::debug_1(
        &format!(
            "[frontend] sending multipoint route request with {} waypoints, close_loop={}",
            waypoints.len(),
            close_loop
        )
        .into(),
    );

    // Use optimized backend endpoint that generates single graph for all waypoints
    let payload = MultiPointRouteRequest {
        waypoints,
        close_loop,
        w_pop,
        w_paved,
    };

    let response = match Request::new(format!("{}/multi", api_root()))
        .method(Method::Post)
        .json(&payload)
    {
        Err(err) => Err(format!("{err:?}")),
        Ok(request) => match request.fetch().await {
            Err(err) => Err(format!("{err:?}")),
            Ok(raw) => match raw.check_status() {
                Err(status_err) => Err(format!("{status_err:?}")),
                Ok(resp) => match resp.json::<RouteResponse>().await {
                    Ok(route) => Ok(route),
                    Err(err) => Err(format!("{err:?}")),
                },
            },
        },
    };

    Msg::RouteFetched(response)
}

async fn send_loop_request(payload: LoopRouteRequest) -> Msg {
    web_sys::console::debug_1(
        &format!(
            "[frontend] sending loop request start=({:.5},{:.5}) target={:.1}km",
            payload.start.lat, payload.start.lon, payload.target_distance_km
        )
        .into(),
    );
    let response = match Request::new(loop_api_root())
        .method(Method::Post)
        .json(&payload)
    {
        Err(err) => Err(format!("{err:?}")),
        Ok(request) => match request.fetch().await {
            Err(err) => Err(format!("{err:?}")),
            Ok(raw) => match raw.check_status() {
                Err(status_err) => Err(format!("{status_err:?}")),
                Ok(resp) => match resp.json::<LoopRouteResponse>().await {
                    Ok(route) => Ok(route),
                    Err(err) => Err(format!("{err:?}")),
                },
            },
        },
    };

    Msg::LoopRouteFetched(response)
}

pub fn view(model: &Model) -> Node<Msg> {
    let header = h1!["Chemins Noirs – générateur GPX anti-bitume"];
    let form = view_form(model);
    let preview = view_preview(model);

    div![C!["app-container"], header, form, preview]
}

fn view_form(model: &Model) -> Node<Msg> {
    let input_field = |label: &str, value: &str, msg: fn(String) -> Msg, disabled: bool| {
        div![
            C!["input-field"],
            label![label],
            input![
                attrs! {
                    At::Value => value,
                    At::AutoComplete => "off",
                    At::SpellCheck => "false",
                    At::Disabled => bool_attr(disabled),
                },
                input_ev(Ev::Input, msg),
            ]
        ]
    };
    let disable_end = model.route_mode == RouteMode::Loop;

    form![
        C!["controls"],
        fieldset![
            legend!["Type de tracé"],
            div![
                C!["route-type"],
                label![
                    input![
                        attrs! {
                            At::Type => "radio",
                            At::Name => "route-mode",
                            At::Checked => bool_attr(model.route_mode == RouteMode::PointToPoint),
                        },
                        ev(Ev::Change, |_| Msg::ToggleRouteMode(
                            RouteMode::PointToPoint
                        )),
                    ],
                    span!["Aller simple"],
                ],
                label![
                    input![
                        attrs! {
                            At::Type => "radio",
                            At::Name => "route-mode",
                            At::Checked => bool_attr(model.route_mode == RouteMode::Loop),
                        },
                        ev(Ev::Change, |_| Msg::ToggleRouteMode(RouteMode::Loop)),
                    ],
                    span!["Boucle"],
                ],
                label![
                    input![
                        attrs! {
                            At::Type => "radio",
                            At::Name => "route-mode",
                            At::Checked => bool_attr(model.route_mode == RouteMode::MultiPoint),
                        },
                        ev(Ev::Change, |_| Msg::ToggleRouteMode(RouteMode::MultiPoint)),
                    ],
                    span!["Multi-points"],
                ],
            ],
        ],
        fieldset![
            legend!["Points"],
            input_field(
                "Latitude départ",
                &model.form.start_lat,
                Msg::StartLatChanged,
                false
            ),
            input_field(
                "Longitude départ",
                &model.form.start_lon,
                Msg::StartLonChanged,
                false
            ),
            input_field(
                "Latitude arrivée",
                &model.form.end_lat,
                Msg::EndLatChanged,
                disable_end
            ),
            input_field(
                "Longitude arrivée",
                &model.form.end_lon,
                Msg::EndLonChanged,
                disable_end
            ),
            if disable_end {
                small!["Les coordonnées d'arrivée sont ignorées en mode boucle."]
            } else {
                empty![]
            },
        ],
        fieldset![
            legend!["Poids"],
            input_field(
                "Éviter population",
                &model.form.w_pop,
                Msg::PopWeightChanged,
                false
            ),
            input_field(
                "Éviter bitume",
                &model.form.w_paved,
                Msg::PavedWeightChanged,
                false
            ),
        ],
        if model.route_mode == RouteMode::Loop {
            fieldset![
                legend!["Options boucle"],
                input_field(
                    "Distance cible (km)",
                    &model.loop_form.distance_km,
                    Msg::LoopDistanceChanged,
                    false
                ),
                input_field(
                    "Tolérance (km)",
                    &model.loop_form.tolerance_km,
                    Msg::LoopToleranceChanged,
                    false
                ),
                input_field(
                    "Nombre de propositions",
                    &model.loop_form.candidate_count,
                    Msg::LoopCandidateCountChanged,
                    false
                ),
                input_field(
                    "D+ max (m)",
                    &model.loop_form.max_ascent_m,
                    Msg::LoopMaxAscentChanged,
                    false
                ),
                input_field(
                    "D+ min (m)",
                    &model.loop_form.min_ascent_m,
                    Msg::LoopMinAscentChanged,
                    false
                ),
                small!["Laissez D+ vide pour obtenir automatiquement la boucle la moins pentue."],
            ]
        } else {
            empty![]
        },
        if model.route_mode == RouteMode::MultiPoint {
            fieldset![
                legend!["Points du tracé"],
                div![
                    C!["waypoints-list"],
                    if model.waypoints.is_empty() {
                        p![
                            style! {St::FontStyle => "italic", St::Color => "#666"},
                            "Cliquez sur la carte pour ajouter des points"
                        ]
                    } else {
                        div![
                            model.waypoints.iter().enumerate().map(|(idx, coord)| {
                                div![
                                    C!["waypoint-item"],
                                    style! {
                                        St::Display => "flex",
                                        St::JustifyContent => "space-between",
                                        St::AlignItems => "center",
                                        St::Padding => "0.5rem",
                                        St::MarginBottom => "0.25rem",
                                        St::Background => "#f5f5f5",
                                        St::BorderRadius => "4px",
                                    },
                                    span![
                                        style! {St::Color => "#333"},
                                        format!("{}. ({:.4}, {:.4})", idx + 1, coord.lat, coord.lon)
                                    ],
                                    button![
                                        style! {
                                            St::Padding => "0.25rem 0.5rem",
                                            St::Background => "#dc3545",
                                            St::Color => "white",
                                            St::Border => "none",
                                            St::BorderRadius => "3px",
                                            St::Cursor => "pointer",
                                        },
                                        "✕",
                                        ev(Ev::Click, move |_| Msg::RemoveWaypoint(idx)),
                                    ]
                                ]
                            }),
                        ]
                    }
                ],
                div![
                    style! {St::MarginTop => "1rem"},
                    label![
                        input![
                            attrs! {
                                At::Type => "checkbox",
                                At::Checked => bool_attr(model.close_loop),
                            },
                            ev(Ev::Change, |_| Msg::ToggleCloseLoop),
                        ],
                        span![" Boucler (retour au point de départ)"],
                    ],
                ],
                div![
                    style! {St::MarginTop => "1rem"},
                    button![
                        style! {
                            St::Padding => "0.5rem 1rem",
                            St::Background => "#6c757d",
                            St::Color => "white",
                            St::Border => "none",
                            St::BorderRadius => "4px",
                            St::Cursor => "pointer",
                            St::Width => "100%",
                        },
                        attrs! {
                            At::Disabled => bool_attr(model.waypoints.is_empty()),
                        },
                        ev(Ev::Click, |_| Msg::ClearWaypoints),
                        "Effacer tous les points",
                    ]
                ],
                small![
                    style! {St::Display => "block", St::MarginTop => "0.5rem"},
                    format!("{} point(s) • Distance: {:.1} km",
                        model.waypoints.len(),
                        model.last_response.as_ref().map(|r| r.distance_km).unwrap_or(0.0)
                    )
                ],
            ]
        } else {
            empty![]
        },
        if model.route_mode != RouteMode::MultiPoint {
            fieldset![
                legend!["Sélection via la carte"],
                div![
                    C!["click-mode"],
                    label![
                        input![
                            attrs! {
                                At::Type => "radio",
                                At::Name => "click-mode",
                                At::Checked => bool_attr(model.click_mode == ClickMode::Start),
                            },
                            ev(Ev::Change, |_| Msg::SetClickMode(ClickMode::Start)),
                        ],
                        span!["Départ"],
                    ],
                    label![
                        input![
                            attrs! {
                                At::Type => "radio",
                                At::Name => "click-mode",
                                At::Checked => bool_attr(model.click_mode == ClickMode::End),
                            },
                            ev(Ev::Change, |_| Msg::SetClickMode(ClickMode::End)),
                        ],
                        span!["Arrivée"],
                    ],
                ],
                small!["Cliquez sur la carte pour remplir la position sélectionnée."],
            ]
        } else {
            empty![]
        },
        fieldset![
            legend!["Vue de la carte"],
            button![
                match model.map_view_mode {
                    MapViewMode::Standard => "Vue Satellite",
                    MapViewMode::Satellite => "Vue Standard",
                },
                ev(Ev::Click, |event| {
                    event.prevent_default();
                    Msg::ToggleMapView
                }),
                C!["map-toggle"],
            ],
        ],
        if model.last_response.is_some() {
            fieldset![
                legend!["Sauvegarder/Charger"],
                button![
                    "💾 Sauvegarder",
                    ev(Ev::Click, |event| {
                        event.prevent_default();
                        Msg::SaveRoute
                    }),
                    C!["save-btn"],
                ],
                button![
                    "📂 Charger",
                    ev(Ev::Click, |event| {
                        event.prevent_default();
                        Msg::LoadRoute
                    }),
                    C!["load-btn"],
                ],
                small!["Les tracés sont sauvegardés sur le serveur."],
            ]
        } else {
            button![
                "📂 Charger tracé sauvegardé",
                ev(Ev::Click, |event| {
                    event.prevent_default();
                    Msg::LoadRoute
                }),
                C!["load-btn"],
            ]
        },
        // Liste des routes sauvegardées
        if model.show_routes_list {
            view_saved_routes_list(model)
        } else {
            empty![]
        },
        button![
            "Tracer l'itinéraire",
            ev(Ev::Click, |event| {
                event.prevent_default();
                Msg::Submit
            }),
            attrs! { At::Disabled => bool_attr(model.pending) },
        ],
        if let Some(error) = &model.error {
            p![C!["error"], error]
        } else {
            empty![]
        }
    ]
}

fn view_preview(model: &Model) -> Node<Msg> {
    if let Some(route) = &model.last_response {
        let stats = div![
            C!["stats"],
            h2!["Dernier tracé"],
            p![format!("{:.2} km parcourus", route.distance_km)],
            small!["Téléchargez le GPX via l'API (payload base64)"],
        ];
        let loop_section = view_loop_candidates(model);

        let path_points = route.path.iter().enumerate().map(|(idx, coord)| {
            let elevation = route
                .elevation_profile
                .as_ref()
                .and_then(|profile| profile.elevations.get(idx).cloned().flatten());

            li![format!(
                "{idx}: {:.5} / {:.5}{}",
                coord.lat,
                coord.lon,
                elevation
                    .map(|e| format!(" — {:.1} m", e))
                    .unwrap_or_else(|| "".to_string())
            )]
        });

        let path_list = ul![C!["path-preview"], path_points];

        let metadata = route
            .metadata
            .as_ref()
            .map(view_metadata)
            .unwrap_or_else(|| empty![]);

        let elevation = route
            .elevation_profile
            .as_ref()
            .map(view_elevation_profile)
            .unwrap_or_else(|| empty![]);

        div![
            C!["preview"],
            loop_section,
            stats,
            metadata,
            elevation,
            path_list
        ]
    } else {
        div![
            C!["preview"],
            h2!["En attente"],
            p!["Soumettez des points pour visualiser un itinéraire."]
        ]
    }
}

fn view_saved_routes_list(model: &Model) -> Node<Msg> {
    fieldset![
        C!["saved-routes-list"],
        legend!["Routes sauvegardées"],
        if model.saved_routes.is_empty() {
            p![
                style! { St::TextAlign => "center", St::Padding => "20px", St::Color => "#666" },
                "Aucune route sauvegardée"
            ]
        } else {
            div![
                C!["routes-list"],
                model.saved_routes.iter().map(|route_info| {
                    div![
                        C!["route-item"],
                        style! {
                            St::Display => "flex",
                            St::JustifyContent => "space-between",
                            St::AlignItems => "center",
                            St::Padding => "10px",
                            St::MarginBottom => "5px",
                            St::Border => "1px solid #ddd",
                            St::BorderRadius => "4px",
                            St::BackgroundColor => "#f9f9f9",
                        },
                        div![
                            C!["route-info"],
                            strong![&route_info.name],
                            br![],
                            small![
                                format!("{:.2} km · ", route_info.distance_km),
                                route_info.saved_at.split('T').next().unwrap_or(&route_info.saved_at)
                            ],
                        ],
                        button![
                            "📥 Charger",
                            C!["load-btn-small"],
                            style! {
                                St::Padding => "5px 10px",
                                St::FontSize => "0.9em",
                            },
                            ev(Ev::Click, {
                                let filename = route_info.filename.clone();
                                move |event| {
                                    event.prevent_default();
                                    Msg::LoadSpecificRoute(filename.clone())
                                }
                            }),
                        ]
                    ]
                })
            ]
        },
        button![
            "✖ Fermer",
            C!["close-btn"],
            style! {
                St::MarginTop => "10px",
            },
            ev(Ev::Click, |event| {
                event.prevent_default();
                Msg::ToggleRoutesList
            }),
        ]
    ]
}

fn view_metadata(meta: &shared::RouteMetadata) -> Node<Msg> {
    let card = |label: &str, content: String| {
        div![
            C!["metadata-card"],
            span![C!["label"], label],
            strong![content],
        ]
    };

    div![
        C!["metadata-grid"],
        card("Points", meta.point_count.to_string()),
        card(
            "Départ",
            format!("{:.4} / {:.4}", meta.start.lat, meta.start.lon)
        ),
        card(
            "Arrivée",
            format!("{:.4} / {:.4}", meta.end.lat, meta.end.lon)
        ),
        card(
            "BBox",
            format!(
                "[{:.3}↔{:.3}] lat / [{:.3}↔{:.3}] lon",
                meta.bounds.min_lat, meta.bounds.max_lat, meta.bounds.min_lon, meta.bounds.max_lon
            )
        ),
    ]
}

fn view_loop_candidates(model: &Model) -> Node<Msg> {
    if model.loop_candidates.is_empty() {
        return empty![];
    }

    let heading = model
        .loop_meta
        .as_ref()
        .map(|meta| {
            format!(
                "Boucles proposées – cible {:.1} km (± {:.1} km)",
                meta.target_distance_km, meta.distance_tolerance_km
            )
        })
        .unwrap_or_else(|| "Boucles proposées".to_string());

    let entries = model
        .loop_candidates
        .iter()
        .enumerate()
        .map(|(idx, candidate)| {
            let ascent_label = candidate
                .route
                .elevation_profile
                .as_ref()
                .map(|profile| format!("{:.0} m D+", profile.total_ascent))
                .unwrap_or_else(|| "D+ ?".to_string());

            let class_name = if model.selected_loop_idx == Some(idx) {
                "loop-choice selected"
            } else {
                "loop-choice"
            };

            button![
                format!(
                    "#{idx} – {:.1} km • {} • Δ{:+.1} km • cap {:.0}°",
                    candidate.route.distance_km,
                    ascent_label,
                    candidate.distance_error_km,
                    candidate.bearing_deg
                ),
                ev(Ev::Click, move |event| {
                    event.prevent_default();
                    Msg::SelectLoopCandidate(idx)
                }),
                C![class_name],
            ]
        });

    div![
        C!["loop-candidates"],
        h3![heading],
        small!["Choisissez la boucle qui vous convient le mieux."],
        div![entries.collect::<Vec<_>>()],
    ]
}

#[wasm_bindgen(start)]
pub fn start() {
    init_map();
    App::start("app", init, update, view);
}

fn apply_route(model: &mut Model, route: RouteResponse) {
    push_route_to_map(&route.path);

    let start_coord = route.path.first().copied();
    let end_coord = route.path.last().copied();
    let metadata = route.metadata.clone();

    if let Some(start) = start_coord {
        model.form.start_lat = format_coord(start.lat);
        model.form.start_lon = format_coord(start.lon);
    }
    if let Some(end) = end_coord {
        model.form.end_lat = format_coord(end.lat);
        model.form.end_lon = format_coord(end.lon);
    }

    if let Some(ref metadata) = metadata {
        if let Ok(bounds_value) = to_value(&metadata.bounds) {
            update_bbox_js(bounds_value);
        }
    }

    model.last_response = Some(route);
    model.error = None;
    sync_selection_markers(&model.form);

    if let (Some(start), Some(end)) = (
        model
            .form
            .coordinate_pair(&model.form.start_lat, &model.form.start_lon),
        model
            .form
            .coordinate_pair(&model.form.end_lat, &model.form.end_lon),
    ) {
        if let (Ok(start_js), Ok(end_js)) = (to_value(&start), to_value(&end)) {
            center_on_markers(start_js, end_js);
        }
    }
}

fn reset_loop_candidates(model: &mut Model) {
    model.loop_candidates.clear();
    model.loop_meta = None;
    model.selected_loop_idx = None;
}

fn push_route_to_map(path: &[Coordinate]) {
    if let Ok(value) = to_value(path) {
        update_route_js(value);
    }
}

fn sync_selection_markers(form: &RouteForm) {
    let start = form
        .coordinate_pair(&form.start_lat, &form.start_lon)
        .and_then(|coord| to_value(&coord).ok())
        .unwrap_or(JsValue::NULL);
    let end = form
        .coordinate_pair(&form.end_lat, &form.end_lon)
        .and_then(|coord| to_value(&coord).ok())
        .unwrap_or(JsValue::NULL);
    update_selection_markers(start, end);
}

impl RouteForm {
    fn coordinate_pair(&self, lat: &str, lon: &str) -> Option<Coordinate> {
        let lat = lat.trim().parse::<f64>().ok()?;
        let lon = lon.trim().parse::<f64>().ok()?;
        Some(Coordinate { lat, lon })
    }
}

fn bool_attr(value: bool) -> AtValue {
    if value {
        AtValue::Some("true".into())
    } else {
        AtValue::Ignored
    }
}

fn format_coord(value: f64) -> String {
    format!("{value:.5}")
}

fn view_elevation_profile(profile: &shared::ElevationProfile) -> Node<Msg> {
    let card = |label: &str, content: String| {
        div![
            C!["metadata-card"],
            span![C!["label"], label],
            strong![content],
        ]
    };

    let elevation_stats = div![
        C!["metadata-grid"],
        card("Dénivelé +", format!("{:.0} m", profile.total_ascent)),
        card("Dénivelé -", format!("{:.0} m", profile.total_descent)),
        card(
            "Altitude min",
            profile
                .min_elevation
                .map(|e| format!("{:.0} m", e))
                .unwrap_or_else(|| "N/A".to_string())
        ),
        card(
            "Altitude max",
            profile
                .max_elevation
                .map(|e| format!("{:.0} m", e))
                .unwrap_or_else(|| "N/A".to_string())
        ),
    ];

    div![
        C!["elevation-section"],
        h3!["Profil d'élévation"],
        elevation_stats
    ]
}

#[derive(Deserialize)]
struct MapClickPayload {
    lat: f64,
    lon: f64,
}

// Save route to disk via API with name
fn save_route_to_disk_with_name(route: &RouteResponse, name: &str) {
    let save_request = SaveRouteRequest {
        name: name.to_string(),
        route: route.clone(),
    };

    spawn_local(async move {
        match Request::new("http://localhost:8080/api/routes/save")
            .method(Method::Post)
            .json(&save_request)
        {
            Err(err) => {
                web_sys::console::error_1(&format!("Failed to build request: {:?}", err).into());
            }
            Ok(request) => match request.fetch().await {
                Err(err) => {
                    web_sys::console::error_1(&format!("Failed to save route: {:?}", err).into());
                }
                Ok(_) => {
                    web_sys::console::log_1(&"Route sauvegardée sur le disque".into());
                }
            },
        }
    });
}

// Load route from disk by filename via API
async fn load_route_from_disk_by_filename(filename: &str) -> Result<RouteResponse, String> {
    let url = format!("http://localhost:8080/api/routes/load?filename={}", filename);
    let request = Request::new(&url).method(Method::Get);
    match request.fetch().await {
        Err(err) => Err(format!("Failed to fetch: {:?}", err)),
        Ok(raw) => match raw.check_status() {
            Err(status_err) => Err(format!("Status error: {:?}", status_err)),
            Ok(resp) => match resp.json::<RouteResponse>().await {
                Ok(route) => {
                    web_sys::console::log_1(&"Route chargée depuis le disque".into());
                    Ok(route)
                }
                Err(err) => Err(format!("Failed to parse JSON: {:?}", err)),
            },
        },
    }
}

// Fetch list of saved routes
async fn fetch_saved_routes_list() -> Result<Vec<SavedRouteInfo>, String> {
    let request = Request::new("http://localhost:8080/api/routes/list").method(Method::Get);
    match request.fetch().await {
        Err(err) => Err(format!("Failed to fetch routes list: {:?}", err)),
        Ok(raw) => match raw.check_status() {
            Err(status_err) => Err(format!("Status error: {:?}", status_err)),
            Ok(resp) => match resp.json::<Vec<SavedRouteInfo>>().await {
                Ok(routes) => Ok(routes),
                Err(err) => Err(format!("Failed to parse routes list: {:?}", err)),
            },
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_view_mode_toggle() {
        assert_eq!(ViewMode::Map2D, ViewMode::Map2D);
        assert_ne!(ViewMode::Map2D, ViewMode::Map3D);

        let mode_2d = ViewMode::Map2D;
        let toggled = match mode_2d {
            ViewMode::Map2D => ViewMode::Map3D,
            ViewMode::Map3D => ViewMode::Map2D,
        };
        assert_eq!(toggled, ViewMode::Map3D);
    }

    #[test]
    fn test_animation_state_transitions() {
        let stopped = AnimationState::Stopped;
        let playing = AnimationState::Playing;
        let paused = AnimationState::Paused;

        assert_eq!(stopped, AnimationState::Stopped);
        assert_ne!(stopped, playing);
        assert_ne!(playing, paused);
    }

    #[test]
    fn test_click_mode_toggle() {
        assert_eq!(ClickMode::Start, ClickMode::Start);
        assert_ne!(ClickMode::Start, ClickMode::End);
    }

    #[test]
    fn test_map_view_mode_toggle() {
        let standard = MapViewMode::Standard;
        let satellite = MapViewMode::Satellite;

        assert_eq!(standard, MapViewMode::Standard);
        assert_ne!(standard, satellite);
    }

    #[test]
    fn test_route_form_to_request_valid() {
        let form = RouteForm {
            start_lat: "45.93".to_string(),
            start_lon: "4.577".to_string(),
            end_lat: "45.94".to_string(),
            end_lon: "4.575".to_string(),
            w_pop: "1.0".to_string(),
            w_paved: "1.0".to_string(),
        };

        let request = form.to_request();
        assert!(request.is_ok(), "Expected Ok, got: {:?}", request);
        let req = request.unwrap();
        assert_eq!(req.start.lat, 45.93);
        assert_eq!(req.start.lon, 4.577);
        assert_eq!(req.end.lat, 45.94);
        assert_eq!(req.end.lon, 4.575);
        assert_eq!(req.w_pop, 1.0);
        assert_eq!(req.w_paved, 1.0);
    }

    #[test]
    fn test_route_form_to_request_invalid_coords() {
        let form = RouteForm {
            start_lat: "45.93".to_string(),
            start_lon: "invalid".to_string(),
            end_lat: "45.94".to_string(),
            end_lon: "4.575".to_string(),
            w_pop: "1.0".to_string(),
            w_paved: "1.0".to_string(),
        };

        let request = form.to_request();
        assert!(request.is_err(), "Expected error for invalid coordinate");
    }
}
